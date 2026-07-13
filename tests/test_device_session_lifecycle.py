from __future__ import annotations

from copy import deepcopy
from threading import Lock

import pytest

import kohdalab.api.session as session_module
from kohdalab.api import Experiment
from kohdalab.api.session import DeviceSession


def config_with_devices() -> dict:
    return {
        "instruments": {
            "lockin": {"main": {"resource": "LOCKIN"}},
            "delay_stage": {"t": {"port": "STAGE"}},
            "scanner": {
                "x": {"port": "SCANNER", "axis": "X"},
                "y": {"port": "SCANNER", "axis": "Y"},
            },
        },
        "measurements": {"marker": 1},
    }


class HealthHandle:
    def __init__(self) -> None:
        self.connected = True
        self.health_error: Exception | None = None

    def is_connected(self) -> bool:
        if self.health_error is not None:
            raise self.health_error
        return self.connected


def test_connected_devices_and_io_fail_closed_for_stale_handle(monkeypatch):
    handle = HealthHandle()
    reads: list[bool] = []
    disconnects: list[dict] = []
    monkeypatch.setattr(session_module, "connect_lockin", lambda _config: handle)
    monkeypatch.setattr(session_module, "disconnect_lockin", disconnects.append)
    monkeypatch.setattr(
        session_module,
        "read_lockin_signal",
        lambda config, *, lockin: reads.append(True) or {"X": 1.0},
    )
    session = DeviceSession(config_with_devices(), auto_connect=False)
    session.connect_device("lockin.main")
    assert session.connected_devices()["lockin.main"]

    handle.connected = False

    assert not session.connected_devices()["lockin.main"]
    with pytest.raises(RuntimeError, match=r"stale: lockin\.main"):
        session.read_lockin_signal("lockin.main")
    with pytest.raises(RuntimeError, match=r"stale: lockin\.main"):
        session.connect_device("lockin.main")
    assert reads == []

    session.disconnect_device("lockin.main")
    assert disconnects == [{"resource": "LOCKIN"}]
    assert not session.connected_devices()["lockin.main"]


def test_connected_devices_treats_health_check_exception_as_disconnected(
    monkeypatch,
):
    handle = HealthHandle()
    handle.health_error = OSError("transport disappeared")
    session = DeviceSession(config_with_devices(), auto_connect=False)
    session.lockins["main"] = handle

    assert not session.connected_devices()["lockin.main"]


def test_connect_all_and_disconnect_all_manage_every_handle(monkeypatch):
    events: list[tuple[str, str]] = []

    monkeypatch.setattr(
        session_module, "connect_lockin", lambda config: f"lockin:{config['resource']}"
    )
    monkeypatch.setattr(
        session_module, "connect_delay_stage", lambda config: f"stage:{config['port']}"
    )
    monkeypatch.setattr(
        session_module, "connect_scanner", lambda config: f"scanner:{config['axis']}"
    )
    monkeypatch.setattr(
        session_module,
        "disconnect_lockin",
        lambda config: events.append(("lockin", config["resource"])),
    )
    monkeypatch.setattr(
        session_module,
        "disconnect_delay_stage",
        lambda config: events.append(("stage", config["port"])),
    )
    monkeypatch.setattr(
        session_module,
        "disconnect_scanner",
        lambda config: events.append(("scanner", config["axis"])),
    )
    session = DeviceSession(config_with_devices(), auto_connect=False)

    session.connect_all()

    assert session.lockins == {"main": "lockin:LOCKIN"}
    assert session.delay_stages == {"t": "stage:STAGE"}
    assert session.scanners == {"x": "scanner:X", "y": "scanner:Y"}
    assert all(session.connected_devices().values())

    session.disconnect_all()

    assert events == [
        ("lockin", "LOCKIN"),
        ("stage", "STAGE"),
        ("scanner", "X"),
        ("scanner", "Y"),
    ]
    assert not any(session.connected_devices().values())


def test_connect_all_rolls_back_new_handles_when_later_connect_fails(monkeypatch):
    events: list[str] = []
    monkeypatch.setattr(
        session_module,
        "connect_lockin",
        lambda _config: events.append("connect lockin") or object(),
    )
    monkeypatch.setattr(
        session_module,
        "connect_delay_stage",
        lambda _config: events.append("connect stage") or object(),
    )

    def fail_scanner(config):
        events.append(f"connect scanner {config['axis']}")
        if config["axis"] == "Y":
            raise OSError("scanner unavailable")
        return object()

    monkeypatch.setattr(session_module, "connect_scanner", fail_scanner)
    monkeypatch.setattr(
        session_module,
        "disconnect_lockin",
        lambda _config: events.append("disconnect lockin"),
    )
    monkeypatch.setattr(
        session_module,
        "disconnect_delay_stage",
        lambda _config: events.append("disconnect stage"),
    )
    monkeypatch.setattr(
        session_module,
        "disconnect_scanner",
        lambda config: events.append(f"disconnect scanner {config['axis']}"),
    )
    session = DeviceSession(config_with_devices(), auto_connect=False)

    with pytest.raises(OSError, match="scanner unavailable"):
        session.connect_all()

    assert events == [
        "connect lockin",
        "connect stage",
        "connect scanner X",
        "connect scanner Y",
        "disconnect scanner X",
        "disconnect stage",
        "disconnect lockin",
    ]
    assert session.lockins == {}
    assert session.delay_stages == {}
    assert session.scanners == {}


def test_connect_all_preserves_handles_connected_before_the_call(monkeypatch):
    existing_lockin = object()
    disconnects: list[str] = []
    monkeypatch.setattr(
        session_module, "connect_lockin", lambda _config: existing_lockin
    )
    monkeypatch.setattr(session_module, "connect_delay_stage", lambda _config: object())
    monkeypatch.setattr(
        session_module,
        "connect_scanner",
        lambda _config: (_ for _ in ()).throw(OSError("scanner unavailable")),
    )
    monkeypatch.setattr(
        session_module,
        "disconnect_lockin",
        lambda _config: disconnects.append("lockin"),
    )
    monkeypatch.setattr(
        session_module,
        "disconnect_delay_stage",
        lambda _config: disconnects.append("stage"),
    )
    session = DeviceSession(config_with_devices(), auto_connect=False)
    session.connect_device("lockin.main")

    with pytest.raises(OSError, match="scanner unavailable"):
        session.connect_all()

    assert session.lockins == {"main": existing_lockin}
    assert session.delay_stages == {}
    assert disconnects == ["stage"]
    session.disconnect_device("lockin.main")


def test_connect_all_preserves_original_error_and_notes_rollback_failures(
    monkeypatch,
):
    monkeypatch.setattr(session_module, "connect_lockin", lambda _config: object())
    monkeypatch.setattr(session_module, "connect_delay_stage", lambda _config: object())
    monkeypatch.setattr(
        session_module,
        "connect_scanner",
        lambda _config: (_ for _ in ()).throw(OSError("scanner unavailable")),
    )
    monkeypatch.setattr(session_module, "disconnect_lockin", lambda _config: None)
    monkeypatch.setattr(
        session_module,
        "disconnect_delay_stage",
        lambda _config: (_ for _ in ()).throw(OSError("stage close failed")),
    )
    session = DeviceSession(config_with_devices(), auto_connect=False)

    with pytest.raises(OSError, match="scanner unavailable") as error:
        session.connect_all()

    assert error.value.__notes__ == [
        "connect_all rollback also failed: delay_stage.t: stage close failed"
    ]
    assert session.lockins == {}
    assert "t" in session.delay_stages
    monkeypatch.setattr(session_module, "disconnect_delay_stage", lambda _config: None)
    session.disconnect_all()


def test_device_session_context_manager_closes_idempotently(monkeypatch):
    handle = object()
    disconnect_calls: list[dict] = []
    monkeypatch.setattr(session_module, "connect_lockin", lambda _config: handle)
    monkeypatch.setattr(session_module, "disconnect_lockin", disconnect_calls.append)

    with DeviceSession(config_with_devices(), auto_connect=False) as session:
        assert session.connect_device("lockin.main") is handle

    session.close()
    assert disconnect_calls == [{"resource": "LOCKIN"}]
    assert session.lockins == {}


def test_device_session_context_manager_preserves_body_error_and_notes_cleanup(
    monkeypatch,
):
    handle = object()
    monkeypatch.setattr(session_module, "connect_lockin", lambda _config: handle)
    monkeypatch.setattr(
        session_module,
        "disconnect_lockin",
        lambda _config: (_ for _ in ()).throw(OSError("close failed")),
    )

    with pytest.raises(ValueError, match="body failed") as error:
        with DeviceSession(config_with_devices(), auto_connect=False) as session:
            session.connect_device("lockin.main")
            raise ValueError("body failed")

    assert error.value.__notes__ == [
        "DeviceSession cleanup also failed: Failed to disconnect one or more "
        "devices: lockin.main: close failed"
    ]


def test_device_session_context_manager_raises_cleanup_error_after_success(
    monkeypatch,
):
    handle = object()
    monkeypatch.setattr(session_module, "connect_lockin", lambda _config: handle)
    monkeypatch.setattr(
        session_module,
        "disconnect_lockin",
        lambda _config: (_ for _ in ()).throw(OSError("close failed")),
    )

    with pytest.raises(RuntimeError, match=r"lockin\.main: close failed"):
        with DeviceSession(config_with_devices(), auto_connect=False) as session:
            session.connect_device("lockin.main")


def test_repeated_connect_in_one_session_is_idempotent(monkeypatch):
    handle = object()
    connect_calls: list[dict] = []
    disconnect_calls: list[dict] = []
    monkeypatch.setattr(
        session_module,
        "connect_lockin",
        lambda config: connect_calls.append(config) or handle,
    )
    monkeypatch.setattr(
        session_module,
        "disconnect_lockin",
        disconnect_calls.append,
    )
    session = DeviceSession(config_with_devices(), auto_connect=False)

    assert session.connect_device("lockin.main") is handle
    assert session.connect_device("lockin.main") is handle
    session.disconnect_device("lockin.main")

    assert len(connect_calls) == 1
    assert len(disconnect_calls) == 1


def test_shared_handle_closes_only_after_last_session_disconnects(monkeypatch):
    handle = object()
    disconnect_calls: list[dict] = []
    monkeypatch.setattr(session_module, "connect_lockin", lambda _config: handle)
    monkeypatch.setattr(
        session_module,
        "disconnect_lockin",
        disconnect_calls.append,
    )
    first = DeviceSession(config_with_devices(), auto_connect=False)
    second = DeviceSession(config_with_devices(), auto_connect=False)

    assert first.connect_device("lockin.main") is handle
    assert second.connect_device("lockin.main") is handle

    first.disconnect_device("lockin.main")
    assert disconnect_calls == []
    assert not first.connected_devices()["lockin.main"]
    assert second.connected_devices()["lockin.main"]

    second.disconnect_device("lockin.main")
    assert len(disconnect_calls) == 1
    assert not second.connected_devices()["lockin.main"]


def test_shared_target_rejects_different_config_before_device_io(monkeypatch):
    handle = object()
    connect_calls: list[dict] = []
    monkeypatch.setattr(
        session_module,
        "connect_lockin",
        lambda config: connect_calls.append(config) or handle,
    )
    monkeypatch.setattr(session_module, "disconnect_lockin", lambda _config: None)
    first_config = config_with_devices()
    second_config = deepcopy(first_config)
    first_config["instruments"]["lockin"]["main"]["timeout"] = 1.0
    second_config["instruments"]["lockin"]["main"]["timeout"] = 2.0
    first = DeviceSession(first_config, auto_connect=False)
    second = DeviceSession(second_config, auto_connect=False)

    first.connect_device("lockin.main")
    with pytest.raises(
        RuntimeError, match=r"different instrument config fields: timeout"
    ):
        second.connect_device("lockin.main")

    assert len(connect_calls) == 1
    assert first.connected_devices()["lockin.main"]
    assert not second.connected_devices()["lockin.main"]
    first.disconnect_device("lockin.main")


def test_shared_target_accepts_new_config_after_final_disconnect(monkeypatch):
    handles = iter([object(), object()])
    connect_calls: list[dict] = []
    monkeypatch.setattr(
        session_module,
        "connect_lockin",
        lambda config: connect_calls.append(config) or next(handles),
    )
    monkeypatch.setattr(session_module, "disconnect_lockin", lambda _config: None)
    first_config = config_with_devices()
    second_config = deepcopy(first_config)
    first_config["instruments"]["lockin"]["main"]["timeout"] = 1.0
    second_config["instruments"]["lockin"]["main"]["timeout"] = 2.0
    first = DeviceSession(first_config, auto_connect=False)
    second = DeviceSession(second_config, auto_connect=False)

    first.connect_device("lockin.main")
    first.disconnect_device("lockin.main")
    second.connect_device("lockin.main")
    second.disconnect_device("lockin.main")

    assert len(connect_calls) == 2


def test_last_owner_retains_shared_handle_when_disconnect_fails(monkeypatch):
    handle = object()
    monkeypatch.setattr(session_module, "connect_lockin", lambda _config: handle)
    first = DeviceSession(config_with_devices(), auto_connect=False)
    second = DeviceSession(config_with_devices(), auto_connect=False)
    first.connect_device("lockin.main")
    second.connect_device("lockin.main")
    first.disconnect_device("lockin.main")

    def fail_disconnect(_config):
        raise OSError("close failed")

    monkeypatch.setattr(session_module, "disconnect_lockin", fail_disconnect)
    with pytest.raises(OSError, match="close failed"):
        second.disconnect_device("lockin.main")

    assert second.connected_devices()["lockin.main"]
    monkeypatch.setattr(session_module, "disconnect_lockin", lambda _config: None)
    second.disconnect_device("lockin.main")


def test_disconnect_unowned_ref_does_not_close_another_session_handle(monkeypatch):
    handle = object()
    disconnect_calls: list[dict] = []
    monkeypatch.setattr(session_module, "connect_lockin", lambda _config: handle)
    monkeypatch.setattr(
        session_module,
        "disconnect_lockin",
        disconnect_calls.append,
    )
    owner = DeviceSession(config_with_devices(), auto_connect=False)
    unconnected = DeviceSession(config_with_devices(), auto_connect=False)
    owner.connect_device("lockin.main")

    unconnected.disconnect_device("lockin.main")

    assert disconnect_calls == []
    assert owner.connected_devices()["lockin.main"]
    owner.disconnect_device("lockin.main")


def test_disconnect_all_continues_after_failure_and_retains_failed_handle(monkeypatch):
    attempted: list[str] = []
    session = DeviceSession(config_with_devices(), auto_connect=False)
    session.lockins["main"] = "lockin"
    session.delay_stages["t"] = "stage"
    session.scanners.update({"x": "x", "y": "y"})

    def fail_lockin(_config):
        attempted.append("lockin")
        raise OSError("VISA close failed")

    monkeypatch.setattr(session_module, "disconnect_lockin", fail_lockin)
    monkeypatch.setattr(
        session_module,
        "disconnect_delay_stage",
        lambda _config: attempted.append("stage"),
    )
    monkeypatch.setattr(
        session_module,
        "disconnect_scanner",
        lambda config: attempted.append(config["axis"]),
    )

    with pytest.raises(RuntimeError, match=r"lockin\.main: VISA close failed"):
        session.disconnect_all()

    assert attempted == ["lockin", "stage", "X", "Y"]
    assert session.lockins == {"main": "lockin"}
    assert session.delay_stages == {}
    assert session.scanners == {}


def test_set_config_rejects_changes_to_connected_devices_but_allows_measurement_changes():
    original = config_with_devices()
    session = DeviceSession(original, auto_connect=False)
    session.lockins["main"] = object()

    measurement_update = deepcopy(original)
    measurement_update["measurements"]["marker"] = 2
    session.set_config(measurement_update)
    assert session.config["measurements"]["marker"] == 2

    device_update = deepcopy(measurement_update)
    device_update["instruments"]["lockin"]["main"]["resource"] = "OTHER"
    with pytest.raises(RuntimeError, match="Disconnect devices.*lockin.main"):
        session.set_config(device_update)
    assert session.config["instruments"]["lockin"]["main"]["resource"] == "LOCKIN"

    disconnected_device_update = deepcopy(measurement_update)
    disconnected_device_update["instruments"]["scanner"]["x"]["port"] = "OTHER"
    session.set_config(disconnected_device_update)
    assert session.config["instruments"]["scanner"]["x"]["port"] == "OTHER"


def test_in_place_device_config_mutation_blocks_io_but_disconnects_original_target(
    monkeypatch,
):
    handle = object()
    disconnected: list[dict] = []
    reads: list[dict] = []
    monkeypatch.setattr(session_module, "connect_lockin", lambda _config: handle)
    monkeypatch.setattr(session_module, "disconnect_lockin", disconnected.append)
    monkeypatch.setattr(
        session_module,
        "read_lockin_signal",
        lambda config, *, lockin: reads.append(config) or {"X": 1.0},
    )
    session = DeviceSession(config_with_devices(), auto_connect=False)
    session.connect_device("lockin.main")

    session.config["instruments"]["lockin"]["main"]["resource"] = "OTHER"

    with pytest.raises(
        RuntimeError,
        match=r"mutated in place: lockin\.main \(resource\)",
    ):
        session.read_lockin_signal("lockin.main")
    assert reads == []

    session.disconnect_device("lockin.main")
    assert disconnected == [{"resource": "LOCKIN"}]
    assert not session.connected_devices()["lockin.main"]


def test_disconnect_all_uses_snapshot_after_connected_config_is_removed(monkeypatch):
    handle = object()
    disconnected: list[dict] = []
    monkeypatch.setattr(session_module, "connect_lockin", lambda _config: handle)
    monkeypatch.setattr(session_module, "disconnect_lockin", disconnected.append)
    session = DeviceSession(config_with_devices(), auto_connect=False)
    session.connect_device("lockin.main")

    del session.config["instruments"]["lockin"]["main"]
    session.disconnect_all()

    assert disconnected == [{"resource": "LOCKIN"}]
    assert session.lockins == {}


def test_set_config_compares_against_pinned_snapshot_after_in_place_mutation(
    monkeypatch,
):
    monkeypatch.setattr(session_module, "connect_lockin", lambda _config: object())
    monkeypatch.setattr(session_module, "disconnect_lockin", lambda _config: None)
    session = DeviceSession(config_with_devices(), auto_connect=False)
    session.connect_device("lockin.main")
    session.config["instruments"]["lockin"]["main"]["resource"] = "OTHER"
    updated = deepcopy(session.config)
    updated["measurements"]["marker"] = 2

    with pytest.raises(RuntimeError, match=r"changing their config: lockin\.main"):
        session.set_config(updated)

    session.disconnect_device("lockin.main")


def test_experiment_config_remains_atomic_when_session_rejects_device_change():
    experiment = Experiment(config_with_devices(), auto_connect=False)
    experiment.session.lockins["main"] = object()
    changed = deepcopy(experiment.config)
    changed["instruments"]["lockin"]["main"]["resource"] = "OTHER"

    with pytest.raises(RuntimeError, match="Disconnect devices.*lockin.main"):
        experiment.config = changed

    assert experiment.config["instruments"]["lockin"]["main"]["resource"] == "LOCKIN"
    assert experiment.session.config == experiment.config


def test_read_position_combines_connected_stage_and_scanners(monkeypatch):
    session = DeviceSession(config_with_devices(), auto_connect=False)
    session.delay_stages["t"] = "stage-handle"
    session.scanners.update({"x": "x-handle", "y": "y-handle"})

    monkeypatch.setattr(
        session_module,
        "read_delay_stage",
        lambda config, *, delay_stage: {
            "t_ps": 12.0,
            "stage_mm": 1.2,
            "stage_pulse": 120,
        },
    )

    def read_scanner(axis, config, *, scanner):
        value = 3.0 if axis == "x" else 4.0
        return {f"{axis}_um": value, f"{axis}_mm": value / 1000.0}

    monkeypatch.setattr(session_module, "read_scanner", read_scanner)

    position = session.read_position()

    assert position.t_ps == 12.0
    assert position.delay_stage_mm == 1.2
    assert position.delay_stage_pulse == 120
    assert position.x_um == 3.0
    assert position.y_um == 4.0
    assert position.scanner_x_value == 0.003
    assert position.scanner_y_value == 0.004


def test_live_status_skips_busy_position_device_without_blocking_lockin(monkeypatch):
    session = DeviceSession(config_with_devices(), auto_connect=False)
    session.lockins["main"] = "lockin-handle"
    session.delay_stages["t"] = "stage-handle"
    session.scanners["x"] = "x-handle"
    session.scanners["y"] = "y-handle"
    busy_stage_lock = Lock()
    busy_scanner_lock = Lock()
    busy_stage_lock.acquire()
    busy_scanner_lock.acquire()
    session._io_locks["delay_stage"]["t"] = busy_stage_lock
    session._io_locks["scanner"]["y"] = busy_scanner_lock

    monkeypatch.setattr(
        session_module,
        "read_delay_stage",
        lambda *_args, **_kwargs: pytest.fail("busy delay stage was read"),
    )
    monkeypatch.setattr(
        session_module,
        "read_scanner",
        lambda axis, _config, *, scanner: {f"{axis}_um": 7.0},
    )
    monkeypatch.setattr(
        session_module,
        "read_lockin_signal",
        lambda _config, *, lockin: {"X": 1.0, "Y": 2.0, "R": 3.0, "Theta": 4.0},
    )
    monkeypatch.setattr(
        session_module,
        "read_lockin_settings",
        lambda _config, *, lockin: {"Sensitivity": 0.1, "Time Constant": 0.2},
    )
    monkeypatch.setattr(
        session_module,
        "read_lockin_overload",
        lambda _config, *, lockin: {"overload": False},
    )

    try:
        status = session.read_live_status(skip_busy_positions=True)
    finally:
        busy_stage_lock.release()
        busy_scanner_lock.release()

    assert status.position.t_ps is None
    assert status.position.x_um == 7.0
    assert status.position.y_um is None
    assert status.signal == {"X": 1.0, "Y": 2.0, "R": 3.0, "Theta": 4.0}


def test_lockin_operations_and_live_status_use_connected_handle(monkeypatch):
    session = DeviceSession(config_with_devices(), auto_connect=False)
    session.lockins["main"] = "lockin-handle"
    monkeypatch.setattr(
        session_module,
        "read_lockin_signal",
        lambda config, *, lockin: {"X": 1.0, "Y": 2.0, "R": 3.0, "Theta": 4.0},
    )
    monkeypatch.setattr(
        session_module,
        "read_lockin_settings",
        lambda config, *, lockin: {"Sensitivity": 0.1, "Time Constant": 0.2},
    )
    monkeypatch.setattr(
        session_module,
        "read_lockin_overload",
        lambda config, *, lockin: {"overload": False},
    )
    monkeypatch.setattr(
        session_module,
        "get_lockin_wait_time",
        lambda config, *, lockin, multiplier: 0.2 * multiplier,
    )
    captured_settings: list[dict] = []

    def set_settings(config, *, lockin, **settings):
        captured_settings.append(settings)
        return {"applied": True}

    monkeypatch.setattr(session_module, "service_set_lockin_settings", set_settings)

    assert session.lockin_wait_time(multiplier=3.0) == pytest.approx(0.6)
    assert session.set_lockin_settings(sensitivity=0.1, slope=12) == {"applied": True}
    assert captured_settings == [
        {
            "sensitivity": 0.1,
            "time_constant": None,
            "ac_gain": None,
            "coupling": None,
            "slope": 12,
        }
    ]

    status = session.read_live_status()
    assert status.connected["lockin.main"]
    assert status.signal == {"X": 1.0, "Y": 2.0, "R": 3.0, "Theta": 4.0}
    assert status.lockin_settings == {"Sensitivity": 0.1, "Time Constant": 0.2}
    assert status.lockin_overload == {"overload": False}


def test_initialize_and_move_operations_propagate_status_and_positions(monkeypatch):
    session = DeviceSession(config_with_devices(), auto_connect=False)
    statuses: list[str] = []
    positions: list[dict] = []

    monkeypatch.setattr(
        session_module, "connect_delay_stage", lambda config: "stage-handle"
    )
    monkeypatch.setattr(
        session_module, "connect_scanner", lambda config: f"scanner-{config['axis']}"
    )
    monkeypatch.setattr(
        session_module,
        "service_initialize_delay_stage",
        lambda config, *, delay_stage, on_status: {
            "initialized": "stage",
            "handle": delay_stage,
        },
    )
    monkeypatch.setattr(
        session_module,
        "service_initialize_scanner",
        lambda axis, config, *, scanner, on_status: {
            "initialized": axis,
            "handle": scanner,
        },
    )

    assert session.initialize_delay_stage(on_status=statuses.append) == {
        "initialized": "stage",
        "handle": "stage-handle",
    }
    assert session.initialize_xy(on_status=statuses.append) == {
        "x": {"initialized": "x", "handle": "scanner-X"},
        "y": {"initialized": "y", "handle": "scanner-Y"},
    }
    assert statuses == ["xy initializing"]

    def move_stage(**kwargs):
        row = {"t_ps": kwargs["value"], "stage_mm": 1.0, "stage_pulse": 10}
        kwargs["on_position"](row)
        return row

    def move_scanner(**kwargs):
        row = {f"{kwargs['axis']}_um": kwargs["value"], f"{kwargs['axis']}_deg": 0.5}
        kwargs["on_position"](row)
        return row

    monkeypatch.setattr(session_module, "move_delay_stage_abs", move_stage)
    monkeypatch.setattr(session_module, "move_scanner_abs", move_scanner)

    stage_position = session.move_delay_stage(
        5.0, on_status=statuses.append, on_position=positions.append
    )
    scanner_position = session.move_scanner(
        "x", 6.0, on_status=statuses.append, on_position=positions.append
    )

    assert statuses[-2:] == ["moving delay stage", "moving scanner x"]
    assert stage_position.t_ps == 5.0
    assert scanner_position.x_um == 6.0
    assert positions == [
        {"t_ps": 5.0, "stage_mm": 1.0, "stage_pulse": 10},
        {"x_um": 6.0, "x_deg": 0.5},
    ]


def test_initialize_failure_releases_only_new_connection(monkeypatch):
    stage = object()
    disconnected: list[dict] = []
    monkeypatch.setattr(session_module, "connect_delay_stage", lambda _config: stage)
    monkeypatch.setattr(session_module, "disconnect_delay_stage", disconnected.append)

    def fail_initialize(config, *, delay_stage, on_status):
        assert delay_stage is stage
        raise OSError("homing failed")

    monkeypatch.setattr(
        session_module,
        "service_initialize_delay_stage",
        fail_initialize,
    )
    session = DeviceSession(config_with_devices(), auto_connect=False)

    with pytest.raises(OSError, match="homing failed"):
        session.initialize_delay_stage()

    assert disconnected == [{"port": "STAGE"}]
    assert session.delay_stages == {}


def test_initialize_failure_preserves_preexisting_connection(monkeypatch):
    stage = object()
    disconnected: list[dict] = []
    monkeypatch.setattr(session_module, "connect_delay_stage", lambda _config: stage)
    monkeypatch.setattr(session_module, "disconnect_delay_stage", disconnected.append)
    monkeypatch.setattr(
        session_module,
        "service_initialize_delay_stage",
        lambda config, *, delay_stage, on_status: (_ for _ in ()).throw(
            OSError("homing failed")
        ),
    )
    session = DeviceSession(config_with_devices(), auto_connect=False)
    session.connect_device("delay_stage.t")

    with pytest.raises(OSError, match="homing failed"):
        session.initialize_delay_stage()

    assert disconnected == []
    assert session.delay_stages == {"t": stage}
    session.disconnect_device("delay_stage.t")


def test_initialize_xy_rolls_back_new_x_connection_when_y_fails(monkeypatch):
    disconnected: list[str] = []
    monkeypatch.setattr(
        session_module,
        "connect_scanner",
        lambda config: f"scanner-{config['axis']}",
    )
    monkeypatch.setattr(
        session_module,
        "disconnect_scanner",
        lambda config: disconnected.append(config["axis"]),
    )

    def initialize(axis, config, *, scanner, on_status):
        if axis == "y":
            raise OSError("y home failed")
        return {"axis": axis}

    monkeypatch.setattr(session_module, "service_initialize_scanner", initialize)
    session = DeviceSession(config_with_devices(), auto_connect=False)

    with pytest.raises(OSError, match="y home failed"):
        session.initialize_xy()

    assert disconnected == ["Y", "X"]
    assert session.scanners == {}


def test_initialize_xy_preserves_preexisting_x_when_new_y_fails(monkeypatch):
    handles = {"X": object(), "Y": object()}
    disconnected: list[str] = []
    monkeypatch.setattr(
        session_module, "connect_scanner", lambda config: handles[config["axis"]]
    )
    monkeypatch.setattr(
        session_module,
        "disconnect_scanner",
        lambda config: disconnected.append(config["axis"]),
    )

    def initialize(axis, config, *, scanner, on_status):
        if axis == "y":
            raise OSError("y home failed")
        return {"axis": axis}

    monkeypatch.setattr(session_module, "service_initialize_scanner", initialize)
    session = DeviceSession(config_with_devices(), auto_connect=False)
    session.connect_device("scanner.x")

    with pytest.raises(OSError, match="y home failed"):
        session.initialize_xy()

    assert disconnected == ["Y"]
    assert session.scanners == {"x": handles["X"]}
    session.close()


@pytest.mark.parametrize(
    ("ref", "expected"),
    [
        ("signal", ("lockin", "main")),
        ("lockin", ("lockin", "main")),
        ("delay", ("delay_stage", "t")),
        ("stage", ("delay_stage", "t")),
        ("scanner_x", ("scanner", "x")),
        (" y ", ("scanner", "y")),
        ("scanner.x", ("scanner", "x")),
    ],
)
def test_resolve_ref_supports_public_aliases_and_whitespace(ref, expected):
    session = DeviceSession(config_with_devices(), auto_connect=False)

    assert session.resolve_ref(ref) == expected


def test_resolve_ref_uses_explicit_default_kind_for_bare_key():
    session = DeviceSession(config_with_devices(), auto_connect=False)

    assert session.resolve_ref("main", default_kind="lockin") == ("lockin", "main")


def test_resolve_ref_rejects_unqualified_unknown_reference():
    session = DeviceSession(config_with_devices(), auto_connect=False)

    with pytest.raises(ValueError, match="Device reference must be"):
        session.resolve_ref("unknown")


def test_set_config_rejects_removing_connected_device_and_keeps_original():
    original = config_with_devices()
    session = DeviceSession(original, auto_connect=False)
    session.lockins["main"] = object()
    changed = deepcopy(original)
    del changed["instruments"]["lockin"]["main"]

    with pytest.raises(RuntimeError, match=r"changing their config: lockin\.main"):
        session.set_config(changed)

    assert session.config is original


def test_initialize_failure_preserves_primary_error_when_rollback_fails(monkeypatch):
    stage = object()
    monkeypatch.setattr(session_module, "connect_delay_stage", lambda _config: stage)
    monkeypatch.setattr(
        session_module,
        "service_initialize_delay_stage",
        lambda config, *, delay_stage, on_status: (_ for _ in ()).throw(
            OSError("homing failed")
        ),
    )
    monkeypatch.setattr(
        session_module,
        "disconnect_delay_stage",
        lambda _config: (_ for _ in ()).throw(OSError("release failed")),
    )
    session = DeviceSession(config_with_devices(), auto_connect=False)

    with pytest.raises(OSError, match="homing failed") as caught:
        session.initialize_delay_stage()

    assert caught.value.__notes__ == [
        "initialize connection rollback also failed: delay_stage.t: release failed"
    ]
    assert session.delay_stages == {"t": stage}

    monkeypatch.setattr(session_module, "disconnect_delay_stage", lambda _config: None)
    session.close()


def test_read_position_skips_connected_scanner_with_non_axis_key(monkeypatch):
    config = config_with_devices()
    config["instruments"]["scanner"]["diagnostic"] = {
        "port": "SCANNER",
        "axis": "AUX",
    }
    session = DeviceSession(config, auto_connect=False)
    session.scanners["diagnostic"] = object()
    monkeypatch.setattr(
        session_module,
        "read_scanner",
        lambda *_args, **_kwargs: pytest.fail("non-axis scanner was read"),
    )

    position = session.read_position()

    assert position.t_ps is None
    assert position.x_um is None
    assert position.y_um is None


def test_read_position_skips_handles_removed_while_waiting_for_device_lock(
    monkeypatch,
):
    session = DeviceSession(config_with_devices(), auto_connect=False)
    session.delay_stages["t"] = object()
    session.scanners["x"] = object()
    session.scanners["y"] = object()

    class RemoveHandleOnEnter:
        def __init__(self, kind: str, key: str):
            self.kind = kind
            self.key = key

        def __enter__(self):
            session._connected_map(self.kind).pop(self.key)

        def __exit__(self, *_args):
            return False

    monkeypatch.setattr(
        session,
        "_device_lock",
        lambda kind, key: RemoveHandleOnEnter(kind, key),
    )
    monkeypatch.setattr(
        session_module,
        "read_delay_stage",
        lambda *_args, **_kwargs: pytest.fail("removed delay-stage handle was read"),
    )
    monkeypatch.setattr(
        session_module,
        "read_scanner",
        lambda *_args, **_kwargs: pytest.fail("removed scanner handle was read"),
    )

    position = session.read_position()

    assert position.t_ps is None
    assert position.x_um is None
    assert position.y_um is None
    assert session.delay_stages == {}
    assert session.scanners == {}


def test_ownership_targets_normalize_controller_models_and_axes():
    assert DeviceSession._ownership_target(
        "lockin", {"lockin_model": " sr830 ", "resource": "GPIB0::1"}
    ) == ("lockin", "SR830", "GPIB0::1")
    assert DeviceSession._ownership_target(
        "delay_stage", {"delay_stage_controller": " gsc01 ", "port": "COM1"}
    ) == ("delay_stage", "GSC01", "COM1")
    assert DeviceSession._ownership_target(
        "scanner",
        {"scanner_controller": " conexagap ", "port": "COM2", "axis": 1},
    ) == ("scanner", "CONEXAGAP", "COM2", "U")
    assert DeviceSession._ownership_target(
        "scanner",
        {"controller": " conexcc ", "port": "COM3", "axis": " 2 "},
    ) == ("scanner", "CONEXCC", "COM3", "2")
    assert DeviceSession._ownership_target(
        "scanner",
        {"controller": "conexcc", "port": "COM3", "axis": "U"},
    ) == ("scanner", "CONEXCC", "COM3", "U")


def test_session_internal_dispatch_rejects_unsupported_device_kind():
    with pytest.raises(ValueError, match="Unsupported device kind: camera"):
        DeviceSession._ownership_target("camera", {})
    with pytest.raises(ValueError, match="Unsupported device kind: camera"):
        DeviceSession._connect_handle("camera", {})
    with pytest.raises(ValueError, match="Unsupported device kind: camera"):
        DeviceSession._disconnect_handle("camera", {})


@pytest.mark.parametrize(
    ("operation", "service_name", "expected"),
    [
        ("signal", "read_lockin_signal", {"X": 1.0}),
        ("settings", "read_lockin_settings", {"Sensitivity": 1.0}),
        ("overload", "read_lockin_overload", {"overload": False}),
        ("wait", "get_lockin_wait_time", 0.75),
        ("set", "service_set_lockin_settings", {"Sensitivity": 2e-6}),
    ],
)
def test_lockin_operations_auto_connect_missing_handle_once(
    monkeypatch, operation: str, service_name: str, expected
):
    handle = object()
    connects: list[dict] = []
    disconnects: list[dict] = []
    service_calls: list[tuple[dict, object, dict]] = []
    monkeypatch.setattr(
        session_module,
        "connect_lockin",
        lambda config: connects.append(config) or handle,
    )
    monkeypatch.setattr(session_module, "disconnect_lockin", disconnects.append)

    def service(config, *, lockin, **kwargs):
        service_calls.append((config, lockin, kwargs))
        return expected

    monkeypatch.setattr(session_module, service_name, service)
    session = DeviceSession(config_with_devices(), auto_connect=True)

    if operation == "signal":
        result = session.read_lockin_signal("lockin.main")
    elif operation == "settings":
        result = session.read_lockin_settings("lockin.main")
    elif operation == "overload":
        result = session.read_lockin_overload("lockin.main")
    elif operation == "wait":
        result = session.lockin_wait_time("lockin.main", multiplier=2.5)
    else:
        result = session.set_lockin_settings("lockin.main", sensitivity=2e-6)

    assert result == expected
    assert connects == [{"resource": "LOCKIN"}]
    assert service_calls[0][0] == {"resource": "LOCKIN"}
    assert service_calls[0][1] is handle
    assert session.lockins == {"main": handle}
    session.close()
    assert disconnects == [{"resource": "LOCKIN"}]


def test_initialize_scanner_reuses_existing_handle_without_rollback(monkeypatch):
    handle = object()
    connect_calls: list[dict] = []
    disconnects: list[dict] = []
    monkeypatch.setattr(
        session_module,
        "connect_scanner",
        lambda config: connect_calls.append(config) or handle,
    )
    monkeypatch.setattr(session_module, "disconnect_scanner", disconnects.append)
    monkeypatch.setattr(
        session_module,
        "service_initialize_scanner",
        lambda axis, config, *, scanner, on_status: {
            "axis": axis,
            "same_handle": scanner is handle,
        },
    )
    session = DeviceSession(config_with_devices(), auto_connect=False)
    session.connect_device("scanner.x")

    assert session.initialize_scanner("x") == {"axis": "x", "same_handle": True}
    assert len(connect_calls) == 1
    assert disconnects == []
    session.close()


def test_device_lock_prefers_shared_anchor_io_lock():
    from threading import RLock

    shared_lock = RLock()

    class Stage:
        _io_lock = shared_lock

    class Handle:
        _stage = Stage()

    session = DeviceSession(config_with_devices(), auto_connect=False)
    session.delay_stages["t"] = Handle()

    assert session._device_lock("delay_stage", "t") is shared_lock


def test_disconnect_skips_handle_removed_while_waiting_for_lock(monkeypatch):
    handle = object()
    disconnects: list[dict] = []
    monkeypatch.setattr(session_module, "connect_lockin", lambda _config: handle)
    monkeypatch.setattr(session_module, "disconnect_lockin", disconnects.append)
    session = DeviceSession(config_with_devices(), auto_connect=False)
    session.connect_device("lockin.main")

    class RemoveOnEnter:
        def __enter__(self):
            session._pop_connected_handle("lockin", "main")

        def __exit__(self, *_args):
            return False

    monkeypatch.setattr(session, "_device_lock", lambda _kind, _key: RemoveOnEnter())

    session.disconnect_device("lockin.main")

    assert disconnects == []
    assert session.lockins == {}


@pytest.mark.parametrize("operation", ["read", "move_delay", "move_scanner"])
def test_session_without_auto_connect_rejects_missing_operation_handle(
    monkeypatch, operation: str
):
    session = DeviceSession(config_with_devices(), auto_connect=False)
    monkeypatch.setattr(
        session_module,
        "connect_lockin",
        lambda _config: pytest.fail("disabled auto-connect reached lock-in connector"),
    )
    monkeypatch.setattr(
        session_module,
        "connect_delay_stage",
        lambda _config: pytest.fail("disabled auto-connect reached stage connector"),
    )
    monkeypatch.setattr(
        session_module,
        "connect_scanner",
        lambda _config: pytest.fail("disabled auto-connect reached scanner connector"),
    )

    with pytest.raises(RuntimeError, match="Device not connected"):
        if operation == "read":
            session.read_lockin_signal("lockin.main")
        elif operation == "move_delay":
            session.move_delay_stage(1.0)
        else:
            session.move_scanner("x", 1.0)


@pytest.mark.parametrize("method", ["initialize", "move"])
def test_scanner_operations_reject_invalid_axis_before_connection(
    monkeypatch, method: str
):
    session = DeviceSession(config_with_devices(), auto_connect=True)
    monkeypatch.setattr(
        session_module,
        "connect_scanner",
        lambda _config: pytest.fail("invalid axis reached scanner connector"),
    )

    with pytest.raises(ValueError, match="axis must be 'x' or 'y'"):
        if method == "initialize":
            session.initialize_scanner("z")
        else:
            session.move_scanner("z", 1.0)


def test_connected_devices_skips_non_mapping_instrument_group():
    config = config_with_devices()
    config["instruments"]["camera"] = ["invalid", "group"]
    session = DeviceSession(config, auto_connect=False)

    connected = session.connected_devices()

    assert "camera.invalid" not in connected
    assert connected["lockin.main"] is False


def test_read_live_status_without_lockin_keeps_lockin_fields_empty(monkeypatch):
    session = DeviceSession(config_with_devices(), auto_connect=False)
    monkeypatch.setattr(
        session,
        "read_lockin_signal",
        lambda *_args: pytest.fail("lock-in signal read without connected lock-in"),
    )
    monkeypatch.setattr(
        session,
        "read_lockin_settings",
        lambda *_args: pytest.fail("lock-in settings read without connected lock-in"),
    )
    monkeypatch.setattr(
        session,
        "read_lockin_overload",
        lambda *_args: pytest.fail("lock-in overload read without connected lock-in"),
    )

    status = session.read_live_status()

    assert status.signal is None
    assert status.lockin_settings is None
    assert status.lockin_overload is None


def test_initialize_existing_scanner_failure_preserves_handle(monkeypatch):
    handle = object()
    disconnects: list[dict] = []
    monkeypatch.setattr(session_module, "connect_scanner", lambda _config: handle)
    monkeypatch.setattr(session_module, "disconnect_scanner", disconnects.append)
    monkeypatch.setattr(
        session_module,
        "service_initialize_scanner",
        lambda *_args, **_kwargs: (_ for _ in ()).throw(OSError("home failed")),
    )
    session = DeviceSession(config_with_devices(), auto_connect=False)
    session.connect_device("scanner.x")

    with pytest.raises(OSError, match="home failed"):
        session.initialize_scanner("x")

    assert session.scanners == {"x": handle}
    assert disconnects == []
    session.close()


def test_connected_handle_without_shared_lock_reuses_session_device_lock():
    session = DeviceSession(config_with_devices(), auto_connect=False)
    session.lockins["main"] = object()

    first = session._device_lock("lockin", "main")
    second = session._device_lock("lockin", "main")

    assert first is second

    missing_first = session._device_lock("lockin", "missing")
    missing_second = session._device_lock("lockin", "missing")
    assert missing_first is missing_second
    assert session._connected_map("unsupported") == {}


def test_unconnected_missing_instrument_config_preserves_value_error():
    session = DeviceSession(config_with_devices(), auto_connect=False)

    with pytest.raises(ValueError, match=r"instruments\.lockin\.'missing'"):
        session._instrument_config("lockin", "missing")


def test_move_services_work_without_status_callbacks(monkeypatch):
    stage = object()
    scanner = object()
    monkeypatch.setattr(session_module, "connect_delay_stage", lambda _config: stage)
    monkeypatch.setattr(session_module, "connect_scanner", lambda _config: scanner)
    monkeypatch.setattr(
        session_module,
        "move_delay_stage_abs",
        lambda **_kwargs: {"t_ps": 1.0},
    )
    monkeypatch.setattr(
        session_module,
        "move_scanner_abs",
        lambda **_kwargs: {"axis": "x", "x_um": 2.0},
    )
    monkeypatch.setattr(session_module, "disconnect_delay_stage", lambda _config: None)
    monkeypatch.setattr(session_module, "disconnect_scanner", lambda _config: None)
    session = DeviceSession(config_with_devices(), auto_connect=True)

    assert session.move_delay_stage(1.0).t_ps == 1.0
    assert session.move_scanner("x", 2.0).x_um == 2.0

    session.close()


def test_shared_handle_close_releases_only_final_session_owner(monkeypatch):
    handle = object()
    disconnects: list[dict] = []
    monkeypatch.setattr(session_module, "connect_lockin", lambda _config: handle)
    monkeypatch.setattr(session_module, "disconnect_lockin", disconnects.append)
    first = DeviceSession(config_with_devices(), auto_connect=False)
    second = DeviceSession(config_with_devices(), auto_connect=False)
    first.connect_device("lockin.main")
    second.connect_device("lockin.main")
    target = DeviceSession._ownership_target("lockin", {"resource": "LOCKIN"})
    anchor_id = id(handle)

    assert DeviceSession._shared_targets[target][1] == anchor_id
    assert len(DeviceSession._shared_owners[anchor_id][1]) == 2

    first.close()

    assert disconnects == []
    assert first.lockins == {}
    assert second.lockins == {"main": handle}
    assert DeviceSession._shared_targets[target][1] == anchor_id
    assert len(DeviceSession._shared_owners[anchor_id][1]) == 1

    second.close()

    assert disconnects == [{"resource": "LOCKIN"}]
    assert second.lockins == {}
    assert target not in DeviceSession._shared_targets
    assert anchor_id not in DeviceSession._shared_owners


def test_in_place_connected_config_removal_blocks_io_but_close_uses_snapshot(
    monkeypatch,
):
    handle = object()
    disconnects: list[dict] = []
    reads = []
    monkeypatch.setattr(session_module, "connect_lockin", lambda _config: handle)
    monkeypatch.setattr(session_module, "disconnect_lockin", disconnects.append)
    monkeypatch.setattr(
        session_module,
        "read_lockin_signal",
        lambda config, *, lockin: reads.append(config) or {"X": 1.0},
    )
    session = DeviceSession(config_with_devices(), auto_connect=False)
    session.connect_device("lockin.main")
    del session.config["instruments"]["lockin"]["main"]

    with pytest.raises(
        RuntimeError, match=r"config was removed in place: lockin\.main"
    ):
        session.read_lockin_signal("lockin.main")

    assert reads == []
    session.close()
    assert disconnects == [{"resource": "LOCKIN"}]
