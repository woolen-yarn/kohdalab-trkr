from __future__ import annotations

import io

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
import kohdalab.apps.trkr_gui_workers as worker_module
from kohdalab import __version__
from kohdalab.api.config import ConfigPathResolution
from kohdalab.apps.trkr_gui import (
    DeviceCommandWorker,
    GuiLogStream,
    LiveStatusWorker,
    MeasurementWorker,
    MoveWorker,
    ResourceListWorker,
    TRKRGui,
    _axis_cor_key,
    _axis_raw_key,
    _axis_unit,
    _default_axis_range,
    _fmt_bound,
    _format_duration,
    _format_value,
    _motion_axis_display_text,
    _normalized_by_abs_max,
    _unique_values,
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

    def move_delay_stage(
        self, value, *, coordinate="measurement", on_status=None, on_position=None
    ):
        self.calls.append(
            ("move_delay_stage", {"value": value, "coordinate": coordinate})
        )
        if on_status is not None:
            on_status(STATUS_MOVING_DELAY_STAGE)
        if on_position is not None:
            on_position({"t_ps": value - 1.0})
        return {"axis": "t", "value": value}

    def move_scanner(
        self, axis, value, *, coordinate="measurement", on_status=None, on_position=None
    ):
        self.calls.append(
            ("move_scanner", {"axis": axis, "value": value, "coordinate": coordinate})
        )
        if on_status is not None:
            on_status(moving_scanner_status(axis))
        if on_position is not None:
            on_position({f"{axis}_um": value - 0.5})
        return {"axis": axis, "value": value}

    def read_live_status(self, *, skip_busy_positions=False):
        self.calls.append(
            (
                "read_live_status",
                {"skip_busy_positions": skip_busy_positions},
            )
        )
        return LiveStatus(
            position=Position(t_ps=1.0),
            signal={"X": 1.0, "Y": 2.0, "R": 3.0, "Theta": 4.0},
            lockin_settings={
                "Sensitivity": 1.0,
                "Time Constant": 1.0,
                "Ref. Freq": 1000.0,
            },
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


def test_gui_log_stream_forwards_complete_nonempty_lines_and_preserves_output():
    stream = io.StringIO()
    log_stream = GuiLogStream(stream)
    lines = _capture_text_signal(log_stream.text_ready)

    log_stream.write("first")
    log_stream.write(" line\n\nsecond line\npartial")

    assert stream.getvalue() == "first line\n\nsecond line\npartial"
    assert lines == ["first line", "second line"]


def test_gui_log_stream_flush_emits_buffered_partial_line_once():
    stream = io.StringIO()
    log_stream = GuiLogStream(stream)
    lines = _capture_text_signal(log_stream.text_ready)

    log_stream.write("partial")
    log_stream.flush()
    log_stream.flush()

    assert stream.getvalue() == "partial"
    assert lines == ["partial"]


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


def test_worker_reports_measurement_failure_and_finishes_with_empty_rows():
    class FailingExperiment(FakeExperiment):
        def run_signal_monitor(self, **kwargs):
            raise RuntimeError("simulated acquisition failure")

    worker = MeasurementWorker(
        experiment=FailingExperiment(),
        measurement="signal_monitor",
        output_path="signal.csv",
    )
    errors = _capture_text_signal(worker.error_occurred)
    finished = _capture_finished(worker)

    worker.run()

    assert errors == ["simulated acquisition failure"]
    assert finished == [[]]


def test_worker_rejects_unsupported_measurement_and_finishes_with_empty_rows():
    worker = MeasurementWorker(
        experiment=FakeExperiment(),
        measurement="unknown",
        output_path="unknown.csv",
    )
    errors = _capture_text_signal(worker.error_occurred)
    finished = _capture_finished(worker)

    worker.run()

    assert errors == ["Unsupported measurement: unknown"]
    assert finished == [[]]


def test_live_status_worker_reads_full_status_and_overload():
    experiment = FakeExperiment()
    worker = LiveStatusWorker(experiment=experiment)
    statuses = _capture_signal_args(worker.live_status_ready)

    worker.read_full()

    assert experiment.calls == [("read_live_status", {"skip_busy_positions": True})]
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


def test_live_status_worker_uses_tracked_lockin_when_health_snapshot_is_false():
    class UnhealthySnapshotExperiment(FakeExperiment):
        def connected_devices(self):
            return {"lockin.main": False}

    experiment = UnhealthySnapshotExperiment()
    worker = LiveStatusWorker(experiment=experiment)
    statuses = _capture_signal_args(worker.lockin_status_ready)

    worker.read_lockin()

    assert experiment.calls == [
        ("read_lockin_settings", {"ref": "lockin.main"}),
        ("read_lockin_signal", {"ref": "lockin.main"}),
        ("read_lockin_overload", {"ref": "lockin.main"}),
    ]
    assert statuses == [({"sensitivity_v": 1.0}, {"X": 1.0}, {"overload": False})]


def test_live_status_worker_reports_full_status_failure_and_recovers_busy_state():
    class FailingExperiment(FakeExperiment):
        def read_live_status(self, *, skip_busy_positions=False):
            del skip_busy_positions
            raise RuntimeError("simulated live status failure")

    worker = LiveStatusWorker(experiment=FailingExperiment())
    errors = _capture_text_signal(worker.error_occurred)

    worker.read_full()
    worker.read_full()

    assert errors == ["simulated live status failure", "simulated live status failure"]
    assert worker._busy is False


def test_live_status_worker_busy_guard_skips_overlapping_reads():
    experiment = FakeExperiment()
    worker = LiveStatusWorker(experiment=experiment)
    full_statuses = _capture_signal_args(worker.live_status_ready)
    lockin_statuses = _capture_signal_args(worker.lockin_status_ready)
    worker._busy = True

    worker.read_full()
    worker.read_lockin()

    assert experiment.calls == []
    assert full_statuses == []
    assert lockin_statuses == []
    assert worker._busy is True


def test_live_status_worker_skips_lockin_read_when_none_is_connected():
    experiment = FakeExperiment()
    experiment.lockins.clear()
    worker = LiveStatusWorker(experiment=experiment)
    statuses = _capture_signal_args(worker.lockin_status_ready)

    worker.read_lockin()

    assert experiment.calls == []
    assert statuses == []
    assert worker._busy is False


def test_live_status_worker_reports_partial_results_when_lockin_reads_fail():
    class FailingExperiment(FakeExperiment):
        def read_lockin_settings(self, ref):
            super().read_lockin_settings(ref)
            raise OSError("settings unavailable")

        def read_lockin_signal(self, ref):
            super().read_lockin_signal(ref)
            raise OSError("signal unavailable")

        def read_lockin_overload(self, ref):
            super().read_lockin_overload(ref)
            raise OSError("overload unavailable")

    experiment = FailingExperiment()
    worker = LiveStatusWorker(experiment=experiment)
    statuses = _capture_signal_args(worker.lockin_status_ready)
    errors = _capture_text_signal(worker.error_occurred)

    worker.read_lockin()

    assert [name for name, _kwargs in experiment.calls] == [
        "read_lockin_settings",
        "read_lockin_signal",
        "read_lockin_overload",
    ]
    assert statuses == [(None, None, {"_error": True})]
    assert errors == []
    assert worker._busy is False


def test_live_status_worker_reports_unexpected_internal_failure_and_recovers_busy_state(
    monkeypatch,
):
    worker = LiveStatusWorker(experiment=FakeExperiment())
    errors = _capture_text_signal(worker.error_occurred)

    def fail_unexpectedly(_ref):
        raise RuntimeError("unexpected worker failure")

    monkeypatch.setattr(worker, "_read_lockin_settings", fail_unexpectedly)

    worker.read_lockin()

    assert errors == ["unexpected worker failure"]
    assert worker._busy is False


def test_resource_list_worker_reads_visa_and_serial_resources(monkeypatch):
    class Port:
        def __init__(self, device):
            self.device = device

    monkeypatch.setattr(
        worker_module, "list_visa_resources", lambda: ["GPIB0::12::INSTR"]
    )
    monkeypatch.setattr(
        worker_module.list_ports, "comports", lambda: [Port("COM9"), Port("COM3")]
    )
    worker = ResourceListWorker()
    resources = _capture_signal_args(worker.resources_ready)
    finished = []
    worker.finished.connect(lambda: finished.append(True))

    worker.run()

    assert resources == [(["GPIB0::12::INSTR"], ["COM3", "COM9"])]
    assert finished == [True]


def test_resource_list_worker_returns_partial_results_with_combined_errors(monkeypatch):
    monkeypatch.setattr(
        worker_module,
        "list_visa_resources",
        lambda: (_ for _ in ()).throw(RuntimeError("VISA unavailable")),
    )
    monkeypatch.setattr(
        worker_module.list_ports,
        "comports",
        lambda: (_ for _ in ()).throw(RuntimeError("serial unavailable")),
    )
    worker = ResourceListWorker()
    resources = _capture_signal_args(worker.resources_ready)
    errors = _capture_text_signal(worker.error_occurred)
    finished: list[bool] = []
    worker.finished.connect(lambda: finished.append(True))

    worker.run()

    assert resources == [([], [])]
    assert errors == [
        "lock-in resources: VISA unavailable; serial ports: serial unavailable"
    ]
    assert finished == [True]


def test_device_command_worker_connects_one_device():
    experiment = FakeExperiment()
    worker = DeviceCommandWorker(
        experiment=experiment, command="connect_device", kind="scanner", key="x"
    )
    finished = _capture_finished(worker)
    statuses = _capture_text_signal(worker.status_changed)

    worker.run()

    assert experiment.calls == [("connect_device", {"ref": "scanner.x"})]
    assert statuses == ["connecting scanner.x"]
    assert finished == [{"command": "connect_device", "ref": "scanner.x"}]


def test_device_command_worker_connects_all_devices():
    experiment = FakeExperiment()
    worker = DeviceCommandWorker(experiment=experiment, command="connect_all")
    finished = _capture_finished(worker)
    statuses = _capture_text_signal(worker.status_changed)

    worker.run()

    assert experiment.calls == [("connect_all", {})]
    assert statuses == ["connecting all"]
    assert finished == [{"command": "connect_all"}]


def test_device_command_worker_disconnects_one_device_by_explicit_ref():
    experiment = FakeExperiment()
    worker = DeviceCommandWorker(
        experiment=experiment, command="disconnect_device", ref="scanner.y"
    )
    finished = _capture_finished(worker)

    worker.run()

    assert experiment.calls == [("disconnect_device", {"ref": "scanner.y"})]
    assert finished == [{"command": "disconnect_device", "ref": "scanner.y"}]


def test_device_command_worker_rejects_device_command_without_reference():
    worker = DeviceCommandWorker(experiment=FakeExperiment(), command="connect_device")
    errors = _capture_text_signal(worker.error_occurred)

    worker.run()

    assert errors == ["Device command requires kind/key or ref."]


def test_device_command_worker_reports_device_failure_without_success_result():
    class FailingExperiment(FakeExperiment):
        def connect_device(self, ref):
            super().connect_device(ref)
            raise OSError("device connection failed")

    experiment = FailingExperiment()
    worker = DeviceCommandWorker(
        experiment=experiment, command="connect_device", ref="scanner.x"
    )
    errors = _capture_text_signal(worker.error_occurred)
    finished = _capture_finished(worker)

    worker.run()

    assert experiment.calls == [("connect_device", {"ref": "scanner.x"})]
    assert errors == ["device connection failed"]
    assert finished == []


def test_device_command_worker_disconnects_all_devices():
    experiment = FakeExperiment()
    worker = DeviceCommandWorker(experiment=experiment, command="disconnect_all")
    finished = _capture_finished(worker)
    statuses = _capture_text_signal(worker.status_changed)

    worker.run()

    assert experiment.calls == [("disconnect_all", {})]
    assert statuses == ["disconnecting all"]
    assert finished == [{"command": "disconnect_all"}]


def test_device_command_worker_disconnects_all_for_shutdown():
    experiment = FakeExperiment()
    worker = DeviceCommandWorker(
        experiment=experiment,
        command="shutdown_disconnect_all",
    )
    finished = _capture_finished(worker)
    statuses = _capture_text_signal(worker.status_changed)

    worker.run()

    assert experiment.calls == [("disconnect_all", {})]
    assert statuses == ["disconnecting all"]
    assert finished == [{"command": "shutdown_disconnect_all"}]


def test_device_command_worker_runs_delay_stage_initialize():
    experiment = FakeExperiment()
    worker = DeviceCommandWorker(
        experiment=experiment, command="initialize_delay_stage"
    )
    finished = _capture_finished(worker)
    statuses = _capture_text_signal(worker.status_changed)

    worker.run()

    assert experiment.calls == [("initialize_delay_stage", {"ref": "delay_stage.t"})]
    assert statuses == ["delay stage initializing", "delay_stage initializing"]
    assert finished == [
        {
            "command": "initialize_delay_stage",
            "kind": "delay_stage",
            "axis": "t",
            "info": {"axis": "t"},
        }
    ]


def test_device_command_worker_runs_scanner_initialize():
    experiment = FakeExperiment()
    worker = DeviceCommandWorker(
        experiment=experiment, command="initialize_scanner", axis="y"
    )
    finished = _capture_finished(worker)
    statuses = _capture_text_signal(worker.status_changed)

    worker.run()

    assert experiment.calls == [
        ("initialize_scanner", {"axis": "y", "ref": "scanner.y"})
    ]
    assert statuses == ["scanner y initializing", "y scanner initializing"]
    assert finished == [
        {
            "command": "initialize_scanner",
            "kind": "scanner",
            "axis": "y",
            "info": {"axis": "y"},
        }
    ]


def test_device_command_worker_reads_lockin_wait_time():
    experiment = FakeExperiment()
    worker = DeviceCommandWorker(
        experiment=experiment,
        command="lockin_wait_time",
        ref="lockin.main",
        multiplier=4.0,
    )
    finished = _capture_finished(worker)

    worker.run()

    assert experiment.calls == [
        ("lockin_wait_time", {"ref": "lockin.main", "multiplier": 4.0})
    ]
    assert finished == [
        {"command": "lockin_wait_time", "ref": "lockin.main", "wait_s": 1.25}
    ]


def test_device_command_worker_reconnects_lockin_after_invalid_session_handle():
    class StaleExperiment(FakeExperiment):
        def __init__(self):
            super().__init__()
            self.attempts = 0

        def lockin_wait_time(self, ref, *, multiplier=4.0):
            self.attempts += 1
            self.calls.append(
                ("lockin_wait_time", {"ref": ref, "multiplier": multiplier})
            )
            if self.attempts == 1:
                raise RuntimeError("Invalid session handle")
            return 2.5

    experiment = StaleExperiment()
    worker = DeviceCommandWorker(
        experiment=experiment, command="lockin_wait_time", multiplier=3.0
    )
    finished = _capture_finished(worker)
    statuses = _capture_text_signal(worker.status_changed)

    worker.run()

    assert [name for name, _kwargs in experiment.calls] == [
        "lockin_wait_time",
        "disconnect_device",
        "connect_device",
        "lockin_wait_time",
    ]
    assert statuses == ["reading lock-in wait time", "reconnecting lock-in"]
    assert finished == [
        {"command": "lockin_wait_time", "ref": "lockin.main", "wait_s": 2.5}
    ]


def test_device_command_worker_does_not_reconnect_for_unrelated_wait_failure():
    class FailingExperiment(FakeExperiment):
        def lockin_wait_time(self, ref, *, multiplier=4.0):
            raise OSError("instrument unavailable")

    worker = DeviceCommandWorker(
        experiment=FailingExperiment(), command="lockin_wait_time"
    )
    errors = _capture_text_signal(worker.error_occurred)

    worker.run()

    assert errors == ["instrument unavailable"]


def test_device_command_worker_rejects_unsupported_command():
    worker = DeviceCommandWorker(experiment=FakeExperiment(), command="unknown")
    errors = _capture_text_signal(worker.error_occurred)

    worker.run()

    assert errors == ["Unsupported device command: unknown"]


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
        (
            "set_lockin_settings",
            {"ref": "lockin.main", "sensitivity": 1e-3, "time_constant": 0.3},
        )
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

    assert experiment.calls == [
        ("move_delay_stage", {"value": 12.5, "coordinate": "measurement"})
    ]
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
    worker = MoveWorker(
        experiment=experiment, axis="x", value=4.0, coordinate="interface"
    )
    finished = _capture_finished(worker)
    statuses = _capture_text_signal(worker.status_changed)
    positions = []
    worker.position_changed.connect(positions.append)

    worker.run()

    assert experiment.calls == [
        ("move_scanner", {"axis": "x", "value": 4.0, "coordinate": "interface"})
    ]
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


def test_move_worker_reports_unsupported_axis_without_success_result():
    worker = MoveWorker(experiment=FakeExperiment(), axis="z", value=1.0)
    errors = _capture_text_signal(worker.error_occurred)
    finished = _capture_finished(worker)

    worker.run()

    assert errors == ["Unsupported axis: z"]
    assert finished == []


def test_move_worker_reports_hardware_failure_without_success_result():
    class FailingExperiment(FakeExperiment):
        def move_delay_stage(self, value, **kwargs):
            raise OSError("stage unavailable")

    worker = MoveWorker(experiment=FailingExperiment(), axis="t", value=1.0)
    errors = _capture_text_signal(worker.error_occurred)
    finished = _capture_finished(worker)

    worker.run()

    assert errors == ["stage unavailable"]
    assert finished == []


def test_gui_position_update_can_preserve_missing_axes():
    class DummyGui:
        def __init__(self):
            self.values = {"t": 1.0, "x": 2.0, "y": 3.0}

        def _set_position_value(self, axis, value):
            self.values[axis] = value

    gui = DummyGui()

    TRKRGui._update_position_from_position(
        gui, Position(x_um=4.0), preserve_missing=True
    )

    assert gui.values == {"t": 1.0, "x": 4.0, "y": 3.0}


def test_gui_runtime_config_preserves_scanner_software_hysteresis(
    monkeypatch, tmp_path
):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    config_path = tmp_path / "config.json"
    config_path.write_text("{}", encoding="utf-8")
    loaded_config = {
        "instruments": {
            "lockin": {"main": {"model": "SR7265", "resource": "GPIB0::12::INSTR"}},
            "delay_stage": {
                "t": {
                    "controller": "SHOT302GS",
                    "stage": "SGSP46-500",
                    "port": "COM6",
                    "direction": 1,
                }
            },
            "scanner": {
                "x": {
                    "controller": "CONEXCC",
                    "actuator": "TRA12CC",
                    "port": "COM5",
                    "axis": 1,
                    "sample_um_per_unit": 582.0,
                    "software_hysteresis": {
                        "enabled": True,
                        "distance_um": 20.0,
                        "direction": "negative",
                    },
                },
                "y": {
                    "controller": "CONEXCC",
                    "actuator": "TRA12CC",
                    "port": "COM4",
                    "axis": 1,
                    "sample_um_per_unit": 412.0,
                },
            },
        },
        "measurements": {"move_abs": {"zero": {"t_ps": 0.0, "x_um": 0.0, "y_um": 0.0}}},
    }
    monkeypatch.setattr(
        gui_module,
        "resolve_config_path",
        lambda: ConfigPathResolution(config_path, "test", []),
    )
    monkeypatch.setattr(gui_module, "load_config", lambda _path: loaded_config)
    monkeypatch.setattr(gui_module, "write_last_config_path", lambda _path: None)
    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)

    QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()

    runtime = gui._runtime_config()

    assert runtime["instruments"]["scanner"]["x"]["software_hysteresis"] == {
        "enabled": True,
        "distance_um": 20.0,
        "direction": "negative",
    }

    gui._shutdown_complete = True
    gui.close()


def test_gui_title_includes_package_version(monkeypatch):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)

    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()

    assert gui.windowTitle() == f"KohdaLab TRKR v{__version__}"

    gui._shutdown_complete = True
    gui.close()


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

    gui.handle_measurement_status("moving scanner x software hysteresis")
    assert gui.position_labels["x"].text() == "BH..."

    gui.handle_measurement_status(STATUS_WAITING)
    assert gui.position_labels["x"].text() == "2.000"
    assert gui.position_labels["t"].text() == "1.000"

    gui.handle_measurement_status(STATUS_STOPPED)
    assert gui.position_labels["x"].text() == "2.000"
    assert gui.position_labels["t"].text() == "1.000"

    gui._shutdown_complete = True
    gui.close()


def test_gui_move_status_shows_short_bh_label(monkeypatch):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)

    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    gui._current_position_values = {"t": 1.0, "x": 2.0, "y": 3.0}

    gui.handle_move_status("moving scanner x software hysteresis")
    assert gui.position_labels["x"].text() == "BH..."

    gui.handle_move_status(moving_scanner_status("x"))
    assert gui.position_labels["x"].text() == "Moving..."

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


def test_gui_axis_helpers_normalize_time_and_spatial_axes():
    assert _axis_cor_key(" T ") == "t_cor_ps"
    assert _axis_cor_key("X") == "x_cor_um"
    assert _axis_raw_key(" T ") == "t_ps"
    assert _axis_raw_key("Y") == "y_um"
    assert _axis_unit("t") == "ps"
    assert _axis_unit("x") == "um"
    assert _default_axis_range("t") == (-50.0, 300.0, 5.0)
    assert _default_axis_range("y") == (-30.0, 30.0, 1.0)


def test_gui_display_helpers_handle_missing_values_and_duplicate_points():
    assert _format_value(None) == "-"
    assert _format_value(1.2345, decimals=2) == "1.23"
    assert _fmt_bound(None, "um") == "-"
    assert _fmt_bound(1.25, "um") == "1.25 um"
    assert _motion_axis_display_text("software hysteresis pre-move") == "BH..."
    assert _motion_axis_display_text("scanner moving") == "Moving..."
    assert _unique_values([None, 1, 1.0, "2", 2.0]) == [1.0, 2.0]


def test_format_duration_for_eta():
    assert _format_duration(None) == "-"
    assert _format_duration(9.4) == "9s"
    assert _format_duration(85.0) == "1:25"
    assert _format_duration(3661.0) == "1:01:01"


def test_gui_device_command_result_routes_disconnect_wait_settings_and_unknown(
    monkeypatch,
):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    full_refreshes: list[bool] = []
    lockin_refreshes: list[bool] = []
    applied: list[dict] = []
    monkeypatch.setattr(
        gui, "_request_full_live_status", lambda: full_refreshes.append(True)
    )
    monkeypatch.setattr(
        gui, "_request_lockin_live_status", lambda: lockin_refreshes.append(True)
    )
    monkeypatch.setattr(gui, "_apply_lockin_settings", applied.append)

    gui.device_command_active = True
    gui.handle_device_command_finished({"command": "disconnect_all"})
    assert gui.status_label.text() == "disconnected"
    gui.handle_device_command_finished(
        {"command": "disconnect_device", "ref": "scanner.x"}
    )
    assert gui.status_label.text() == "scanner.x disconnected"

    gui.pending_wait_spin = gui.trkr_wait_spin
    gui.handle_device_command_finished({"command": "lockin_wait_time", "wait_s": 1.25})
    assert gui.trkr_wait_spin.value() == 1.25
    assert gui.pending_wait_spin is None

    gui.handle_device_command_finished(
        {"command": "set_lockin_settings", "settings": {"Sensitivity": 1e-3}}
    )
    gui.handle_device_command_finished(
        {"command": "set_lockin_settings", "settings": "invalid"}
    )
    gui.handle_device_command_finished({"command": "future_command"})

    assert full_refreshes == [True, True]
    assert lockin_refreshes == [True, True]
    assert applied == [{"Sensitivity": 1e-3}]
    assert "Device command finished: future_command" in gui.log.toPlainText()
    assert gui.device_command_active is False
    gui._shutdown_complete = True
    gui.close()


def test_gui_config_load_and_save_without_path_report_validation_errors(monkeypatch):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    gui.config_path.clear()
    warnings: list[tuple[str, str]] = []
    monkeypatch.setattr(
        gui_module,
        "resolve_config_path",
        lambda _path: ConfigPathResolution(None, "none", []),
    )
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )

    gui.load_config_file()
    gui.save_config_file()

    assert warnings == [
        ("Config Error", "Choose a config path before loading."),
        ("Config Error", "Choose a config path before saving."),
    ]
    gui._shutdown_complete = True
    gui.close()


def test_gui_full_and_partial_live_status_route_to_ui_handlers(monkeypatch):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    calls: list[tuple[str, object]] = []
    monkeypatch.setattr(
        gui,
        "_apply_pending_origin",
        lambda position: calls.append(("origin", position)),
    )
    monkeypatch.setattr(
        gui,
        "_apply_live_status",
        lambda status, *, overload: calls.append(("live", (status, overload))),
    )
    monkeypatch.setattr(
        gui, "_refresh_scan_limit_hints", lambda: calls.append(("hints", None))
    )
    monkeypatch.setattr(
        gui,
        "_apply_lockin_settings",
        lambda settings: calls.append(("settings", settings)),
    )
    monkeypatch.setattr(
        gui, "_apply_signal", lambda signal: calls.append(("signal", signal))
    )
    monkeypatch.setattr(
        gui,
        "_apply_overload_status",
        lambda overload: calls.append(("overload", overload)),
    )
    position = Position(x_um=1.0)
    status = LiveStatus(position=position)

    gui.handle_live_status_ready(status, {"overload": False})
    gui.handle_lockin_status_ready(
        {"Sensitivity": 1e-3}, {"X": 1.0}, {"overload": True}
    )
    gui.handle_lockin_status_ready(None, "invalid", None)

    assert calls == [
        ("origin", position),
        ("live", (status, {"overload": False})),
        ("hints", None),
        ("settings", {"Sensitivity": 1e-3}),
        ("signal", {"X": 1.0}),
        ("overload", {"overload": True}),
        ("overload", None),
    ]
    gui._shutdown_complete = True
    gui.close()


def test_gui_move_result_error_and_device_status_update_ui(monkeypatch):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    gui.running_move_axis = "x"
    gui._current_position_values["x"] = 3.0
    positions: list[object] = []
    warnings: list[tuple[str, str]] = []
    monkeypatch.setattr(gui, "handle_move_position", positions.append)
    monkeypatch.setattr(
        gui,
        "_refresh_scan_limit_hints",
        lambda: (_ for _ in ()).throw(OSError("hint refresh failed")),
    )
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )

    gui.handle_device_status("connecting scanner.x")
    assert gui.status_label.text() == "connecting scanner.x"
    gui.handle_move_finished({"axis": "x", "value": 4.5, "position": {"x_um": 4.5}})
    assert positions == [{"x_um": 4.5}]
    assert gui.status_label.text() == "move complete"
    assert (
        "Could not refresh move position hints: hint refresh failed"
        in gui.log.toPlainText()
    )

    gui.handle_move_error("limit reached")
    assert gui.position_labels["x"].text() == "3.000"
    assert gui.status_label.text() == "move error"
    assert warnings == [("Move Error", "limit reached")]
    gui._shutdown_complete = True
    gui.close()


def test_gui_output_fields_load_from_config_and_store_current_values(
    monkeypatch, tmp_path
):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    output_dir = str(tmp_path / "trkr-output")
    gui.config["measurements"]["trkr"]["output"] = {
        "dir": output_dir,
        "filename": "custom-trkr",
        "auto_timestamp_suffix": False,
    }
    gui.output_settings_by_mode.pop("trkr", None)

    gui._apply_output_settings("trkr")

    assert gui.output_dir_edit.text() == output_dir
    assert gui.output_name_edit.text() == "custom-trkr"
    assert gui.auto_suffix_check.isChecked() is False

    gui.output_name_edit.setText("stored-trkr")
    gui.auto_suffix_check.setChecked(True)
    gui._store_output_settings("trkr")
    assert gui.output_settings_by_mode["trkr"] == {
        "output_dir": output_dir,
        "filename": "stored-trkr",
        "auto_timestamp_suffix": True,
    }

    gui.config["measurements"]["srkr"] = "invalid"
    gui.output_settings_by_mode.pop("srkr", None)
    assert gui._measurement_output_from_config("srkr") == {}
    gui._shutdown_complete = True
    gui.close()


def test_gui_device_initialization_and_error_routes_cleanup_and_warning(monkeypatch):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    warnings: list[tuple[str, str]] = []
    gui.experiment = object()  # type: ignore[assignment]
    monkeypatch.setattr(
        gui,
        "_request_full_live_status",
        lambda: (_ for _ in ()).throw(OSError("refresh failed")),
    )
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )

    gui.handle_device_initialized({"kind": "delay_stage", "axis": "t"})
    assert gui.status_label.text() == "delay stage initialized"
    assert (
        "Could not refresh initialized position: refresh failed"
        in gui.log.toPlainText()
    )

    gui.device_command_active = True
    gui.pending_wait_spin = gui.trkr_wait_spin
    gui.handle_device_error("connection lost")
    assert gui.status_label.text() == "device error"
    assert gui.device_command_active is False
    assert gui.pending_wait_spin is None
    assert warnings == [("Device Error", "connection lost")]

    gui.pending_origin_axis = "y"
    gui.handle_live_status_error("read failed")
    assert gui.pending_origin_axis is None
    assert "Live status error: read failed" in gui.log.toPlainText()

    gui.initialize_device("camera", axis="z")
    assert warnings[-1] == (
        "Initialize Error",
        "Unsupported initialize target: camera z",
    )
    gui.experiment = None
    gui._shutdown_complete = True
    gui.close()


def test_gui_pending_origin_updates_each_axis_and_rejects_unknown_axis(monkeypatch):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    warnings: list[tuple[str, str]] = []
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )
    position = Position(t_ps=1.5, x_um=2.5, y_um=3.5)

    for axis in ("t", "x", "y"):
        gui.pending_origin_axis = axis
        gui._apply_pending_origin(position)

    assert gui.t_zero_spin.value() == 1.5
    assert gui.x_zero_spin.value() == 2.5
    assert gui.y_zero_spin.value() == 3.5
    assert gui.pending_origin_axis is None

    gui.pending_origin_axis = "z"
    gui._apply_pending_origin(position)
    assert warnings == [("Origin Error", "z position is unavailable.")]
    assert gui.pending_origin_axis is None
    gui._shutdown_complete = True
    gui.close()


def test_gui_move_fallback_result_invalid_start_and_thread_cleanup(monkeypatch):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    warnings: list[tuple[str, str]] = []
    row_updates: list[object] = []
    position_updates: list[tuple[object, bool]] = []
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )
    monkeypatch.setattr(gui, "_update_position_from_row", row_updates.append)
    monkeypatch.setattr(
        gui,
        "_update_position_from_position",
        lambda position, *, preserve_missing: position_updates.append(
            (position, preserve_missing)
        ),
    )
    monkeypatch.setattr(gui, "_refresh_scan_limit_hints", lambda: None)

    gui.handle_move_position({"x_um": 4.0})
    position = Position(y_um=5.0)
    gui.handle_move_position(position)
    assert row_updates == [{"x_um": 4.0}]
    assert position_updates == [(position, True)]

    gui.running_move_axis = "y"
    gui.move_y_spin.setValue(6.5)
    gui.handle_move_finished(object())
    assert gui.status_label.text() == "move complete"
    assert "Moved scanner Y to 6.500 um." in gui.log.toPlainText()

    gui.running_move_axis = None
    gui.handle_move_error("unassigned move failed")
    assert warnings == [("Move Error", "unassigned move failed")]

    gui.move_absolute("z")
    assert warnings[-1] == ("Move Error", "Unsupported axis: z")

    gui.device_thread = object()  # type: ignore[assignment]
    gui.device_worker = object()  # type: ignore[assignment]
    gui.device_command_active = True
    gui.pending_wait_spin = gui.trkr_wait_spin
    gui.cleanup_device_thread()
    assert gui.device_thread is None
    assert gui.device_worker is None
    assert gui.device_command_active is False
    assert gui.pending_wait_spin is None

    gui.live_thread = object()  # type: ignore[assignment]
    gui.live_worker = object()  # type: ignore[assignment]
    gui.lockin_live_thread = object()  # type: ignore[assignment]
    gui.lockin_live_worker = object()  # type: ignore[assignment]
    gui.cleanup_live_thread()
    gui.cleanup_lockin_live_thread()
    assert gui.live_thread is None
    assert gui.live_worker is None
    assert gui.lockin_live_thread is None
    assert gui.lockin_live_worker is None
    gui._shutdown_complete = True
    gui.close()


def test_gui_measurement_cleanup_refreshes_normally_but_not_during_shutdown(
    monkeypatch,
):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    scheduled: list[tuple[int, object]] = []
    monkeypatch.setattr(
        gui_module.QtCore.QTimer,
        "singleShot",
        lambda delay, callback: scheduled.append((delay, callback)),
    )
    gui.experiment = object()  # type: ignore[assignment]
    gui.running_measurement = gui._measurement_name()
    gui.measurement_thread = object()  # type: ignore[assignment]
    gui.worker = object()  # type: ignore[assignment]

    gui.cleanup_thread()

    assert gui.measurement_thread is None
    assert gui.worker is None
    assert gui.running_measurement is None
    assert scheduled == [(0, gui._request_full_live_status)]

    scheduled.clear()
    gui.running_measurement = gui._measurement_name()
    gui.measurement_thread = object()  # type: ignore[assignment]
    gui.worker = object()  # type: ignore[assignment]
    gui._shutdown_requested = True
    gui.cleanup_thread()

    assert scheduled == []
    gui.experiment = None
    gui._shutdown_complete = True
    gui.close()


def test_gui_live_timer_routing_and_shutdown_device_error(monkeypatch):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    full: list[bool] = []
    lockin: list[bool] = []
    scheduled: list[tuple[int, object]] = []
    warnings: list[tuple[str, str]] = []
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
    monkeypatch.setattr(gui_module.time, "perf_counter", lambda: 10.0)
    monkeypatch.setattr(gui, "_request_full_live_status", lambda: full.append(True))
    monkeypatch.setattr(gui, "_request_lockin_live_status", lambda: lockin.append(True))

    gui.experiment = object()  # type: ignore[assignment]
    gui.measurement_thread = object()  # type: ignore[assignment]
    gui.refresh_live_status()
    gui.measurement_thread = None
    gui.device_command_active = True
    gui.refresh_live_status()
    gui.device_command_active = False
    gui._last_live_refresh = 9.5
    gui.refresh_live_status()
    gui._last_live_refresh = 0.0
    gui.move_thread = object()  # type: ignore[assignment]
    gui.refresh_live_status()
    gui.move_thread = None
    gui._last_live_refresh = 0.0
    gui.refresh_live_status()

    assert lockin == [True]
    assert full == [True]

    gui._shutdown_requested = True
    gui.device_command_active = True
    gui.handle_device_error("shutdown disconnect failed")
    assert gui._shutdown_complete is True
    assert gui.device_command_active is False
    assert scheduled == [(0, gui.close)]
    assert warnings == []
    scheduled.clear()
    gui.experiment = None
    gui.close()


def test_gui_resource_results_preserve_current_and_reject_non_lists(monkeypatch):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    gui.lockin_resource_combo.addItem("GPIB0::OLD")
    gui.lockin_resource_combo.setCurrentText("GPIB0::OLD")
    gui.t_port_combo.addItem("COM-OLD")
    gui.t_port_combo.setCurrentText("COM-OLD")

    gui.handle_resource_list_ready(["GPIB0::NEW", "GPIB0::OLD"], ["COM1", "COM-OLD"])

    assert gui.lockin_resource_combo.currentText() == "GPIB0::OLD"
    assert gui.t_port_combo.currentText() == "COM-OLD"
    assert {
        gui.lockin_resource_combo.itemText(index)
        for index in range(gui.lockin_resource_combo.count())
    } == {"GPIB0::NEW", "GPIB0::OLD"}

    gui.handle_resource_list_ready("invalid", object())
    assert gui.lockin_resource_combo.count() == 1
    assert gui.lockin_resource_combo.currentText() == "GPIB0::OLD"
    assert gui.t_port_combo.currentText() == "COM-OLD"

    gui.resource_thread = object()  # type: ignore[assignment]
    gui.resource_worker = object()  # type: ignore[assignment]
    gui.cleanup_resource_thread()
    assert gui.resource_thread is None
    assert gui.resource_worker is None
    gui._shutdown_complete = True
    gui.close()


def test_gui_device_command_queue_busy_guards_controls_and_tc_pending(monkeypatch):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    requests: list[dict] = []
    warnings: list[tuple[str, str]] = []
    monkeypatch.setattr(gui, "_ensure_experiment", lambda: object())
    monkeypatch.setattr(gui, "_ensure_device_worker", lambda: object())
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )
    gui.device_command_requested.connect(requests.append)

    gui.connect_device("scanner", "x")
    assert requests == [
        {
            "command": "connect_device",
            "kind": "scanner",
            "key": "x",
            "axis": None,
            "ref": None,
            "multiplier": 4.0,
            "settings": {},
        }
    ]
    assert gui.device_command_active is True
    assert gui.connect_button.isEnabled() is False
    gui.disconnect_all()
    assert len(requests) == 1

    gui.cleanup_device_command()
    assert gui.connect_button.isEnabled() is True
    gui.measurement_thread = object()  # type: ignore[assignment]
    gui.connect_all()
    assert warnings == [
        ("Device Error", "Stop the running measurement first."),
    ]
    assert len(requests) == 1
    gui.measurement_thread = None

    tc_requests: list[tuple[str, dict]] = []
    monkeypatch.setattr(
        gui,
        "_start_device_command",
        lambda command, **kwargs: tc_requests.append((command, kwargs)),
    )
    gui.use_tc_wait_time(gui.trkr_wait_spin)
    assert gui.pending_wait_spin is gui.trkr_wait_spin
    assert tc_requests == [
        (
            "lockin_wait_time",
            {
                "label": "Read lock-in wait time",
                "ref": "lockin.main",
                "multiplier": 4.0,
            },
        )
    ]
    gui.device_command_active = True
    gui.use_tc_wait_time(gui.srkr_wait_spin)
    assert gui.pending_wait_spin is gui.trkr_wait_spin
    gui.device_command_active = False
    gui._shutdown_complete = True
    gui.close()


def test_gui_browse_output_and_device_command_wrappers(monkeypatch, tmp_path):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    selected = str(tmp_path / "selected-output")
    dialog_results = iter([selected, ""])
    monkeypatch.setattr(
        gui_module.QtWidgets.QFileDialog,
        "getExistingDirectory",
        lambda *_args: next(dialog_results),
    )

    gui.browse_output_dir()
    assert gui.output_dir_edit.text() == selected
    gui.browse_output_dir()
    assert gui.output_dir_edit.text() == selected

    commands: list[tuple[str, dict]] = []
    warnings: list[tuple[str, str]] = []
    monkeypatch.setattr(
        gui,
        "_start_device_command",
        lambda command, **kwargs: commands.append((command, kwargs)),
    )
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )

    gui.connect_all()
    gui.connect_device("lockin", "main")
    gui.disconnect_device("scanner", "x")
    gui.disconnect_all()
    gui.initialize_device("delay_stage")
    gui.initialize_device("scanner", "y")
    gui.initialize_device("scanner", "z")

    assert commands == [
        ("connect_all", {"label": "Connect all"}),
        (
            "connect_device",
            {"label": "Connect lockin.main", "kind": "lockin", "key": "main"},
        ),
        (
            "disconnect_device",
            {"label": "Disconnect scanner.x", "kind": "scanner", "key": "x"},
        ),
        ("disconnect_all", {"label": "Disconnect all"}),
        (
            "initialize_delay_stage",
            {"label": "Initialize delay stage", "kind": "delay_stage", "axis": "t"},
        ),
        (
            "initialize_scanner",
            {"label": "Initialize scanner y", "kind": "scanner", "axis": "y"},
        ),
    ]
    assert warnings == [
        ("Initialize Error", "Unsupported initialize target: scanner z")
    ]
    gui._shutdown_complete = True
    gui.close()


def test_gui_experiment_missing_devices_origin_and_motion_block_routing(monkeypatch):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    created: list[object] = []

    class ConfigExperiment:
        def __init__(self, config, *, auto_connect):
            self.config = config
            self.auto_connect = auto_connect
            self.missing_calls: list[tuple[str, dict]] = []
            created.append(self)

        def missing_devices(self, measurement, **kwargs):
            self.missing_calls.append((measurement, kwargs))
            return ["scanner.x"]

    monkeypatch.setattr(gui_module, "Experiment", ConfigExperiment)
    experiment = gui._ensure_experiment()
    assert experiment.auto_connect is False
    assert gui._ensure_experiment() is experiment

    gui.strkr_fast_axis_combo.setCurrentText("t")
    gui.strkr_slow_axis_combo.setCurrentText("x")
    assert gui._missing_required_devices("strkr") == ["scanner.x"]
    assert experiment.missing_calls[-1] == (
        "strkr",
        {
            "axis": gui.srkr_axis_combo.currentText().lower(),
            "fast_axis": "t",
            "slow_axis": "x",
        },
    )

    refreshes: list[bool] = []
    warnings: list[tuple[str, str]] = []
    monkeypatch.setattr(
        gui, "_request_full_live_status", lambda: refreshes.append(True)
    )
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )
    gui.set_origin_from_current(" Y ")
    assert gui.pending_origin_axis == "y"
    assert refreshes == [True]
    gui.set_origin_from_current("z")
    assert gui.pending_origin_axis is None
    assert warnings == [("Origin Error", "Unsupported axis: z")]

    gui.measurement_thread = object()  # type: ignore[assignment]
    gui.running_motion_axes = {"x"}
    assert gui._motion_axis_is_blocked_by_measurement("x") is True
    assert gui._motion_axis_is_blocked_by_measurement("y") is False
    gui.measurement_thread = None
    assert gui._motion_axis_is_blocked_by_measurement("x") is False
    gui.experiment = None
    gui._shutdown_complete = True
    gui.close()


def test_gui_live_apply_and_scan2d_heatmap_empty_and_single_point(monkeypatch):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    calls: list[tuple[str, object]] = []
    monkeypatch.setattr(
        gui,
        "_update_position_from_position",
        lambda position: calls.append(("position", position)),
    )
    monkeypatch.setattr(
        gui,
        "_apply_lockin_settings",
        lambda settings: calls.append(("settings", settings)),
    )
    monkeypatch.setattr(
        gui, "_apply_signal", lambda signal: calls.append(("signal", signal))
    )
    monkeypatch.setattr(
        gui,
        "_apply_overload_status",
        lambda overload: calls.append(("overload", overload)),
    )
    position = Position(x_um=1.0, y_um=2.0)
    status = LiveStatus(
        position=position,
        lockin_settings={
            "Sensitivity": 1e-3,
            "Time Constant": 0.3,
            "Ref. Freq": 137.0,
        },
        signal={"X": 1.0, "Y": 2.0, "R": 3.0, "Theta": 4.0},
        lockin_overload={"overload": False},
    )

    gui._apply_live_status(status)
    gui._apply_live_status(status, overload={"overload": True})

    assert calls == [
        ("position", position),
        (
            "settings",
            {"Sensitivity": 1e-3, "Time Constant": 0.3, "Ref. Freq": 137.0},
        ),
        ("signal", {"X": 1.0, "Y": 2.0, "R": 3.0, "Theta": 4.0}),
        ("overload", {"overload": False}),
        ("position", position),
        (
            "settings",
            {"Sensitivity": 1e-3, "Time Constant": 0.3, "Ref. Freq": 137.0},
        ),
        ("signal", {"X": 1.0, "Y": 2.0, "R": 3.0, "Theta": 4.0}),
        ("overload", {"overload": True}),
    ]

    signal_index = next(iter(gui.scan2d_heatmaps))
    gui._set_scan2d_heatmap(
        signal_index=signal_index,
        signal_key="X_V",
        scale=1.0,
        rows=[],
        fast_axis="x",
        slow_axis="y",
    )
    gui._set_scan2d_heatmap(
        signal_index=signal_index,
        signal_key="X_V",
        scale=1.0,
        rows=[
            {"target_x_cor_um": 1.0, "target_y_cor_um": 2.0, "X_V": 3.0},
            {"target_x_cor_um": 1.0, "target_y_cor_um": 2.0, "X_V": None},
            {"target_x_cor_um": 99.0, "target_y_cor_um": 2.0, "X_V": 4.0},
        ],
        fast_axis="x",
        slow_axis="y",
    )
    image = gui.scan2d_heatmaps[signal_index].image
    assert image is not None
    assert image.shape == (2, 1)
    assert image[0, 0] == 0.75
    assert image[1, 0] == 1.0
    gui._shutdown_complete = True
    gui.close()


def test_gui_async_shutdown_drain_and_worker_thread_finalization(monkeypatch):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    scheduled: list[tuple[int, object]] = []
    stop_calls: list[bool] = []
    commands: list[tuple[str, dict]] = []
    monkeypatch.setattr(
        gui_module.QtCore.QTimer,
        "singleShot",
        lambda delay, callback: scheduled.append((delay, callback)),
    )
    monkeypatch.setattr(gui, "stop_measurement", lambda: stop_calls.append(True))
    monkeypatch.setattr(
        gui,
        "_start_device_command",
        lambda command, **kwargs: commands.append((command, kwargs)),
    )

    gui._request_async_shutdown()
    gui._request_async_shutdown()
    assert gui._shutdown_requested is True
    assert stop_calls == [True]
    assert len(scheduled) == 1

    gui.measurement_thread = object()  # type: ignore[assignment]
    gui._drain_before_shutdown()
    assert stop_calls == [True, True]
    assert scheduled[-1][0] == 100
    gui.measurement_thread = None
    gui.experiment = object()  # type: ignore[assignment]
    gui._drain_before_shutdown()
    assert commands == [
        (
            "shutdown_disconnect_all",
            {"label": "Shutdown disconnect", "allow_during_shutdown": True},
        )
    ]

    class Thread:
        def __init__(self):
            self.calls: list[object] = []

        def quit(self):
            self.calls.append("quit")

        def wait(self, timeout):
            self.calls.append(("wait", timeout))

    threads = [Thread() for _ in range(6)]
    gui.measurement_thread = threads[0]  # type: ignore[assignment]
    gui.move_thread = threads[1]  # type: ignore[assignment]
    gui.live_thread = threads[2]  # type: ignore[assignment]
    gui.lockin_live_thread = threads[3]  # type: ignore[assignment]
    gui.resource_thread = threads[4]  # type: ignore[assignment]
    gui.device_thread = threads[5]  # type: ignore[assignment]
    gui._close_worker_threads()
    assert all(thread.calls == ["quit", ("wait", 2000)] for thread in threads)
    gui.measurement_thread = None
    gui.move_thread = None
    gui.live_thread = None
    gui.lockin_live_thread = None
    gui.resource_thread = None
    gui.device_thread = None
    gui.experiment = None
    scheduled.clear()
    gui._shutdown_complete = True
    gui.close()


def test_gui_corrected_move_measurement_status_and_plot_clear(monkeypatch):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    moves: list[str] = []
    curve_updates: list[bool] = []
    monkeypatch.setattr(gui, "move_absolute", moves.append)
    monkeypatch.setattr(gui, "_update_curves", lambda: curve_updates.append(True))
    gui.x_zero_spin.setValue(2.0)
    gui.x_cor_spin.setValue(3.5)

    gui.move_corrected("x")

    assert gui.move_x_spin.value() == 5.5
    assert moves == ["x"]

    gui.running_motion_axes = {"x", "invalid"}
    gui._current_position_values["x"] = 4.25
    gui.position_labels["x"].setText("Moving...")
    gui.handle_measurement_status(STATUS_STOPPED)
    assert gui.position_labels["x"].text() == "4.250"
    gui.position_labels["x"].setText("Moving...")
    gui.handle_measurement_status("reading lock-in")
    assert gui.position_labels["x"].text() == "4.250"

    measurement = gui._measurement_name()
    gui.rows_by_mode[measurement] = [{"measurement": measurement}]
    gui.point_text_by_mode[measurement] = "1/1"
    gui.eta_text_by_mode[measurement] = "1s"
    gui.snapshot_table.setRowCount(1)
    gui.clear_plot()

    assert gui.rows_by_mode[measurement] == []
    assert gui.point_label.text() == "-"
    assert gui.eta_label.text() == "-"
    assert gui.snapshot_table.rowCount() == 0
    assert curve_updates == [True]
    assert "Cleared plot data." in gui.log.toPlainText()
    gui._shutdown_complete = True
    gui.close()


def test_gui_device_success_results_and_shutdown_without_experiment(monkeypatch):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    full_refreshes: list[bool] = []
    scheduled: list[tuple[int, object]] = []
    monkeypatch.setattr(
        gui, "_request_full_live_status", lambda: full_refreshes.append(True)
    )
    monkeypatch.setattr(
        gui_module.QtCore.QTimer,
        "singleShot",
        lambda delay, callback: scheduled.append((delay, callback)),
    )

    gui.handle_device_command_finished({"command": "connect_all"})
    assert gui.status_label.text() == "connected"
    gui.handle_device_command_finished(
        {"command": "connect_device", "ref": "lockin.main"}
    )
    assert gui.status_label.text() == "lockin.main connected"
    assert full_refreshes == [True, True]

    gui.handle_device_command_finished({"command": "shutdown_disconnect_all"})
    assert gui._shutdown_complete is True
    assert scheduled == [(0, gui.close)]

    gui._shutdown_complete = False
    gui.experiment = None
    scheduled.clear()
    gui._drain_before_shutdown()
    assert gui._shutdown_complete is True
    assert scheduled == [(0, gui.close)]
    scheduled.clear()
    gui.close()


def test_gui_actual_lockin_signal_overload_and_live_invocation_ui(monkeypatch):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    invoked: list[str] = []
    lockin_invoked: list[bool] = []
    errors: list[str] = []
    monkeypatch.setattr(gui, "_invoke_live_worker", invoked.append)
    monkeypatch.setattr(
        gui, "_invoke_lockin_live_worker", lambda: lockin_invoked.append(True)
    )
    monkeypatch.setattr(gui, "handle_live_status_error", errors.append)

    gui._apply_lockin_settings(
        {"Sensitivity": 1e-3, "Time Constant": 0.3, "Ref. Freq": 137.0}
    )
    gui._apply_signal({"X": 1.0, "Y": 2.0, "R": 3.0, "Theta": 4.0})
    assert gui.signal_labels["X"].text() != "-"
    assert gui.signal_labels["Theta"].text() == "4.000 deg"
    gui._apply_lockin_settings({"Time Constant": 1.0})
    assert gui.tc_label.text() != "-"

    gui._apply_overload_status(None)
    gui._apply_overload_status("invalid")
    assert gui.overload_label.text() == "?"
    gui._apply_overload_status({"_error": True})
    assert gui.overload_label.text() == "?"
    gui._apply_overload_status({"overload": False})
    assert gui.overload_label.text() != "?"

    gui._request_full_live_status()
    gui._request_lockin_live_status()
    gui.handle_live_status_ready(object(), None)
    assert invoked == ["read_full"]
    assert lockin_invoked == [True]
    assert errors == ["Unexpected live status payload."]
    gui._shutdown_complete = True
    gui.close()


def test_gui_eta_guards_move_status_and_save_guards(monkeypatch):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    warnings: list[tuple[str, str]] = []
    information: list[tuple[str, str]] = []
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "information",
        lambda _parent, title, message: information.append((title, message)),
    )

    gui.running_measurement = "signal_monitor"
    gui._scan2d_fast_point_count = 2
    gui._update_scan2d_eta_from_slow_ready()
    gui.running_measurement = "strkr"
    gui.rows_by_mode["strkr"] = [{}]
    gui._scan2d_eta_anchor_at = None
    gui._update_scan2d_eta_from_slow_ready()
    assert gui.eta_text_by_mode["strkr"] == "-"

    gui.handle_move_status("unrelated status")
    assert gui.status_label.text() == "unrelated status"

    gui.measurement_thread = object()  # type: ignore[assignment]
    gui.save_rows()
    assert warnings == [
        ("Save Error", "Stop the running measurement before saving collected rows.")
    ]
    gui.measurement_thread = None
    gui.rows_by_mode[gui._measurement_name()] = []
    gui.save_rows()
    assert information == [("No Data", "No rows to save.")]
    gui.running_measurement = None
    gui._shutdown_complete = True
    gui.close()


def test_gui_move_controls_and_close_event_shutdown_branches(monkeypatch):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtGui, QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    gui.running_move_axis = "x"

    gui._set_move_running(True)

    assert gui.load_button.isEnabled() is False
    assert gui.connect_button.isEnabled() is False
    assert gui.read_status_button.isEnabled() is True
    assert gui.position_labels["x"].text() == "Moving..."

    gui.running_move_axis = None
    gui._set_move_running(True)
    assert gui.position_labels["x"].text() == "Moving..."

    gui.move_thread = None
    gui.device_command_active = True
    gui._set_move_running(False)
    assert gui.connect_button.isEnabled() is False
    gui.device_command_active = False
    gui._set_move_running(False)
    assert gui.connect_button.isEnabled() is True

    shutdown_requests: list[bool] = []
    monkeypatch.setattr(
        gui, "_request_async_shutdown", lambda: shutdown_requests.append(True)
    )
    pending_event = QtGui.QCloseEvent()
    gui._shutdown_complete = False
    gui.closeEvent(pending_event)
    assert pending_event.isAccepted() is False
    assert shutdown_requests == [True]

    cleanup: list[str] = []
    monkeypatch.setattr(gui, "_close_worker_threads", lambda: cleanup.append("threads"))
    monkeypatch.setattr(gui, "_restore_log_streams", lambda: cleanup.append("logs"))
    final_event = QtGui.QCloseEvent()
    gui._shutdown_complete = True
    gui.closeEvent(final_event)
    assert final_event.isAccepted() is True
    assert cleanup == ["threads", "logs"]


def test_gui_resize_event_reapplies_panel_sizes(monkeypatch):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtCore, QtGui, QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    panel_updates: list[bool] = []
    monkeypatch.setattr(gui, "_apply_panel_sizes", lambda: panel_updates.append(True))
    event = QtGui.QResizeEvent(QtCore.QSize(1000, 700), QtCore.QSize(900, 600))

    gui.resizeEvent(event)

    assert panel_updates == [True]
    gui._shutdown_complete = True
    gui.close()


def test_gui_plot_label_fallback_and_main_entrypoint(monkeypatch):
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PySide6 import QtWidgets

    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    del gui.signal_title_labels

    gui._refresh_plot_labels()

    gui._shutdown_complete = True
    gui.close()

    calls: list[object] = []

    class App:
        def __init__(self, argv):
            calls.append(("app", argv))

        def exec(self):
            calls.append("exec")
            return 7

    class Window:
        def __init__(self):
            calls.append("window")

        def show(self):
            calls.append("show")

    monkeypatch.setattr(gui_module.QtWidgets, "QApplication", App)
    monkeypatch.setattr(gui_module, "TRKRGui", Window)
    monkeypatch.setattr(gui_module.sys, "argv", ["kohdalab-trkr"])
    monkeypatch.setattr(
        gui_module.sys, "exit", lambda code: calls.append(("exit", code))
    )

    gui_module.main()

    assert calls == [
        ("app", ["kohdalab-trkr"]),
        "window",
        "show",
        "exec",
        ("exit", 7),
    ]
