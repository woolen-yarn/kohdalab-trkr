from __future__ import annotations

import csv
import json
import os
from collections.abc import Callable
from copy import deepcopy
from pathlib import Path
from typing import Any

import pytest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PySide6 import QtWidgets

import kohdalab.apps.trkr_gui as gui_module
from kohdalab.api import (
    MeasurementPoint,
    Position,
    Srkr2DPlan,
    SrkrPlan,
    StrkrPlan,
    TrkrPlan,
)
from kohdalab.api.config import ConfigPathResolution, normalize_config
from kohdalab.apps.trkr_gui import TRKRGui


class FakeSignal:
    def __init__(self):
        self._callbacks: list[Callable[..., Any]] = []

    def connect(self, callback: Callable[..., Any]):
        self._callbacks.append(callback)

    def emit(self, *args: object):
        for callback in list(self._callbacks):
            callback(*args)


class FakeThread:
    def __init__(self, _parent: object = None):
        self.started = FakeSignal()
        self.finished = FakeSignal()
        self.start_called = False
        self.deleted = False

    def start(self):
        self.start_called = True

    def quit(self, *_args: object):
        self.finished.emit()

    def deleteLater(self, *_args: object):
        self.deleted = True


class FakeMeasurementWorker:
    latest: FakeMeasurementWorker | None = None

    def __init__(self, **kwargs: object):
        self.kwargs = kwargs
        self.point_ready = FakeSignal()
        self.status_changed = FakeSignal()
        self.error_occurred = FakeSignal()
        self.finished = FakeSignal()
        self.stop_called = False
        self.deleted = False
        self.thread: FakeThread | None = None
        type(self).latest = self

    def moveToThread(self, thread: FakeThread):
        self.thread = thread

    def run(self):
        return None

    def stop(self):
        self.stop_called = True

    def deleteLater(self, *_args: object):
        self.deleted = True


class FakeDeviceWorker:
    latest: FakeDeviceWorker | None = None

    def __init__(self, *, experiment: object):
        self.experiment = experiment
        self.status_changed = FakeSignal()
        self.finished = FakeSignal()
        self.error_occurred = FakeSignal()
        self.requests: list[object] = []
        self.thread: FakeThread | None = None
        self.deleted = False
        type(self).latest = self

    def moveToThread(self, thread: FakeThread):
        self.thread = thread

    def run_command(self, request: object):
        self.requests.append(request)

    def deleteLater(self, *_args: object):
        self.deleted = True


class FakeMoveWorker:
    latest: FakeMoveWorker | None = None

    def __init__(self, **kwargs: object):
        self.kwargs = kwargs
        self.status_changed = FakeSignal()
        self.position_changed = FakeSignal()
        self.finished = FakeSignal()
        self.error_occurred = FakeSignal()
        self.thread: FakeThread | None = None
        self.deleted = False
        type(self).latest = self

    def moveToThread(self, thread: FakeThread):
        self.thread = thread

    def run(self):
        return None

    def deleteLater(self, *_args: object):
        self.deleted = True


class FakeLiveWorker:
    latest: FakeLiveWorker | None = None

    def __init__(self, *, experiment: object):
        self.experiment = experiment
        self.live_status_ready = FakeSignal()
        self.lockin_status_ready = FakeSignal()
        self.error_occurred = FakeSignal()
        self.thread: FakeThread | None = None
        self.deleted = False
        type(self).latest = self

    def moveToThread(self, thread: FakeThread):
        self.thread = thread

    def deleteLater(self, *_args: object):
        self.deleted = True


class FakeResourceWorker:
    latest: FakeResourceWorker | None = None

    def __init__(self):
        self.resources_ready = FakeSignal()
        self.error_occurred = FakeSignal()
        self.finished = FakeSignal()
        self.thread: FakeThread | None = None
        self.deleted = False
        type(self).latest = self

    def moveToThread(self, thread: FakeThread):
        self.thread = thread

    def run(self):
        return None

    def deleteLater(self, *_args: object):
        self.deleted = True


def _new_gui(monkeypatch) -> TRKRGui:
    refresh_all_ports = TRKRGui.refresh_all_ports
    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    monkeypatch.setattr(TRKRGui, "refresh_all_ports", refresh_all_ports)
    gui.live_timer.stop()
    return gui


def _close_gui(gui: TRKRGui):
    gui._shutdown_complete = True
    gui.close()


def test_gui_measurement_start_stop_error_and_cleanup(monkeypatch, tmp_path: Path):
    gui = _new_gui(monkeypatch)
    experiment = object()
    warnings: list[tuple[str, str]] = []
    errors: list[tuple[str, str]] = []
    monkeypatch.setattr(gui_module.QtCore, "QThread", FakeThread)
    monkeypatch.setattr(gui_module, "MeasurementWorker", FakeMeasurementWorker)
    monkeypatch.setattr(
        gui, "_missing_required_devices", lambda _measurement, _axis: []
    )
    monkeypatch.setattr(gui, "_ensure_experiment", lambda: experiment)
    monkeypatch.setattr(gui, "_output_path", lambda: tmp_path / "signal.csv")
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "critical",
        lambda _parent, title, message: errors.append((title, message)),
    )

    gui.start_measurement()

    worker = FakeMeasurementWorker.latest
    thread = gui.measurement_thread
    assert worker is not None
    assert isinstance(thread, FakeThread)
    assert thread.start_called is True
    assert worker.thread is thread
    assert worker.kwargs == {
        "experiment": experiment,
        "measurement": "signal_monitor",
        "output_path": str(tmp_path / "signal.csv"),
        "scan_plan": None,
        "axis": None,
        "interval_s": gui.signal_interval_spin.value(),
        "n_points": gui.signal_points_spin.value(),
        "wait_s": None,
        "return_to_zero": None,
    }
    assert gui.running_measurement == "signal_monitor"
    assert gui.start_button.isEnabled() is False
    assert gui.stop_button.isEnabled() is True
    assert warnings == []

    gui.stop_measurement()
    assert worker.stop_called is True
    assert "Stop requested." in gui.log.toPlainText()

    worker.error_occurred.emit("simulated acquisition failure")
    assert errors == [("Measurement Error", "simulated acquisition failure")]
    assert "Error: simulated acquisition failure" in gui.log.toPlainText()

    worker.finished.emit([])
    assert gui.measurement_thread is None
    assert gui.worker is None
    assert gui.running_measurement is None
    assert gui.start_button.isEnabled() is True
    assert gui.stop_button.isEnabled() is False
    assert worker.deleted is True
    assert thread.deleted is True
    assert "Finished. 0 points collected." in gui.log.toPlainText()

    _close_gui(gui)


def test_gui_rejects_start_while_device_command_is_active(monkeypatch):
    gui = _new_gui(monkeypatch)
    warnings: list[tuple[str, str]] = []
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )
    gui.device_command_active = True

    gui.start_measurement()

    assert gui.measurement_thread is None
    assert gui.worker is None
    assert warnings == [
        (
            "Run Error",
            "Wait for the active device operation to finish first.",
        )
    ]

    _close_gui(gui)


def test_gui_rejects_start_when_required_devices_are_missing(
    monkeypatch, tmp_path: Path
):
    gui = _new_gui(monkeypatch)
    warnings: list[tuple[str, str]] = []
    FakeMeasurementWorker.latest = None
    monkeypatch.setattr(gui_module.QtCore, "QThread", FakeThread)
    monkeypatch.setattr(gui_module, "MeasurementWorker", FakeMeasurementWorker)
    monkeypatch.setattr(gui, "_output_path", lambda: tmp_path / "signal.csv")
    monkeypatch.setattr(
        gui,
        "_missing_required_devices",
        lambda _measurement, _axis: ["lockin.main", "delay_stage.t"],
    )
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )

    gui.start_measurement()

    assert gui.measurement_thread is None
    assert gui.worker is None
    assert FakeMeasurementWorker.latest is None
    assert warnings == [
        (
            "Run Error",
            "Connect required devices before starting: lockin.main, delay_stage.t",
        )
    ]
    _close_gui(gui)


def test_gui_rejects_invalid_scan_before_device_check(monkeypatch, tmp_path: Path):
    gui = _new_gui(monkeypatch)
    warnings: list[tuple[str, str]] = []
    device_checks: list[bool] = []
    FakeMeasurementWorker.latest = None
    monkeypatch.setattr(gui_module.QtCore, "QThread", FakeThread)
    monkeypatch.setattr(gui_module, "MeasurementWorker", FakeMeasurementWorker)
    monkeypatch.setattr(gui, "_output_path", lambda: tmp_path / "trkr.csv")
    monkeypatch.setattr(
        gui,
        "_missing_required_devices",
        lambda _measurement, _axis: device_checks.append(True) or [],
    )
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )
    gui.measurement_tabs.setCurrentIndex(1)
    gui.trkr_min_spin.setValue(10.0)
    gui.trkr_max_spin.setValue(-10.0)
    gui.trkr_step_spin.setValue(1.0)

    gui.start_measurement()

    assert gui.measurement_thread is None
    assert gui.worker is None
    assert FakeMeasurementWorker.latest is None
    assert device_checks == []
    assert warnings == [("Run Error", "No scan points generated. Check min/max/step.")]
    _close_gui(gui)


@pytest.mark.parametrize("collision", ["csv", "metadata"])
def test_gui_rejects_output_collision_before_worker_start(
    monkeypatch,
    tmp_path: Path,
    collision: str,
):
    gui = _new_gui(monkeypatch)
    output = tmp_path / "signal.csv"
    collision_path = output if collision == "csv" else tmp_path / "signal.csv.meta.json"
    collision_path.write_text("existing\n", encoding="utf-8")
    warnings: list[tuple[str, str]] = []
    device_checks: list[bool] = []
    FakeMeasurementWorker.latest = None
    monkeypatch.setattr(gui_module.QtCore, "QThread", FakeThread)
    monkeypatch.setattr(gui_module, "MeasurementWorker", FakeMeasurementWorker)
    monkeypatch.setattr(gui, "_output_path", lambda: output)
    monkeypatch.setattr(
        gui,
        "_missing_required_devices",
        lambda _measurement, _axis: device_checks.append(True) or [],
    )
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )

    gui.start_measurement()

    assert gui.measurement_thread is None
    assert gui.worker is None
    assert FakeMeasurementWorker.latest is None
    assert device_checks == []
    assert warnings == [
        (
            "Run Error",
            f"Measurement {'output' if collision == 'csv' else 'metadata'} already exists: {collision_path}",
        )
    ]
    assert collision_path.read_text(encoding="utf-8") == "existing\n"
    _close_gui(gui)


@pytest.mark.parametrize(
    ("tab_index", "measurement", "plan_type"),
    [
        (1, "trkr", TrkrPlan),
        (2, "srkr", SrkrPlan),
        (3, "strkr", StrkrPlan),
        (4, "srkr_2d", Srkr2DPlan),
    ],
)
def test_gui_scan_modes_pass_validated_plans_to_worker(
    monkeypatch,
    tmp_path: Path,
    tab_index: int,
    measurement: str,
    plan_type: type[TrkrPlan | SrkrPlan | StrkrPlan | Srkr2DPlan],
):
    gui = _new_gui(monkeypatch)
    experiment = object()
    monkeypatch.setattr(gui_module.QtCore, "QThread", FakeThread)
    monkeypatch.setattr(gui_module, "MeasurementWorker", FakeMeasurementWorker)
    monkeypatch.setattr(
        gui, "_missing_required_devices", lambda _measurement, _axis: []
    )
    monkeypatch.setattr(gui, "_ensure_experiment", lambda: experiment)
    monkeypatch.setattr(
        gui,
        "_output_path",
        lambda: tmp_path / f"{measurement}.csv",
    )
    gui.measurement_tabs.setCurrentIndex(tab_index)
    QtWidgets.QApplication.processEvents()

    if measurement == "trkr":
        gui.trkr_min_spin.setValue(-10.0)
        gui.trkr_max_spin.setValue(10.0)
        gui.trkr_step_spin.setValue(10.0)
        gui.t_zero_spin.setValue(5.0)
        gui.trkr_wait_spin.setValue(0.25)
        gui.trkr_return_check.setChecked(True)
        expected_axis = None
        expected_wait = 0.25
        expected_return = True
        expected_motion_axes = {"t"}
    elif measurement == "srkr":
        gui.srkr_axis_combo.setCurrentText("y")
        gui.srkr_min_spin.setValue(-2.0)
        gui.srkr_max_spin.setValue(2.0)
        gui.srkr_step_spin.setValue(2.0)
        gui.y_zero_spin.setValue(7.0)
        gui.srkr_wait_spin.setValue(0.5)
        gui.srkr_return_check.setChecked(True)
        gui.rows_by_mode["srkr"] = [
            {"fast_axis": "x", "X_V": 1.0},
            {"fast_axis": "y", "X_V": 2.0},
        ]
        expected_axis = "y"
        expected_wait = 0.5
        expected_return = True
        expected_motion_axes = {"y"}
    elif measurement == "strkr":
        gui.strkr_wait_spin.setValue(0.75)
        expected_axis = None
        expected_wait = 0.75
        expected_return = None
        expected_motion_axes = {
            gui.strkr_fast_axis_combo.currentText().lower(),
            gui.strkr_slow_axis_combo.currentText().lower(),
        }
    else:
        gui.srkr_2d_wait_spin.setValue(1.25)
        expected_axis = None
        expected_wait = 1.25
        expected_return = None
        expected_motion_axes = {
            gui.srkr_2d_fast_axis_combo.currentText().lower(),
            gui.srkr_2d_slow_axis_combo.currentText().lower(),
        }

    gui.start_measurement()

    worker = FakeMeasurementWorker.latest
    assert worker is not None
    plan = worker.kwargs["scan_plan"]
    assert isinstance(plan, plan_type)
    assert worker.kwargs["experiment"] is experiment
    assert worker.kwargs["measurement"] == measurement
    assert worker.kwargs["output_path"] == str(tmp_path / f"{measurement}.csv")
    assert worker.kwargs["axis"] == expected_axis
    assert worker.kwargs["wait_s"] == expected_wait
    assert worker.kwargs["return_to_zero"] is expected_return
    assert worker.kwargs["interval_s"] is None
    assert worker.kwargs["n_points"] is None
    assert gui.running_motion_axes == expected_motion_axes

    if isinstance(plan, TrkrPlan):
        assert plan.scan_points == [-5.0, 5.0, 15.0]
        assert plan.target_points == [-10.0, 0.0, 10.0]
    elif isinstance(plan, SrkrPlan):
        assert plan.axis == "y"
        assert plan.scan_points == [5.0, 7.0, 9.0]
        assert gui.rows_by_mode["srkr"] == [{"fast_axis": "x", "X_V": 1.0}]
    else:
        assert {plan.fast_axis, plan.slow_axis} == expected_motion_axes
        assert plan.fast_point_count > 0
        assert plan.slow_point_count > 0
        assert gui._scan2d_fast_point_count == plan.fast_point_count
        assert gui._scan2d_slow_point_count == plan.slow_point_count

    worker.finished.emit([])
    _close_gui(gui)


def test_gui_shutdown_waits_for_measurement_then_requests_disconnect(
    monkeypatch,
    tmp_path: Path,
):
    gui = _new_gui(monkeypatch)
    experiment = object()
    FakeMeasurementWorker.latest = None
    monkeypatch.setattr(gui_module.QtCore, "QThread", FakeThread)
    monkeypatch.setattr(gui_module, "MeasurementWorker", FakeMeasurementWorker)
    monkeypatch.setattr(
        gui, "_missing_required_devices", lambda _measurement, _axis: []
    )
    monkeypatch.setattr(gui, "_ensure_experiment", lambda: experiment)
    monkeypatch.setattr(gui, "_output_path", lambda: tmp_path / "signal.csv")
    gui.start_measurement()
    gui.experiment = experiment  # type: ignore[assignment]
    worker = FakeMeasurementWorker.latest
    assert worker is not None

    scheduled: list[tuple[int, Callable[[], Any]]] = []
    disconnect_requests: list[tuple[str, dict[str, object]]] = []
    monkeypatch.setattr(
        gui_module.QtCore.QTimer,
        "singleShot",
        lambda delay, callback: scheduled.append((delay, callback)),
    )
    monkeypatch.setattr(
        gui,
        "_start_device_command",
        lambda command, **kwargs: disconnect_requests.append((command, kwargs)),
    )

    gui._request_async_shutdown()
    gui._request_async_shutdown()

    assert gui._shutdown_requested is True
    assert gui.status_label.text() == "closing"
    assert gui.centralWidget().isEnabled() is False
    assert worker.stop_called is True
    assert len(scheduled) == 1
    assert disconnect_requests == []

    delay, drain = scheduled.pop(0)
    assert delay == 0
    drain()
    assert disconnect_requests == []
    assert len(scheduled) == 1
    assert scheduled[0][0] == 100

    worker.finished.emit([])
    assert gui.measurement_thread is None
    _, drain = scheduled.pop(0)
    drain()

    assert disconnect_requests == [
        (
            "shutdown_disconnect_all",
            {
                "label": "Shutdown disconnect",
                "allow_during_shutdown": True,
            },
        )
    ]
    assert scheduled == []
    _close_gui(gui)


def test_gui_shutdown_without_experiment_schedules_final_close(monkeypatch):
    gui = _new_gui(monkeypatch)
    scheduled: list[tuple[int, Callable[[], Any]]] = []
    monkeypatch.setattr(
        gui_module.QtCore.QTimer,
        "singleShot",
        lambda delay, callback: scheduled.append((delay, callback)),
    )

    gui._request_async_shutdown()
    _, drain = scheduled.pop(0)
    drain()

    assert gui._shutdown_complete is True
    assert len(scheduled) == 1
    assert scheduled[0][0] == 0
    assert scheduled[0][1] == gui.close
    scheduled.clear()
    _close_gui(gui)


@pytest.mark.parametrize("failed", [False, True])
def test_gui_shutdown_disconnect_result_allows_close(monkeypatch, failed: bool):
    gui = _new_gui(monkeypatch)
    scheduled: list[tuple[int, Callable[[], Any]]] = []
    warnings: list[tuple[str, str]] = []
    gui._shutdown_requested = True
    gui.device_command_active = True
    monkeypatch.setattr(
        gui_module.QtCore.QTimer,
        "singleShot",
        lambda delay, callback: scheduled.append((delay, callback)),
    )
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )

    if failed:
        gui.handle_device_error("disconnect failed")
    else:
        gui.handle_device_command_finished({"command": "shutdown_disconnect_all"})

    assert gui._shutdown_complete is True
    assert gui.device_command_active is False
    assert len(scheduled) == 1
    assert scheduled[0][0] == 0
    assert scheduled[0][1] == gui.close
    assert warnings == []
    expected_log = (
        "Device command error: disconnect failed"
        if failed
        else "Disconnected for shutdown."
    )
    assert expected_log in gui.log.toPlainText()
    scheduled.clear()
    _close_gui(gui)


def test_gui_final_close_quits_and_waits_for_all_worker_threads(monkeypatch):
    class WaitableThread:
        def __init__(self):
            self.calls: list[object] = []

        def quit(self):
            self.calls.append("quit")

        def wait(self, timeout_ms: int):
            self.calls.append(("wait", timeout_ms))
            return True

    gui = _new_gui(monkeypatch)
    threads = [WaitableThread() for _ in range(5)]
    gui.measurement_thread = threads[0]  # type: ignore[assignment]
    gui.move_thread = threads[1]  # type: ignore[assignment]
    gui.live_thread = threads[2]  # type: ignore[assignment]
    gui.resource_thread = threads[3]  # type: ignore[assignment]
    gui.device_thread = threads[4]  # type: ignore[assignment]

    gui._close_worker_threads()

    assert [thread.calls for thread in threads] == [
        ["quit", ("wait", 2000)],
        ["quit", ("wait", 2000)],
        ["quit", ("wait", 2000)],
        ["quit", ("wait", 2000)],
        ["quit", ("wait", 2000)],
    ]
    gui.measurement_thread = None
    gui.move_thread = None
    gui.live_thread = None
    gui.resource_thread = None
    gui.device_thread = None
    _close_gui(gui)


def test_gui_device_command_success_reuses_worker_and_restores_controls(monkeypatch):
    gui = _new_gui(monkeypatch)
    experiment = object()
    refreshes: list[bool] = []
    FakeDeviceWorker.latest = None
    monkeypatch.setattr(gui_module.QtCore, "QThread", FakeThread)
    monkeypatch.setattr(gui_module, "DeviceCommandWorker", FakeDeviceWorker)
    monkeypatch.setattr(gui, "_ensure_experiment", lambda: experiment)
    monkeypatch.setattr(
        gui, "_request_full_live_status", lambda: refreshes.append(True)
    )

    gui.connect_device("lockin", "main")

    worker = FakeDeviceWorker.latest
    thread = gui.device_thread
    assert worker is not None
    assert isinstance(thread, FakeThread)
    assert thread.start_called is True
    assert worker.thread is thread
    assert worker.requests == [
        {
            "command": "connect_device",
            "kind": "lockin",
            "key": "main",
            "axis": None,
            "ref": None,
            "multiplier": 4.0,
            "settings": {},
        }
    ]
    assert gui.device_command_active is True
    assert gui.connect_button.isEnabled() is False

    worker.status_changed.emit("connecting lockin.main")
    worker.finished.emit({"command": "connect_device", "ref": "lockin.main"})

    assert gui.device_command_active is False
    assert gui.connect_button.isEnabled() is True
    assert gui.status_label.text() == "lockin.main connected"
    assert refreshes == [True]

    gui.disconnect_device("lockin", "main")
    assert gui.device_worker is worker
    assert gui.device_thread is thread
    assert len(worker.requests) == 2
    worker.finished.emit({"command": "disconnect_device", "ref": "lockin.main"})
    assert gui.device_command_active is False
    assert gui.status_label.text() == "lockin.main disconnected"
    assert refreshes == [True, True]

    thread.finished.emit()
    assert worker.deleted is True
    assert thread.deleted is True
    assert gui.device_worker is None
    assert gui.device_thread is None
    _close_gui(gui)


def test_gui_device_command_failure_recovers_for_retry(monkeypatch):
    gui = _new_gui(monkeypatch)
    experiment = object()
    warnings: list[tuple[str, str]] = []
    FakeDeviceWorker.latest = None
    monkeypatch.setattr(gui_module.QtCore, "QThread", FakeThread)
    monkeypatch.setattr(gui_module, "DeviceCommandWorker", FakeDeviceWorker)
    monkeypatch.setattr(gui, "_ensure_experiment", lambda: experiment)
    monkeypatch.setattr(gui, "_request_full_live_status", lambda: None)
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )

    gui.connect_all()
    worker = FakeDeviceWorker.latest
    assert worker is not None
    worker.error_occurred.emit("simulated connection failure")

    assert gui.device_command_active is False
    assert gui.connect_button.isEnabled() is True
    assert gui.status_label.text() == "device error"
    assert warnings == [("Device Error", "simulated connection failure")]

    gui.connect_all()
    assert gui.device_command_active is True
    assert len(worker.requests) == 2
    worker.finished.emit({"command": "connect_all"})
    assert gui.device_command_active is False

    assert isinstance(gui.device_thread, FakeThread)
    gui.device_thread.finished.emit()
    _close_gui(gui)


def test_gui_unexpected_device_thread_exit_clears_active_command(monkeypatch):
    gui = _new_gui(monkeypatch)
    experiment = object()
    FakeDeviceWorker.latest = None
    monkeypatch.setattr(gui_module.QtCore, "QThread", FakeThread)
    monkeypatch.setattr(gui_module, "DeviceCommandWorker", FakeDeviceWorker)
    monkeypatch.setattr(gui, "_ensure_experiment", lambda: experiment)

    gui.connect_all()
    worker = FakeDeviceWorker.latest
    thread = gui.device_thread
    assert worker is not None
    assert isinstance(thread, FakeThread)
    assert gui.device_command_active is True

    thread.finished.emit()

    assert gui.device_thread is None
    assert gui.device_worker is None
    assert gui.device_command_active is False
    assert gui.connect_button.isEnabled() is True
    assert worker.deleted is True
    _close_gui(gui)


def test_gui_scanner_initialization_updates_status_and_refreshes(monkeypatch):
    gui = _new_gui(monkeypatch)
    experiment = object()
    refreshes: list[bool] = []
    FakeDeviceWorker.latest = None
    gui.experiment = experiment  # type: ignore[assignment]
    monkeypatch.setattr(gui_module.QtCore, "QThread", FakeThread)
    monkeypatch.setattr(gui_module, "DeviceCommandWorker", FakeDeviceWorker)
    monkeypatch.setattr(gui, "_ensure_experiment", lambda: experiment)
    monkeypatch.setattr(
        gui, "_request_full_live_status", lambda: refreshes.append(True)
    )

    gui.initialize_device("scanner", "x")
    worker = FakeDeviceWorker.latest
    assert worker is not None
    assert worker.requests[0]["command"] == "initialize_scanner"
    assert worker.requests[0]["axis"] == "x"
    worker.finished.emit(
        {
            "command": "initialize_scanner",
            "kind": "scanner",
            "axis": "x",
            "info": {"axis": "x"},
        }
    )

    assert gui.device_command_active is False
    assert gui.status_label.text() == "scanner x initialized"
    assert "Initialized scanner x." in gui.log.toPlainText()
    assert refreshes == [True]

    assert isinstance(gui.device_thread, FakeThread)
    gui.device_thread.finished.emit()
    _close_gui(gui)


def test_gui_move_success_updates_position_and_cleans_thread(monkeypatch):
    gui = _new_gui(monkeypatch)
    experiment = object()
    hint_refreshes: list[bool] = []
    FakeMoveWorker.latest = None
    gui.move_x_spin.setValue(12.5)
    monkeypatch.setattr(gui_module.QtCore, "QThread", FakeThread)
    monkeypatch.setattr(gui_module, "MoveWorker", FakeMoveWorker)
    monkeypatch.setattr(gui, "_ensure_experiment", lambda: experiment)
    monkeypatch.setattr(
        gui, "_refresh_scan_limit_hints", lambda: hint_refreshes.append(True)
    )

    gui.move_absolute("x")

    worker = FakeMoveWorker.latest
    thread = gui.move_thread
    assert worker is not None
    assert isinstance(thread, FakeThread)
    assert worker.kwargs == {
        "experiment": experiment,
        "axis": "x",
        "value": 12.5,
        "coordinate": "measurement",
    }
    assert worker.thread is thread
    assert thread.start_called is True
    assert gui.running_move_axis == "x"
    assert gui.connect_button.isEnabled() is False

    worker.status_changed.emit("moving scanner x")
    worker.position_changed.emit({"x_um": 12.0})
    worker.finished.emit(
        {
            "axis": "x",
            "value": 12.5,
            "coordinate": "measurement",
            "position": {"x_um": 12.5},
        }
    )

    assert gui._current_position_values["x"] == 12.5
    assert gui.position_labels["x"].text() == "12.500"
    assert gui.status_label.text() == "move complete"
    assert gui.move_thread is None
    assert gui.move_worker is None
    assert gui.running_move_axis is None
    assert gui.connect_button.isEnabled() is True
    assert hint_refreshes == [True]
    assert worker.deleted is True
    assert thread.deleted is True
    _close_gui(gui)


def test_gui_move_failure_restores_position_and_controls(monkeypatch):
    gui = _new_gui(monkeypatch)
    experiment = object()
    warnings: list[tuple[str, str]] = []
    FakeMoveWorker.latest = None
    gui._current_position_values["t"] = 4.0
    monkeypatch.setattr(gui_module.QtCore, "QThread", FakeThread)
    monkeypatch.setattr(gui_module, "MoveWorker", FakeMoveWorker)
    monkeypatch.setattr(gui, "_ensure_experiment", lambda: experiment)
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )

    gui.move_absolute("t")
    worker = FakeMoveWorker.latest
    assert worker is not None
    worker.status_changed.emit("moving delay stage")
    worker.error_occurred.emit("simulated move failure")

    assert gui.move_thread is None
    assert gui.move_worker is None
    assert gui.running_move_axis is None
    assert gui.position_labels["t"].text() == "4.000"
    assert gui.status_label.text() == "move error"
    assert gui.connect_button.isEnabled() is True
    assert warnings == [("Move Error", "simulated move failure")]
    _close_gui(gui)


def test_gui_live_worker_applies_status_and_cleans_unexpected_exit(monkeypatch):
    gui = _new_gui(monkeypatch)
    experiment = object()
    FakeLiveWorker.latest = None
    monkeypatch.setattr(gui_module.QtCore, "QThread", FakeThread)
    monkeypatch.setattr(gui_module, "LiveStatusWorker", FakeLiveWorker)
    monkeypatch.setattr(gui, "_ensure_experiment", lambda: experiment)
    monkeypatch.setattr(gui, "_refresh_scan_limit_hints", lambda: None)

    worker = gui._ensure_live_worker()
    thread = gui.live_thread
    assert worker is FakeLiveWorker.latest
    assert isinstance(thread, FakeThread)
    assert worker.experiment is experiment
    assert worker.thread is thread
    assert thread.start_called is True

    worker.live_status_ready.emit(
        gui_module.LiveStatus(
            position=gui_module.Position(t_ps=1.0, x_um=2.0, y_um=3.0),
            signal={"X": 0.25, "Y": 0.0, "R": 0.25, "Theta": 0.0},
            lockin_overload={"overload": False},
        ),
        {"overload": False},
    )
    assert gui._current_position_values == {"t": 1.0, "x": 2.0, "y": 3.0}
    assert gui.signal_labels["X"].text() == "0.250 V"

    thread.finished.emit()
    assert worker.deleted is True
    assert thread.deleted is True
    assert gui.live_thread is None
    assert gui.live_worker is None
    _close_gui(gui)


def test_gui_existing_experiment_is_reused_without_config_update_for_live_worker(
    monkeypatch,
):
    gui = _new_gui(monkeypatch)
    experiment = object()
    gui.experiment = experiment  # type: ignore[assignment]
    FakeLiveWorker.latest = None
    monkeypatch.setattr(gui_module.QtCore, "QThread", FakeThread)
    monkeypatch.setattr(gui_module, "LiveStatusWorker", FakeLiveWorker)
    monkeypatch.setattr(
        gui,
        "_ensure_experiment",
        lambda: pytest.fail("live status must not reapply the experiment config"),
    )

    worker = gui._ensure_live_worker()

    assert worker.experiment is experiment
    assert gui.live_thread is not None
    gui.live_thread.finished.emit()
    _close_gui(gui)


def test_gui_live_status_rejects_malformed_payload(monkeypatch):
    gui = _new_gui(monkeypatch)
    gui.pending_origin_axis = "x"

    gui.handle_live_status_ready({"position": {}}, None)

    assert gui.pending_origin_axis is None
    assert "Unexpected live status payload." in gui.log.toPlainText()
    _close_gui(gui)


def test_gui_does_not_apply_stale_full_position_during_move_or_measurement(
    monkeypatch,
):
    gui = _new_gui(monkeypatch)
    status = gui_module.LiveStatus(
        position=gui_module.Position(t_ps=99.0, x_um=98.0, y_um=97.0),
        signal={"X": 0.25, "Y": 0.0, "R": 0.25, "Theta": 0.0},
    )
    gui.move_thread = object()  # type: ignore[assignment]

    gui.handle_live_status_ready(status, None)

    assert gui._current_position_values == {"t": None, "x": None, "y": None}
    assert gui.signal_labels["X"].text() == "0.250 V"

    gui.move_thread = None
    gui.measurement_thread = object()  # type: ignore[assignment]
    gui.handle_live_status_ready(
        gui_module.LiveStatus(
            position=gui_module.Position(t_ps=1.0, x_um=2.0, y_um=3.0),
            signal={"X": 0.5, "Y": 0.0, "R": 0.5, "Theta": 0.0},
        ),
        None,
    )

    assert gui._current_position_values == {"t": None, "x": None, "y": None}
    assert gui.signal_labels["X"].text() == "0.250 V"
    gui.measurement_thread = None
    _close_gui(gui)


def test_gui_resource_refresh_updates_all_choices_and_cleans_thread(monkeypatch):
    gui = _new_gui(monkeypatch)
    FakeResourceWorker.latest = None
    gui.lockin_resource_combo.addItem("GPIB0::OLD::INSTR")
    gui.lockin_resource_combo.setCurrentText("GPIB0::OLD::INSTR")
    previous_ports = {
        "t": gui.t_port_combo.currentText(),
        "x": gui.x_port_combo.currentText(),
        "y": gui.y_port_combo.currentText(),
    }
    monkeypatch.setattr(gui_module.QtCore, "QThread", FakeThread)
    monkeypatch.setattr(gui_module, "ResourceListWorker", FakeResourceWorker)

    gui.refresh_all_ports()

    worker = FakeResourceWorker.latest
    thread = gui.resource_thread
    assert worker is not None
    assert isinstance(thread, FakeThread)
    assert worker.thread is thread
    assert thread.start_called is True

    worker.resources_ready.emit(
        ["GPIB0::12::INSTR", "GPIB0::OLD::INSTR"],
        ["COM3", "COM9"],
    )
    assert gui.lockin_resource_combo.currentText() == "GPIB0::OLD::INSTR"
    for axis, combo in {
        "t": gui.t_port_combo,
        "x": gui.x_port_combo,
        "y": gui.y_port_combo,
    }.items():
        assert combo.currentText() == previous_ports[axis]
        assert {"COM3", "COM9"}.issubset(
            {combo.itemText(i) for i in range(combo.count())}
        )

    worker.error_occurred.emit("serial ports unavailable")
    assert (
        "Could not refresh hardware resources: serial ports unavailable"
        in gui.log.toPlainText()
    )
    worker.finished.emit()
    assert gui.resource_thread is None
    assert gui.resource_worker is None
    assert worker.deleted is True
    assert thread.deleted is True
    _close_gui(gui)


def test_gui_load_config_applies_fields_and_updates_experiment(
    monkeypatch, tmp_path: Path
):
    class ConfigExperiment:
        def __init__(self):
            self.config: dict[str, Any] = {}

    gui = _new_gui(monkeypatch)
    config_path = tmp_path / "loaded.json"
    candidate = normalize_config({})
    candidate["measurements"]["move_abs"]["zero"]["t_ps"] = 42.5
    candidate["measurements"]["trkr"]["scan"] = {
        "min": -20.0,
        "max": 20.0,
        "step": 10.0,
    }
    experiment = ConfigExperiment()
    last_paths: list[Path] = []
    warnings: list[tuple[str, str]] = []
    gui.experiment = experiment  # type: ignore[assignment]
    gui.config_path.setText(str(config_path))
    monkeypatch.setattr(
        gui_module,
        "resolve_config_path",
        lambda _path: ConfigPathResolution(config_path, "explicit", []),
    )
    monkeypatch.setattr(gui_module, "load_config", lambda _path: candidate)
    monkeypatch.setattr(
        gui_module, "write_last_config_path", lambda path: last_paths.append(path)
    )
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )

    gui.load_config_file()

    assert gui.config is candidate
    assert gui.config_path.text() == str(config_path)
    assert gui.t_zero_spin.value() == 42.5
    assert gui.trkr_min_spin.value() == -20.0
    assert gui.trkr_max_spin.value() == 20.0
    assert gui.trkr_step_spin.value() == 10.0
    assert experiment.config["measurements"]["move_abs"]["zero"]["t_ps"] == 42.5
    assert last_paths == [config_path]
    assert warnings == []
    assert f"Loaded config (explicit): {config_path}" in gui.log.toPlainText()
    _close_gui(gui)


def test_gui_load_config_rolls_back_when_field_application_fails(
    monkeypatch,
    tmp_path: Path,
):
    gui = _new_gui(monkeypatch)
    previous_config = deepcopy(gui.config)
    previous_zero = gui.t_zero_spin.value()
    config_path = tmp_path / "broken.json"
    candidate = normalize_config({})
    candidate["measurements"]["move_abs"]["zero"]["t_ps"] = 99.0
    warnings: list[tuple[str, str]] = []
    last_paths: list[Path] = []
    apply_fields = gui._load_config_into_fields

    def fail_for_candidate(config: dict[str, Any]):
        apply_fields(config)
        if config is candidate:
            raise RuntimeError("simulated field application failure")

    gui.config_path.setText(str(config_path))
    monkeypatch.setattr(
        gui_module,
        "resolve_config_path",
        lambda _path: ConfigPathResolution(config_path, "explicit", []),
    )
    monkeypatch.setattr(gui_module, "load_config", lambda _path: candidate)
    monkeypatch.setattr(gui, "_load_config_into_fields", fail_for_candidate)
    monkeypatch.setattr(
        gui_module, "write_last_config_path", lambda path: last_paths.append(path)
    )
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )

    gui.load_config_file()

    assert gui.config == previous_config
    assert gui.config_path.text() == str(config_path)
    assert gui.t_zero_spin.value() == previous_zero
    assert last_paths == []
    assert warnings == [("Config Error", "simulated field application failure")]
    _close_gui(gui)


def test_gui_save_config_validates_and_updates_experiment(monkeypatch, tmp_path: Path):
    class ConfigExperiment:
        def __init__(self):
            self.config: dict[str, Any] = {}

    gui = _new_gui(monkeypatch)
    output = tmp_path / "saved.json"
    experiment = ConfigExperiment()
    last_paths: list[Path] = []
    warnings: list[tuple[str, str]] = []
    gui.experiment = experiment  # type: ignore[assignment]
    gui.config_path.setText(str(output))
    gui.t_zero_spin.setValue(12.0)
    gui.trkr_min_spin.setValue(-10.0)
    gui.trkr_max_spin.setValue(10.0)
    gui.trkr_step_spin.setValue(5.0)
    monkeypatch.setattr(
        gui_module, "write_last_config_path", lambda path: last_paths.append(path)
    )
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )

    gui.save_config_file()

    saved = json.loads(output.read_text(encoding="utf-8"))
    assert saved["measurements"]["move_abs"]["zero"]["t_ps"] == 12.0
    assert saved["measurements"]["trkr"]["scan"] == {
        "min": -10.0,
        "max": 10.0,
        "step": 5.0,
    }
    assert gui.config == saved
    assert experiment.config == saved
    assert last_paths == [output]
    assert warnings == []
    _close_gui(gui)


def test_gui_save_config_rejects_invalid_scan_without_mutating_state(
    monkeypatch,
    tmp_path: Path,
):
    class ConfigExperiment:
        def __init__(self):
            self.config = {"sentinel": True}

    gui = _new_gui(monkeypatch)
    output = tmp_path / "invalid.json"
    previous_config = deepcopy(gui.config)
    experiment = ConfigExperiment()
    warnings: list[tuple[str, str]] = []
    gui.experiment = experiment  # type: ignore[assignment]
    gui.config_path.setText(str(output))
    gui.trkr_min_spin.setValue(10.0)
    gui.trkr_max_spin.setValue(-10.0)
    gui.trkr_step_spin.setValue(1.0)
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )

    gui.save_config_file()

    assert output.exists() is False
    assert gui.config == previous_config
    assert experiment.config == {"sentinel": True}
    assert warnings == [
        ("Config Error", "No scan points generated. Check min/max/step.")
    ]
    _close_gui(gui)


def test_gui_save_rows_atomically_replaces_explicit_output(monkeypatch, tmp_path: Path):
    gui = _new_gui(monkeypatch)
    output = tmp_path / "manual.csv"
    sidecar = tmp_path / "manual.csv.meta.json"
    output.write_text("old csv\n", encoding="utf-8")
    sidecar.write_text('{"old": true}\n', encoding="utf-8")
    gui.rows_by_mode["signal_monitor"] = [
        {
            "measurement": "signal_monitor",
            "elapsed_s": 0.0,
            "X_V": 1.5,
        }
    ]
    gui.output_dir_edit.setText(str(tmp_path))
    gui.output_name_edit.setText("manual.csv")
    gui.auto_suffix_check.setChecked(False)

    gui.save_rows()

    with output.open(newline="", encoding="utf-8") as stream:
        saved_rows = list(csv.DictReader(stream))
    assert len(saved_rows) == 1
    assert saved_rows[0]["measurement"] == "signal_monitor"
    assert saved_rows[0]["elapsed_s"] == "0.0"
    assert float(saved_rows[0]["X_V"]) == 1.5
    metadata = json.loads(sidecar.read_text(encoding="utf-8"))
    assert metadata["status"] == "completed"
    assert metadata["rows_written"] == 1
    assert "Saved 1 rows" in gui.log.toPlainText()
    _close_gui(gui)


def test_gui_save_rows_rejects_running_measurement_and_handles_io_error(
    monkeypatch,
):
    gui = _new_gui(monkeypatch)
    gui.rows_by_mode["signal_monitor"] = [{"measurement": "signal_monitor"}]
    warnings: list[tuple[str, str]] = []
    writes: list[bool] = []
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )
    monkeypatch.setattr(
        gui_module,
        "write_measurement_rows",
        lambda *_args, **_kwargs: writes.append(True),
    )
    gui.measurement_thread = object()  # type: ignore[assignment]
    gui._set_running(True)

    gui.save_rows()

    assert gui.save_rows_button.isEnabled() is False
    assert writes == []
    assert warnings == [
        (
            "Save Error",
            "Stop the running measurement before saving collected rows.",
        )
    ]

    gui.measurement_thread = None
    gui._set_running(False)
    monkeypatch.setattr(
        gui_module,
        "write_measurement_rows",
        lambda *_args, **_kwargs: (_ for _ in ()).throw(
            PermissionError("output is read-only")
        ),
    )
    gui.save_rows()
    assert gui.save_rows_button.isEnabled() is True
    assert warnings[-1] == ("Save Error", "output is read-only")
    _close_gui(gui)


def _signal_values() -> dict[str, float]:
    return {"X_V": 1.0, "Y_V": 2.0, "R_V": 3.0, "Theta_deg": 4.0}


def test_gui_signal_point_updates_rows_snapshot_position_and_curves(monkeypatch):
    gui = _new_gui(monkeypatch)
    row: dict[str, Any] = {
        "measurement": "signal_monitor",
        "target_elapsed_s": 0.5,
        "elapsed_s": 0.51,
        "t_ps": 10.0,
        "x_um": 20.0,
        "y_um": 30.0,
        **_signal_values(),
    }

    gui.handle_point(MeasurementPoint(index=1, total_points=2, row=row))

    assert gui.rows_by_mode["signal_monitor"] == [row]
    assert gui.point_label.text() == "1/2"
    assert gui.eta_label.text() == "-"
    assert gui._current_position_values == {"t": 10.0, "x": 20.0, "y": 30.0}
    assert gui.signal_labels["X"].text() == "1.000 V"
    assert gui.snapshot_table.rowCount() > 0
    snapshot = {
        gui.snapshot_table.item(index, 0).text(): gui.snapshot_table.item(
            index, 1
        ).text()
        for index in range(gui.snapshot_table.rowCount())
    }
    assert snapshot["elapsed_s"] == "0.510"
    curve_x, curve_y = gui.curve1.getData()
    assert curve_x.tolist() == [0.51]
    assert curve_y.tolist() == [1.0]
    _close_gui(gui)


@pytest.mark.parametrize(
    ("tab_index", "measurement", "row", "curve_key", "expected_x"),
    [
        (
            1,
            "trkr",
            {
                "measurement": "trkr",
                "target_t_cor_ps": -10.0,
                "t_cor_ps": None,
                "t_ps": None,
                **_signal_values(),
            },
            "standard",
            -10.0,
        ),
        (
            2,
            "srkr",
            {
                "measurement": "srkr",
                "fast_axis": "x",
                "target_x_cor_um": 5.0,
                "x_cor_um": None,
                "x_um": None,
                **_signal_values(),
            },
            "srkr",
            5.0,
        ),
        (
            4,
            "srkr_2d",
            {
                "measurement": "srkr_2d",
                "fast_axis": "x",
                "slow_axis": "y",
                "target_x_cor_um": 2.0,
                "target_y_cor_um": 3.0,
                "x_cor_um": None,
                **_signal_values(),
            },
            "scan2d",
            2.0,
        ),
    ],
)
def test_gui_plot_uses_scan_target_when_measured_position_is_missing(
    monkeypatch,
    tab_index: int,
    measurement: str,
    row: dict[str, Any],
    curve_key: str,
    expected_x: float,
):
    gui = _new_gui(monkeypatch)
    gui.measurement_tabs.setCurrentIndex(tab_index)
    gui.running_measurement = measurement
    gui.t_zero_spin.setValue(100.0)
    gui.x_zero_spin.setValue(10.0)
    QtWidgets.QApplication.processEvents()

    gui.handle_point(MeasurementPoint(index=1, total_points=1, row=row))

    if curve_key == "standard":
        curve_x, _ = gui.curve1.getData()
    elif curve_key == "srkr":
        curve_x, _ = gui.srkr_curves[("x", 1)].getData()
    else:
        curve_x, _ = gui.scan2d_line_curves[1].getData()
        image = gui.scan2d_heatmaps[1].image
        assert image.shape == (1, 1)
        assert float(image[0, 0]) == 1.0
    assert curve_x.tolist() == [expected_x]
    _close_gui(gui)


def _tampered_measurement_point(
    *, index: int = 1, row: dict[str, Any]
) -> MeasurementPoint:
    point = MeasurementPoint(
        index=1,
        total_points=1,
        row={"measurement": "signal_monitor", **_signal_values()},
    )
    point.index = index
    point.row = row
    return point


@pytest.mark.parametrize(
    "payload",
    [
        object(),
        _tampered_measurement_point(
            index=0,
            row={
                "measurement": "signal_monitor",
                "target_elapsed_s": 0.0,
                "elapsed_s": 0.0,
                **_signal_values(),
            },
        ),
        _tampered_measurement_point(
            row={
                "measurement": "trkr",
                "target_elapsed_s": 0.0,
                "elapsed_s": 0.0,
                **_signal_values(),
            },
        ),
        _tampered_measurement_point(
            row={
                "measurement": "signal_monitor",
                "target_elapsed_s": 0.0,
                "elapsed_s": 0.0,
                "X_V": float("nan"),
                "Y_V": 2.0,
                "R_V": 3.0,
                "Theta_deg": 4.0,
            },
        ),
    ],
)
def test_gui_rejects_invalid_measurement_point_and_stops_worker(
    monkeypatch,
    payload: object,
):
    gui = _new_gui(monkeypatch)
    worker = FakeMeasurementWorker()
    errors: list[tuple[str, str]] = []
    gui.worker = worker  # type: ignore[assignment]
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "critical",
        lambda _parent, title, message: errors.append((title, message)),
    )

    gui.handle_point(payload)

    assert gui.rows_by_mode["signal_monitor"] == []
    assert worker.stop_called is True
    assert len(errors) == 1
    assert errors[0][0] == "Measurement Error"
    assert errors[0][1].startswith("Invalid measurement point:")
    gui.worker = None
    _close_gui(gui)


def test_gui_finished_rejects_non_list_payload_without_crashing(monkeypatch):
    gui = _new_gui(monkeypatch)
    curve_updates: list[bool] = []
    monkeypatch.setattr(gui, "_update_curves", lambda: curve_updates.append(True))

    gui.handle_finished({"not": "a list"})

    assert "invalid row payload" in gui.log.toPlainText()
    assert "Finished. 0 points collected." in gui.log.toPlainText()
    assert curve_updates == [True]
    _close_gui(gui)


def test_gui_load_config_requires_resolved_path_without_mutating_state(monkeypatch):
    gui = _new_gui(monkeypatch)
    previous_config = deepcopy(gui.config)
    previous_path = gui.config_path.text()
    warnings: list[tuple[str, str]] = []
    monkeypatch.setattr(
        gui_module,
        "resolve_config_path",
        lambda _path: ConfigPathResolution(None, "none", []),
    )
    monkeypatch.setattr(
        gui_module,
        "load_config",
        lambda _path: pytest.fail("load attempted without resolved path"),
    )
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )

    gui.load_config_file()

    assert gui.config == previous_config
    assert gui.config_path.text() == previous_path
    assert warnings == [("Config Error", "Choose a config path before loading.")]
    _close_gui(gui)


def test_gui_close_event_defers_until_async_shutdown_completes(monkeypatch):
    gui = _new_gui(monkeypatch)
    shutdown_requests: list[bool] = []

    class Event:
        ignored = False

        def ignore(self):
            self.ignored = True

    event = Event()
    monkeypatch.setattr(
        gui, "_request_async_shutdown", lambda: shutdown_requests.append(True)
    )

    gui.closeEvent(event)

    assert event.ignored is True
    assert shutdown_requests == [True]
    _close_gui(gui)


def test_gui_read_live_status_routes_for_busy_move_and_idle_states(monkeypatch):
    gui = _new_gui(monkeypatch)
    full_requests: list[bool] = []
    lockin_requests: list[bool] = []
    monkeypatch.setattr(gui, "_ensure_experiment", lambda: object())
    monkeypatch.setattr(
        gui, "_request_full_live_status", lambda: full_requests.append(True)
    )
    monkeypatch.setattr(
        gui, "_request_lockin_live_status", lambda: lockin_requests.append(True)
    )

    gui.device_command_active = True
    gui.read_live_status()
    gui.device_command_active = False
    gui.move_thread = object()  # type: ignore[assignment]
    gui.read_live_status()
    gui.move_thread = None
    gui.read_live_status()

    assert lockin_requests == [True]
    assert full_requests == [True]
    _close_gui(gui)


def test_gui_pending_origin_handles_missing_then_available_position(monkeypatch):
    gui = _new_gui(monkeypatch)
    warnings: list[tuple[str, str]] = []
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )

    gui.pending_origin_axis = "x"
    gui._apply_pending_origin(Position())
    assert gui.pending_origin_axis is None
    assert warnings == [("Origin Error", "x position is unavailable.")]

    gui.pending_origin_axis = "x"
    gui._apply_pending_origin(Position(x_um=12.5))
    assert gui.pending_origin_axis is None
    assert gui.x_zero_spin.value() == 12.5
    _close_gui(gui)


def test_gui_device_command_is_blocked_during_measurement_with_warning(monkeypatch):
    gui = _new_gui(monkeypatch)
    warnings: list[tuple[str, str]] = []
    emitted: list[object] = []
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )
    gui.device_command_requested.connect(emitted.append)
    gui.measurement_thread = object()  # type: ignore[assignment]

    gui.connect_all()

    assert warnings == [("Device Error", "Stop the running measurement first.")]
    assert emitted == []
    assert gui.device_command_active is False
    gui.measurement_thread = None
    _close_gui(gui)


def test_gui_initialize_rejects_unsupported_target_without_worker(monkeypatch):
    gui = _new_gui(monkeypatch)
    warnings: list[tuple[str, str]] = []
    starts: list[tuple] = []
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )
    monkeypatch.setattr(
        gui,
        "_start_device_command",
        lambda *args, **kwargs: starts.append((args, kwargs)),
    )

    gui.initialize_device("scanner", "z")

    assert warnings == [
        ("Initialize Error", "Unsupported initialize target: scanner z")
    ]
    assert starts == []
    _close_gui(gui)


def test_gui_device_result_routes_disconnect_wait_and_lockin_settings(monkeypatch):
    gui = _new_gui(monkeypatch)
    full_refreshes: list[bool] = []
    lockin_refreshes: list[bool] = []
    applied: list[dict] = []
    cleanups: list[bool] = []
    monkeypatch.setattr(
        gui, "_request_full_live_status", lambda: full_refreshes.append(True)
    )
    monkeypatch.setattr(
        gui, "_request_lockin_live_status", lambda: lockin_refreshes.append(True)
    )
    monkeypatch.setattr(gui, "_apply_lockin_settings", applied.append)
    monkeypatch.setattr(gui, "cleanup_device_command", lambda: cleanups.append(True))

    gui.handle_device_command_finished({"command": "disconnect_all"})
    gui.pending_wait_spin = gui.trkr_wait_spin
    gui.handle_device_command_finished({"command": "lockin_wait_time", "wait_s": 1.25})
    settings = {"Sensitivity": 1e-3, "Time Constant": 0.5}
    gui.handle_device_command_finished(
        {"command": "set_lockin_settings", "settings": settings}
    )

    assert full_refreshes == [True]
    assert lockin_refreshes == [True]
    assert gui.trkr_wait_spin.value() == 1.25
    assert applied == [settings]
    assert cleanups == [True, True, True]
    assert gui.status_label.text() == "lock-in settings applied"
    _close_gui(gui)


def test_gui_shutdown_request_without_live_timer_is_idempotent(monkeypatch):
    gui = _new_gui(monkeypatch)
    scheduled: list[tuple[int, Callable[[], Any]]] = []
    stops: list[bool] = []
    del gui.live_timer
    monkeypatch.setattr(gui, "stop_measurement", lambda: stops.append(True))
    monkeypatch.setattr(
        gui_module.QtCore.QTimer,
        "singleShot",
        lambda delay, callback: scheduled.append((delay, callback)),
    )

    gui._request_async_shutdown()
    gui._request_async_shutdown()

    assert gui._shutdown_requested is True
    assert stops == [True]
    assert len(scheduled) == 1
    assert scheduled[0][0] == 0
    scheduled.clear()
    _close_gui(gui)


@pytest.mark.parametrize("busy_kind", ["move", "device"])
def test_gui_shutdown_drain_retries_for_nonmeasurement_worker(busy_kind, monkeypatch):
    gui = _new_gui(monkeypatch)
    scheduled: list[tuple[int, Callable[[], Any]]] = []
    stops: list[bool] = []
    gui._shutdown_requested = True
    gui.experiment = object()  # type: ignore[assignment]
    if busy_kind == "move":
        gui.move_thread = object()  # type: ignore[assignment]
    else:
        gui.device_command_active = True
    monkeypatch.setattr(gui, "stop_measurement", lambda: stops.append(True))
    monkeypatch.setattr(
        gui_module.QtCore.QTimer,
        "singleShot",
        lambda delay, callback: scheduled.append((delay, callback)),
    )

    gui._drain_before_shutdown()

    assert stops == [True]
    assert len(scheduled) == 1
    assert scheduled[0][0] == 100
    gui.move_thread = None
    gui.device_command_active = False
    gui.experiment = None
    scheduled.clear()
    _close_gui(gui)


def test_gui_measurement_thread_cleanup_during_shutdown_suppresses_live_refresh(
    monkeypatch,
):
    gui = _new_gui(monkeypatch)
    refreshes: list[bool] = []
    running_states: list[bool] = []
    gui._shutdown_requested = True
    gui.experiment = object()  # type: ignore[assignment]
    gui.measurement_thread = object()  # type: ignore[assignment]
    gui.worker = object()  # type: ignore[assignment]
    gui.running_measurement = "trkr"
    gui.running_srkr_axis = "x"
    gui.running_motion_axes = {"t", "x"}
    gui._scan2d_fast_point_count = 2
    gui._scan2d_slow_point_count = 3
    gui._scan2d_eta_anchor_at = 1.0
    gui._scan2d_eta_line_cycle_s = 2.0
    monkeypatch.setattr(gui, "_set_running", running_states.append)
    monkeypatch.setattr(
        gui, "_request_full_live_status", lambda: refreshes.append(True)
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
    assert running_states == [False]
    assert refreshes == []
    gui.experiment = None
    _close_gui(gui)


def test_gui_worker_cleanup_clears_device_and_live_state(monkeypatch):
    gui = _new_gui(monkeypatch)
    initializing: list[bool] = []
    gui.device_thread = object()  # type: ignore[assignment]
    gui.device_worker = object()  # type: ignore[assignment]
    gui.device_command_active = True
    gui.pending_wait_spin = gui.trkr_wait_spin
    gui.live_thread = object()  # type: ignore[assignment]
    gui.live_worker = object()  # type: ignore[assignment]
    monkeypatch.setattr(gui, "_set_device_initializing", initializing.append)

    gui.cleanup_device_thread()
    gui.cleanup_live_thread()

    assert gui.device_thread is None
    assert gui.device_worker is None
    assert gui.device_command_active is False
    assert gui.pending_wait_spin is None
    assert gui.live_thread is None
    assert gui.live_worker is None
    assert initializing == [False]
    _close_gui(gui)


def test_gui_load_config_parse_failure_preserves_config_path_and_ui(
    monkeypatch, tmp_path: Path
):
    gui = _new_gui(monkeypatch)
    config_path = tmp_path / "corrupt.json"
    previous_config = deepcopy(gui.config)
    previous_zero = gui.t_zero_spin.value()
    previous_min = gui.trkr_min_spin.value()
    warnings: list[tuple[str, str]] = []
    last_paths: list[Path] = []
    gui.config_path.setText(str(config_path))
    monkeypatch.setattr(
        gui_module,
        "resolve_config_path",
        lambda _path: ConfigPathResolution(config_path, "explicit", []),
    )
    monkeypatch.setattr(
        gui_module,
        "load_config",
        lambda _path: (_ for _ in ()).throw(ValueError("invalid JSON config")),
    )
    monkeypatch.setattr(
        gui_module, "write_last_config_path", lambda path: last_paths.append(path)
    )
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )

    gui.load_config_file()

    assert gui.config == previous_config
    assert gui.config_path.text() == str(config_path)
    assert gui.t_zero_spin.value() == previous_zero
    assert gui.trkr_min_spin.value() == previous_min
    assert last_paths == []
    assert warnings == [("Config Error", "invalid JSON config")]
    _close_gui(gui)


def test_gui_save_config_io_failure_preserves_runtime_state_and_experiment(
    monkeypatch, tmp_path: Path
):
    class ConfigExperiment:
        def __init__(self):
            self.config = {"sentinel": True}

    gui = _new_gui(monkeypatch)
    output = tmp_path / "read-only" / "config.json"
    previous_config = deepcopy(gui.config)
    experiment = ConfigExperiment()
    warnings: list[tuple[str, str]] = []
    last_paths: list[Path] = []
    gui.experiment = experiment  # type: ignore[assignment]
    gui.config_path.setText(str(output))
    gui.t_zero_spin.setValue(88.0)
    monkeypatch.setattr(
        gui_module,
        "save_config",
        lambda *_args, **_kwargs: (_ for _ in ()).throw(
            PermissionError("config directory is read-only")
        ),
    )
    monkeypatch.setattr(
        gui_module, "write_last_config_path", lambda path: last_paths.append(path)
    )
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )

    gui.save_config_file()

    assert gui.config == previous_config
    assert experiment.config == {"sentinel": True}
    assert gui.t_zero_spin.value() == 88.0
    assert not output.exists()
    assert last_paths == []
    assert warnings == [("Config Error", "config directory is read-only")]
    gui.experiment = None
    _close_gui(gui)


def test_gui_save_config_requires_nonempty_path_before_runtime_collection(monkeypatch):
    gui = _new_gui(monkeypatch)
    warnings: list[tuple[str, str]] = []
    gui.config_path.clear()
    monkeypatch.setattr(
        gui,
        "_runtime_config",
        lambda: pytest.fail("runtime config collected without output path"),
    )
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )

    gui.save_config_file()

    assert warnings == [("Config Error", "Choose a config path before saving.")]
    _close_gui(gui)


def test_gui_start_is_noop_when_measurement_thread_already_exists(monkeypatch):
    gui = _new_gui(monkeypatch)
    existing_thread = object()
    warnings: list[tuple[str, str]] = []
    gui.measurement_thread = existing_thread  # type: ignore[assignment]
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )

    gui.start_measurement()

    assert gui.measurement_thread is existing_thread
    assert gui.worker is None
    assert warnings == []
    gui.measurement_thread = None
    _close_gui(gui)


def test_gui_rejects_start_while_move_worker_is_active(monkeypatch):
    gui = _new_gui(monkeypatch)
    warnings: list[tuple[str, str]] = []
    gui.move_thread = object()  # type: ignore[assignment]
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )

    gui.start_measurement()

    assert gui.measurement_thread is None
    assert gui.worker is None
    assert warnings == [
        ("Run Error", "Wait for the active device operation to finish first.")
    ]
    gui.move_thread = None
    _close_gui(gui)


def test_gui_rejects_unknown_measurement_before_device_check(monkeypatch, tmp_path):
    gui = _new_gui(monkeypatch)
    warnings: list[tuple[str, str]] = []
    device_checks: list[bool] = []
    monkeypatch.setattr(gui, "_measurement_name", lambda: "unknown")
    monkeypatch.setattr(gui, "_output_path", lambda: tmp_path / "unknown.csv")
    monkeypatch.setattr(
        gui,
        "_missing_required_devices",
        lambda *_args: device_checks.append(True) or [],
    )
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )

    gui.start_measurement()

    assert gui.measurement_thread is None
    assert gui.worker is None
    assert device_checks == []
    assert warnings == [("Run Error", "Unsupported measurement: unknown")]
    _close_gui(gui)


def test_gui_invalid_measurement_point_stops_worker_and_reports_critical(monkeypatch):
    gui = _new_gui(monkeypatch)
    worker = FakeMeasurementWorker()
    errors: list[tuple[str, str]] = []
    gui.worker = worker  # type: ignore[assignment]
    gui.running_measurement = "signal_monitor"
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "critical",
        lambda _parent, title, message: errors.append((title, message)),
    )

    gui.handle_point({"not": "a measurement point"})

    assert worker.stop_called is True
    assert len(errors) == 1
    assert errors[0][0] == "Measurement Error"
    assert errors[0][1].startswith("Invalid measurement point:")
    assert gui.rows_by_mode["signal_monitor"] == []
    gui.worker = None
    gui.running_measurement = None
    _close_gui(gui)


def test_gui_normal_measurement_cleanup_resets_state_and_schedules_live_refresh(
    monkeypatch,
):
    gui = _new_gui(monkeypatch)
    scheduled: list[tuple[int, Callable[[], Any]]] = []
    curve_updates: list[bool] = []
    gui.experiment = object()  # type: ignore[assignment]
    gui.measurement_thread = object()  # type: ignore[assignment]
    gui.worker = object()  # type: ignore[assignment]
    gui.running_measurement = gui._measurement_name()
    gui.running_motion_axes = {"t"}
    monkeypatch.setattr(gui, "_update_curves", lambda: curve_updates.append(True))
    monkeypatch.setattr(
        gui_module.QtCore.QTimer,
        "singleShot",
        lambda delay, callback: scheduled.append((delay, callback)),
    )

    gui.cleanup_thread()

    assert gui.measurement_thread is None
    assert gui.worker is None
    assert gui.running_measurement is None
    assert gui.running_motion_axes == set()
    assert curve_updates == [True]
    assert scheduled == [(0, gui._request_full_live_status)]
    scheduled.clear()
    gui.experiment = None
    _close_gui(gui)


def test_gui_signal_form_values_are_passed_as_worker_arguments(monkeypatch, tmp_path):
    gui = _new_gui(monkeypatch)
    experiment = object()
    FakeMeasurementWorker.latest = None
    monkeypatch.setattr(gui_module.QtCore, "QThread", FakeThread)
    monkeypatch.setattr(gui_module, "MeasurementWorker", FakeMeasurementWorker)
    monkeypatch.setattr(gui, "_missing_required_devices", lambda *_args: [])
    monkeypatch.setattr(gui, "_ensure_experiment", lambda: experiment)
    monkeypatch.setattr(gui, "_output_path", lambda: tmp_path / "signal-form.csv")
    gui.measurement_tabs.setCurrentIndex(0)
    gui.signal_interval_spin.setValue(0.13)
    gui.signal_points_spin.setValue(7)

    gui.start_measurement()

    worker = FakeMeasurementWorker.latest
    assert worker is not None
    assert worker.kwargs["measurement"] == "signal_monitor"
    assert worker.kwargs["scan_plan"] is None
    assert worker.kwargs["interval_s"] == pytest.approx(0.13)
    assert worker.kwargs["n_points"] == 7
    assert worker.kwargs["wait_s"] is None
    assert worker.kwargs["return_to_zero"] is None
    assert gui.running_motion_axes == set()
    worker.finished.emit([])
    _close_gui(gui)


@pytest.mark.parametrize(
    ("tab_index", "mode", "fast_axis", "slow_axis"),
    [(3, "strkr", "t", "y"), (4, "srkr_2d", "y", "x")],
)
def test_gui_2d_form_roles_and_zero_values_build_exact_plan(
    tab_index, mode, fast_axis, slow_axis, monkeypatch, tmp_path
):
    gui = _new_gui(monkeypatch)
    FakeMeasurementWorker.latest = None
    monkeypatch.setattr(gui_module.QtCore, "QThread", FakeThread)
    monkeypatch.setattr(gui_module, "MeasurementWorker", FakeMeasurementWorker)
    monkeypatch.setattr(gui, "_missing_required_devices", lambda *_args: [])
    monkeypatch.setattr(gui, "_ensure_experiment", lambda: object())
    monkeypatch.setattr(gui, "_output_path", lambda: tmp_path / f"{mode}-form.csv")
    gui.measurement_tabs.setCurrentIndex(tab_index)
    QtWidgets.QApplication.processEvents()
    fast_combo = (
        gui.strkr_fast_axis_combo if mode == "strkr" else gui.srkr_2d_fast_axis_combo
    )
    slow_combo = (
        gui.strkr_slow_axis_combo if mode == "strkr" else gui.srkr_2d_slow_axis_combo
    )
    role_spins = gui.strkr_role_spins if mode == "strkr" else gui.srkr_2d_role_spins
    fast_combo.setCurrentText(fast_axis)
    slow_combo.setCurrentText(slow_axis)
    gui._handle_2d_axis_changed(mode)
    for key, value in {"min": -1.0, "max": 1.0, "step": 1.0}.items():
        role_spins["fast_axis"][key].setValue(value)
    for key, value in {"min": 10.0, "max": 20.0, "step": 10.0}.items():
        role_spins["slow_axis"][key].setValue(value)
    gui.t_zero_spin.setValue(100.0)
    gui.x_zero_spin.setValue(200.0)
    gui.y_zero_spin.setValue(300.0)

    gui.start_measurement()

    worker = FakeMeasurementWorker.latest
    assert worker is not None
    plan = worker.kwargs["scan_plan"]
    assert plan.fast_axis == fast_axis
    assert plan.slow_axis == slow_axis
    assert plan.fast_target_points == [-1.0, 0.0, 1.0]
    assert plan.slow_target_points == [10.0, 20.0]
    assert plan.zero == {"t_ps": 100.0, "x_um": 200.0, "y_um": 300.0}
    assert plan.return_to_zero == {"fast_axis": True, "slow_axis": True}
    assert gui.running_motion_axes == {fast_axis, slow_axis}
    assert gui._scan2d_fast_point_count == 3
    assert gui._scan2d_slow_point_count == 2
    worker.finished.emit([])
    _close_gui(gui)


@pytest.mark.parametrize(("tab_index", "mode"), [(2, "srkr"), (3, "strkr")])
def test_gui_invalid_scan_form_warns_before_device_check_and_worker_creation(
    tab_index, mode, monkeypatch, tmp_path
):
    gui = _new_gui(monkeypatch)
    warnings: list[tuple[str, str]] = []
    device_checks: list[bool] = []
    FakeMeasurementWorker.latest = None
    monkeypatch.setattr(gui_module.QtCore, "QThread", FakeThread)
    monkeypatch.setattr(gui_module, "MeasurementWorker", FakeMeasurementWorker)
    monkeypatch.setattr(gui, "_output_path", lambda: tmp_path / f"invalid-{mode}.csv")
    monkeypatch.setattr(
        gui,
        "_missing_required_devices",
        lambda *_args: device_checks.append(True) or [],
    )
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )
    gui.measurement_tabs.setCurrentIndex(tab_index)
    QtWidgets.QApplication.processEvents()
    if mode == "srkr":
        gui.srkr_min_spin.setValue(5.0)
        gui.srkr_max_spin.setValue(-5.0)
        gui.srkr_step_spin.setValue(1.0)
    else:
        gui.strkr_role_spins["fast_axis"]["min"].setValue(5.0)
        gui.strkr_role_spins["fast_axis"]["max"].setValue(-5.0)
        gui.strkr_role_spins["fast_axis"]["step"].setValue(1.0)

    gui.start_measurement()

    assert gui.measurement_thread is None
    assert gui.worker is None
    assert FakeMeasurementWorker.latest is None
    assert device_checks == []
    assert warnings == [("Run Error", "No scan points generated. Check min/max/step.")]
    _close_gui(gui)


def test_gui_startup_without_resolved_config_uses_normalized_defaults(monkeypatch):
    monkeypatch.setattr(
        gui_module,
        "resolve_config_path",
        lambda: ConfigPathResolution(None, "none", []),
    )
    monkeypatch.setattr(
        gui_module,
        "load_config",
        lambda _path: pytest.fail("load attempted without resolved config"),
    )

    gui = _new_gui(monkeypatch)

    assert gui.config_path.text() == ""
    assert gui.config["profile"]["name"] == "default"
    assert gui.config["instruments"] == {}
    assert gui.status_label.text() == "idle"
    assert gui.measurement_thread is None
    assert gui.worker is None
    assert all(rows == [] for rows in gui.rows_by_mode.values())
    assert "No config loaded. Choose a config path and click Load." in (
        gui.log.toPlainText()
    )
    assert gui.signal_points_spin.value() == 360
    assert gui.start_button.isEnabled() is True
    _close_gui(gui)


def test_gui_startup_loads_resolved_config_updates_fields_and_records_path(
    monkeypatch, tmp_path
):
    path = tmp_path / "startup.json"
    candidate = normalize_config({})
    candidate["measurements"]["move_abs"]["zero"].update(
        {"t_ps": 11.0, "x_um": 22.0, "y_um": 33.0}
    )
    candidate["measurements"]["signal_monitor"].update(
        {"interval_s": 0.25, "n_points": 9}
    )
    recorded: list[Path] = []
    monkeypatch.setattr(
        gui_module,
        "resolve_config_path",
        lambda: ConfigPathResolution(path, "last", []),
    )
    monkeypatch.setattr(gui_module, "load_config", lambda loaded_path: candidate)
    monkeypatch.setattr(
        gui_module, "write_last_config_path", lambda value: recorded.append(value)
    )

    gui = _new_gui(monkeypatch)

    assert gui.config is candidate
    assert gui.config_path.text() == str(path)
    assert gui.t_zero_spin.value() == 11.0
    assert gui.x_zero_spin.value() == 22.0
    assert gui.y_zero_spin.value() == 33.0
    assert gui.signal_interval_spin.value() == pytest.approx(0.25)
    assert gui.signal_points_spin.value() == 9
    assert recorded == [path]
    assert f"Loaded config (last): {path}" in gui.log.toPlainText()
    _close_gui(gui)


def test_gui_startup_load_failure_propagates_without_recording_last_path(
    monkeypatch, tmp_path
):
    path = tmp_path / "broken-startup.json"
    recorded: list[Path] = []
    monkeypatch.setattr(
        gui_module,
        "resolve_config_path",
        lambda: ConfigPathResolution(path, "explicit", []),
    )
    monkeypatch.setattr(
        gui_module,
        "load_config",
        lambda _path: (_ for _ in ()).throw(ValueError("startup config is corrupt")),
    )
    monkeypatch.setattr(
        gui_module, "write_last_config_path", lambda value: recorded.append(value)
    )
    QtWidgets.QApplication.instance() or QtWidgets.QApplication([])

    with pytest.raises(ValueError, match="startup config is corrupt"):
        TRKRGui()

    assert recorded == []


def test_gui_measurement_tab_change_failure_sets_status_and_log(monkeypatch):
    gui = _new_gui(monkeypatch)
    monkeypatch.setattr(
        gui,
        "_store_output_settings",
        lambda _measurement: (_ for _ in ()).throw(
            RuntimeError("output state unavailable")
        ),
    )

    gui._handle_measurement_tab_changed(1)

    assert gui.status_label.text() == "tab error"
    assert "Tab change error: output state unavailable" in gui.log.toPlainText()
    _close_gui(gui)


@pytest.mark.parametrize("is_current", [False, True])
def test_gui_2d_axis_change_runs_helpers_and_updates_only_current_mode(
    is_current, monkeypatch
):
    gui = _new_gui(monkeypatch)
    calls: list[str] = []
    monkeypatch.setattr(
        gui,
        "_sync_scan2d_role_values_to_axis_ranges",
        lambda mode: calls.append(f"sync:{mode}"),
    )
    monkeypatch.setattr(
        gui,
        "_normalize_2d_axis_controls",
        lambda mode: calls.append(f"normalize:{mode}"),
    )
    monkeypatch.setattr(
        gui,
        "_load_scan2d_role_ranges",
        lambda mode: calls.append(f"load:{mode}"),
    )
    monkeypatch.setattr(gui, "_refresh_scan_limit_hints", lambda: calls.append("hints"))
    monkeypatch.setattr(gui, "_update_curves", lambda: calls.append("curves"))
    monkeypatch.setattr(
        gui, "_measurement_name", lambda: "strkr" if is_current else "trkr"
    )

    gui._handle_2d_axis_changed("strkr")

    assert calls == [
        "sync:strkr",
        "normalize:strkr",
        "load:strkr",
        "hints",
        *(["curves"] if is_current else []),
    ]
    _close_gui(gui)


def test_gui_unknown_2d_mode_helpers_return_safe_empty_defaults(monkeypatch):
    gui = _new_gui(monkeypatch)

    assert gui._scan2d_axis_range_widgets("unknown") == {}
    assert gui._scan2d_role_spin_widgets("unknown") == {}
    assert gui._scan2d_role_label_widgets("unknown") == {}
    assert gui._scan2d_role_hint_widgets("unknown") == {}
    assert gui._scan2d_control_axes("unknown") == ("x", "y")

    gui._load_scan2d_role_ranges("unknown")
    _close_gui(gui)


def test_gui_default_widget_panel_status_and_log_state(monkeypatch):
    gui = _new_gui(monkeypatch)

    assert gui.windowTitle().startswith("KohdaLab TRKR v")
    assert gui.status_label.text() == "idle"
    assert gui.connect_button.text() == "Connect All"
    assert gui.disconnect_button.text() == "Disconnect All"
    assert gui.start_button.isEnabled() is True
    assert gui.stop_button.isEnabled() is False
    assert gui.point_label.text() == "-"
    assert gui.eta_label.text() == "-"
    assert gui.log.toPlainText()

    gui.toggle_right_panel(True)
    assert gui.right_content.isVisible() is False
    assert gui.right_panel_toggle.text() == "<"
    gui.toggle_right_panel(False)
    assert gui.right_panel_toggle.text() == ">"
    _close_gui(gui)


def test_gui_refresh_scanner_choices_applies_conexagap_axis_ranges(monkeypatch):
    gui = _new_gui(monkeypatch)
    hints: list[bool] = []
    scales: list[str] = []
    gui.x_controller_combo.setCurrentText("CONEXAGAP")
    gui.y_controller_combo.setCurrentText("CONEXAGAP")
    monkeypatch.setattr(
        gui, "_actuators_for_controller", lambda _controller: ["AG-M100D"]
    )
    monkeypatch.setattr(
        gui, "_refresh_scanner_scale_label", lambda axis: scales.append(axis)
    )
    monkeypatch.setattr(gui, "_refresh_scan_limit_hints", lambda: hints.append(True))

    gui.refresh_scanner_choices("x")

    assert gui.x_actuator_combo.currentText() == "AG-M100D"
    assert (gui.x_axis_spin.minimum(), gui.x_axis_spin.maximum()) == (1, 2)
    assert (gui.y_axis_spin.minimum(), gui.y_axis_spin.maximum()) == (1, 8)
    assert gui.x_axis_spin.value() == 1
    assert gui.y_axis_spin.value() == 2
    assert scales == ["x"]
    assert hints == [True]
    _close_gui(gui)


def test_gui_panel_sizing_handles_uninitialized_and_both_toggle_states(monkeypatch):
    gui = _new_gui(monkeypatch)
    left_panel = gui.left_panel
    del gui.left_panel
    gui._apply_panel_sizes()
    gui.left_panel = left_panel

    gui.resize(2000, 1200)
    gui.right_panel_toggle.setChecked(False)
    gui._apply_panel_sizes()
    assert gui.left_panel.width() == 400
    assert gui.center_top_widget.height() == 240
    assert gui.log.height() == 240
    assert gui.right_panel.width() == 400

    gui.right_panel_toggle.setChecked(True)
    gui._apply_panel_sizes()
    assert gui.right_panel.width() == gui.right_panel_toggle.sizeHint().width() + 8
    _close_gui(gui)


def test_gui_resource_refresh_ignores_reentry_and_malformed_results(monkeypatch):
    gui = _new_gui(monkeypatch)
    sentinel_thread = object()
    gui.resource_thread = sentinel_thread
    monkeypatch.setattr(
        gui_module, "ResourceListWorker", lambda: pytest.fail("worker recreated")
    )

    gui.refresh_lockin_resources()

    assert gui.resource_thread is sentinel_thread
    gui.resource_thread = None
    for combo, current in (
        (gui.lockin_resource_combo, "stale-visa"),
        (gui.t_port_combo, "stale-port"),
        (gui.x_port_combo, "stale-port"),
        (gui.y_port_combo, "stale-port"),
    ):
        combo.clear()
        combo.addItem(current)
        combo.setCurrentText(current)
    synced: list[str] = []
    monkeypatch.setattr(gui, "sync_conexagap_ports", synced.append)

    gui.handle_resource_list_ready(("not", "a", "list"), "not-a-list")

    assert gui.lockin_resource_combo.currentText() == "stale-visa"
    assert gui.lockin_resource_combo.count() == 1
    for combo in (gui.t_port_combo, gui.x_port_combo, gui.y_port_combo):
        assert combo.currentText() == "stale-port"
        assert combo.count() == 1
    assert synced == ["x"]
    _close_gui(gui)


def test_gui_store_current_output_settings_uses_selected_measurement(monkeypatch):
    gui = _new_gui(monkeypatch)
    stored: list[str] = []
    monkeypatch.setattr(gui, "_measurement_name", lambda: "srkr_2d")
    monkeypatch.setattr(gui, "_store_output_settings", stored.append)

    gui._store_current_output_settings()

    assert stored == ["srkr_2d"]
    _close_gui(gui)


def test_gui_delay_hint_ignores_cached_microstep_failure(monkeypatch):
    gui = _new_gui(monkeypatch)

    class BrokenStage:
        axis = 2

        def get_cached_microstep_division(self, *, axis):
            raise RuntimeError(f"axis {axis} unavailable")

    class DelayStage:
        _stage = BrokenStage()

    class Session:
        delay_stages = {"t": DelayStage()}

    class Experiment:
        session = Session()

    class Limits:
        minimum = -1.0
        maximum = 2.0
        minimum_step = 0.5

    calls: list[object] = []
    gui.experiment = Experiment()

    def fake_limits(**kwargs):
        calls.append(kwargs["microstep_division"])
        return Limits()

    monkeypatch.setattr(gui_module, "delay_stage_scan_limits", fake_limits)

    assert gui._delay_stage_hint_values() == (-1.0, 2.0, 0.5)
    assert calls == [None]
    _close_gui(gui)


def test_gui_scan2d_role_hints_fall_back_from_invalid_axis(monkeypatch):
    gui = _new_gui(monkeypatch)
    gui.scan2d_role_axes["strkr"] = {
        "fast_axis": "invalid",
        "slow_axis": "t",
    }
    axes: list[str] = []

    def hint_values(axis):
        axes.append(axis)
        return 1.0, 2.0, 0.25

    monkeypatch.setattr(gui, "_axis_hint_values", hint_values)

    gui._refresh_scan2d_role_hints("strkr")

    assert axes == ["x", "t"]
    assert gui.strkr_role_hints["fast_axis"]["min"].text() == "min > 1 um"
    assert gui.strkr_role_hints["slow_axis"]["step"].text() == "step > 0.25 ps"
    _close_gui(gui)


@pytest.mark.parametrize("select_file", [False, True])
def test_gui_browse_config_applies_only_nonempty_selection(
    monkeypatch, tmp_path: Path, select_file: bool
):
    gui = _new_gui(monkeypatch)
    original = str(tmp_path / "original-config.json")
    selected = str(tmp_path / "selected-config.json") if select_file else ""
    gui.config_path.setText(original)
    starts: list[str] = []

    def choose(_parent, _title, start_dir, _filter):
        starts.append(start_dir)
        return selected, ""

    monkeypatch.setattr(gui_module.QtWidgets.QFileDialog, "getOpenFileName", choose)

    gui.browse_config()

    assert starts == [original]
    assert gui.config_path.text() == (selected or original)
    _close_gui(gui)


@pytest.mark.parametrize("index", [-1, 999])
def test_gui_attach_output_widget_rejects_out_of_range_tab(monkeypatch, index):
    gui = _new_gui(monkeypatch)
    parent_before = gui.output_run_widget.parent()

    gui._attach_output_run_to_tab(index)

    assert gui.output_run_widget.parent() is parent_before
    _close_gui(gui)


def test_gui_sync_scan2d_ranges_skips_roles_without_valid_mappings(monkeypatch):
    gui = _new_gui(monkeypatch)
    gui.scan2d_role_axes["strkr"] = {
        "fast_axis": "invalid",
        "slow_axis": "also-invalid",
    }
    original = {
        axis: {key: spin.value() for key, spin in ranges.items()}
        for axis, ranges in gui.strkr_range_spins.items()
    }
    for role in gui.strkr_role_spins.values():
        for spin in role.values():
            spin.setValue(spin.value() + 7.0)

    gui._sync_scan2d_role_values_to_axis_ranges("strkr")

    assert {
        axis: {key: spin.value() for key, spin in ranges.items()}
        for axis, ranges in gui.strkr_range_spins.items()
    } == original
    _close_gui(gui)


def test_gui_replace_combo_with_empty_current_does_not_restore_item(monkeypatch):
    gui = _new_gui(monkeypatch)
    combo = gui.lockin_resource_combo
    combo.clear()

    gui._replace_combo_preserving_current(combo, ["new-resource"])

    assert combo.count() == 1
    assert combo.currentText() == "new-resource"
    _close_gui(gui)


def test_gui_output_and_hint_helpers_are_safe_before_optional_widgets(monkeypatch):
    gui = _new_gui(monkeypatch)
    tabs = gui.measurement_tabs
    del gui.measurement_tabs
    stored: list[str] = []
    monkeypatch.setattr(gui, "_store_output_settings", stored.append)

    gui._store_current_output_settings()
    gui._refresh_scan2d_role_hints("unknown")

    assert stored == []
    gui.measurement_tabs = tabs
    _close_gui(gui)


def test_gui_load_config_without_experiment_skips_runtime_update(
    monkeypatch, tmp_path: Path
):
    gui = _new_gui(monkeypatch)
    path = tmp_path / "loaded.json"
    candidate = normalize_config({})
    gui.experiment = None
    gui.config_path.setText(str(path))
    monkeypatch.setattr(
        gui_module,
        "resolve_config_path",
        lambda _path: ConfigPathResolution(path, "explicit", []),
    )
    monkeypatch.setattr(gui_module, "load_config", lambda _path: candidate)
    monkeypatch.setattr(gui, "_load_config_into_fields", lambda _config: None)
    monkeypatch.setattr(
        gui,
        "_runtime_config",
        lambda: pytest.fail("runtime update attempted without experiment"),
    )
    recorded: list[Path] = []
    monkeypatch.setattr(gui_module, "write_last_config_path", recorded.append)

    gui.load_config_file()

    assert gui.config is candidate
    assert recorded == [path]
    _close_gui(gui)


def test_gui_save_config_without_experiment_skips_experiment_update(
    monkeypatch, tmp_path: Path
):
    gui = _new_gui(monkeypatch)
    path = tmp_path / "saved.json"
    runtime = normalize_config({})
    gui.experiment = None
    gui.config_path.setText(str(path))
    monkeypatch.setattr(gui, "_runtime_config", lambda: runtime)
    saved: list[tuple[dict[str, Any], Path, bool]] = []
    monkeypatch.setattr(
        gui_module,
        "save_config",
        lambda config, output, *, validate: saved.append((config, output, validate)),
    )
    recorded: list[Path] = []
    monkeypatch.setattr(gui_module, "write_last_config_path", recorded.append)

    gui.save_config_file()

    assert saved == [(runtime, path, True)]
    assert gui.config == runtime
    assert recorded == [path]
    _close_gui(gui)


def test_gui_live_worker_reuse_and_queued_invocation(monkeypatch):
    gui = _new_gui(monkeypatch)
    worker = object()
    experiment = object()
    gui.live_worker = worker  # type: ignore[assignment]
    monkeypatch.setattr(gui, "_ensure_experiment", lambda: experiment)
    invoked: list[tuple[object, str, object]] = []
    monkeypatch.setattr(
        gui_module.QtCore.QMetaObject,
        "invokeMethod",
        lambda target, slot, connection: (
            invoked.append((target, slot, connection)) or True
        ),
    )

    assert gui._ensure_live_worker() is worker
    gui._invoke_live_worker("request_full_status")

    assert invoked == [
        (
            worker,
            "request_full_status",
            gui_module.QtCore.Qt.ConnectionType.QueuedConnection,
        )
    ]
    gui.live_worker = None
    _close_gui(gui)


@pytest.mark.parametrize(
    ("measurement", "expected_fast", "expected_slow"),
    [
        ("strkr", "t", "x"),
        ("srkr_2d", "x", "y"),
        ("srkr", None, None),
    ],
)
@pytest.mark.parametrize("reuse_experiment", [False, True])
def test_gui_missing_required_devices_builds_axis_context(
    monkeypatch,
    measurement,
    expected_fast,
    expected_slow,
    reuse_experiment,
):
    gui = _new_gui(monkeypatch)
    runtime = {"runtime": "config"}
    calls: list[dict[str, object]] = []

    class FakeExperiment:
        def __init__(self, config=None, auto_connect=None):
            self.config = config
            self.auto_connect = auto_connect

        def missing_devices(self, name, **kwargs):
            calls.append({"measurement": name, **kwargs})
            return ["missing-device"]

    existing = FakeExperiment({"old": True})
    gui.experiment = existing if reuse_experiment else None  # type: ignore[assignment]
    gui.strkr_fast_axis_combo.setCurrentText("t")
    gui.strkr_slow_axis_combo.setCurrentText("x")
    gui.srkr_2d_fast_axis_combo.setCurrentText("x")
    gui.srkr_2d_slow_axis_combo.setCurrentText("y")
    monkeypatch.setattr(gui, "_runtime_config", lambda: runtime)
    monkeypatch.setattr(gui_module, "Experiment", FakeExperiment)

    assert gui._missing_required_devices(measurement, "y") == ["missing-device"]
    assert calls == [
        {
            "measurement": measurement,
            "axis": "y",
            "fast_axis": expected_fast,
            "slow_axis": expected_slow,
        }
    ]
    if reuse_experiment:
        assert existing.config is runtime
    _close_gui(gui)


@pytest.mark.parametrize("with_pending_spin", [False, True])
def test_gui_lockin_wait_completion_updates_optional_pending_spin(
    monkeypatch, with_pending_spin
):
    gui = _new_gui(monkeypatch)
    cleaned: list[bool] = []
    monkeypatch.setattr(gui, "cleanup_device_command", lambda: cleaned.append(True))
    spin = gui.trkr_wait_spin
    spin.setValue(0.0)
    gui.pending_wait_spin = spin if with_pending_spin else None

    gui.handle_device_command_finished({"command": "lockin_wait_time", "wait_s": 1.25})

    assert spin.value() == (1.25 if with_pending_spin else 0.0)
    assert gui.status_label.text() == "lock-in wait read"
    assert cleaned == [True]
    _close_gui(gui)


@pytest.mark.parametrize("has_experiment", [False, True])
def test_gui_initialized_position_refresh_is_optional_and_failure_is_logged(
    monkeypatch, has_experiment
):
    gui = _new_gui(monkeypatch)
    gui.experiment = object() if has_experiment else None  # type: ignore[assignment]
    calls: list[bool] = []

    def refresh():
        calls.append(True)
        raise RuntimeError("live refresh unavailable")

    monkeypatch.setattr(gui, "_request_full_live_status", refresh)

    gui.handle_device_initialized({"kind": "delay_stage", "axis": "t"})

    assert calls == ([True] if has_experiment else [])
    if has_experiment:
        assert "Could not refresh initialized position: live refresh unavailable" in (
            gui.log.toPlainText()
        )
    _close_gui(gui)


def test_gui_partial_live_settings_signal_and_overload_branches(monkeypatch):
    gui = _new_gui(monkeypatch)
    refreshes: list[bool] = []
    monkeypatch.setattr(gui, "_refresh_plot_labels", lambda: refreshes.append(True))

    gui._apply_lockin_settings({"Time Constant": 0.25})
    assert gui.tc_label.text() == gui_module.time_constant_display(0.25)
    assert refreshes == [True]

    original = {key: label.text() for key, label in gui.signal_labels.items()}
    gui._apply_signal({})
    assert {key: label.text() for key, label in gui.signal_labels.items()} == original

    gui._voltage_scale = 1000.0
    gui._voltage_unit = "mV"
    gui._apply_signal({"X_V": 0.001, "Y_V": 0.002, "R_V": 0.003, "Theta_deg": 45})
    assert gui.signal_labels["X"].text() == "1.000 mV"
    assert gui.signal_labels["Y"].text() == "2.000 mV"
    assert gui.signal_labels["R"].text() == "3.000 mV"
    assert gui.signal_labels["Theta"].text() == "45.000 deg"

    gui.overload_label.setText("unchanged")
    gui._apply_overload_status(None)
    assert gui.overload_label.text() == "unchanged"
    gui._apply_overload_status("malformed")
    assert gui.overload_label.text() == "?"
    gui._apply_overload_status({"_error": "unavailable"})
    assert gui.overload_label.text() == "?"
    _close_gui(gui)


def test_gui_live_status_skips_empty_optional_payloads(monkeypatch):
    gui = _new_gui(monkeypatch)
    applied: list[tuple[str, object]] = []
    monkeypatch.setattr(
        gui,
        "_update_position_from_position",
        lambda value: applied.append(("p", value)),
    )
    monkeypatch.setattr(
        gui, "_apply_lockin_settings", lambda value: applied.append(("s", value))
    )
    monkeypatch.setattr(
        gui, "_apply_signal", lambda value: applied.append(("v", value))
    )
    monkeypatch.setattr(
        gui, "_apply_overload_status", lambda value: applied.append(("o", value))
    )
    position = Position(t_ps=1.0)

    class EmptyOptionalStatus:
        signal: dict[str, object] = {}
        lockin_settings: dict[str, object] = {}
        lockin_overload = {"overload": False}

        def __init__(self, position):
            self.position = position

    gui._apply_live_status(EmptyOptionalStatus(position))  # type: ignore[arg-type]

    assert applied == [("p", position), ("o", {"overload": False})]
    _close_gui(gui)


@pytest.mark.parametrize(
    ("guard", "expected_warning"),
    [
        ("duplicate", None),
        ("cooldown", None),
        ("device", "Wait for device initialization to finish first."),
        ("measurement", "Stop the running measurement for this axis first."),
    ],
)
def test_gui_move_absolute_guard_branches(monkeypatch, guard, expected_warning):
    gui = _new_gui(monkeypatch)
    warnings: list[tuple[str, str]] = []
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )
    monkeypatch.setattr(
        gui, "_ensure_experiment", lambda: pytest.fail("guard did not stop move")
    )
    if guard == "duplicate":
        gui.move_thread = object()  # type: ignore[assignment]
    elif guard == "cooldown":
        gui._move_block_until = gui_module.time.perf_counter() + 60.0
    elif guard == "device":
        gui.device_command_active = True
    else:
        gui.measurement_thread = object()  # type: ignore[assignment]
        gui.running_motion_axes = {"x"}

    gui.move_absolute("x")

    assert warnings == (
        [("Move Error", expected_warning)] if expected_warning is not None else []
    )
    gui.move_thread = None
    gui.measurement_thread = None
    _close_gui(gui)


def test_gui_motion_block_and_cleanup_cover_valid_and_inactive_axes(monkeypatch):
    gui = _new_gui(monkeypatch)
    assert gui._motion_axis_is_blocked_by_measurement("x") is False
    gui.measurement_thread = object()  # type: ignore[assignment]
    gui.running_motion_axes = {"x"}
    assert gui._motion_axis_is_blocked_by_measurement("x") is True
    assert gui._motion_axis_is_blocked_by_measurement("y") is False

    restored: list[tuple[str, float]] = []
    running: list[bool] = []
    monkeypatch.setattr(
        gui, "_set_position_value", lambda axis, value: restored.append((axis, value))
    )
    monkeypatch.setattr(gui, "_set_move_running", running.append)
    gui._current_position_values["x"] = 12.0
    gui.running_move_axis = "x"
    gui.move_thread = object()  # type: ignore[assignment]
    gui.move_worker = object()  # type: ignore[assignment]

    gui.cleanup_move_thread()

    assert restored == [("x", 12.0)]
    assert running == [False]
    assert gui.move_thread is None
    assert gui.move_worker is None
    assert gui.running_move_axis is None

    gui.running_move_axis = None
    gui.cleanup_move_thread()
    assert restored == [("x", 12.0)]
    assert running == [False, False]
    gui.measurement_thread = None
    _close_gui(gui)


def test_gui_noop_live_measurement_and_move_exit_branches(monkeypatch):
    gui = _new_gui(monkeypatch)
    plot_refreshes: list[bool] = []
    restores: list[bool] = []
    hint_refreshes: list[bool] = []
    position_updates: list[tuple[str, str]] = []
    monkeypatch.setattr(
        gui, "_refresh_plot_labels", lambda: plot_refreshes.append(True)
    )
    monkeypatch.setattr(
        gui,
        "_restore_running_motion_axis_values",
        lambda: restores.append(True),
    )
    monkeypatch.setattr(
        gui,
        "_set_position_value",
        lambda axis, value: position_updates.append((axis, str(value))),
    )
    monkeypatch.setattr(
        gui, "_refresh_scan_limit_hints", lambda: hint_refreshes.append(True)
    )

    gui._apply_lockin_settings({})
    gui.handle_measurement_status("unrecognized measurement status")
    gui._set_measurement_axis_status("invalid", "ignored")
    before_log = gui.log.toPlainText()
    gui.handle_move_finished({"axis": "invalid", "value": 99.0})

    assert plot_refreshes == [True]
    assert restores == [True]
    assert position_updates == []
    assert hint_refreshes == [True]
    assert gui.status_label.text() == "move complete"
    assert gui.log.toPlainText() == before_log
    _close_gui(gui)
