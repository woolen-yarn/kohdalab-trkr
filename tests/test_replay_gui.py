from __future__ import annotations

import csv
import os
import time
from pathlib import Path

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

import pytest
from PySide6 import QtWidgets

from kohdalab.api.models import MeasurementPoint
from kohdalab.api.status import (
    STATUS_READING_LOCKIN,
    STATUS_SLOW_AXIS_READY,
    STATUS_WAITING,
    moving_axis_status,
)
from kohdalab.apps import replay_gui
from kohdalab.apps.replay import ReplayDataset
from kohdalab.apps.replay_gui import ReplayGui, ReplayWorker, build_parser
from kohdalab.apps.trkr_gui import TRKRGui


def dataset(measurement: str = "trkr") -> ReplayDataset:
    axes = {
        "trkr": ("t", None),
        "srkr": ("x", None),
        "strkr": ("t", "x"),
        "srkr_2d": ("x", "y"),
    }
    fast_axis, slow_axis = axes[measurement]
    row = {
        "measurement": measurement,
        "fast_axis": fast_axis,
        "slow_axis": slow_axis,
        "X_V": 1.0,
        "Y_V": 2.0,
        "R_V": 3.0,
        "Theta_deg": 4.0,
    }
    if fast_axis == "t":
        row["t_cor_ps"] = 0.0
        row["target_t_cor_ps"] = 0.0
    else:
        row[f"{fast_axis}_cor_um"] = 0.0
        row[f"target_{fast_axis}_cor_um"] = 0.0
    if slow_axis is not None:
        row[f"{slow_axis}_cor_um"] = 10.0
        row[f"target_{slow_axis}_cor_um"] = 10.0
    return ReplayDataset(
        path=Path.cwd() / f"{measurement}.csv",
        measurement=measurement,
        rows=(row,),
        fast_axis=fast_axis,
        slow_axis=slow_axis,
    )


def repeated_dataset(measurement: str, count: int) -> ReplayDataset:
    source = dataset(measurement)
    return ReplayDataset(
        path=source.path,
        measurement=source.measurement,
        rows=tuple(dict(source.rows[0]) for _ in range(count)),
        fast_axis=source.fast_axis,
        slow_axis=source.slow_axis,
    )


def qapp() -> QtWidgets.QApplication:
    return QtWidgets.QApplication.instance() or QtWidgets.QApplication([])


def process_until(predicate, timeout_s: float = 2.0) -> None:
    deadline = time.monotonic() + timeout_s
    while not predicate() and time.monotonic() < deadline:
        QtWidgets.QApplication.processEvents()
        time.sleep(0.005)
    assert predicate()


def write_dataset_csv(path: Path, replay_dataset: ReplayDataset) -> None:
    fields = list(replay_dataset.rows[0])
    with path.open("w", newline="", encoding="utf-8") as stream:
        writer = csv.DictWriter(stream, fieldnames=fields)
        writer.writeheader()
        writer.writerows(replay_dataset.rows)


def test_parser_defaults_to_demo_csv():
    args = build_parser().parse_args([])

    assert args.data_dir == Path("demo_csv")
    assert args.interval is None


def test_worker_repeats_measurement_points_until_stopped():
    replay_dataset = dataset()
    worker = ReplayWorker(replay_dataset, interval_s=0.0)
    cycles: list[int] = []
    points: list[MeasurementPoint] = []
    finished: list[object] = []
    statuses: list[str] = []

    def on_cycle(cycle: int) -> None:
        cycles.append(cycle)
        if cycle == 2:
            worker.stop()

    worker.cycle_started.connect(on_cycle)
    worker.points_ready.connect(points.extend)
    worker.points_ready.connect(lambda _points: worker.acknowledge_point_rendered())
    worker.status_changed.connect(statuses.append)
    worker.finished.connect(finished.append)
    worker.run()

    assert cycles == [1, 2]
    assert len(points) == 1
    assert points[0].row["measurement"] == "trkr"
    assert finished == [[dict(replay_dataset.rows[0])]]
    assert moving_axis_status("t") in statuses
    assert STATUS_WAITING in statuses
    assert STATUS_READING_LOCKIN in statuses


@pytest.mark.parametrize("interval", [-1.0, float("nan")])
def test_worker_rejects_invalid_interval(interval):
    with pytest.raises(ValueError, match="finite and non-negative"):
        ReplayWorker(dataset(), interval)


def test_worker_stops_during_wait_and_reports_errors(monkeypatch):
    replay_dataset = dataset()
    worker = ReplayWorker(replay_dataset, interval_s=0.001)
    points: list[MeasurementPoint] = []

    def stop_on_point(point: MeasurementPoint) -> None:
        points.append(point)
        worker.stop()

    worker.point_ready.connect(stop_on_point)
    worker.run()
    assert len(points) == 1

    failing = ReplayWorker(replay_dataset, interval_s=0.001)
    errors: list[str] = []
    failing.error_occurred.connect(errors.append)

    def fail_wait(_fraction: float) -> bool:
        raise RuntimeError("replay failed")

    monkeypatch.setattr(failing, "_wait", fail_wait)
    failing.run()
    assert errors == ["replay failed"]


@pytest.mark.parametrize(
    "stop_status",
    [
        moving_axis_status("y"),
        moving_axis_status("x"),
        STATUS_WAITING,
        STATUS_READING_LOCKIN,
    ],
)
def test_worker_can_stop_during_each_replay_phase(monkeypatch, stop_status):
    worker = ReplayWorker(dataset("srkr_2d"), interval_s=0.001)

    def stop_at_status(status: str, _fraction: float) -> bool:
        if status == stop_status:
            worker.stop()
            return True
        return False

    monkeypatch.setattr(worker, "_status_phase", stop_at_status)
    worker.run()

    assert worker._stop_event.is_set()


def test_worker_does_not_repeat_slow_motion_for_same_line():
    source = dataset("srkr_2d")
    repeated = ReplayDataset(
        path=source.path,
        measurement=source.measurement,
        rows=(dict(source.rows[0]), dict(source.rows[0])),
        fast_axis=source.fast_axis,
        slow_axis=source.slow_axis,
    )
    worker = ReplayWorker(repeated, interval_s=0.001)
    statuses: list[str] = []
    points: list[MeasurementPoint] = []

    def collect(point: MeasurementPoint) -> None:
        points.append(point)
        if len(points) == 2:
            worker.stop()
        else:
            worker.acknowledge_point_rendered()

    worker.status_changed.connect(statuses.append)
    worker.point_ready.connect(collect)
    worker.run()

    assert statuses.count(moving_axis_status("y")) == 1


def test_fast_worker_batches_points_and_caps_plot_frames():
    worker = ReplayWorker(repeated_dataset("srkr_2d", 33), interval_s=0.0)
    batch_sizes: list[int] = []

    def collect(points: list[MeasurementPoint]) -> None:
        batch_sizes.append(len(points))
        worker.acknowledge_point_rendered()

    worker.points_ready.connect(collect)

    assert not worker._run_fast_cycle()
    assert batch_sizes == [16, 16, 1]


def test_timed_worker_cycle_completion_and_pre_stopped_paths():
    worker = ReplayWorker(dataset(), interval_s=0.001)
    worker.point_ready.connect(lambda _point: worker.acknowledge_point_rendered())

    assert not worker._run_timed_cycle()

    worker.stop()
    assert worker._run_timed_cycle()


def test_replay_gui_uses_original_layout_and_disables_hardware_controls():
    qapp()
    window = ReplayGui({"trkr": dataset()}, interval_s=0.0)

    assert isinstance(window, TRKRGui)
    assert window.windowTitle() == "KohdaLab TRKR — DEMO MODE — NO HARDWARE"
    assert "DEMO MODE" in window.demo_badge.text()
    assert "v0." not in window.windowTitle()
    assert window.left_panel is not None
    assert window.plot_stack is not None
    assert window.measurement_tabs.count() == 5
    assert not window.measurement_tabs.isTabEnabled(0)
    assert window.measurement_tabs.isTabEnabled(1)
    assert window.measurement_tabs.isTabEnabled(2)
    assert window.measurement_tabs.currentIndex() == 1
    assert window.connect_button.isEnabled()
    assert not window.move_t_button.isEnabled()
    assert window.output_browse_button.isEnabled()
    assert window.save_rows_button.isEnabled()
    assert window.save_rows_button.text() == "Load"
    assert not window.start_button.isEnabled()
    assert not window.stop_button.isEnabled()
    assert window.output_name_edit.text() == "trkr.csv"

    fixed_widths = {
        (label.minimumWidth(), label.maximumWidth())
        for labels in (
            window.position_labels,
            window.offset_labels,
            window.corrected_labels,
        )
        for label in labels.values()
    }
    assert len(fixed_widths) == 1
    fixed_width = next(iter(fixed_widths))
    assert fixed_width[0] == fixed_width[1]
    window.position_labels["t"].setText("Moving...")
    assert (
        window.position_labels["t"].minimumWidth(),
        window.position_labels["t"].maximumWidth(),
    ) == fixed_width
    window.show()
    QtWidgets.QApplication.processEvents()
    assert window.left_panel.width() == 384
    assert (
        window.left_panel.viewport().width()
        >= window.left_panel.widget().sizeHint().width()
    )
    assert (
        window.position_labels["t"].geometry().right()
        < window.move_t_spin.geometry().left()
    )
    for button in (
        window.save_button,
        window.move_t_button,
        window.use_current_t_button,
        window.t_cor_button,
    ):
        assert button.geometry().right() < button.parentWidget().width()
    assert window.move_t_spin.width() == replay_gui.MOTION_SPIN_WIDTH
    assert window.move_t_button.width() == replay_gui.MOTION_BUTTON_WIDTH
    assert window.use_current_t_button.width() == replay_gui.USE_CURRENT_BUTTON_WIDTH
    window._apply_signal(
        {
            "X_V": -0.0000036955,
            "Y_V": -0.000002563,
            "R_V": 0.0,
            "Theta_deg": -145.602,
        }
    )
    signal_widths = {
        (label.minimumWidth(), label.maximumWidth())
        for label in window.signal_labels.values()
    }
    assert signal_widths == {
        (
            replay_gui.LOCKIN_SIGNAL_VALUE_WIDTH,
            replay_gui.LOCKIN_SIGNAL_VALUE_WIDTH,
        )
    }
    theta_label = window.signal_labels["Theta"]
    assert (
        theta_label.fontMetrics().horizontalAdvance(theta_label.text())
        < theta_label.width()
    )

    initial_columns = tuple(
        window.snapshot_table.columnWidth(index) for index in range(2)
    )
    snapshot_row = dict(dataset().rows[0])
    snapshot_row["a_very_long_annotation_field_name"] = "a very long value"
    window._update_snapshot(snapshot_row)
    assert (
        tuple(window.snapshot_table.columnWidth(index) for index in range(2))
        == initial_columns
    )

    window.close()


def test_dataset_axes_are_applied_to_original_measurement_controls():
    qapp()
    datasets = {
        measurement: dataset(measurement)
        for measurement in ("trkr", "srkr", "strkr", "srkr_2d")
    }
    window = ReplayGui(datasets, interval_s=0.0)

    window.measurement_tabs.setCurrentIndex(2)
    assert window.srkr_axis_combo.currentText() == "x"
    window.measurement_tabs.setCurrentIndex(3)
    assert window.strkr_fast_axis_combo.currentText() == "t"
    assert window.strkr_slow_axis_combo.currentText() == "x"
    window.measurement_tabs.setCurrentIndex(4)
    assert window.srkr_2d_fast_axis_combo.currentText() == "x"
    assert window.srkr_2d_slow_axis_combo.currentText() == "y"

    window.close()


def test_replay_range_controls_and_defensive_payload_paths(monkeypatch):
    qapp()
    window = ReplayGui({"trkr": dataset()}, interval_s=0.0)

    window._set_range_controls(
        window.trkr_min_spin,
        window.trkr_max_spin,
        window.trkr_step_spin,
        [3.0, 1.0, 2.0],
    )
    assert window.trkr_min_spin.value() == 1.0
    assert window.trkr_max_spin.value() == 3.0
    assert window.trkr_step_spin.value() == 1.0

    window._set_scan2d_range_controls(dataset("trkr"))
    window.datasets["custom"] = dataset()
    window._apply_replay_dataset_to_controls("custom")

    errors: list[str] = []
    monkeypatch.setattr(window, "handle_error", errors.append)
    window.handle_point("invalid")
    assert errors

    row = dict(dataset().rows[0])
    row["t_cor_ps"] = None
    row["target_t_cor_ps"] = 5.0
    window.running_measurement = "trkr"
    window.handle_point(MeasurementPoint(index=1, total_points=1, row=row))
    assert window._current_position_values["t"] == window.t_zero_spin.value() + 5.0
    window.close()


def test_batch_handler_off_tab_and_invalid_payload(monkeypatch):
    qapp()
    window = ReplayGui({"trkr": dataset()}, interval_s=0.0)
    point = MeasurementPoint(index=1, total_points=1, row=dict(dataset().rows[0]))
    window.running_measurement = "trkr"
    window.measurement_tabs.setCurrentIndex(0)

    window.handle_points([point])
    assert len(window.rows_by_mode["trkr"]) == 1

    errors: list[str] = []
    monkeypatch.setattr(window, "handle_error", errors.append)
    worker = ReplayWorker(dataset(), interval_s=0.0)
    window.replay_worker = worker
    window.handle_points([])
    assert worker._stop_event.is_set()
    assert errors
    window.replay_worker = None
    window.handle_points([object()])
    assert len(errors) == 2
    window.close()


def test_start_stop_replays_through_original_point_and_plot_pipeline():
    qapp()
    window = ReplayGui({"trkr": dataset()}, interval_s=1.0)

    window.connect_all()
    window.start_measurement()
    window.start_measurement()
    process_until(lambda: len(window.rows_by_mode["trkr"]) == 1)

    assert window.signal_labels["X"].text() == "1.000 V"
    assert window.point_label.text() == "1/1"
    assert window.measurement_thread is not None
    assert window.replay_worker is not None
    assert not window.start_button.isEnabled()
    assert window.stop_button.isEnabled()

    window.stop_measurement()
    process_until(lambda: window.measurement_thread is None)

    assert window.replay_worker is None
    assert window.start_button.isEnabled()
    assert window.save_rows_button.isEnabled()
    window.close()


def test_wait_field_controls_replay_and_zero_stays_responsive():
    qapp()
    window = ReplayGui({"trkr": dataset()}, interval_s=0.5)
    assert window.trkr_wait_spin.isEnabled()
    assert window.trkr_wait_spin.lineEdit().isEnabled()
    assert window.trkr_wait_spin.value() == 0.5
    window.trkr_wait_spin.setValue(0.0)
    window.connect_all()

    window.start_measurement()
    assert window.replay_worker is not None
    assert window.replay_worker.interval_s == 0.0
    assert not window.trkr_wait_spin.isEnabled()
    assert not window.trkr_wait_spin.lineEdit().isEnabled()
    process_until(lambda: len(window.rows_by_mode["trkr"]) == 1)

    window.stop_measurement()
    process_until(lambda: window.measurement_thread is None)
    assert window.trkr_wait_spin.isEnabled()
    assert window.trkr_wait_spin.lineEdit().isEnabled()
    window.close()


def test_replay_cycle_clears_only_the_running_measurement():
    qapp()
    window = ReplayGui({"trkr": dataset()}, interval_s=0.0)
    window.rows_by_mode["trkr"] = [dict(dataset().rows[0])]

    window._start_replay_cycle(1)
    assert len(window.rows_by_mode["trkr"]) == 1

    window.running_measurement = "trkr"
    window._start_replay_cycle(2)
    assert window.rows_by_mode["trkr"] == []
    assert window.point_label.text() == "Loop 2"

    window.measurement_tabs.setCurrentIndex(0)
    window.rows_by_mode["trkr"] = [dict(dataset().rows[0])]
    window._start_replay_cycle(3)
    assert window.rows_by_mode["trkr"] == []
    window.close()


def test_unavailable_measurement_cannot_start():
    qapp()
    window = ReplayGui({"trkr": dataset()}, interval_s=0.0)
    window.measurement_tabs.setCurrentIndex(0)

    window._refresh_measurement_availability()
    window.start_measurement()
    window.stop_measurement()
    window.refresh_all_ports()
    window.refresh_live_status()

    assert window.measurement_thread is None
    assert not window.start_button.isEnabled()
    assert "Connect All" in window.start_button.toolTip()
    window.close()


def test_close_stops_an_active_replay():
    qapp()
    window = ReplayGui({"trkr": dataset()}, interval_s=1.0)
    window.connect_all()
    window.start_measurement()
    process_until(lambda: window.replay_worker is not None)

    window.close()

    assert window._stdout_original is None


def test_main_reports_bad_interval(capsys):
    assert replay_gui.main(["--interval", "nan"]) == 2
    assert "finite and non-negative" in capsys.readouterr().err


def test_main_builds_and_shows_window(monkeypatch):
    events: list[object] = []

    class FakeApplication:
        @staticmethod
        def instance():
            return None

        def __init__(self, argv):
            events.append(("app", argv))

        def exec(self):
            events.append("exec")
            return 17

    class FakeWindow:
        def __init__(self, *, interval_s, initial_dir):
            events.append(("window", interval_s, initial_dir))

        def show(self):
            events.append("show")

    monkeypatch.setattr(replay_gui.QtWidgets, "QApplication", FakeApplication)
    monkeypatch.setattr(replay_gui, "ReplayGui", FakeWindow)

    result = replay_gui.main(["--data-dir", ".", "--interval", "0.25"])

    assert result == 17
    assert events[-1] == "exec"
    assert "show" in events


def test_config_load_virtual_connect_and_disconnect_flow(tmp_path):
    qapp()
    expected_paths = {}
    for measurement in ("trkr", "srkr", "strkr", "srkr_2d"):
        path = tmp_path / f"{measurement}.csv"
        write_dataset_csv(path, dataset(measurement))
        expected_paths[measurement] = path.resolve()

    window = ReplayGui(initial_dir=tmp_path)
    window.datasets["trkr"] = dataset()
    window.config_path.setText(str(Path("config/default.json").resolve()))

    window.load_config_file()
    assert not window.virtual_connected
    assert window.lockin_model_combo.currentText() == "SR7265"
    assert window.status_label.text() == "config loaded; click Connect All"
    assert window.datasets == {}
    assert window.selected_paths == expected_paths
    assert window.output_dir_edit.text() == str(tmp_path.resolve())
    assert window.output_name_edit.text() == "trkr.csv"
    assert window.save_rows_button.isEnabled()
    assert not window.start_button.isEnabled()

    window.connect_all()
    assert window.virtual_connected
    assert window.status_label.text() == "connected (demo)"
    assert not window.connect_button.isEnabled()
    assert window.disconnect_button.isEnabled()
    assert window.corrected_labels["t"].text() == "0.000"

    window.disconnect_all()
    assert not window.virtual_connected
    assert window.status_label.text() == "disconnected (demo)"
    window.close()


def test_config_and_connection_actions_are_ignored_during_replay():
    qapp()
    window = ReplayGui()
    placeholder = replay_gui.QtCore.QThread(window)
    window.measurement_thread = placeholder

    window.load_config_file()
    window.connect_all()
    assert not window.virtual_connected
    window.virtual_connected = True
    window.disconnect_all()
    assert window.virtual_connected

    window.measurement_thread = None
    placeholder.deleteLater()
    window.close()


def test_browse_then_load_csv_for_current_tab(tmp_path, monkeypatch):
    qapp()
    path = tmp_path / "demo-trkr.csv"
    write_dataset_csv(path, dataset())
    window = ReplayGui(initial_dir=tmp_path)

    monkeypatch.setattr(
        QtWidgets.QFileDialog,
        "getOpenFileName",
        lambda *_args, **_kwargs: (str(path), "CSV Files (*.csv)"),
    )
    window.browse_output_dir()

    assert window.output_dir_edit.text() == str(tmp_path)
    assert window.output_name_edit.text() == path.name
    assert "trkr" not in window.datasets
    assert window.save_rows_button.isEnabled()
    assert not window.start_button.isEnabled()

    window.save_rows()
    assert window.datasets["trkr"].point_count == 1
    assert window.status_label.text() == "CSV loaded"

    window.connect_all()
    assert window.start_button.isEnabled()
    window.close()


def test_browse_cancel_wrong_tab_and_csv_error_paths(tmp_path, monkeypatch):
    qapp()
    window = ReplayGui(initial_dir=tmp_path)
    assert not window._select_default_replay_csvs()

    monkeypatch.setattr(
        QtWidgets.QFileDialog,
        "getOpenFileName",
        lambda *_args, **_kwargs: ("", ""),
    )
    window.browse_output_dir()
    assert window.selected_paths == {}

    window.measurement_tabs.setCurrentIndex(0)
    window.browse_output_dir()
    window.save_rows()
    assert window.selected_paths == {}

    wrong = tmp_path / "wrong.csv"
    write_dataset_csv(wrong, dataset("srkr"))
    window.measurement_tabs.setCurrentIndex(1)
    window.selected_paths["trkr"] = wrong
    warnings: list[str] = []
    monkeypatch.setattr(
        QtWidgets.QMessageBox,
        "warning",
        lambda _parent, _title, message: warnings.append(message),
    )
    window.save_rows()
    assert "trkr" not in window.datasets
    assert window.status_label.text() == "CSV load error"
    assert warnings
    window.close()


def test_2d_worker_reports_slow_axis_motion():
    replay_dataset = dataset("srkr_2d")
    worker = ReplayWorker(replay_dataset, interval_s=0.0)
    statuses: list[str] = []

    worker.status_changed.connect(statuses.append)
    worker.points_ready.connect(lambda _points: worker.stop())
    worker.run()

    assert moving_axis_status("y") in statuses
    assert STATUS_SLOW_AXIS_READY in statuses


def test_2d_start_sets_original_gui_progress_dimensions():
    qapp()
    replay_dataset = dataset("srkr_2d")
    window = ReplayGui({"srkr_2d": replay_dataset}, interval_s=1.0)
    window.connect_all()

    window.start_measurement()

    assert window._scan2d_fast_point_count == 1
    assert window._scan2d_slow_point_count == 1
    window.stop_measurement()
    process_until(lambda: window.measurement_thread is None)
    window.close()
