from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
from threading import Barrier, Event

import pytest

import kohdalab.interfaces.delay_stage as delay_stage_module


class FakeDelayStageController:
    instances: list["FakeDelayStageController"] = []
    fail_positions = False
    construction_started: Event | None = None
    construction_release: Event | None = None

    def __init__(
        self,
        *,
        port: str,
        baudrate: int = 9600,
        timeout: float = 1.0,
        write_termination: str = "\r\n",
        read_termination: str = "\r\n",
        axis_count: int = 1,
        default_axis: int = 1,
        pos_unit: str = "pulse",
    ):
        if self.construction_started is not None:
            self.construction_started.set()
        if self.construction_release is not None and not self.construction_release.wait(
            2
        ):
            raise RuntimeError("timed out waiting to release controller construction")
        self.port = port
        self.baudrate = baudrate
        self.timeout = timeout
        self.write_termination = write_termination
        self.read_termination = read_termination
        self.axis_count = axis_count
        self.default_axis = default_axis
        self.pos_unit = pos_unit
        self.connected = True
        self.configure_calls: list[dict[str, object]] = []
        self.configure_errors: list[Exception] = []
        self.close_count = 0
        self.close_error: Exception | None = None
        self.positions: dict[int, int] = {default_axis: 0}
        self.microstep_calls: list[int] = []
        self.command_calls: list[tuple[str, object]] = []
        FakeDelayStageController.instances.append(self)

    def configure(self, *, axis_count: int, default_axis: int, pos_unit: str):
        self.configure_calls.append(
            {
                "axis_count": axis_count,
                "default_axis": default_axis,
                "pos_unit": pos_unit,
            }
        )
        self.axis_count = axis_count
        self.default_axis = default_axis
        self.pos_unit = pos_unit
        self.positions.setdefault(default_axis, 0)
        if self.configure_errors:
            raise self.configure_errors.pop(0)

    def close(self):
        self.close_count += 1
        if self.close_error is not None:
            raise self.close_error
        self.connected = False

    def is_connected(self) -> bool:
        return self.connected

    def get_pos_raw(self, *, axis: int) -> int:
        if self.fail_positions:
            raise OSError("initial position read failed")
        return self.positions.get(axis, 0)

    def move_abs_raw(self, pos_raw: int, *, axis: int, on_position=None) -> int:
        self.positions[axis] = int(pos_raw)
        if on_position is not None:
            on_position(self.positions[axis])
        return int(pos_raw)

    def move_rel_raw(self, delta_raw: int, *, axis: int, on_position=None) -> int:
        self.positions[axis] = self.positions.get(axis, 0) + int(delta_raw)
        if on_position is not None:
            on_position(self.positions[axis])
        return self.positions[axis]

    def get_microstep_division(self, *, axis: int) -> int:
        self.microstep_calls.append(axis)
        return 1

    def get_positions(self) -> list[int]:
        return [self.positions.get(axis, 0) for axis in sorted(self.positions)]

    def get_status(self) -> str:
        return "ready"

    def is_ready(self) -> bool:
        return True

    def home(self, *, axis: int):
        self.command_calls.append(("home", axis))
        self.positions[axis] = 0

    def stop(self):
        self.command_calls.append(("stop", None))

    def execute_drive(self):
        self.command_calls.append(("execute_drive", None))

    def jog(self, *, positive: bool, axis: int):
        self.command_calls.append(("jog", (positive, axis)))

    def set_excitation(self, *, enabled: bool, axis: int):
        self.command_calls.append(("set_excitation", (enabled, axis)))

    def set_logical_zero(self):
        self.command_calls.append(("set_logical_zero", None))

    def query_internal(self, code: str) -> str:
        self.command_calls.append(("query_internal", code))
        return f"value:{code}"


class OtherFakeDelayStageController(FakeDelayStageController):
    instances: list["OtherFakeDelayStageController"] = []

    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        OtherFakeDelayStageController.instances.append(self)


@pytest.fixture(autouse=True)
def fake_delay_stage_backend(monkeypatch):
    delay_stage_module._DELAY_STAGE_CONNECTIONS.clear()
    FakeDelayStageController.instances.clear()
    FakeDelayStageController.fail_positions = False
    FakeDelayStageController.construction_started = None
    FakeDelayStageController.construction_release = None
    OtherFakeDelayStageController.instances.clear()
    monkeypatch.setitem(
        delay_stage_module.DELAY_STAGE_CONTROLLERS,
        "FAKE_STAGE",
        FakeDelayStageController,
    )
    monkeypatch.setitem(
        delay_stage_module.DELAY_STAGE_CONTROLLERS,
        "OTHER_FAKE_STAGE",
        OtherFakeDelayStageController,
    )
    yield
    delay_stage_module._DELAY_STAGE_CONNECTIONS.clear()


def stage_config(
    *,
    port: str = "COM6",
    controller: str = "FAKE_STAGE",
    controller_axis: int = 1,
    pos_unit: str = "pulse",
) -> dict[str, object]:
    return {
        "controller": controller,
        "port": port,
        "controller_axis": controller_axis,
        "pos_unit": pos_unit,
        "pos_um_per_pulse": 1.0,
    }


def test_connect_delay_stage_rejects_unknown_controller_without_caching_handle():
    with pytest.raises(
        RuntimeError, match="Unsupported delay stage controller: UNKNOWN"
    ):
        delay_stage_module.connect_delay_stage(stage_config(controller="UNKNOWN"))

    assert FakeDelayStageController.instances == []
    assert delay_stage_module._DELAY_STAGE_CONNECTIONS == {}


def test_connect_delay_stage_rejects_controller_incompatible_with_stage(
    monkeypatch,
):
    monkeypatch.setitem(
        delay_stage_module.STAGES,
        "RESTRICTED_STAGE",
        {"controllers": ["OTHER_FAKE_STAGE"]},
    )
    config = stage_config(controller="FAKE_STAGE")
    config["stage"] = "RESTRICTED_STAGE"

    with pytest.raises(
        ValueError,
        match=r"RESTRICTED_STAGE.*only supported by \['OTHER_FAKE_STAGE'\].*FAKE_STAGE",
    ):
        delay_stage_module.connect_delay_stage(config)

    assert FakeDelayStageController.instances == []
    assert delay_stage_module._DELAY_STAGE_CONNECTIONS == {}


def test_connect_delay_stage_reuses_cached_handle_for_same_controller_and_port():
    first = delay_stage_module.connect_delay_stage(stage_config(controller_axis=1))
    second = delay_stage_module.connect_delay_stage(
        stage_config(controller_axis=2, pos_unit="mm")
    )

    assert second is first
    assert len(FakeDelayStageController.instances) == 1
    assert first.controller.configure_calls == [
        {"axis_count": 1, "default_axis": 2, "pos_unit": "mm"},
    ]
    assert first.axis == 2
    assert first.pos_unit == "mm"


def test_delay_stage_configure_failure_rolls_back_controller_and_config():
    stage = delay_stage_module.connect_delay_stage(stage_config(controller_axis=1))
    previous_config = stage.config
    stage.controller.configure_errors = [OSError("configure failed")]

    with pytest.raises(OSError, match="configure failed"):
        stage.configure(stage_config(controller_axis=2, pos_unit="mm"))

    assert stage.config is previous_config
    assert stage.controller.default_axis == 1
    assert stage.controller.pos_unit == "pulse"
    assert stage.controller.configure_calls == [
        {"axis_count": 1, "default_axis": 2, "pos_unit": "mm"},
        {"axis_count": 1, "default_axis": 1, "pos_unit": "pulse"},
    ]


def test_delay_stage_configure_reports_primary_and_rollback_failures():
    stage = delay_stage_module.connect_delay_stage(stage_config(controller_axis=1))
    previous_config = stage.config
    stage.controller.configure_errors = [
        OSError("configure failed"),
        OSError("rollback failed"),
    ]

    with pytest.raises(
        RuntimeError,
        match=r"configuration failed: configure failed; rollback failed: rollback failed",
    ) as exc_info:
        stage.configure(stage_config(controller_axis=2, pos_unit="mm"))

    assert isinstance(exc_info.value.__cause__, OSError)
    assert str(exc_info.value.__cause__) == "configure failed"
    assert stage.config is previous_config


def test_delay_stage_requires_conversion_parameters():
    config = stage_config()
    config.pop("pos_um_per_pulse")
    controller = FakeDelayStageController(port="COM6")
    stage = delay_stage_module.DelayStage(controller=controller, config=config)

    with pytest.raises(
        ValueError,
        match="requires 'pos_um_per_pulse'.*'screw_lead_mm_per_rev'.*'step_angle_deg'",
    ):
        stage.get_pos_um_per_pulse()


def test_delay_stage_conversion_fallback_uses_microstep_for_explicit_axis():
    config = stage_config(controller_axis=1)
    config.pop("pos_um_per_pulse")
    config.update({"screw_lead_mm_per_rev": 2.0, "step_angle_deg": 1.8})
    controller = FakeDelayStageController(port="COM6", axis_count=2, default_axis=1)
    controller.get_microstep_division = lambda *, axis: 8
    stage = delay_stage_module.DelayStage(controller=controller, config=config)

    assert stage.get_pos_um_per_pulse(axis=2) == pytest.approx(1.25)
    assert stage.pulse_to_pos_mm(800, axis=2) == pytest.approx(1.0)
    assert stage.pos_mm_to_pulse(1.0, axis=2) == 800
    assert stage.get_cached_microstep_division(axis=2) == 8


def test_delay_stage_direct_conversion_does_not_query_microstep_controller():
    controller = FakeDelayStageController(port="COM6")
    stage = delay_stage_module.DelayStage(
        controller=controller,
        config=stage_config(),
    )

    assert stage.get_pos_um_per_pulse(axis=1) == 1.0
    assert controller.microstep_calls == []


def test_delay_stage_rejects_positions_outside_configured_limits():
    config = stage_config()
    config.update({"min_pulse": 1000, "max_pulse": 2000})
    controller = FakeDelayStageController(port="COM6")
    stage = delay_stage_module.DelayStage(controller=controller, config=config)

    with pytest.raises(ValueError, match=r"position=0.999 mm is below limit 1.0 mm"):
        stage.move_pos_mm(0.999)
    with pytest.raises(ValueError, match=r"position=2.001 mm is above limit 2.0 mm"):
        stage.move_pos_mm(2.001)

    assert controller.positions == {1: 0}


def test_cached_delay_stage_probe_failure_restores_previous_config():
    stage = delay_stage_module.connect_delay_stage(stage_config(controller_axis=1))
    previous_config = stage.config
    FakeDelayStageController.fail_positions = True

    with pytest.raises(RuntimeError, match="initial position read failed"):
        delay_stage_module.connect_delay_stage(
            stage_config(controller_axis=2, pos_unit="mm")
        )

    assert stage.config is previous_config
    assert stage.controller.default_axis == 1
    assert stage.controller.pos_unit == "pulse"
    assert stage.controller.configure_calls == [
        {"axis_count": 1, "default_axis": 2, "pos_unit": "mm"},
        {"axis_count": 1, "default_axis": 1, "pos_unit": "pulse"},
    ]


def test_cached_delay_stage_reports_probe_and_rollback_failures(monkeypatch):
    stage = delay_stage_module.connect_delay_stage(stage_config(controller_axis=1))
    original_configure = stage.controller.configure
    configure_count = 0

    def fail_first_rollback(**settings):
        nonlocal configure_count
        configure_count += 1
        if configure_count == 2:
            raise OSError("rollback configure failed")
        return original_configure(**settings)

    monkeypatch.setattr(stage.controller, "configure", fail_first_rollback)
    FakeDelayStageController.fail_positions = True

    with pytest.raises(
        RuntimeError,
        match=(
            r"Cached delay-stage probe failed: initial position read failed; "
            r"configuration rollback failed: rollback configure failed"
        ),
    ) as exc_info:
        delay_stage_module.connect_delay_stage(
            stage_config(controller_axis=2, pos_unit="mm")
        )

    connection_cause = exc_info.value.__cause__
    assert isinstance(connection_cause, RuntimeError)
    assert isinstance(connection_cause.__cause__, OSError)
    assert str(connection_cause.__cause__) == "initial position read failed"
    assert delay_stage_module._DELAY_STAGE_CONNECTIONS[("FAKE_STAGE", "COM6")] is stage


def test_concurrent_connect_delay_stage_creates_one_cached_handle():
    callers_ready = Barrier(4)
    construction_started = Event()
    construction_release = Event()
    FakeDelayStageController.construction_started = construction_started
    FakeDelayStageController.construction_release = construction_release

    def connect_after_all_callers_are_ready():
        callers_ready.wait()
        return delay_stage_module.connect_delay_stage(stage_config())

    with ThreadPoolExecutor(max_workers=4) as executor:
        futures = [
            executor.submit(connect_after_all_callers_are_ready) for _ in range(4)
        ]
        assert construction_started.wait(2)
        construction_release.set()
        handles = [future.result(timeout=2) for future in futures]

    assert all(handle is handles[0] for handle in handles)
    assert len(FakeDelayStageController.instances) == 1


def test_delay_stage_move_position_callback_reports_pulses():
    stage = delay_stage_module.connect_delay_stage(stage_config())
    positions: list[int] = []

    stage.move_pos_mm(0.005, on_position=positions.append)

    assert positions == [5]


def test_delay_stage_caches_microstep_division_for_conversions():
    config = stage_config()
    config.pop("pos_um_per_pulse")
    config["screw_lead_mm_per_rev"] = 1.0
    config["step_angle_deg"] = 1.8
    stage = delay_stage_module.connect_delay_stage(config)
    controller = stage.controller

    stage.get_pos_mm()
    stage.get_pos_mm()
    assert controller.microstep_calls == [1]
    assert stage.get_cached_microstep_division() == 1


def test_delay_stage_successful_configure_clears_conversion_cache():
    config = stage_config()
    config.pop("pos_um_per_pulse")
    config.update({"screw_lead_mm_per_rev": 1.0, "step_angle_deg": 1.8})
    stage = delay_stage_module.connect_delay_stage(config)

    assert stage.get_microstep_division() == 1
    assert stage.get_cached_microstep_division() == 1

    replacement = dict(config, controller_axis=2)
    stage.configure(replacement)

    assert stage.config is replacement
    assert stage.axis == 2
    assert stage.get_cached_microstep_division() is None
    assert stage.get_microstep_division() == 1
    assert stage.controller.microstep_calls == [1, 2]


def test_delay_stage_travel_limit_is_inclusive_and_relative_progress_is_raw():
    config = stage_config()
    config.update({"min_pulse": None, "max_pulse": None, "travel_mm": 0.010})
    controller = FakeDelayStageController(port="COM6")
    controller.positions[1] = 5
    stage = delay_stage_module.DelayStage(controller=controller, config=config)
    progress: list[int] = []

    assert stage.get_limits() == (None, 0.010)
    assert stage.move_relative_pos_mm(0.005, on_position=progress.append) == 0.010
    assert progress == [10]
    assert controller.positions == {1: 10}

    with pytest.raises(ValueError, match="above limit 0.01 mm"):
        stage.move_relative_pos_mm(0.001)
    assert controller.positions == {1: 10}


def test_delay_stage_initialize_and_control_methods_delegate_selected_axis():
    config = stage_config(controller_axis=2)
    controller = FakeDelayStageController(port="COM6", axis_count=2, default_axis=2)
    controller.positions[2] = 20
    stage = delay_stage_module.DelayStage(controller=controller, config=config)

    info = stage.initialize(home=True)
    stage.execute_drive()
    stage.jog(positive=False)
    stage.set_excitation(True)
    stage.set_logical_zero()
    assert stage.query_internal("S2") == "value:S2"
    stage.stop()

    assert info["axis"] == 2
    assert info["pos_raw"] == 0
    assert info["pos_mm"] == 0.0
    assert info["ready"] is True
    assert controller.command_calls == [
        ("home", 2),
        ("execute_drive", None),
        ("jog", (False, 2)),
        ("set_excitation", (True, 2)),
        ("set_logical_zero", None),
        ("query_internal", "S2"),
        ("stop", None),
    ]


def test_delay_stage_read_and_raw_move_aliases_delegate_explicit_axis():
    config = stage_config(controller_axis=1)
    controller = FakeDelayStageController(port="COM6", axis_count=2, default_axis=1)
    controller.positions.update({1: 10, 2: 20})
    stage = delay_stage_module.DelayStage(controller=controller, config=config)

    assert stage.get_pulse(axis=2) == 20
    assert stage.get_positions() == [10, 20]
    assert stage.get_status() == "ready"
    assert stage.move_pulse(25, axis=2) == 25
    assert stage.move_relative_pos_raw(-5, axis=2) == 20
    assert controller.positions == {1: 10, 2: 20}


def test_delay_stage_initialize_without_home_preserves_position_and_axis():
    config = stage_config(controller_axis=1)
    controller = FakeDelayStageController(port="COM6", axis_count=2, default_axis=1)
    controller.positions[2] = 250
    stage = delay_stage_module.DelayStage(controller=controller, config=config)

    info = stage.initialize(home=False, axis=2)

    assert info["axis"] == 2
    assert info["pos_raw"] == 250
    assert info["pos_mm"] == 0.25
    assert not controller.command_calls


def test_connect_delay_stage_replaces_stale_cached_handle():
    first = delay_stage_module.connect_delay_stage(stage_config())
    first.controller.connected = False

    second = delay_stage_module.connect_delay_stage(stage_config())

    assert second is not first
    assert first.controller.close_count == 1
    assert len(FakeDelayStageController.instances) == 2
    assert delay_stage_module._DELAY_STAGE_CONNECTIONS[("FAKE_STAGE", "COM6")] is second


def test_connect_delay_stage_rolls_back_failed_initial_position_read():
    FakeDelayStageController.fail_positions = True

    with pytest.raises(RuntimeError, match="initial position read failed"):
        delay_stage_module.connect_delay_stage(stage_config())

    controller = FakeDelayStageController.instances[0]
    assert controller.close_count == 1
    assert not controller.connected
    assert delay_stage_module._DELAY_STAGE_CONNECTIONS == {}


def test_connect_delay_stage_reports_initial_probe_and_cleanup_failures(monkeypatch):
    original_init = FakeDelayStageController.__init__

    def initialize_with_close_failure(self, **kwargs):
        original_init(self, **kwargs)
        self.close_error = OSError("close after probe failed")

    monkeypatch.setattr(
        FakeDelayStageController, "__init__", initialize_with_close_failure
    )
    FakeDelayStageController.fail_positions = True

    with pytest.raises(
        RuntimeError,
        match=r"initial position read failed; cleanup failed: close after probe failed",
    ):
        delay_stage_module.connect_delay_stage(stage_config())

    assert delay_stage_module._DELAY_STAGE_CONNECTIONS == {}


def test_delay_stage_cache_key_includes_controller_and_port():
    first = delay_stage_module.connect_delay_stage(
        stage_config(port="COM6", controller="FAKE_STAGE")
    )
    second = delay_stage_module.connect_delay_stage(
        stage_config(port="COM7", controller="FAKE_STAGE")
    )
    third = delay_stage_module.connect_delay_stage(
        stage_config(port="COM6", controller="OTHER_FAKE_STAGE")
    )

    assert first is not second
    assert first is not third
    assert len(delay_stage_module._DELAY_STAGE_CONNECTIONS) == 3
    assert len(FakeDelayStageController.instances) == 3


def test_disconnect_delay_stage_closes_specific_cached_handle():
    first = delay_stage_module.connect_delay_stage(stage_config(port="COM6"))
    second = delay_stage_module.connect_delay_stage(stage_config(port="COM7"))

    delay_stage_module.disconnect_delay_stage(stage_config(port="COM6"))

    assert first.controller.close_count == 1
    assert second.controller.close_count == 0
    assert ("FAKE_STAGE", "COM6") not in delay_stage_module._DELAY_STAGE_CONNECTIONS
    assert delay_stage_module._DELAY_STAGE_CONNECTIONS[("FAKE_STAGE", "COM7")] is second


def test_disconnect_delay_stage_ignores_uncached_target():
    delay_stage_module.disconnect_delay_stage(stage_config(port="MISSING"))

    assert delay_stage_module._DELAY_STAGE_CONNECTIONS == {}


def test_disconnect_all_delay_stages_closes_each_cached_handle_once():
    first = delay_stage_module.connect_delay_stage(stage_config(port="COM6"))
    second = delay_stage_module.connect_delay_stage(stage_config(port="COM7"))

    delay_stage_module.disconnect_delay_stage()

    assert first.controller.close_count == 1
    assert second.controller.close_count == 1
    assert delay_stage_module._DELAY_STAGE_CONNECTIONS == {}


def test_disconnect_all_delay_stages_reports_failures_and_keeps_failed_handle():
    failed = delay_stage_module.connect_delay_stage(stage_config(port="COM6"))
    closed = delay_stage_module.connect_delay_stage(stage_config(port="COM7"))
    failed.controller.close_error = OSError("serial close failed")

    with pytest.raises(RuntimeError, match=r"FAKE_STAGE @ COM6: serial close failed"):
        delay_stage_module.disconnect_delay_stage()

    assert failed.controller.close_count == 1
    assert closed.controller.close_count == 1
    assert delay_stage_module._DELAY_STAGE_CONNECTIONS == {
        ("FAKE_STAGE", "COM6"): failed
    }

    failed.controller.close_error = None
    delay_stage_module.disconnect_delay_stage()
    assert failed.controller.close_count == 2
    assert delay_stage_module._DELAY_STAGE_CONNECTIONS == {}


def test_delay_stage_controller_construction_uses_explicit_transport_settings():
    config = stage_config(controller_axis=2, pos_unit="mm")
    config.update(
        {
            "baudrate": 19200,
            "timeout": 2.5,
            "write_termination": "\\r",
            "read_termination": "\\n",
            "axis_count": 2,
        }
    )

    stage = delay_stage_module.connect_delay_stage(config)
    controller = stage.controller

    assert controller.baudrate == 19200
    assert controller.timeout == 2.5
    assert controller.write_termination == "\\r"
    assert controller.read_termination == "\\n"
    assert controller.axis_count == 2
    assert controller.default_axis == 2
    assert controller.pos_unit == "mm"


def test_delay_stage_configure_without_optional_controller_hook_updates_wrapper():
    class ControllerWithoutConfigure:
        pass

    controller = ControllerWithoutConfigure()
    stage = delay_stage_module.DelayStage(
        controller=controller,  # type: ignore[arg-type]
        config=stage_config(controller_axis=1),
    )
    stage._microstep_divisions[1] = 8
    replacement = stage_config(controller_axis=2, pos_unit="mm")

    stage.configure(replacement)

    assert stage.config is replacement
    assert stage.axis == 2
    assert stage._microstep_divisions == {}


def test_delay_stage_axis_falls_back_to_legacy_default_axis():
    config = stage_config()
    config.pop("controller_axis")
    config["default_axis"] = 2
    controller = FakeDelayStageController(port="COM6", axis_count=2, default_axis=2)
    stage = delay_stage_module.DelayStage(controller=controller, config=config)

    assert stage.axis == 2
    assert stage.get_pos_raw() == 0


def test_delay_stage_raw_moves_forward_callbacks_for_explicit_axis():
    controller = FakeDelayStageController(port="COM6", axis_count=2, default_axis=1)
    stage = delay_stage_module.DelayStage(
        controller=controller,
        config=stage_config(controller_axis=1),
    )
    positions: list[int] = []

    assert stage.move_pos_raw(25, axis=2, on_position=positions.append) == 25
    assert stage.move_relative_pos_raw(-5, axis=2, on_position=positions.append) == 20
    assert positions == [25, 20]


def test_delay_stage_position_moves_support_no_callback_path():
    controller = FakeDelayStageController(port="COM6")
    stage = delay_stage_module.DelayStage(controller=controller, config=stage_config())

    assert stage.move_pos_mm(0.025) == pytest.approx(0.025)
    assert stage.move_relative_pos_mm(-0.005) == pytest.approx(0.02)
    assert controller.positions[1] == 20


def test_unrestricted_stage_metadata_allows_any_registered_controller(monkeypatch):
    monkeypatch.setitem(delay_stage_module.STAGES, "UNRESTRICTED", {"travel_mm": 1.0})
    config = stage_config()
    config["stage"] = "UNRESTRICTED"

    stage = delay_stage_module.connect_delay_stage(config)

    assert stage.stage_name == "UNRESTRICTED"


def test_restricted_stage_metadata_accepts_an_allowed_controller(monkeypatch):
    monkeypatch.setitem(
        delay_stage_module.STAGES,
        "RESTRICTED_ALLOWED",
        {"travel_mm": 1.0, "controllers": ["FAKE_STAGE"]},
    )
    config = stage_config()
    config["stage"] = "RESTRICTED_ALLOWED"

    stage = delay_stage_module.connect_delay_stage(config)

    assert stage.stage_name == "RESTRICTED_ALLOWED"
