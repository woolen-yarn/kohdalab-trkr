from __future__ import annotations

import csv
import math
import os
import time
from collections.abc import Iterator
from pathlib import Path
from typing import Any, Callable, cast

from kohdalab.api.config import (
    MAX_SCAN_POINTS_TOTAL,
    build_range_points,
    measurement_output_settings,
    measurement_settings,
    move_abs_zero,
    output_path,
    scan_settings,
)
from kohdalab.api.measurement_rows import (
    fields_for_row,
    fields_for_rows,
    output_row,
    output_rows,
    signal_monitor_row,
    srkr_row,
    trkr_row,
)
from kohdalab.api.measurement_rows import scan2d_row
from kohdalab.api.models import MeasurementPoint
from kohdalab.api.run_metadata import RunMetadata, metadata_path, utc_now_iso
from kohdalab.api.scan_plan import (
    Scan2DPlan,
    SignalMonitorPlan,
    Srkr2DPlan,
    SrkrPlan,
    StrkrPlan,
    TrkrPlan,
    normalize_coordinate,
    normalize_scanner_coordinate,
    srkr_2d_plan_from_config,
    strkr_plan_from_config,
)
from kohdalab.api.scan_limits import preflight_axis_targets
from kohdalab.api.session import DeviceSession
from kohdalab.api.status import (
    STATUS_READING_LOCKIN,
    STATUS_RUNNING,
    STATUS_SLOW_AXIS_READY,
    STATUS_STOPPED,
    STATUS_WAITING,
    StatusCallback,
)

PointCallback = Callable[[Any], None]
ContinueCallback = Callable[[], bool]
CleanupCallback = Callable[[], None]


def _validated_non_negative(value: float | int, name: str) -> float:
    result = float(value)
    if not math.isfinite(result) or result < 0:
        raise ValueError(f"{name} must be finite and non-negative.")
    return result


def scan_points_from_config(
    config: dict[str, Any], measurement_name: str
) -> list[float]:
    scan = scan_settings(config, measurement_name)
    return build_range_points(
        float(scan["min"]), float(scan["max"]), float(scan["step"])
    )


def _continue(should_continue: ContinueCallback | None) -> bool:
    return True if should_continue is None else bool(should_continue())


def _sleep_interruptible(
    duration_s: float, should_continue: ContinueCallback | None
) -> bool:
    deadline = time.monotonic() + max(0.0, float(duration_s))
    while time.monotonic() < deadline:
        if not _continue(should_continue):
            return False
        time.sleep(min(0.05, deadline - time.monotonic()))
    return _continue(should_continue)


def _emit_status(on_status: StatusCallback | None, status: str) -> None:
    if on_status is not None:
        on_status(status)


def _disconnect_owned_session(
    session: DeviceSession, owns_session: bool, failure: BaseException | None
) -> None:
    if not owns_session:
        return
    try:
        session.disconnect_all()
    except BaseException as cleanup_error:
        if failure is None:
            raise
        failure.add_note(f"Measurement session cleanup also failed: {cleanup_error}")


def _write_rows(
    rows: list[dict[str, Any]],
    *,
    output: str | Path | None,
    point_iter: Iterator[MeasurementPoint],
    on_point: PointCallback | None,
    config: dict[str, Any],
    measurement_name: str,
    expected_points: int,
    on_unstarted_cleanup: CleanupCallback | None = None,
) -> list[dict[str, Any]]:
    csv_file = None
    writer = None
    run_metadata = None
    failure: BaseException | None = None
    iterator_started = False
    try:
        if output is not None:
            path = Path(output)
            path.parent.mkdir(parents=True, exist_ok=True)
            sidecar = metadata_path(path)
            if sidecar.exists():
                raise FileExistsError(f"Measurement metadata already exists: {sidecar}")
            csv_file = path.open("x", newline="", encoding="utf-8")
            try:
                candidate_metadata = RunMetadata(
                    output_path=path,
                    measurement=measurement_name,
                    config=config,
                    expected_points=expected_points,
                )
                candidate_metadata.write()
                run_metadata = candidate_metadata
            except BaseException:
                csv_file.close()
                csv_file = None
                path.unlink(missing_ok=True)
                raise
        iterator_started = True
        for point in point_iter:
            if csv_file is not None:
                if writer is None:
                    writer = csv.DictWriter(
                        csv_file, fieldnames=fields_for_row(point.row)
                    )
                    writer.writeheader()
                writer.writerow(output_row(point.row))
                rows.append(point.row)
                csv_file.flush()
            else:
                rows.append(point.row)
            if on_point is not None:
                on_point(point)
    except BaseException as error:
        failure = error
        raise
    finally:
        terminal_error = failure
        close_iterator = getattr(point_iter, "close", None)
        if callable(close_iterator):
            try:
                close_iterator()
            except BaseException as iterator_error:
                if terminal_error is None:
                    terminal_error = iterator_error
                else:
                    terminal_error.add_note(
                        f"Measurement iterator cleanup also failed: {iterator_error}"
                    )
        if not iterator_started and on_unstarted_cleanup is not None:
            try:
                on_unstarted_cleanup()
            except BaseException as cleanup_error:
                cast(BaseException, terminal_error).add_note(
                    f"Measurement session cleanup also failed: {cleanup_error}"
                )
        if csv_file is not None:
            try:
                csv_file.close()
            except BaseException as close_error:
                if terminal_error is None:
                    terminal_error = close_error
                else:
                    terminal_error.add_note(f"CSV close also failed: {close_error}")
        if run_metadata is not None:
            if terminal_error is not None:
                status = (
                    "interrupted"
                    if isinstance(terminal_error, (KeyboardInterrupt, SystemExit))
                    else "failed"
                )
            else:
                status = "completed" if len(rows) == expected_points else "stopped"
            try:
                run_metadata.finish(
                    status=status,
                    rows_written=len(rows),
                    error=terminal_error,
                )
            except BaseException as metadata_error:
                if terminal_error is None:
                    terminal_error = metadata_error
                else:
                    terminal_error.add_note(
                        f"Run metadata finalization also failed: {metadata_error}"
                    )
        if failure is None and terminal_error is not None:
            raise terminal_error
    return rows


def write_measurement_rows(
    rows: list[dict[str, Any]],
    *,
    output: str | Path,
    config: dict[str, Any],
    measurement_name: str,
    overwrite: bool = False,
) -> Path:
    """Atomically export collected rows and create a matching provenance sidecar."""
    path = Path(output)
    path.parent.mkdir(parents=True, exist_ok=True)
    run_metadata = RunMetadata(
        output_path=path,
        measurement=measurement_name,
        config=config,
        expected_points=len(rows),
        allow_overwrite=overwrite,
    )
    if not overwrite:
        if path.exists():
            raise FileExistsError(f"Measurement output already exists: {path}")
        if run_metadata.path.exists():
            raise FileExistsError(
                f"Measurement metadata already exists: {run_metadata.path}"
            )
    temporary = path.with_name(f".{path.name}.{run_metadata.data['run_id']}.tmp")
    output_backup = path.with_name(f".{path.name}.{run_metadata.data['run_id']}.bak")
    metadata_backup = run_metadata.path.with_name(
        f".{run_metadata.path.name}.{run_metadata.data['run_id']}.bak"
    )
    had_output = path.exists()
    had_metadata = run_metadata.path.exists()
    published = False
    failure: BaseException | None = None
    preserve_backups = False
    try:
        with temporary.open("x", newline="", encoding="utf-8") as stream:
            writer = csv.DictWriter(stream, fieldnames=fields_for_rows(rows))
            writer.writeheader()
            writer.writerows(output_rows(rows))
            stream.flush()
            os.fsync(stream.fileno())
        if overwrite:
            if had_output:
                os.link(path, output_backup)
            if had_metadata:
                os.link(run_metadata.path, metadata_backup)
            temporary.replace(path)
        else:
            os.link(temporary, path)
            temporary.unlink()
        published = True
        run_metadata.finish(status="completed", rows_written=len(rows))
    except BaseException as error:
        failure = error
        if published:
            try:
                if had_output:
                    output_backup.replace(path)
                else:
                    path.unlink(missing_ok=True)
                if had_metadata:
                    metadata_backup.replace(run_metadata.path)
                elif run_metadata._written or overwrite:
                    run_metadata.path.unlink(missing_ok=True)
            except BaseException as rollback_error:
                preserve_backups = True
                error.add_note(
                    f"Measurement export rollback also failed: {rollback_error}"
                )
        raise
    finally:
        cleanup_paths = [temporary]
        if not preserve_backups:
            cleanup_paths.extend((output_backup, metadata_backup))
        cleanup_failure: BaseException | None = None
        for cleanup_path in cleanup_paths:
            try:
                cleanup_path.unlink(missing_ok=True)
            except BaseException as cleanup_error:
                primary_error = failure or cleanup_failure
                if primary_error is None:
                    cleanup_failure = cleanup_error
                else:
                    primary_error.add_note(
                        f"Measurement export cleanup also failed for {cleanup_path}: "
                        f"{cleanup_error}"
                    )
        if failure is None and cleanup_failure is not None:
            raise cleanup_failure
    return path


def _zero_for_axis(zero: dict[str, float], axis: str) -> float:
    if axis == "t":
        return float(zero.get("t_ps", 0.0))
    return float(zero.get(f"{axis}_um", 0.0))


def _absolute_measurement_target(
    zero: dict[str, float], axis: str, corrected_target: float
) -> float:
    return _zero_for_axis(zero, axis) + float(corrected_target)


def _move_axis(
    session: DeviceSession,
    axis: str,
    corrected_target: float,
    *,
    zero: dict[str, float],
    apply_software_hysteresis: bool = True,
    on_status: StatusCallback | None = None,
) -> None:
    target = _absolute_measurement_target(zero, axis, corrected_target)
    if axis == "t":
        session.move_delay_stage(target, coordinate="measurement", on_status=on_status)
        return
    if axis in {"x", "y"}:
        session.move_scanner(
            axis,
            target,
            coordinate="measurement",
            apply_software_hysteresis=apply_software_hysteresis,
            on_status=on_status,
        )
        return
    raise ValueError(f"Unsupported scan axis: {axis}")


def _run_scan2d(
    config: dict[str, Any],
    *,
    measurement_name: str,
    plan: Scan2DPlan,
    wait_s: float | None,
    output: str | Path | None,
    on_status: StatusCallback | None,
    on_point: PointCallback | None,
    should_continue: ContinueCallback | None,
    session: DeviceSession | None,
) -> list[dict[str, Any]]:
    settings = measurement_settings(config, measurement_name)
    wait = _validated_non_negative(
        wait_s if wait_s is not None else settings.get("wait_s", 1.0),
        f"{measurement_name}.wait_s",
    )
    if plan.total_points <= 0 or plan.total_points > MAX_SCAN_POINTS_TOTAL:
        raise ValueError(
            f"{measurement_name} scan point count must be between 1 and {MAX_SCAN_POINTS_TOTAL}."
        )
    for axis, corrected_targets, return_key in (
        (plan.fast_axis, plan.fast_target_points, "fast_axis"),
        (plan.slow_axis, plan.slow_target_points, "slow_axis"),
    ):
        absolute_targets = [
            _absolute_measurement_target(plan.zero, axis, value)
            for value in corrected_targets
        ]
        if plan.return_to_zero.get(return_key, False):
            absolute_targets.append(_zero_for_axis(plan.zero, axis))
        preflight_axis_targets(
            config,
            measurement_name=measurement_name,
            axis=axis,
            targets=absolute_targets,
            coordinate="measurement",
        )
    owns_session = session is None
    session = session or DeviceSession(config)
    out = output or output_path(
        measurement_output_settings(config, measurement_name),
        f"{measurement_name}_run.csv",
    )
    zero = plan.zero

    def points() -> Iterator[MeasurementPoint]:
        measurement_failure: BaseException | None = None
        try:
            _emit_status(on_status, STATUS_RUNNING)
            total = plan.total_points
            index = 0
            fast_first = plan.fast_target_points[0]
            scanner_axes_approached: set[str] = set()
            for slow_index, slow_target in enumerate(plan.slow_target_points):
                if not _continue(should_continue):
                    break
                slow_apply_hysteresis = (
                    plan.slow_axis in {"x", "y"}
                    and plan.slow_axis not in scanner_axes_approached
                )
                _move_axis(
                    session,
                    plan.slow_axis,
                    slow_target,
                    zero=zero,
                    apply_software_hysteresis=slow_apply_hysteresis,
                    on_status=on_status,
                )
                if plan.slow_axis in {"x", "y"}:
                    scanner_axes_approached.add(plan.slow_axis)
                _emit_status(on_status, STATUS_SLOW_AXIS_READY)
                for fast_target in plan.fast_target_points:
                    if not _continue(should_continue):
                        break
                    index += 1
                    fast_apply_hysteresis = (
                        plan.fast_axis in {"x", "y"}
                        and plan.fast_axis not in scanner_axes_approached
                    )
                    _move_axis(
                        session,
                        plan.fast_axis,
                        fast_target,
                        zero=zero,
                        apply_software_hysteresis=fast_apply_hysteresis,
                        on_status=on_status,
                    )
                    if plan.fast_axis in {"x", "y"}:
                        scanner_axes_approached.add(plan.fast_axis)
                    _emit_status(on_status, STATUS_WAITING)
                    if not _sleep_interruptible(wait, should_continue):
                        break
                    _emit_status(on_status, STATUS_READING_LOCKIN)
                    position = session.read_position()
                    signal = session.read_lockin_signal()
                    row = scan2d_row(
                        timestamp=utc_now_iso(),
                        measurement=measurement_name,
                        fast_axis=plan.fast_axis,
                        slow_axis=plan.slow_axis,
                        targets={
                            plan.fast_axis: fast_target,
                            plan.slow_axis: slow_target,
                        },
                        position=position,
                        zero=zero,
                        signal=signal,
                    )
                    yield MeasurementPoint(index=index, total_points=total, row=row)
                if slow_index < len(plan.slow_target_points) - 1 and _continue(
                    should_continue
                ):
                    _move_axis(
                        session,
                        plan.fast_axis,
                        fast_first,
                        zero=zero,
                        apply_software_hysteresis=False,
                        on_status=on_status,
                    )
            if _continue(should_continue):
                if plan.return_to_zero.get("fast_axis", False):
                    _move_axis(
                        session,
                        plan.fast_axis,
                        0.0,
                        zero=zero,
                        apply_software_hysteresis=plan.fast_axis in {"x", "y"},
                        on_status=on_status,
                    )
                if plan.return_to_zero.get("slow_axis", False):
                    _move_axis(
                        session,
                        plan.slow_axis,
                        0.0,
                        zero=zero,
                        apply_software_hysteresis=plan.slow_axis in {"x", "y"},
                        on_status=on_status,
                    )
            _emit_status(on_status, STATUS_STOPPED)
        except GeneratorExit:
            raise
        except BaseException as error:
            measurement_failure = error
            raise
        finally:
            _disconnect_owned_session(session, owns_session, measurement_failure)

    return _write_rows(
        [],
        output=out,
        point_iter=points(),
        on_point=on_point,
        config=config,
        measurement_name=measurement_name,
        expected_points=plan.total_points,
        on_unstarted_cleanup=(session.disconnect_all if owns_session else None),
    )


def run_signal_monitor(
    config: dict[str, Any],
    *,
    plan: SignalMonitorPlan | None = None,
    interval_s: float | None = None,
    n_points: int | None = None,
    output: str | Path | None = None,
    on_status: StatusCallback | None = None,
    on_point: PointCallback | None = None,
    should_continue: ContinueCallback | None = None,
    session: DeviceSession | None = None,
) -> list[dict[str, Any]]:
    """Run Signal Monitor.

    A temporary session is created and disconnected when `session` is omitted.
    Supplied sessions are reused and left connected for the caller to own.
    """
    settings = measurement_settings(config, "signal_monitor")
    interval_default = (
        plan.interval_s if plan is not None else settings.get("interval_s", 1.0)
    )
    total_default = plan.n_points if plan is not None else settings.get("n_points", 360)
    interval = _validated_non_negative(
        interval_s if interval_s is not None else interval_default,
        "signal_monitor.interval_s",
    )
    raw_total = n_points if n_points is not None else total_default
    total = int(raw_total)
    if isinstance(raw_total, bool) or float(raw_total) != total:
        raise ValueError("signal_monitor.n_points must be an integer.")
    if total <= 0 or total > MAX_SCAN_POINTS_TOTAL:
        raise ValueError(
            f"signal_monitor.n_points must be between 1 and {MAX_SCAN_POINTS_TOTAL}."
        )
    owns_session = session is None
    session = session or DeviceSession(config)
    out = output or output_path(
        measurement_output_settings(config, "signal_monitor"), "signal_monitor_run.csv"
    )

    def points() -> Iterator[MeasurementPoint]:
        measurement_failure: BaseException | None = None
        try:
            _emit_status(on_status, STATUS_RUNNING)
            start = time.monotonic()
            for index in range(1, total + 1):
                if not _continue(should_continue):
                    break
                target_time = start + (index - 1) * interval
                if not _sleep_interruptible(
                    max(0.0, target_time - time.monotonic()), should_continue
                ):
                    break
                signal = session.read_lockin_signal()
                row = signal_monitor_row(
                    timestamp=utc_now_iso(),
                    target_elapsed_s=target_time - start,
                    elapsed_s=target_time - start,
                    signal=signal,
                )
                yield MeasurementPoint(index=index, total_points=total, row=row)
            _emit_status(on_status, STATUS_STOPPED)
        except GeneratorExit:
            raise
        except BaseException as error:
            measurement_failure = error
            raise
        finally:
            _disconnect_owned_session(session, owns_session, measurement_failure)

    return _write_rows(
        [],
        output=out,
        point_iter=points(),
        on_point=on_point,
        config=config,
        measurement_name="signal_monitor",
        expected_points=total,
        on_unstarted_cleanup=(session.disconnect_all if owns_session else None),
    )


def run_trkr(
    config: dict[str, Any],
    *,
    plan: TrkrPlan | None = None,
    scan_points: list[float | int] | None = None,
    target_points: list[float | int] | None = None,
    coordinate: str | None = None,
    wait_s: float | None = None,
    output: str | Path | None = None,
    return_to_zero: bool | None = None,
    on_status: StatusCallback | None = None,
    on_point: PointCallback | None = None,
    should_continue: ContinueCallback | None = None,
    session: DeviceSession | None = None,
) -> list[dict[str, Any]]:
    """Run TRKR.

    A temporary session is created and disconnected when `session` is omitted.
    Supplied sessions are reused and left connected for the caller to own.
    """
    settings = measurement_settings(config, "trkr")
    if plan is not None:
        points_list = plan.scan_points
        targets_list = plan.target_points
        coord = coordinate or plan.coordinate
    else:
        points_list = scan_points or scan_points_from_config(config, "trkr")
        targets_list = target_points or points_list
        coord = coordinate or settings.get("coordinate", "measurement")
    coord = normalize_coordinate(coord)
    if len(targets_list) != len(points_list):
        raise ValueError("target_points length must match scan_points length.")
    wait = _validated_non_negative(
        wait_s if wait_s is not None else settings.get("wait_s", 1.0), "trkr.wait_s"
    )
    do_return = bool(
        return_to_zero
        if return_to_zero is not None
        else settings.get("return_to_zero", True)
    )
    zero_ps = move_abs_zero(config).get("t_ps")
    preflight_axis_targets(
        config,
        measurement_name="trkr",
        axis="t",
        targets=points_list,
        coordinate=coord,
    )
    if do_return and zero_ps is not None:
        preflight_axis_targets(
            config,
            measurement_name="trkr return",
            axis="t",
            targets=[float(zero_ps)],
            coordinate="measurement",
        )
    owns_session = session is None
    session = session or DeviceSession(config)
    out = output or output_path(
        measurement_output_settings(config, "trkr"), "trkr_run.csv"
    )

    def points() -> Iterator[MeasurementPoint]:
        measurement_failure: BaseException | None = None
        try:
            _emit_status(on_status, STATUS_RUNNING)
            total = len(points_list)
            for index, target in enumerate(points_list, start=1):
                if not _continue(should_continue):
                    break
                row_target = targets_list[index - 1]
                position = session.move_delay_stage(
                    float(target), coordinate=coord, on_status=on_status
                )
                _emit_status(on_status, STATUS_WAITING)
                if not _sleep_interruptible(wait, should_continue):
                    break
                _emit_status(on_status, STATUS_READING_LOCKIN)
                signal = session.read_lockin_signal()
                row = trkr_row(
                    timestamp=utc_now_iso(),
                    target_t_cor_ps=row_target,
                    t_cor_ps=None
                    if zero_ps is None or position.t_ps is None
                    else position.t_ps - float(zero_ps),
                    t_ps=position.t_ps,
                    signal=signal,
                    coordinate=coord,
                    delay_stage_mm=position.delay_stage_mm,
                    delay_stage_pulse=position.delay_stage_pulse,
                )
                yield MeasurementPoint(index=index, total_points=total, row=row)
            if do_return and zero_ps is not None and _continue(should_continue):
                session.move_delay_stage(
                    float(zero_ps), coordinate="measurement", on_status=on_status
                )
            _emit_status(on_status, STATUS_STOPPED)
        except GeneratorExit:
            raise
        except BaseException as error:
            measurement_failure = error
            raise
        finally:
            _disconnect_owned_session(session, owns_session, measurement_failure)

    return _write_rows(
        [],
        output=out,
        point_iter=points(),
        on_point=on_point,
        config=config,
        measurement_name="trkr",
        expected_points=len(points_list),
        on_unstarted_cleanup=(session.disconnect_all if owns_session else None),
    )


def run_srkr(
    config: dict[str, Any],
    *,
    plan: SrkrPlan | None = None,
    axis: str | None = None,
    scan_points: list[float | int] | None = None,
    target_points: list[float | int] | None = None,
    coordinate: str | None = None,
    wait_s: float | None = None,
    output: str | Path | None = None,
    return_to_zero: bool | None = None,
    on_status: StatusCallback | None = None,
    on_point: PointCallback | None = None,
    should_continue: ContinueCallback | None = None,
    session: DeviceSession | None = None,
) -> list[dict[str, Any]]:
    """Run SRKR.

    A temporary session is created and disconnected when `session` is omitted.
    Supplied sessions are reused and left connected for the caller to own.
    """
    settings = measurement_settings(config, "srkr")
    scan = scan_settings(config, "srkr")
    fast_axis = (
        (axis or (plan.axis if plan is not None else None) or scan.get("axis", "x"))
        .strip()
        .lower()
    )
    if fast_axis not in {"x", "y"}:
        raise ValueError("SRKR axis must be 'x' or 'y'.")
    if plan is not None:
        points_list = plan.scan_points
        targets_list = plan.target_points
        coord = coordinate or plan.coordinate
    else:
        points_list = scan_points or scan_points_from_config(config, "srkr")
        targets_list = target_points or points_list
        coord = coordinate or settings.get("coordinate", "measurement")
    coord = normalize_scanner_coordinate(coord)
    if len(targets_list) != len(points_list):
        raise ValueError("target_points length must match scan_points length.")
    wait = _validated_non_negative(
        wait_s if wait_s is not None else settings.get("wait_s", 1.0), "srkr.wait_s"
    )
    do_return = bool(
        return_to_zero
        if return_to_zero is not None
        else settings.get("return_to_zero", True)
    )
    zeros = move_abs_zero(config)
    zero_x = float(zeros.get("x_um", 0.0))
    zero_y = float(zeros.get("y_um", 0.0))
    preflight_axis_targets(
        config,
        measurement_name="srkr",
        axis=fast_axis,
        targets=points_list,
        coordinate=coord,
    )
    if do_return:
        preflight_axis_targets(
            config,
            measurement_name="srkr return",
            axis=fast_axis,
            targets=[zero_x if fast_axis == "x" else zero_y],
            coordinate="measurement",
        )
    owns_session = session is None
    session = session or DeviceSession(config)
    out = output or output_path(
        measurement_output_settings(config, "srkr"), "srkr_run.csv"
    )

    def points() -> Iterator[MeasurementPoint]:
        measurement_failure: BaseException | None = None
        try:
            _emit_status(on_status, STATUS_RUNNING)
            total = len(points_list)
            for index, target in enumerate(points_list, start=1):
                if not _continue(should_continue):
                    break
                row_target = targets_list[index - 1]
                session.move_scanner(
                    fast_axis,
                    float(target),
                    coordinate=coord,
                    apply_software_hysteresis=index == 1,
                    on_status=on_status,
                )
                _emit_status(on_status, STATUS_WAITING)
                if not _sleep_interruptible(wait, should_continue):
                    break
                _emit_status(on_status, STATUS_READING_LOCKIN)
                position = session.read_position()
                signal = session.read_lockin_signal()
                position_um = position.x_um if fast_axis == "x" else position.y_um
                row = srkr_row(
                    timestamp=utc_now_iso(),
                    fast_axis=fast_axis,
                    target_cor_um=row_target,
                    cor_um=None
                    if position_um is None
                    else (
                        position_um - zero_x
                        if fast_axis == "x"
                        else position_um - zero_y
                    ),
                    position_um=position_um,
                    signal=signal,
                    coordinate=coord,
                    scanner_unit=position.scanner_x_unit
                    if fast_axis == "x"
                    else position.scanner_y_unit,
                    scanner_value=position.scanner_x_value
                    if fast_axis == "x"
                    else position.scanner_y_value,
                )
                yield MeasurementPoint(index=index, total_points=total, row=row)
            if do_return and _continue(should_continue):
                zero = zero_x if fast_axis == "x" else zero_y
                session.move_scanner(
                    fast_axis,
                    zero,
                    coordinate="measurement",
                    apply_software_hysteresis=True,
                    on_status=on_status,
                )
            _emit_status(on_status, STATUS_STOPPED)
        except GeneratorExit:
            raise
        except BaseException as error:
            measurement_failure = error
            raise
        finally:
            _disconnect_owned_session(session, owns_session, measurement_failure)

    return _write_rows(
        [],
        output=out,
        point_iter=points(),
        on_point=on_point,
        config=config,
        measurement_name="srkr",
        expected_points=len(points_list),
        on_unstarted_cleanup=(session.disconnect_all if owns_session else None),
    )


def run_strkr(
    config: dict[str, Any],
    *,
    plan: StrkrPlan | None = None,
    wait_s: float | None = None,
    output: str | Path | None = None,
    on_status: StatusCallback | None = None,
    on_point: PointCallback | None = None,
    should_continue: ContinueCallback | None = None,
    session: DeviceSession | None = None,
) -> list[dict[str, Any]]:
    """Run a two-dimensional spatio-temporal scan.

    The plan's fast/slow axes must combine `t` with either `x` or `y`.
    Coordinates are always corrected measurement coordinates.
    """
    plan = plan or strkr_plan_from_config(config)
    return _run_scan2d(
        config,
        measurement_name="strkr",
        plan=plan,
        wait_s=wait_s,
        output=output,
        on_status=on_status,
        on_point=on_point,
        should_continue=should_continue,
        session=session,
    )


def run_srkr_2d(
    config: dict[str, Any],
    *,
    plan: Srkr2DPlan | None = None,
    wait_s: float | None = None,
    output: str | Path | None = None,
    on_status: StatusCallback | None = None,
    on_point: PointCallback | None = None,
    should_continue: ContinueCallback | None = None,
    session: DeviceSession | None = None,
) -> list[dict[str, Any]]:
    """Run a two-dimensional spatial scan over x/y."""
    plan = plan or srkr_2d_plan_from_config(config)
    return _run_scan2d(
        config,
        measurement_name="srkr_2d",
        plan=plan,
        wait_s=wait_s,
        output=output,
        on_status=on_status,
        on_point=on_point,
        should_continue=should_continue,
        session=session,
    )
