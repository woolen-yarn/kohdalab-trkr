from __future__ import annotations

import os
import math
from pathlib import Path
from types import SimpleNamespace

import pytest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PySide6 import QtWidgets

import kohdalab.apps.trkr_gui as gui_module
from kohdalab.api.models import MeasurementPoint
from kohdalab.apps.trkr_gui import (
    TRKRGui,
    _format_duration,
    _motion_axis_display_text,
    _valid_scan2d_axes,
    _validated_measurement_point,
)
from kohdalab.apps.trkr_gui_measurement import (
    signal_monitor_plan,
    srkr_2d_plan,
    srkr_plan,
    strkr_plan,
    trkr_plan,
)


def test_signal_monitor_plan_summary_and_values():
    plan = signal_monitor_plan(interval_s=0.5, n_points=12)

    assert plan.interval_s == 0.5
    assert plan.n_points == 12
    assert plan.summary == "12 points, dt=0.5 s"


def test_trkr_plan_offsets_measurement_scan_points_by_zero():
    plan = trkr_plan(
        minimum_ps=-50.0,
        maximum_ps=50.0,
        step_ps=50.0,
        t_zero_ps=-122.0,
        coordinate="measurement",
    )

    assert plan.coordinate == "measurement"
    assert plan.scan_points == [-172.0, -122.0, -72.0]
    assert plan.target_points == [-50.0, 0.0, 50.0]
    assert plan.t_zero_ps == -122.0
    assert plan.summary == "3 points (measurement)"


def test_trkr_plan_keeps_interface_scan_points_unoffset():
    plan = trkr_plan(
        minimum_ps=0.0,
        maximum_ps=1.0,
        step_ps=0.5,
        t_zero_ps=-122.0,
        coordinate="control",
    )

    assert plan.coordinate == "interface"
    assert plan.scan_points == [0.0, 0.5, 1.0]
    assert plan.target_points == [0.0, 0.5, 1.0]
    assert plan.summary == "3 points (interface)"


def test_srkr_plan_offsets_measurement_scan_points_by_active_axis_zero():
    plan = srkr_plan(
        axis="x",
        minimum_um=-10.0,
        maximum_um=10.0,
        step_um=10.0,
        zero_by_axis={"x": 61.5, "y": 477.0},
        coordinate="measurement",
    )

    assert plan.axis == "x"
    assert plan.coordinate == "measurement"
    assert plan.scan_points == [51.5, 61.5, 71.5]
    assert plan.target_points == [-10.0, 0.0, 10.0]
    assert plan.zero == {"x": 61.5, "y": 477.0}
    assert plan.summary == "X measurement, 3 points"


def test_srkr_plan_treats_instrument_alias_as_interface_points():
    plan = srkr_plan(
        axis="Y",
        minimum_um=0.0,
        maximum_um=2.0,
        step_um=1.0,
        zero_by_axis={"x": 61.5, "y": 477.0},
        coordinate="device",
    )

    assert plan.axis == "y"
    assert plan.coordinate == "interface"
    assert plan.scan_points == [0.0, 1.0, 2.0]
    assert plan.target_points == [0.0, 1.0, 2.0]
    assert plan.summary == "Y interface, 3 points"


def test_srkr_plan_rejects_invalid_axis():
    with pytest.raises(ValueError, match="SRKR axis"):
        srkr_plan(
            axis="z",
            minimum_um=0.0,
            maximum_um=1.0,
            step_um=1.0,
            zero_by_axis={"x": 0.0, "y": 0.0},
            coordinate="measurement",
        )


def test_strkr_plan_requires_t_and_one_spatial_axis():
    plan = strkr_plan(
        fast_axis="x",
        slow_axis="t",
        ranges={
            "t": {"min": 0.0, "max": 10.0, "step": 10.0},
            "x": {"min": -1.0, "max": 1.0, "step": 1.0},
            "y": {"min": -2.0, "max": 2.0, "step": 1.0},
        },
        zero_by_axis={"t_ps": 10.0, "x_um": 1.0, "y_um": 2.0},
    )

    assert plan.measurement == "strkr"
    assert plan.fast_axis == "x"
    assert plan.slow_axis == "t"
    assert plan.fast_target_points == [-1.0, 0.0, 1.0]
    assert plan.slow_target_points == [0.0, 10.0]
    assert plan.ranges["y"] == {"min": -2.0, "max": 2.0, "step": 1.0}
    assert plan.total_points == 6


def test_strkr_plan_rejects_spatial_spatial_axes():
    with pytest.raises(ValueError, match="STRKR axes"):
        strkr_plan(
            fast_axis="x",
            slow_axis="y",
            ranges={
                "x": {"min": 0.0, "max": 1.0, "step": 1.0},
                "y": {"min": 0.0, "max": 1.0, "step": 1.0},
            },
        )


def test_srkr_2d_plan_uses_x_y_axes():
    plan = srkr_2d_plan(
        fast_axis="y",
        slow_axis="x",
        ranges={
            "x": {"min": 0.0, "max": 1.0, "step": 1.0},
            "y": {"min": 10.0, "max": 20.0, "step": 10.0},
        },
    )

    assert plan.measurement == "srkr_2d"
    assert plan.fast_axis == "y"
    assert plan.slow_axis == "x"
    assert plan.fast_target_points == [10.0, 20.0]
    assert plan.slow_target_points == [0.0, 1.0]


@pytest.mark.parametrize(
    ("seconds", "expected"),
    [(None, "-"), (-2.0, "0s"), (9.6, "10s"), (65.0, "1:05"), (3661.0, "1:01:01")],
)
def test_gui_eta_duration_formatting(seconds, expected):
    assert _format_duration(seconds) == expected


def test_gui_motion_status_distinguishes_hysteresis_approach():
    assert _motion_axis_display_text("moving scanner x") == "Moving..."
    assert _motion_axis_display_text("moving scanner x software hysteresis") == "BH..."


def test_gui_validates_complete_scan2d_measurement_point():
    point = MeasurementPoint(
        index=2,
        total_points=4,
        row={
            "measurement": "srkr_2d",
            "fast_axis": "x",
            "slow_axis": "y",
            "target_x_cor_um": 1.0,
            "target_y_cor_um": 2.0,
            "X_V": 1e-3,
            "Y_V": 2e-3,
            "R_V": 3e-3,
            "Theta_deg": 45.0,
        },
    )

    assert _validated_measurement_point(point, "srkr_2d") is point


@pytest.mark.parametrize(
    ("mutation", "message"),
    [
        ({"measurement": "strkr"}, "does not match"),
        ({"slow_axis": "x"}, "Invalid srkr_2d point axes"),
        ({"target_x_cor_um": float("nan")}, "must be finite"),
    ],
)
def test_gui_rejects_inconsistent_scan2d_measurement_rows(mutation, message):
    row = {
        "measurement": "srkr_2d",
        "fast_axis": "x",
        "slow_axis": "y",
        "target_x_cor_um": 1.0,
        "target_y_cor_um": 2.0,
        "X_V": 1e-3,
        "Y_V": 2e-3,
        "R_V": 3e-3,
        "Theta_deg": 45.0,
    }
    point = MeasurementPoint(index=1, total_points=1, row=row)
    point.row.update(mutation)

    with pytest.raises(ValueError, match=message):
        _validated_measurement_point(point, "srkr_2d")


@pytest.mark.parametrize(
    ("mode", "fast", "slow", "expected"),
    [
        ("strkr", "t", "bad", ("t", "x")),
        ("strkr", "bad", "y", ("t", "y")),
        ("srkr_2d", "bad", "x", ("y", "x")),
        ("srkr_2d", "bad", "bad", ("x", "y")),
    ],
)
def test_gui_2d_axis_normalization_has_deterministic_fallbacks(
    mode, fast, slow, expected
):
    assert _valid_scan2d_axes(mode, fast, slow) == expected


def _new_measurement_gui(monkeypatch) -> TRKRGui:
    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    return gui


def _close_measurement_gui(gui: TRKRGui) -> None:
    gui._shutdown_complete = True
    gui.close()


def test_gui_measurement_start_rejects_missing_devices_before_worker(
    monkeypatch, tmp_path: Path
):
    gui = _new_measurement_gui(monkeypatch)
    warnings: list[tuple[str, str]] = []
    monkeypatch.setattr(gui, "_output_path", lambda: tmp_path / "signal.csv")
    monkeypatch.setattr(gui_module, "validate_new_output_path", lambda path: Path(path))
    monkeypatch.setattr(
        gui, "_missing_required_devices", lambda *_args: ["lockin.main"]
    )
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )

    gui.start_measurement()

    assert warnings == [
        ("Run Error", "Connect required devices before starting: lockin.main")
    ]
    assert gui.measurement_thread is None
    assert gui.worker is None
    _close_measurement_gui(gui)


def test_gui_stop_measurement_routes_to_worker_and_logs(monkeypatch):
    gui = _new_measurement_gui(monkeypatch)

    class Worker:
        stop_calls = 0

        def stop(self):
            self.stop_calls += 1

    worker = Worker()
    gui.worker = worker  # type: ignore[assignment]

    gui.stop_measurement()

    assert worker.stop_calls == 1
    assert "Stop requested." in gui.log.toPlainText()
    gui.worker = None
    _close_measurement_gui(gui)


def test_gui_invalid_point_stops_worker_and_routes_measurement_error(monkeypatch):
    gui = _new_measurement_gui(monkeypatch)
    errors: list[str] = []

    class Worker:
        stop_calls = 0

        def stop(self):
            self.stop_calls += 1

    worker = Worker()
    gui.worker = worker  # type: ignore[assignment]
    gui.running_measurement = "trkr"
    monkeypatch.setattr(gui, "handle_error", errors.append)

    gui.handle_point({"measurement": "trkr"})

    assert worker.stop_calls == 1
    assert errors == ["Invalid measurement point: payload must be a MeasurementPoint."]
    gui.worker = None
    _close_measurement_gui(gui)


def test_gui_valid_scan2d_point_updates_rows_eta_and_ui_routes(monkeypatch):
    gui = _new_measurement_gui(monkeypatch)
    gui.running_measurement = "srkr_2d"
    gui._scan2d_fast_point_count = 2
    gui._scan2d_slow_point_count = 2
    gui._scan2d_eta_line_cycle_s = 4.0
    calls: list[tuple[str, object]] = []
    monkeypatch.setattr(gui, "_measurement_name", lambda: "srkr_2d")
    monkeypatch.setattr(gui, "_apply_signal", lambda row: calls.append(("signal", row)))
    monkeypatch.setattr(
        gui, "_update_position_from_row", lambda row: calls.append(("position", row))
    )
    monkeypatch.setattr(
        gui, "_update_snapshot", lambda row: calls.append(("snapshot", row))
    )
    monkeypatch.setattr(gui, "_update_curves", lambda: calls.append(("curves", None)))
    point = MeasurementPoint(
        index=2,
        total_points=4,
        row={
            "measurement": "srkr_2d",
            "fast_axis": "x",
            "slow_axis": "y",
            "target_x_cor_um": 1.0,
            "target_y_cor_um": 2.0,
            "X_V": 1e-3,
            "Y_V": 2e-3,
            "R_V": 3e-3,
            "Theta_deg": 45.0,
        },
    )

    gui.handle_point(point)

    assert gui.rows_by_mode["srkr_2d"] == [point.row]
    assert gui.point_text_by_mode["srkr_2d"] == "2/4"
    assert gui.eta_text_by_mode["srkr_2d"] == "4s"
    assert gui.point_label.text() == "2/4"
    assert gui.eta_label.text() == "4s"
    assert [name for name, _value in calls] == [
        "signal",
        "position",
        "snapshot",
        "curves",
    ]
    gui.running_measurement = None
    _close_measurement_gui(gui)


def test_gui_scan2d_plot_routes_current_slow_line_and_both_heatmaps(monkeypatch):
    gui = _new_measurement_gui(monkeypatch)
    monkeypatch.setattr(gui, "_measurement_name", lambda: "srkr_2d")

    class Curve:
        def __init__(self):
            self.calls = []

        def setData(self, xs, ys):
            self.calls.append((list(xs), list(ys)))

    curves = {1: Curve(), 2: Curve()}
    gui.scan2d_line_curves = curves
    heatmaps: list[tuple] = []
    monkeypatch.setattr(
        gui,
        "_set_scan2d_heatmap",
        lambda *args: heatmaps.append(args),
    )
    rows = [
        {
            "fast_axis": "x",
            "slow_axis": "y",
            "target_x_cor_um": 0.0,
            "target_y_cor_um": 0.0,
            "x_cor_um": 0.1,
            "X_V": 1.0,
            "Theta_deg": 10.0,
        },
        {
            "fast_axis": "x",
            "slow_axis": "y",
            "target_x_cor_um": 1.0,
            "target_y_cor_um": 1.0,
            "x_cor_um": 1.1,
            "X_V": 2.0,
            "Theta_deg": 20.0,
        },
    ]
    gui._voltage_scale = 1000.0
    view = SimpleNamespace(signal1_key="X_V", signal2_key="Theta_deg")

    gui._update_scan2d_plots(rows, view)

    assert curves[1].calls == [([1.1], [2000.0])]
    assert curves[2].calls == [([1.1], [20.0])]
    assert [(call[0], call[1], call[2]) for call in heatmaps] == [
        (1, "X_V", 1000.0),
        (2, "Theta_deg", 1.0),
    ]
    _close_measurement_gui(gui)


@pytest.mark.parametrize(
    ("measurement", "row"),
    [
        (
            "trkr",
            {
                "measurement": "trkr",
                "target_t_cor_ps": 1.0,
                "X_V": 1.0,
                "Y_V": 2.0,
                "R_V": 3.0,
                "Theta_deg": 4.0,
            },
        ),
        (
            "srkr",
            {
                "measurement": "srkr",
                "fast_axis": "x",
                "target_x_cor_um": 1.0,
                "X_V": 1.0,
                "Y_V": 2.0,
                "R_V": 3.0,
                "Theta_deg": 4.0,
            },
        ),
        (
            "strkr",
            {
                "measurement": "strkr",
                "fast_axis": "t",
                "slow_axis": "x",
                "target_t_cor_ps": 1.0,
                "target_x_cor_um": 2.0,
                "X_V": 1.0,
                "Y_V": 2.0,
                "R_V": 3.0,
                "Theta_deg": 4.0,
            },
        ),
    ],
)
def test_gui_valid_measurement_points_route_rows_and_curve_updates(
    monkeypatch, measurement, row
):
    gui = _new_measurement_gui(monkeypatch)
    gui.running_measurement = measurement
    gui.rows_by_mode[measurement].clear()
    updates: list[str] = []
    monkeypatch.setattr(gui, "_measurement_name", lambda: measurement)
    monkeypatch.setattr(gui, "_apply_signal", lambda _row: updates.append("signal"))
    monkeypatch.setattr(
        gui, "_update_position_from_row", lambda _row: updates.append("position")
    )
    monkeypatch.setattr(
        gui, "_update_snapshot", lambda _row: updates.append("snapshot")
    )
    monkeypatch.setattr(gui, "_update_curves", lambda: updates.append("curves"))
    point = MeasurementPoint(index=1, total_points=2, row=row)

    gui.handle_point(point)

    assert gui.rows_by_mode[measurement] == [point.row]
    assert gui.point_text_by_mode[measurement] == "1/2"
    assert updates == ["signal", "position", "snapshot", "curves"]
    gui.running_measurement = None
    _close_measurement_gui(gui)


@pytest.mark.parametrize(
    ("measurement", "row", "message"),
    [
        (
            "srkr",
            {
                "fast_axis": "z",
                "X_V": 1.0,
                "Y_V": 2.0,
                "R_V": 3.0,
                "Theta_deg": 4.0,
            },
            "SRKR point fast_axis",
        ),
        (
            "strkr",
            {
                "fast_axis": "x",
                "slow_axis": "y",
                "target_x_cor_um": 1.0,
                "target_y_cor_um": 2.0,
                "X_V": 1.0,
                "Y_V": 2.0,
                "R_V": 3.0,
                "Theta_deg": 4.0,
            },
            "Invalid strkr point axes",
        ),
        (
            "unknown",
            {"X_V": 1.0, "Y_V": 2.0, "R_V": 3.0, "Theta_deg": 4.0},
            "Unsupported measurement point type",
        ),
    ],
)
def test_gui_measurement_point_validation_rejects_invalid_axes_and_mode(
    measurement, row, message
):
    point = MeasurementPoint(index=1, total_points=1, row=row)

    with pytest.raises(ValueError, match=message):
        _validated_measurement_point(point, measurement)


def test_gui_measurement_point_rejects_nonmapping_row_after_payload_mutation():
    point = MeasurementPoint(
        index=1,
        total_points=1,
        row={"X_V": 1.0, "Y_V": 2.0, "R_V": 3.0, "Theta_deg": 4.0},
    )
    point.row = []  # type: ignore[assignment]

    with pytest.raises(TypeError, match="point row must be a mapping"):
        _validated_measurement_point(point, "trkr")


def test_gui_measurement_finished_handles_valid_and_invalid_row_payloads(monkeypatch):
    gui = _new_measurement_gui(monkeypatch)
    redraws: list[bool] = []
    monkeypatch.setattr(gui, "_update_curves", lambda: redraws.append(True))

    gui.handle_finished([{"measurement": "trkr"}])
    gui.handle_finished({"measurement": "trkr"})
    gui.handle_finished([{"measurement": "trkr"}, "bad"])

    log = gui.log.toPlainText()
    assert "Finished. 1 points collected." in log
    assert log.count("Finished with an invalid row payload") == 2
    assert log.count("Finished. 0 points collected.") == 2
    assert redraws == [True, True, True]
    _close_measurement_gui(gui)


def test_gui_cleanup_thread_resets_measurement_and_schedules_live_refresh(monkeypatch):
    gui = _new_measurement_gui(monkeypatch)
    gui.running_measurement = "strkr"
    gui.measurement_thread = object()  # type: ignore[assignment]
    gui.worker = object()  # type: ignore[assignment]
    gui.running_srkr_axis = "x"
    gui.running_motion_axes = {"t", "x"}
    gui._scan2d_fast_point_count = 2
    gui._scan2d_slow_point_count = 3
    gui._scan2d_eta_anchor_at = 1.0
    gui._scan2d_eta_line_cycle_s = 2.0
    gui.experiment = object()  # type: ignore[assignment]
    gui._shutdown_requested = False
    redraws: list[bool] = []
    running: list[bool] = []
    timers: list[tuple[int, object]] = []
    monkeypatch.setattr(gui, "_measurement_name", lambda: "strkr")
    monkeypatch.setattr(gui, "_update_curves", lambda: redraws.append(True))
    monkeypatch.setattr(gui, "_set_running", running.append)
    monkeypatch.setattr(
        gui_module.QtCore.QTimer,
        "singleShot",
        lambda delay, callback: timers.append((delay, callback)),
    )

    gui.cleanup_thread()

    assert gui.measurement_thread is None
    assert gui.worker is None
    assert gui.running_measurement is None
    assert gui.running_srkr_axis is None
    assert gui.running_motion_axes == set()
    assert gui._scan2d_fast_point_count == gui._scan2d_slow_point_count == 0
    assert gui._scan2d_eta_anchor_at is None
    assert gui._scan2d_eta_line_cycle_s is None
    assert running == [False]
    assert redraws == [True]
    assert timers == [(0, gui._request_full_live_status)]
    gui.experiment = None
    _close_measurement_gui(gui)


def test_gui_scan2d_eta_requires_complete_timing_state(monkeypatch):
    gui = _new_measurement_gui(monkeypatch)

    assert gui._scan2d_eta_text(1) == "-"
    gui._scan2d_eta_line_cycle_s = 4.0
    gui._scan2d_fast_point_count = 2
    gui._scan2d_slow_point_count = 3

    assert gui._scan2d_eta_text(2) == "8s"
    assert gui._scan2d_eta_text(8) == "0s"
    assert gui._eta_text("trkr", 1, 2) == "-"
    _close_measurement_gui(gui)


def test_gui_empty_scan2d_heatmap_clears_existing_image(monkeypatch):
    gui = _new_measurement_gui(monkeypatch)

    class Heatmap:
        clear_calls = 0

        def clear(self):
            self.clear_calls += 1

    heatmap = Heatmap()
    gui.scan2d_heatmaps = {1: heatmap}

    gui._set_scan2d_heatmap(
        1,
        "X_V",
        1.0,
        [{"fast_axis": "x", "slow_axis": "y", "X_V": 1.0}],
        "x",
        "y",
    )

    assert heatmap.clear_calls == 1
    _close_measurement_gui(gui)


def test_gui_measurement_status_routes_motion_wait_slow_ready_and_read(monkeypatch):
    gui = _new_measurement_gui(monkeypatch)
    gui.running_motion_axes = {"t", "x"}
    axis_statuses: list[tuple[str, str]] = []
    restores: list[bool] = []
    eta_updates: list[bool] = []
    monkeypatch.setattr(
        gui,
        "_set_measurement_axis_status",
        lambda axis, text: axis_statuses.append((axis, text)),
    )
    monkeypatch.setattr(
        gui, "_restore_running_motion_axis_values", lambda: restores.append(True)
    )
    monkeypatch.setattr(
        gui, "_update_scan2d_eta_from_slow_ready", lambda: eta_updates.append(True)
    )

    gui.handle_measurement_status("moving scanner x software hysteresis")
    gui.handle_measurement_status(gui_module.STATUS_WAITING)
    gui.handle_measurement_status(gui_module.STATUS_SLOW_AXIS_READY)
    gui.handle_measurement_status(gui_module.STATUS_READING_LOCKIN)
    gui.handle_measurement_status(gui_module.STATUS_STOPPED)

    assert axis_statuses == [("x", "BH...")]
    assert restores == [True, True, True, True]
    assert eta_updates == [True]
    assert gui.status_label.text() == gui_module.STATUS_STOPPED
    _close_measurement_gui(gui)


def test_gui_scan2d_slow_ready_establishes_anchor_then_line_cycle(monkeypatch):
    gui = _new_measurement_gui(monkeypatch)
    gui.running_measurement = "srkr_2d"
    gui._scan2d_fast_point_count = 2
    gui._scan2d_slow_point_count = 3
    gui.rows_by_mode["srkr_2d"] = []
    times = iter([10.0, 18.0])
    monkeypatch.setattr(gui_module.time, "perf_counter", lambda: next(times))
    monkeypatch.setattr(gui, "_measurement_name", lambda: "srkr_2d")

    gui._update_scan2d_eta_from_slow_ready()
    assert gui._scan2d_eta_anchor_at == 10.0

    gui.rows_by_mode["srkr_2d"] = [{}, {}, {}, {}]
    gui._update_scan2d_eta_from_slow_ready()

    assert gui._scan2d_eta_line_cycle_s == 4.0
    assert gui.eta_text_by_mode["srkr_2d"] == "4s"
    assert gui.eta_label.text() == "4s"
    gui.running_measurement = None
    _close_measurement_gui(gui)


def test_gui_move_position_accepts_row_and_position_payloads(monkeypatch):
    gui = _new_measurement_gui(monkeypatch)
    rows: list[dict] = []
    positions: list[tuple[object, bool]] = []
    monkeypatch.setattr(gui, "_update_position_from_row", rows.append)
    monkeypatch.setattr(
        gui,
        "_update_position_from_position",
        lambda position, *, preserve_missing=False: positions.append(
            (position, preserve_missing)
        ),
    )
    position = gui_module.Position(x_um=1.0)

    gui.handle_move_position({"x_um": 1.0})
    gui.handle_move_position(position)
    gui.handle_move_position("invalid")

    assert rows == [{"x_um": 1.0}]
    assert positions == [(position, True)]
    _close_measurement_gui(gui)


def test_gui_move_error_restores_axis_and_warns(monkeypatch):
    gui = _new_measurement_gui(monkeypatch)
    gui.running_move_axis = "y"
    gui._current_position_values["y"] = 12.5
    restored: list[tuple[str, object]] = []
    warnings: list[tuple[str, str]] = []
    monkeypatch.setattr(
        gui, "_set_position_value", lambda axis, value: restored.append((axis, value))
    )
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )

    gui.handle_move_error("controller stopped")

    assert restored == [("y", 12.5)]
    assert gui.status_label.text() == "move error"
    assert warnings == [("Move Error", "controller stopped")]
    gui.running_move_axis = None
    _close_measurement_gui(gui)


def test_gui_signal_monitor_start_builds_worker_args_and_starts_thread(
    monkeypatch, tmp_path: Path
):
    gui = _new_measurement_gui(monkeypatch)

    class Signal:
        def __init__(self):
            self.callbacks = []

        def connect(self, callback):
            self.callbacks.append(callback)

    class Thread:
        latest = None

        def __init__(self, _parent=None):
            self.started = Signal()
            self.finished = Signal()
            self.started_called = False
            type(self).latest = self

        def start(self):
            self.started_called = True

        def quit(self):
            return None

        def deleteLater(self):
            return None

    class Worker:
        latest = None

        def __init__(self, **kwargs):
            self.kwargs = kwargs
            self.point_ready = Signal()
            self.status_changed = Signal()
            self.error_occurred = Signal()
            self.finished = Signal()
            self.thread = None
            type(self).latest = self

        def moveToThread(self, thread):
            self.thread = thread

        def run(self):
            return None

        def deleteLater(self):
            return None

    experiment = object()
    output = tmp_path / "signal.csv"
    monkeypatch.setattr(gui_module.QtCore, "QThread", Thread)
    monkeypatch.setattr(gui_module, "MeasurementWorker", Worker)
    monkeypatch.setattr(gui, "_output_path", lambda: output)
    monkeypatch.setattr(gui_module, "validate_new_output_path", lambda path: Path(path))
    monkeypatch.setattr(gui, "_missing_required_devices", lambda *_args: [])
    monkeypatch.setattr(gui, "_ensure_experiment", lambda: experiment)
    monkeypatch.setattr(gui, "_update_curves", lambda: None)
    gui.signal_interval_spin.setValue(0.25)
    gui.signal_points_spin.setValue(3)

    gui.start_measurement()

    worker = Worker.latest
    thread = Thread.latest
    assert worker is not None
    assert thread is not None
    assert worker.kwargs == {
        "experiment": experiment,
        "measurement": "signal_monitor",
        "output_path": str(output),
        "scan_plan": None,
        "axis": None,
        "interval_s": 0.25,
        "n_points": 3,
        "wait_s": None,
        "return_to_zero": None,
    }
    assert worker.thread is thread
    assert thread.started.callbacks == [worker.run]
    assert thread.started_called is True
    assert gui.running_measurement == "signal_monitor"
    assert gui.start_button.isEnabled() is False
    gui.measurement_thread = None
    gui.worker = None
    gui.running_measurement = None
    _close_measurement_gui(gui)


def test_gui_device_command_setup_failure_cleans_state_and_shows_critical(monkeypatch):
    gui = _new_measurement_gui(monkeypatch)
    criticals: list[tuple[str, str]] = []
    cleanups: list[bool] = []
    monkeypatch.setattr(
        gui,
        "_ensure_experiment",
        lambda: (_ for _ in ()).throw(RuntimeError("invalid runtime config")),
    )
    monkeypatch.setattr(gui, "cleanup_device_command", lambda: cleanups.append(True))
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "critical",
        lambda _parent, title, message: criticals.append((title, message)),
    )

    gui.connect_all()

    assert cleanups == [True]
    assert gui.status_label.text() == "error"
    assert criticals == [("Device Error", "invalid runtime config")]
    _close_measurement_gui(gui)


def test_gui_refresh_live_status_routes_idle_move_throttle_and_errors(monkeypatch):
    gui = _new_measurement_gui(monkeypatch)
    gui.experiment = object()  # type: ignore[assignment]
    gui._last_live_refresh = 0.0
    full: list[bool] = []
    lockin: list[bool] = []
    monkeypatch.setattr(gui_module.time, "perf_counter", lambda: 10.0)
    monkeypatch.setattr(gui, "_request_full_live_status", lambda: full.append(True))
    monkeypatch.setattr(gui, "_request_lockin_live_status", lambda: lockin.append(True))

    gui.refresh_live_status()
    gui.refresh_live_status()
    gui._last_live_refresh = 0.0
    gui.move_thread = object()  # type: ignore[assignment]
    gui.refresh_live_status()

    assert full == [True]
    assert lockin == [True]

    gui._last_live_refresh = 0.0
    monkeypatch.setattr(
        gui,
        "_request_lockin_live_status",
        lambda: (_ for _ in ()).throw(RuntimeError("worker unavailable")),
    )
    gui.refresh_live_status()
    assert gui._last_live_refresh == 10.0
    gui.move_thread = None
    gui.experiment = None
    _close_measurement_gui(gui)


def test_gui_lockin_status_payload_routes_only_valid_mappings(monkeypatch):
    gui = _new_measurement_gui(monkeypatch)
    settings_calls: list[dict] = []
    signal_calls: list[dict] = []
    overload_calls: list[object] = []
    monkeypatch.setattr(gui, "_apply_lockin_settings", settings_calls.append)
    monkeypatch.setattr(gui, "_apply_signal", signal_calls.append)
    monkeypatch.setattr(gui, "_apply_overload_status", overload_calls.append)

    gui.handle_lockin_status_ready([], "bad", None)
    gui.handle_lockin_status_ready(
        {"Time Constant": 1.0}, {"X": 1.0}, {"overload": False}
    )

    assert settings_calls == [{"Time Constant": 1.0}]
    assert signal_calls == [{"X": 1.0}]
    assert overload_calls == [None, {"overload": False}]
    _close_measurement_gui(gui)


def test_gui_signal_aliases_and_overload_error_display(monkeypatch):
    gui = _new_measurement_gui(monkeypatch)
    gui._voltage_scale = 1000.0
    gui._voltage_unit = "mV"

    gui._apply_signal({"X_V": 1e-3, "Y_V": 2e-3, "R_V": 3e-3, "Theta_deg": 45.0})
    assert gui.signal_labels["X"].text() == "1.000 mV"
    assert gui.signal_labels["Y"].text() == "2.000 mV"
    assert gui.signal_labels["R"].text() == "3.000 mV"
    assert gui.signal_labels["Theta"].text() == "45.000 deg"

    gui._apply_overload_status(None)
    gui._apply_overload_status("invalid")
    assert gui.overload_label.text() == "?"
    gui._apply_overload_status({"_error": "timeout"})
    assert gui.overload_label.text() == "?"
    _close_measurement_gui(gui)


def test_gui_set_origin_rejects_axis_and_clears_pending_on_request_failure(monkeypatch):
    gui = _new_measurement_gui(monkeypatch)
    warnings: list[tuple[str, str]] = []
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )

    gui.set_origin_from_current("z")
    monkeypatch.setattr(
        gui,
        "_request_full_live_status",
        lambda: (_ for _ in ()).throw(RuntimeError("live worker failed")),
    )
    gui.set_origin_from_current("x")

    assert gui.pending_origin_axis is None
    assert warnings == [
        ("Origin Error", "Unsupported axis: z"),
        ("Origin Error", "live worker failed"),
    ]
    _close_measurement_gui(gui)


def test_gui_snapshot_table_formats_measurement_row(monkeypatch):
    gui = _new_measurement_gui(monkeypatch)
    gui._voltage_scale = 1000.0

    gui._update_snapshot({"measurement": "trkr", "target_t_cor_ps": 1.25, "X_V": 2e-3})

    snapshot = {
        gui.snapshot_table.item(index, 0).text(): gui.snapshot_table.item(
            index, 1
        ).text()
        for index in range(gui.snapshot_table.rowCount())
    }
    assert snapshot["measurement"] == "trkr"
    assert snapshot["target_t_cor_ps"] == "1.250000"
    assert snapshot["X_V"] == "2.000000e-03"
    _close_measurement_gui(gui)


def test_gui_complete_scan2d_heatmap_sets_image_rect_and_aspect(monkeypatch):
    gui = _new_measurement_gui(monkeypatch)

    class Heatmap:
        lookup = None
        image = None
        image_kwargs = None
        rect = None

        def setLookupTable(self, lookup):
            self.lookup = lookup

        def setImage(self, image, **kwargs):
            self.image = image
            self.image_kwargs = kwargs

        def setRect(self, rect):
            self.rect = rect

    class ViewBox:
        calls = []

        def setAspectLocked(self, locked, *, ratio):
            self.calls.append((locked, ratio))

    heatmap = Heatmap()
    view_box = ViewBox()
    gui.scan2d_heatmaps = {1: heatmap}
    gui.scan2d_heatmap_plots = {1: SimpleNamespace(getViewBox=lambda: view_box)}
    rows = [
        {"target_x_cor_um": 0.0, "target_y_cor_um": 10.0, "X_V": -2.0},
        {"target_x_cor_um": 1.0, "target_y_cor_um": 10.0, "X_V": 1.0},
        {"target_x_cor_um": 0.0, "target_y_cor_um": 20.0, "X_V": 2.0},
    ]

    gui._set_scan2d_heatmap(1, "X_V", 1.0, rows, "x", "y")

    assert heatmap.lookup is gui_module.RDBU_R_LUT
    assert heatmap.image_kwargs == {"autoLevels": False, "levels": (-1.0, 1.0)}
    image = heatmap.image.tolist()
    assert image[0] == [-1.0, 1.0]
    assert image[1][0] == 0.5
    assert math.isnan(image[1][1])
    assert heatmap.rect.x() == 0.0
    assert heatmap.rect.y() == 10.0
    assert heatmap.rect.width() == 1.0
    assert heatmap.rect.height() == 10.0
    assert view_box.calls == [(True, 1.0)]
    _close_measurement_gui(gui)


def test_gui_save_rows_handles_empty_success_and_writer_failure(monkeypatch, tmp_path):
    gui = _new_measurement_gui(monkeypatch)
    infos: list[tuple[str, str]] = []
    warnings: list[tuple[str, str]] = []
    writes: list[dict] = []
    output = tmp_path / "rows.csv"
    monkeypatch.setattr(gui, "_measurement_name", lambda: "trkr")
    monkeypatch.setattr(gui, "_output_path", lambda: output)
    monkeypatch.setattr(gui, "_runtime_config", lambda: {"profile": "test"})
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "information",
        lambda _parent, title, message: infos.append((title, message)),
    )
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )
    monkeypatch.setattr(
        gui_module,
        "write_measurement_rows",
        lambda rows, **kwargs: writes.append({"rows": rows, **kwargs}),
    )

    gui.rows_by_mode["trkr"] = []
    gui.save_rows()
    assert infos == [("No Data", "No rows to save.")]

    gui.rows_by_mode["trkr"] = [{"measurement": "trkr"}]
    gui.save_rows()
    assert writes[0]["output"] == output
    assert writes[0]["overwrite"] is True
    assert "Saved 1 rows" in gui.log.toPlainText()

    monkeypatch.setattr(
        gui_module,
        "write_measurement_rows",
        lambda *_args, **_kwargs: (_ for _ in ()).throw(OSError("disk full")),
    )
    gui.save_rows()
    assert warnings == [("Save Error", "disk full")]
    _close_measurement_gui(gui)


def test_gui_invalid_point_without_worker_routes_error_without_stop(monkeypatch):
    gui = _new_measurement_gui(monkeypatch)
    gui.worker = None
    gui.running_measurement = "trkr"
    errors: list[str] = []
    monkeypatch.setattr(gui, "handle_error", errors.append)

    gui.handle_point(object())

    assert errors == ["Invalid measurement point: payload must be a MeasurementPoint."]
    gui.running_measurement = None
    _close_measurement_gui(gui)


def test_gui_background_measurement_point_does_not_replace_visible_labels_or_curves(
    monkeypatch,
):
    gui = _new_measurement_gui(monkeypatch)
    gui.running_measurement = "trkr"
    gui.rows_by_mode["trkr"].clear()
    gui.point_label.setText("visible point")
    gui.eta_label.setText("visible eta")
    curve_updates: list[bool] = []
    monkeypatch.setattr(gui, "_measurement_name", lambda: "signal_monitor")
    monkeypatch.setattr(gui, "_apply_signal", lambda _row: None)
    monkeypatch.setattr(gui, "_update_position_from_row", lambda _row: None)
    monkeypatch.setattr(gui, "_update_snapshot", lambda _row: None)
    monkeypatch.setattr(gui, "_update_curves", lambda: curve_updates.append(True))
    point = MeasurementPoint(
        index=1,
        total_points=2,
        row={
            "measurement": "trkr",
            "target_t_cor_ps": 1.0,
            "X_V": 1.0,
            "Y_V": 2.0,
            "R_V": 3.0,
            "Theta_deg": 4.0,
        },
    )

    gui.handle_point(point)

    assert gui.rows_by_mode["trkr"] == [point.row]
    assert gui.point_text_by_mode["trkr"] == "1/2"
    assert gui.point_label.text() == "visible point"
    assert gui.eta_label.text() == "visible eta"
    assert curve_updates == []
    gui.running_measurement = None
    _close_measurement_gui(gui)


def test_gui_position_update_preserves_missing_axes_only_when_requested(monkeypatch):
    gui = _new_measurement_gui(monkeypatch)
    updates: list[tuple[str, object]] = []
    monkeypatch.setattr(
        gui, "_set_position_value", lambda axis, value: updates.append((axis, value))
    )

    gui._update_position_from_position(
        gui_module.Position(t_ps=1.0), preserve_missing=True
    )
    assert updates == [("t", 1.0)]

    updates.clear()
    gui._update_position_from_position(
        gui_module.Position(x_um=2.0), preserve_missing=True
    )
    assert updates == [("x", 2.0)]

    updates.clear()
    gui._update_position_from_position(gui_module.Position(), preserve_missing=False)
    assert updates == [("t", None), ("x", None), ("y", None)]
    _close_measurement_gui(gui)


def test_gui_set_running_during_move_keeps_only_stop_and_read_status_available(
    monkeypatch,
):
    gui = _new_measurement_gui(monkeypatch)
    gui.move_thread = object()  # type: ignore[assignment]
    motion_states: list[tuple[str, bool]] = []
    monkeypatch.setattr(
        gui,
        "_set_motion_axis_enabled",
        lambda axis, enabled: motion_states.append((axis, enabled)),
    )

    gui._set_running(True)

    assert gui.save_rows_button.isEnabled() is False
    assert gui.start_button.isEnabled() is False
    assert gui.stop_button.isEnabled() is True
    assert gui.load_button.isEnabled() is False
    assert gui.save_button.isEnabled() is False
    assert gui.connect_button.isEnabled() is False
    assert gui.disconnect_button.isEnabled() is False
    assert gui.read_status_button.isEnabled() is True
    assert motion_states == [("t", False), ("x", False), ("y", False)]
    gui.move_thread = None
    _close_measurement_gui(gui)
