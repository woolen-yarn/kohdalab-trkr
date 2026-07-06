from __future__ import annotations

import csv
import time
from datetime import datetime
from pathlib import Path
from typing import Any, Callable

from kohdalab.api.config import (
    build_range_points,
    measurement_output_settings,
    measurement_settings,
    move_abs_zero,
    output_path,
    scan_settings,
)
from kohdalab.api.measurement_rows import fields_for_row, output_row, signal_monitor_row, srkr_row, trkr_row
from kohdalab.api.measurement_rows import scan2d_row
from kohdalab.api.models import MeasurementPoint
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


def scan_points_from_config(config: dict[str, Any], measurement_name: str) -> list[float]:
    scan = scan_settings(config, measurement_name)
    return build_range_points(float(scan["min"]), float(scan["max"]), float(scan["step"]))


def _continue(should_continue: ContinueCallback | None) -> bool:
    return True if should_continue is None else bool(should_continue())


def _sleep_interruptible(duration_s: float, should_continue: ContinueCallback | None) -> bool:
    deadline = time.monotonic() + max(0.0, float(duration_s))
    while time.monotonic() < deadline:
        if not _continue(should_continue):
            return False
        time.sleep(min(0.05, deadline - time.monotonic()))
    return _continue(should_continue)


def _emit_status(on_status: StatusCallback | None, status: str) -> None:
    if on_status is not None:
        on_status(status)


def _write_rows(
    rows: list[dict[str, Any]],
    *,
    output: str | Path | None,
    point_iter,
    on_point: PointCallback | None,
) -> list[dict[str, Any]]:
    csv_file = None
    writer = None
    try:
        if output is not None:
            path = Path(output)
            path.parent.mkdir(parents=True, exist_ok=True)
            csv_file = path.open("w", newline="", encoding="utf-8")
        for point in point_iter:
            rows.append(point.row)
            if csv_file is not None:
                if writer is None:
                    writer = csv.DictWriter(csv_file, fieldnames=fields_for_row(point.row))
                    writer.writeheader()
                writer.writerow(output_row(point.row))
                csv_file.flush()
            if on_point is not None:
                on_point(point)
    finally:
        if csv_file is not None:
            csv_file.close()
    return rows


def _zero_for_axis(zero: dict[str, float], axis: str) -> float:
    if axis == "t":
        return float(zero.get("t_ps", 0.0))
    return float(zero.get(f"{axis}_um", 0.0))


def _absolute_measurement_target(zero: dict[str, float], axis: str, corrected_target: float) -> float:
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
    wait = float(wait_s if wait_s is not None else settings.get("wait_s", 1.0))
    owns_session = session is None
    session = session or DeviceSession(config)
    out = output or output_path(measurement_output_settings(config, measurement_name), f"{measurement_name}_run.csv")
    zero = plan.zero

    def points():
        try:
            _emit_status(on_status, STATUS_RUNNING)
            total = plan.total_points
            index = 0
            fast_first = plan.fast_target_points[0]
            scanner_axes_approached: set[str] = set()
            for slow_index, slow_target in enumerate(plan.slow_target_points):
                if not _continue(should_continue):
                    break
                slow_apply_hysteresis = plan.slow_axis in {"x", "y"} and plan.slow_axis not in scanner_axes_approached
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
                    fast_apply_hysteresis = plan.fast_axis in {"x", "y"} and plan.fast_axis not in scanner_axes_approached
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
                        timestamp=datetime.now().isoformat(timespec="milliseconds"),
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
                if slow_index < len(plan.slow_target_points) - 1 and _continue(should_continue):
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
        finally:
            if owns_session:
                session.disconnect_all()

    return _write_rows([], output=out, point_iter=points(), on_point=on_point)


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
    interval_default = plan.interval_s if plan is not None else settings.get("interval_s", 1.0)
    total_default = plan.n_points if plan is not None else settings.get("n_points", 360)
    interval = float(interval_s if interval_s is not None else interval_default)
    total = int(n_points if n_points is not None else total_default)
    owns_session = session is None
    session = session or DeviceSession(config)
    out = output or output_path(measurement_output_settings(config, "signal_monitor"), "signal_monitor_run.csv")

    def points():
        try:
            _emit_status(on_status, STATUS_RUNNING)
            start = time.monotonic()
            for index in range(1, total + 1):
                if not _continue(should_continue):
                    break
                target_time = start + (index - 1) * interval
                if not _sleep_interruptible(max(0.0, target_time - time.monotonic()), should_continue):
                    break
                signal = session.read_lockin_signal()
                row = signal_monitor_row(
                    timestamp=datetime.now().isoformat(timespec="milliseconds"),
                    target_elapsed_s=target_time - start,
                    elapsed_s=target_time - start,
                    signal=signal,
                )
                yield MeasurementPoint(index=index, total_points=total, row=row)
            _emit_status(on_status, STATUS_STOPPED)
        finally:
            if owns_session:
                session.disconnect_all()

    return _write_rows([], output=out, point_iter=points(), on_point=on_point)


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
    wait = float(wait_s if wait_s is not None else settings.get("wait_s", 1.0))
    do_return = bool(return_to_zero if return_to_zero is not None else settings.get("return_to_zero", True))
    zero_ps = move_abs_zero(config).get("t_ps")
    owns_session = session is None
    session = session or DeviceSession(config)
    out = output or output_path(measurement_output_settings(config, "trkr"), "trkr_run.csv")

    def points():
        try:
            _emit_status(on_status, STATUS_RUNNING)
            total = len(points_list)
            for index, target in enumerate(points_list, start=1):
                if not _continue(should_continue):
                    break
                row_target = targets_list[index - 1]
                position = session.move_delay_stage(float(target), coordinate=coord, on_status=on_status)
                _emit_status(on_status, STATUS_WAITING)
                if not _sleep_interruptible(wait, should_continue):
                    break
                _emit_status(on_status, STATUS_READING_LOCKIN)
                signal = session.read_lockin_signal()
                row = trkr_row(
                    timestamp=datetime.now().isoformat(timespec="milliseconds"),
                    target_t_cor_ps=row_target,
                    t_cor_ps=None if zero_ps is None or position.t_ps is None else position.t_ps - float(zero_ps),
                    t_ps=position.t_ps,
                    signal=signal,
                    coordinate=coord,
                    delay_stage_mm=position.delay_stage_mm,
                    delay_stage_pulse=position.delay_stage_pulse,
                )
                yield MeasurementPoint(index=index, total_points=total, row=row)
            if do_return and zero_ps is not None and _continue(should_continue):
                session.move_delay_stage(float(zero_ps), coordinate="measurement", on_status=on_status)
            _emit_status(on_status, STATUS_STOPPED)
        finally:
            if owns_session:
                session.disconnect_all()

    return _write_rows([], output=out, point_iter=points(), on_point=on_point)


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
    fast_axis = (axis or (plan.axis if plan is not None else None) or scan.get("axis", "x")).strip().lower()
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
    wait = float(wait_s if wait_s is not None else settings.get("wait_s", 1.0))
    do_return = bool(return_to_zero if return_to_zero is not None else settings.get("return_to_zero", True))
    zeros = move_abs_zero(config)
    zero_x = float(zeros.get("x_um", 0.0))
    zero_y = float(zeros.get("y_um", 0.0))
    owns_session = session is None
    session = session or DeviceSession(config)
    out = output or output_path(measurement_output_settings(config, "srkr"), "srkr_run.csv")

    def points():
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
                    timestamp=datetime.now().isoformat(timespec="milliseconds"),
                    fast_axis=fast_axis,
                    target_cor_um=row_target,
                    cor_um=None if position_um is None else (position_um - zero_x if fast_axis == "x" else position_um - zero_y),
                    position_um=position_um,
                    signal=signal,
                    coordinate=coord,
                    scanner_unit=position.scanner_x_unit if fast_axis == "x" else position.scanner_y_unit,
                    scanner_value=position.scanner_x_value if fast_axis == "x" else position.scanner_y_value,
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
        finally:
            if owns_session:
                session.disconnect_all()

    return _write_rows([], output=out, point_iter=points(), on_point=on_point)


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
