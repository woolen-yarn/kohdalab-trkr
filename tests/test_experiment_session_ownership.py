from __future__ import annotations

from copy import deepcopy
import json

import pytest

from kohdalab.api import Experiment, measurements, signal_monitor_plan
from kohdalab.api import experiment as experiment_module


def minimal_config(tmp_path):
    return {
        "measurements": {
            "move_abs": {"zero": {"t_ps": 0.0, "x_um": 0.0, "y_um": 0.0}},
            "signal_monitor": {
                "output": {
                    "dir": str(tmp_path),
                    "filename": "signal",
                    "auto_timestamp_suffix": False,
                }
            },
            "trkr": {
                "scan": {"min": 0.0, "max": 0.0, "step": 1.0},
                "output": {
                    "dir": str(tmp_path),
                    "filename": "trkr",
                    "auto_timestamp_suffix": False,
                },
            },
            "srkr": {
                "scan": {"axis": "x", "min": 0.0, "max": 0.0, "step": 1.0},
                "output": {
                    "dir": str(tmp_path),
                    "filename": "srkr",
                    "auto_timestamp_suffix": False,
                },
            },
            "strkr": {
                "scan": {
                    "fast_axis": "t",
                    "slow_axis": "x",
                    "ranges": {
                        "t": {"min": 0.0, "max": 0.0, "step": 1.0},
                        "x": {"min": 0.0, "max": 0.0, "step": 1.0},
                        "y": {"min": 0.0, "max": 0.0, "step": 1.0},
                    },
                },
                "output": {
                    "dir": str(tmp_path),
                    "filename": "strkr",
                    "auto_timestamp_suffix": False,
                },
            },
            "srkr_2d": {
                "scan": {
                    "fast_axis": "x",
                    "slow_axis": "y",
                    "ranges": {
                        "x": {"min": 0.0, "max": 0.0, "step": 1.0},
                        "y": {"min": 0.0, "max": 0.0, "step": 1.0},
                    },
                },
                "output": {
                    "dir": str(tmp_path),
                    "filename": "srkr_2d",
                    "auto_timestamp_suffix": False,
                },
            },
        }
    }


class RaisingSession:
    def __init__(self, config):
        self.config = config
        self.disconnected = False

    def read_lockin_signal(self):
        raise RuntimeError("boom")

    def disconnect_all(self):
        self.disconnected = True


def test_experiment_run_signal_monitor_passes_existing_session(monkeypatch, tmp_path):
    experiment = Experiment(minimal_config(tmp_path), auto_connect=False)
    captured = {}

    def run_signal_monitor(config, **kwargs):
        captured.update(kwargs)
        return [{"ok": True}]

    monkeypatch.setattr(measurements, "run_signal_monitor", run_signal_monitor)

    plan = signal_monitor_plan(interval_s=0.0, n_points=1)

    assert experiment.run_signal_monitor(plan=plan) == [{"ok": True}]
    assert captured["plan"] is plan
    assert captured["session"] is experiment.session


def test_experiment_context_manager_closes_its_session(monkeypatch, tmp_path):
    experiment = Experiment(minimal_config(tmp_path), auto_connect=False)
    close_calls: list[bool] = []
    monkeypatch.setattr(experiment.session, "close", lambda: close_calls.append(True))

    with experiment as entered:
        assert entered is experiment

    experiment.close()
    assert close_calls == [True]
    assert experiment.closed


def test_experiment_context_manager_preserves_body_error_when_cleanup_fails(
    monkeypatch, tmp_path
):
    experiment = Experiment(minimal_config(tmp_path), auto_connect=False)
    monkeypatch.setattr(
        experiment.session,
        "close",
        lambda: (_ for _ in ()).throw(OSError("disconnect failed")),
    )

    with pytest.raises(ValueError, match="measurement failed") as caught:
        with experiment:
            raise ValueError("measurement failed")

    assert caught.value.__notes__ == [
        "Experiment cleanup also failed: disconnect failed"
    ]
    assert not experiment.closed


def test_experiment_context_manager_raises_cleanup_error_without_body_error(
    monkeypatch, tmp_path
):
    experiment = Experiment(minimal_config(tmp_path), auto_connect=False)
    monkeypatch.setattr(
        experiment.session,
        "close",
        lambda: (_ for _ in ()).throw(OSError("disconnect failed")),
    )

    with pytest.raises(OSError, match="disconnect failed"):
        with experiment:
            pass


def test_experiment_from_config_forwards_loaded_config_and_policy(
    monkeypatch, tmp_path
):
    config = minimal_config(tmp_path)
    path = tmp_path / "experiment.toml"
    loaded_paths = []

    def load_config(value):
        loaded_paths.append(value)
        return config

    monkeypatch.setattr(experiment_module, "load_config", load_config)

    experiment = Experiment.from_config(path, auto_connect=False)

    assert loaded_paths == [path]
    assert (
        experiment.config["measurements"]["signal_monitor"]["output"]
        == config["measurements"]["signal_monitor"]["output"]
    )
    assert experiment.auto_connect is False


@pytest.mark.parametrize(
    ("method_name", "session_method", "args", "kwargs"),
    [
        ("connect_device", "connect_device", ("lockin.main",), {}),
        ("read_position", "read_position", (), {}),
        ("read_lockin_signal", "read_lockin_signal", ("lockin.main",), {}),
        ("read_lockin_settings", "read_lockin_settings", ("lockin.main",), {}),
        ("read_lockin_overload", "read_lockin_overload", ("lockin.main",), {}),
        (
            "lockin_wait_time",
            "lockin_wait_time",
            ("lockin.main",),
            {"multiplier": 2.5},
        ),
        (
            "set_lockin_settings",
            "set_lockin_settings",
            ("lockin.main",),
            {
                "sensitivity": 1e-3,
                "time_constant": 0.3,
                "ac_gain": 10.0,
                "coupling": "AC",
                "slope": 12,
            },
        ),
        ("read_live_status", "read_live_status", (), {}),
        (
            "initialize_delay_stage",
            "initialize_delay_stage",
            ("delay_stage.t",),
            {"on_status": None},
        ),
        (
            "initialize_scanner",
            "initialize_scanner",
            ("x", "scanner.x"),
            {"on_status": None},
        ),
        ("initialize_xy", "initialize_xy", (), {"on_status": None}),
        (
            "move_delay_stage",
            "move_delay_stage",
            (1.5,),
            {
                "coordinate": "interface",
                "ref": "delay_stage.t",
                "on_status": None,
                "on_position": None,
            },
        ),
        (
            "move_scanner",
            "move_scanner",
            ("x", 2.5),
            {
                "coordinate": "interface",
                "ref": "scanner.x",
                "apply_software_hysteresis": False,
                "on_status": None,
                "on_position": None,
            },
        ),
    ],
)
def test_experiment_forwards_device_operations_exactly(
    monkeypatch, tmp_path, method_name, session_method, args, kwargs
):
    experiment = Experiment(minimal_config(tmp_path), auto_connect=False)
    calls = []
    sentinel = object()

    def delegated(*actual_args, **actual_kwargs):
        calls.append((actual_args, actual_kwargs))
        return sentinel

    monkeypatch.setattr(experiment.session, session_method, delegated)

    assert getattr(experiment, method_name)(*args, **kwargs) is sentinel
    assert calls == [(args, kwargs)]


@pytest.mark.parametrize("auto_connect", [0, 1, "false", None])
def test_experiment_requires_boolean_auto_connect(tmp_path, auto_connect):
    with pytest.raises(TypeError, match="auto_connect must be boolean"):
        Experiment(minimal_config(tmp_path), auto_connect=auto_connect)


def test_experiment_config_and_device_maps_are_defensive_views(tmp_path):
    experiment = Experiment(minimal_config(tmp_path), auto_connect=False)
    public_config = experiment.config
    public_config["measurements"]["move_abs"]["zero"]["t_ps"] = 99.0
    experiment.session.lockins["main"] = object()
    experiment.session.delay_stages["t"] = object()
    experiment.session.scanners["x"] = object()
    public_lockins = experiment.lockins
    public_delay_stages = experiment.delay_stages
    public_scanners = experiment.scanners
    public_lockins.clear()
    public_delay_stages.clear()
    public_scanners.clear()

    assert experiment.config["measurements"]["move_abs"]["zero"]["t_ps"] == 0.0
    assert "main" in experiment.session.lockins
    assert "t" in experiment.session.delay_stages
    assert "x" in experiment.session.scanners


def test_experiment_connection_aliases_and_connected_devices_delegate(
    monkeypatch, tmp_path
):
    experiment = Experiment(minimal_config(tmp_path), auto_connect=False)
    calls: list[str] = []
    monkeypatch.setattr(
        experiment.session, "connect_all", lambda: calls.append("connect_all")
    )
    monkeypatch.setattr(
        experiment.session, "disconnect_all", lambda: calls.append("disconnect_all")
    )
    monkeypatch.setattr(
        experiment.session,
        "disconnect_device",
        lambda ref: calls.append(f"disconnect:{ref}"),
    )
    monkeypatch.setattr(
        experiment.session,
        "connected_devices",
        lambda: {"lockin.main": True, "scanner.x": False},
    )

    experiment.connect()
    experiment.disconnect()
    experiment.disconnect_device("scanner.x")

    assert experiment.connected_devices() == {
        "lockin.main": True,
        "scanner.x": False,
    }
    assert calls == ["connect_all", "disconnect_all", "disconnect:scanner.x"]


def test_experiment_required_and_missing_devices_forward_axes_and_state(
    monkeypatch, tmp_path
):
    experiment = Experiment(minimal_config(tmp_path), auto_connect=False)
    monkeypatch.setattr(
        experiment.session,
        "connected_devices",
        lambda: {"lockin.main": True, "scanner.x": False},
    )
    required_calls: list[tuple[dict, str, dict]] = []
    missing_calls: list[tuple[dict, dict, str, dict]] = []

    def fake_required(config, measurement_name, **axes):
        required_calls.append((config, measurement_name, axes))
        return ["lockin.main", "scanner.x"]

    def fake_missing(config, connected, measurement_name, **axes):
        missing_calls.append((config, connected, measurement_name, axes))
        return ["scanner.x"]

    monkeypatch.setattr(experiment_module, "required_devices", fake_required)
    monkeypatch.setattr(experiment_module, "missing_devices", fake_missing)

    axes = {"axis": "x", "fast_axis": "t", "slow_axis": "y"}
    assert experiment.required_devices("srkr_2d", **axes) == [
        "lockin.main",
        "scanner.x",
    ]
    assert experiment.missing_devices("srkr_2d", **axes) == ["scanner.x"]
    assert required_calls[0][1:] == ("srkr_2d", axes)
    assert missing_calls[0][1:] == (
        {"lockin.main": True, "scanner.x": False},
        "srkr_2d",
        axes,
    )


def test_experiment_rejects_operations_and_config_changes_after_close(
    monkeypatch, tmp_path
):
    experiment = Experiment(minimal_config(tmp_path), auto_connect=False)
    experiment.close()
    monkeypatch.setattr(
        experiment.session,
        "connect_all",
        lambda: pytest.fail("closed Experiment reached the session"),
    )

    with pytest.raises(RuntimeError, match="Experiment is closed"):
        experiment.connect_all()
    with pytest.raises(RuntimeError, match="Experiment is closed"):
        experiment.read_position()
    with pytest.raises(RuntimeError, match="Experiment is closed"):
        experiment.run_signal_monitor(interval_s=0.0, n_points=1)
    with pytest.raises(RuntimeError, match="Experiment is closed"):
        experiment.config = minimal_config(tmp_path)
    with pytest.raises(RuntimeError, match="Experiment is closed"):
        experiment.__enter__()

    experiment.disconnect_all()
    experiment.disconnect_device("lockin.main")


def test_failed_close_can_be_retried(monkeypatch, tmp_path):
    experiment = Experiment(minimal_config(tmp_path), auto_connect=False)
    calls = 0

    def close():
        nonlocal calls
        calls += 1
        if calls == 1:
            raise OSError("close failed")

    monkeypatch.setattr(experiment.session, "close", close)

    with pytest.raises(OSError, match="close failed"):
        experiment.close()
    assert not experiment.closed

    experiment.close()
    assert experiment.closed
    assert calls == 2


def test_active_measurement_blocks_close_and_config_change(monkeypatch, tmp_path):
    experiment = Experiment(minimal_config(tmp_path), auto_connect=False)
    updated = deepcopy(experiment.config)
    updated["measurements"]["marker"] = 1
    observed: list[str] = []

    def run_signal_monitor(_config, **_kwargs):
        with pytest.raises(RuntimeError, match="operation is active"):
            experiment.close()
        observed.append("close blocked")
        with pytest.raises(RuntimeError, match="operation is active"):
            experiment.config = updated
        observed.append("config blocked")
        return []

    monkeypatch.setattr(measurements, "run_signal_monitor", run_signal_monitor)

    assert experiment.run_signal_monitor(interval_s=0.0, n_points=1) == []
    assert observed == ["close blocked", "config blocked"]
    experiment.config = updated
    assert experiment.config["measurements"]["marker"] == 1


def test_measurement_receives_config_snapshot(monkeypatch, tmp_path):
    experiment = Experiment(minimal_config(tmp_path), auto_connect=False)

    def run_signal_monitor(config, **_kwargs):
        config["measurements"]["move_abs"]["zero"]["t_ps"] = 123.0
        return []

    monkeypatch.setattr(measurements, "run_signal_monitor", run_signal_monitor)

    experiment.run_signal_monitor(interval_s=0.0, n_points=1)

    assert experiment.config["measurements"]["move_abs"]["zero"]["t_ps"] == 0.0


def test_experiment_rejects_non_boolean_hysteresis_before_session_io(
    monkeypatch, tmp_path
):
    experiment = Experiment(minimal_config(tmp_path), auto_connect=False)
    monkeypatch.setattr(
        experiment.session,
        "move_scanner",
        lambda *_args, **_kwargs: pytest.fail("invalid input reached session"),
    )

    with pytest.raises(TypeError, match="apply_software_hysteresis must be boolean"):
        experiment.move_scanner(
            "x",
            1.0,
            apply_software_hysteresis=1,  # type: ignore[arg-type]
        )


def test_experiment_run_trkr_passes_existing_session(monkeypatch, tmp_path):
    experiment = Experiment(minimal_config(tmp_path), auto_connect=False)
    captured = {}

    def run_trkr(config, **kwargs):
        captured.update(kwargs)
        return [{"ok": True}]

    monkeypatch.setattr(measurements, "run_trkr", run_trkr)

    assert experiment.run_trkr(scan_points=[0.0], wait_s=0.0) == [{"ok": True}]
    assert captured["session"] is experiment.session


def test_experiment_run_srkr_passes_existing_session(monkeypatch, tmp_path):
    experiment = Experiment(minimal_config(tmp_path), auto_connect=False)
    captured = {}

    def run_srkr(config, **kwargs):
        captured.update(kwargs)
        return [{"ok": True}]

    monkeypatch.setattr(measurements, "run_srkr", run_srkr)

    assert experiment.run_srkr(axis="x", scan_points=[0.0], wait_s=0.0) == [
        {"ok": True}
    ]
    assert captured["session"] is experiment.session


def test_experiment_run_strkr_passes_existing_session(monkeypatch, tmp_path):
    experiment = Experiment(minimal_config(tmp_path), auto_connect=False)
    captured = {}

    def run_strkr(config, **kwargs):
        captured.update(kwargs)
        return [{"ok": True}]

    monkeypatch.setattr(measurements, "run_strkr", run_strkr)

    assert experiment.run_strkr(wait_s=0.0) == [{"ok": True}]
    assert captured["session"] is experiment.session


def test_experiment_run_srkr_2d_passes_existing_session(monkeypatch, tmp_path):
    experiment = Experiment(minimal_config(tmp_path), auto_connect=False)
    captured = {}

    def run_srkr_2d(config, **kwargs):
        captured.update(kwargs)
        return [{"ok": True}]

    monkeypatch.setattr(measurements, "run_srkr_2d", run_srkr_2d)

    assert experiment.run_srkr_2d(wait_s=0.0) == [{"ok": True}]
    assert captured["session"] is experiment.session


def test_standalone_measurement_disconnects_temporary_session_on_error(
    monkeypatch, tmp_path
):
    sessions: list[RaisingSession] = []

    def make_session(config):
        session = RaisingSession(config)
        sessions.append(session)
        return session

    monkeypatch.setattr(measurements, "DeviceSession", make_session)

    with pytest.raises(RuntimeError, match="boom"):
        measurements.run_signal_monitor(
            minimal_config(tmp_path), interval_s=0.0, n_points=1
        )

    assert sessions[0].disconnected
    metadata = json.loads(
        (tmp_path / "signal.csv.meta.json").read_text(encoding="utf-8")
    )
    assert metadata["status"] == "failed"
    assert metadata["rows_written"] == 0
    assert metadata["error"] == {"type": "RuntimeError", "message": "boom"}


def test_provided_session_is_not_disconnected_on_error(tmp_path):
    session = RaisingSession(minimal_config(tmp_path))

    with pytest.raises(RuntimeError, match="boom"):
        measurements.run_signal_monitor(
            minimal_config(tmp_path),
            interval_s=0.0,
            n_points=1,
            session=session,
        )

    assert not session.disconnected
