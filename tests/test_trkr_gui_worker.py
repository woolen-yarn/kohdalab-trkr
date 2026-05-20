from __future__ import annotations

import numpy as np

from kohdalab.api.models import LiveStatus, Position
from kohdalab.api.status import (
    STATUS_MOVING_DELAY_STAGE,
    STATUS_SLOW_AXIS_READY,
    STATUS_STOPPED,
    STATUS_WAITING,
    moving_scanner_status,
)
import kohdalab.apps.trkr_gui as gui_module
from kohdalab.apps.trkr_gui import (
    DeviceCommandWorker,
    LiveStatusWorker,
    MeasurementWorker,
    MoveWorker,
    ResourceListWorker,
    TRKRGui,
    _format_duration,
    _normalized_by_abs_max,
)


class FakeExperiment:
    def __init__(self):
        self.calls: list[tuple[str, dict]] = []
        self.lockins = {"main": object()}

    def run_signal_monitor(self, **kwargs):
        self.calls.append(("signal_monitor", kwargs))
        return [{"mode": "signal_monitor"}]

    def run_trkr(self, **kwargs):
        self.calls.append(("trkr", kwargs))
        return [{"mode": "trkr"}]

    def run_srkr(self, **kwargs):
        self.calls.append(("srkr", kwargs))
        return [{"mode": "srkr"}]

    def run_strkr(self, **kwargs):
        self.calls.append(("strkr", kwargs))
        return [{"mode": "strkr"}]

    def run_srkr_2d(self, **kwargs):
        self.calls.append(("srkr_2d", kwargs))
        return [{"mode": "srkr_2d"}]

    def initialize_delay_stage(self, ref, *, on_status=None):
        self.calls.append(("initialize_delay_stage", {"ref": ref}))
        if on_status is not None:
            on_status("delay_stage initializing")
        return {"axis": "t"}

    def initialize_scanner(self, axis, ref=None, *, on_status=None):
        self.calls.append(("initialize_scanner", {"axis": axis, "ref": ref}))
        if on_status is not None:
            on_status(f"{axis} scanner initializing")
        return {"axis": axis}

    def connect_all(self):
        self.calls.append(("connect_all", {}))

    def connect_device(self, ref):
        self.calls.append(("connect_device", {"ref": ref}))

    def disconnect_all(self):
        self.calls.append(("disconnect_all", {}))

    def disconnect_device(self, ref):
        self.calls.append(("disconnect_device", {"ref": ref}))

    def connected_devices(self):
        return {"lockin.main": bool(self.lockins)}

    def lockin_wait_time(self, ref, *, multiplier=4.0):
        self.calls.append(("lockin_wait_time", {"ref": ref, "multiplier": multiplier}))
        return 1.25

    def set_lockin_settings(self, ref, **settings):
        self.calls.append(("set_lockin_settings", {"ref": ref, **settings}))
        applied = {}
        if "sensitivity" in settings:
            applied["Sensitivity"] = settings["sensitivity"]
        if "time_constant" in settings:
            applied["Time Constant"] = settings["time_constant"]
        return applied

    def move_delay_stage(self, value, *, coordinate="measurement", on_status=None, on_position=None):
        self.calls.append(("move_delay_stage", {"value": value, "coordinate": coordinate}))
        if on_status is not None:
            on_status(STATUS_MOVING_DELAY_STAGE)
        if on_position is not None:
            on_position({"t_ps": value - 1.0})
        return {"axis": "t", "value": value}

    def move_scanner(self, axis, value, *, coordinate="measurement", on_status=None, on_position=None):
        self.calls.append(("move_scanner", {"axis": axis, "value": value, "coordinate": coordinate}))
        if on_status is not None:
            on_status(moving_scanner_status(axis))
        if on_position is not None:
            on_position({f"{axis}_um": value - 0.5})
        return {"axis": axis, "value": value}

    def read_live_status(self):
        self.calls.append(("read_live_status", {}))
        return LiveStatus(
            position=Position(t_ps=1.0),
            signal={"X": 1.0},
            lockin_settings={"sensitivity_v": 1.0},
            lockin_overload={"overload": False},
        )

    def read_lockin_settings(self, ref):
        self.calls.append(("read_lockin_settings", {"ref": ref}))
        return {"sensitivity_v": 1.0}

    def read_lockin_signal(self, ref):
        self.calls.append(("read_lockin_signal", {"ref": ref}))
        return {"X": 1.0}

    def read_lockin_overload(self, ref):
        self.calls.append(("read_lockin_overload", {"ref": ref}))
        return {"overload": False}


def _capture_finished(worker: MeasurementWorker) -> list[object]:
    finished: list[object] = []
    worker.finished.connect(finished.append)
    return finished


def _capture_text_signal(signal) -> list[str]:
    values: list[str] = []
    signal.connect(values.append)
    return values


def _capture_signal_args(signal) -> list[tuple[object, ...]]:
    values: list[tuple[object, ...]] = []
    signal.connect(lambda *args: values.append(args))
    return values


def test_worker_uses_supplied_experiment_for_signal_monitor():
    experiment = FakeExperiment()
    worker = MeasurementWorker(
        experiment=experiment,
        measurement="signal_monitor",
        output_path="signal.csv",
        interval_s=0.5,
        n_points=3,
    )
    finished = _capture_finished(worker)

    worker.run()

    assert finished == [[{"mode": "signal_monitor"}]]
    name, kwargs = experiment.calls[0]
    assert name == "signal_monitor"
    assert kwargs["output"] == "signal.csv"
    assert kwargs["interval_s"] == 0.5
    assert kwargs["n_points"] == 3
    assert kwargs["should_continue"]() is True


def test_worker_uses_supplied_experiment_for_trkr():
    experiment = FakeExperiment()
    plan = object()
    worker = MeasurementWorker(
        experiment=experiment,
        measurement="trkr",
        output_path="trkr.csv",
        scan_plan=plan,
        wait_s=2.0,
        return_to_zero=True,
    )
    finished = _capture_finished(worker)

    worker.run()

    assert finished == [[{"mode": "trkr"}]]
    name, kwargs = experiment.calls[0]
    assert name == "trkr"
    assert kwargs["plan"] is plan
    assert kwargs["output"] == "trkr.csv"
    assert kwargs["wait_s"] == 2.0
    assert kwargs["return_to_zero"] is True


def test_worker_uses_supplied_experiment_for_srkr():
    experiment = FakeExperiment()
    plan = object()
    worker = MeasurementWorker(
        experiment=experiment,
        measurement="srkr",
        output_path="srkr.csv",
        scan_plan=plan,
        axis="y",
        wait_s=3.0,
        return_to_zero=False,
    )
    finished = _capture_finished(worker)

    worker.run()

    assert finished == [[{"mode": "srkr"}]]
    name, kwargs = experiment.calls[0]
    assert name == "srkr"
    assert kwargs["axis"] == "y"
    assert kwargs["plan"] is plan
    assert kwargs["output"] == "srkr.csv"
    assert kwargs["wait_s"] == 3.0
    assert kwargs["return_to_zero"] is False


def test_worker_uses_supplied_experiment_for_strkr():
    experiment = FakeExperiment()
    plan = object()
    worker = MeasurementWorker(
        experiment=experiment,
        measurement="strkr",
        output_path="strkr.csv",
        scan_plan=plan,
        wait_s=2.5,
    )
    finished = _capture_finished(worker)

    worker.run()

    assert finished == [[{"mode": "strkr"}]]
    name, kwargs = experiment.calls[0]
    assert name == "strkr"
    assert kwargs["plan"] is plan
    assert kwargs["output"] == "strkr.csv"
    assert kwargs["wait_s"] == 2.5


def test_worker_uses_supplied_experiment_for_srkr_2d():
    experiment = FakeExperiment()
    plan = object()
    worker = MeasurementWorker(
        experiment=experiment,
        measurement="srkr_2d",
        output_path="srkr_2d.csv",
        scan_plan=plan,
        wait_s=3.5,
    )
    finished = _capture_finished(worker)

    worker.run()

    assert finished == [[{"mode": "srkr_2d"}]]
    name, kwargs = experiment.calls[0]
    assert name == "srkr_2d"
    assert kwargs["plan"] is plan
    assert kwargs["output"] == "srkr_2d.csv"
    assert kwargs["wait_s"] == 3.5


def test_worker_stop_updates_should_continue_callback():
    experiment = FakeExperiment()
    worker = MeasurementWorker(
        experiment=experiment,
        measurement="signal_monitor",
        output_path="signal.csv",
    )

    worker.stop()
    worker.run()

    kwargs = experiment.calls[0][1]
    assert kwargs["should_continue"]() is False


def test_live_status_worker_reads_full_status_and_overload():
    experiment = FakeExperiment()
    worker = LiveStatusWorker(experiment=experiment)
    statuses = _capture_signal_args(worker.live_status_ready)

    worker.read_full()

    assert [call[0] for call in experiment.calls] == ["read_live_status"]
    status, overload = statuses[0]
    assert status.position.t_ps == 1.0
    assert overload == {"overload": False}


def test_live_status_worker_reads_lockin_only_status():
    experiment = FakeExperiment()
    worker = LiveStatusWorker(experiment=experiment)
    statuses = _capture_signal_args(worker.lockin_status_ready)

    worker.read_lockin()

    assert experiment.calls == [
        ("read_lockin_settings", {"ref": "lockin.main"}),
        ("read_lockin_signal", {"ref": "lockin.main"}),
        ("read_lockin_overload", {"ref": "lockin.main"}),
    ]
    assert statuses == [({"sensitivity_v": 1.0}, {"X": 1.0}, {"overload": False})]


def test_resource_list_worker_reads_visa_and_serial_resources(monkeypatch):
    class Port:
        def __init__(self, device):
            self.device = device

    monkeypatch.setattr(gui_module, "list_visa_resources", lambda: ["GPIB0::12::INSTR"])
    monkeypatch.setattr(gui_module.list_ports, "comports", lambda: [Port("COM9"), Port("COM3")])
    worker = ResourceListWorker()
    resources = _capture_signal_args(worker.resources_ready)
    finished = []
    worker.finished.connect(lambda: finished.append(True))

    worker.run()

    assert resources == [(["GPIB0::12::INSTR"], ["COM3", "COM9"])]
    assert finished == [True]


def test_device_command_worker_connects_one_device():
    experiment = FakeExperiment()
    worker = DeviceCommandWorker(experiment=experiment, command="connect_device", kind="scanner", key="x")
    finished = _capture_finished(worker)
    statuses = _capture_text_signal(worker.status_changed)

    worker.run()

    assert experiment.calls == [("connect_device", {"ref": "scanner.x"})]
    assert statuses == ["connecting scanner.x"]
    assert finished == [{"command": "connect_device", "ref": "scanner.x"}]


def test_device_command_worker_disconnects_all_devices():
    experiment = FakeExperiment()
    worker = DeviceCommandWorker(experiment=experiment, command="disconnect_all")
    finished = _capture_finished(worker)
    statuses = _capture_text_signal(worker.status_changed)

    worker.run()

    assert experiment.calls == [("disconnect_all", {})]
    assert statuses == ["disconnecting all"]
    assert finished == [{"command": "disconnect_all"}]


def test_device_command_worker_runs_delay_stage_initialize():
    experiment = FakeExperiment()
    worker = DeviceCommandWorker(experiment=experiment, command="initialize_delay_stage")
    finished = _capture_finished(worker)
    statuses = _capture_text_signal(worker.status_changed)

    worker.run()

    assert experiment.calls == [("initialize_delay_stage", {"ref": "delay_stage.t"})]
    assert statuses == ["delay stage initializing", "delay_stage initializing"]
    assert finished == [{"command": "initialize_delay_stage", "kind": "delay_stage", "axis": "t", "info": {"axis": "t"}}]


def test_device_command_worker_runs_scanner_initialize():
    experiment = FakeExperiment()
    worker = DeviceCommandWorker(experiment=experiment, command="initialize_scanner", axis="y")
    finished = _capture_finished(worker)
    statuses = _capture_text_signal(worker.status_changed)

    worker.run()

    assert experiment.calls == [("initialize_scanner", {"axis": "y", "ref": "scanner.y"})]
    assert statuses == ["scanner y initializing", "y scanner initializing"]
    assert finished == [{"command": "initialize_scanner", "kind": "scanner", "axis": "y", "info": {"axis": "y"}}]


def test_device_command_worker_reads_lockin_wait_time():
    experiment = FakeExperiment()
    worker = DeviceCommandWorker(experiment=experiment, command="lockin_wait_time", ref="lockin.main", multiplier=4.0)
    finished = _capture_finished(worker)

    worker.run()

    assert experiment.calls == [("lockin_wait_time", {"ref": "lockin.main", "multiplier": 4.0})]
    assert finished == [{"command": "lockin_wait_time", "ref": "lockin.main", "wait_s": 1.25}]


def test_device_command_worker_applies_lockin_settings():
    experiment = FakeExperiment()
    worker = DeviceCommandWorker(
        experiment=experiment,
        command="set_lockin_settings",
        ref="lockin.main",
        settings={"sensitivity": 1e-3, "time_constant": 0.3},
    )
    finished = _capture_finished(worker)

    worker.run()

    assert experiment.calls == [
        ("set_lockin_settings", {"ref": "lockin.main", "sensitivity": 1e-3, "time_constant": 0.3})
    ]
    assert finished == [
        {
            "command": "set_lockin_settings",
            "ref": "lockin.main",
            "settings": {"Sensitivity": 1e-3, "Time Constant": 0.3},
        }
    ]


def test_move_worker_runs_delay_stage_move():
    experiment = FakeExperiment()
    worker = MoveWorker(experiment=experiment, axis="t", value=12.5)
    finished = _capture_finished(worker)
    statuses = _capture_text_signal(worker.status_changed)
    positions = []
    worker.position_changed.connect(positions.append)

    worker.run()

    assert experiment.calls == [("move_delay_stage", {"value": 12.5, "coordinate": "measurement"})]
    assert statuses == [STATUS_MOVING_DELAY_STAGE]
    assert positions == [{"t_ps": 11.5}]
    assert finished == [
        {
            "axis": "t",
            "value": 12.5,
            "coordinate": "measurement",
            "position": {"axis": "t", "value": 12.5},
        }
    ]


def test_move_worker_runs_scanner_move():
    experiment = FakeExperiment()
    worker = MoveWorker(experiment=experiment, axis="x", value=4.0, coordinate="interface")
    finished = _capture_finished(worker)
    statuses = _capture_text_signal(worker.status_changed)
    positions = []
    worker.position_changed.connect(positions.append)

    worker.run()

    assert experiment.calls == [("move_scanner", {"axis": "x", "value": 4.0, "coordinate": "interface"})]
    assert statuses == [moving_scanner_status("x")]
    assert positions == [{"x_um": 3.5}]
    assert finished == [
        {
            "axis": "x",
            "value": 4.0,
            "coordinate": "interface",
            "position": {"axis": "x", "value": 4.0},
        }
    ]


def test_gui_position_update_can_preserve_missing_axes():
    class DummyGui:
        def __init__(self):
            self.values = {"t": 1.0, "x": 2.0, "y": 3.0}

        def _set_position_value(self, axis, value):
            self.values[axis] = value

    gui = DummyGui()

    TRKRGui._update_position_from_position(gui, Position(x_um=4.0), preserve_missing=True)

    assert gui.values == {"t": 1.0, "x": 4.0, "y": 3.0}


def test_gui_switches_to_2d_scan_tabs(monkeypatch):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)

    app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()

    gui.measurement_tabs.setCurrentIndex(3)
    app.processEvents()
    assert gui._measurement_name() == "strkr"
    assert gui.plot_stack.currentWidget() is gui.scan2d_plot_widget
    assert not hasattr(gui, "strkr_return_fast_check")
    assert not hasattr(gui, "strkr_return_slow_check")
    assert gui.strkr_role_labels["fast_axis"]["min"].text() == "min cor (ps)"
    assert gui.strkr_role_labels["slow_axis"]["min"].text() == "min cor (um)"
    gui.strkr_fast_axis_combo.setCurrentText("x")
    app.processEvents()
    assert gui.strkr_slow_axis_combo.currentText() == "t"
    assert gui.strkr_role_labels["fast_axis"]["min"].text() == "min cor (um)"
    gui.strkr_role_spins["fast_axis"]["min"].setValue(-12.0)
    gui.strkr_fast_axis_combo.setCurrentText("y")
    app.processEvents()
    assert gui.strkr_range_spins["x"]["min"].value() == -12.0
    assert gui.strkr_role_labels["fast_axis"]["min"].text() == "min cor (um)"

    gui.measurement_tabs.setCurrentIndex(4)
    app.processEvents()
    assert gui._measurement_name() == "srkr_2d"
    assert gui.plot_stack.currentWidget() is gui.scan2d_plot_widget
    gui.srkr_2d_fast_axis_combo.setCurrentText("y")
    app.processEvents()
    assert gui.srkr_2d_slow_axis_combo.currentText() == "x"

    gui._shutdown_complete = True
    gui.close()


def test_gui_measurement_motion_status_updates_axis_labels(monkeypatch):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)

    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    gui.running_motion_axes = {"t", "x"}
    gui._current_position_values = {"t": 1.0, "x": 2.0, "y": 3.0}

    gui.handle_measurement_status(moving_scanner_status("x"))
    assert gui.position_labels["x"].text() == "Moving..."
    assert gui.position_labels["t"].text() == "1.000"

    gui.handle_measurement_status(STATUS_WAITING)
    assert gui.position_labels["x"].text() == "2.000"
    assert gui.position_labels["t"].text() == "1.000"

    gui.handle_measurement_status(STATUS_STOPPED)
    assert gui.position_labels["x"].text() == "2.000"
    assert gui.position_labels["t"].text() == "1.000"

    gui._shutdown_complete = True
    gui.close()


def test_gui_scan2d_eta_starts_after_next_slow_move(monkeypatch):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    times = iter([100.0, 115.0])
    monkeypatch.setattr(gui_module.time, "perf_counter", lambda: next(times))

    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    gui.running_measurement = "strkr"
    gui._scan2d_fast_point_count = 2
    gui._scan2d_slow_point_count = 3

    gui.handle_measurement_status(STATUS_SLOW_AXIS_READY)
    assert gui.eta_text_by_mode["strkr"] == "-"

    gui.rows_by_mode["strkr"] = [{}, {}]
    gui.handle_measurement_status(STATUS_SLOW_AXIS_READY)
    assert gui.eta_text_by_mode["strkr"] == "30s"

    gui._shutdown_complete = True
    gui.close()


def test_scan2d_heatmap_normalizes_by_abs_max():
    image = np.array([[1.0, -2.0], [0.0, np.nan]])

    normalized = _normalized_by_abs_max(image)

    assert np.nanmax(normalized) == 0.5
    assert np.nanmin(normalized) == -1.0


def test_format_duration_for_eta():
    assert _format_duration(None) == "-"
    assert _format_duration(9.4) == "9s"
    assert _format_duration(85.0) == "1:25"
    assert _format_duration(3661.0) == "1:01:01"
