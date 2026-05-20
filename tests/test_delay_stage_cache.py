from __future__ import annotations

import pytest

import kohdalab.interfaces.delay_stage as delay_stage_module


class FakeDelayStageController:
    instances: list["FakeDelayStageController"] = []

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
        self.close_count = 0
        self.positions: dict[int, int] = {default_axis: 0}
        self.microstep_calls: list[int] = []
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

    def close(self):
        self.close_count += 1
        self.connected = False

    def is_connected(self) -> bool:
        return self.connected

    def get_pos_raw(self, *, axis: int) -> int:
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
        self.positions[axis] = 0

    def stop(self):
        return None


class OtherFakeDelayStageController(FakeDelayStageController):
    instances: list["OtherFakeDelayStageController"] = []

    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        OtherFakeDelayStageController.instances.append(self)


@pytest.fixture(autouse=True)
def fake_delay_stage_backend(monkeypatch):
    delay_stage_module._DELAY_STAGE_CONNECTIONS.clear()
    FakeDelayStageController.instances.clear()
    OtherFakeDelayStageController.instances.clear()
    monkeypatch.setitem(delay_stage_module.DELAY_STAGE_CONTROLLERS, "FAKE_STAGE", FakeDelayStageController)
    monkeypatch.setitem(delay_stage_module.DELAY_STAGE_CONTROLLERS, "OTHER_FAKE_STAGE", OtherFakeDelayStageController)
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


def test_connect_delay_stage_reuses_cached_handle_for_same_controller_and_port():
    first = delay_stage_module.connect_delay_stage(stage_config(controller_axis=1))
    second = delay_stage_module.connect_delay_stage(stage_config(controller_axis=2, pos_unit="mm"))

    assert second is first
    assert len(FakeDelayStageController.instances) == 1
    assert first.controller.configure_calls == [
        {"axis_count": 1, "default_axis": 2, "pos_unit": "mm"},
    ]
    assert first.axis == 2
    assert first.pos_unit == "mm"


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


def test_connect_delay_stage_replaces_stale_cached_handle():
    first = delay_stage_module.connect_delay_stage(stage_config())
    first.controller.connected = False

    second = delay_stage_module.connect_delay_stage(stage_config())

    assert second is not first
    assert len(FakeDelayStageController.instances) == 2
    assert delay_stage_module._DELAY_STAGE_CONNECTIONS[("FAKE_STAGE", "COM6")] is second


def test_delay_stage_cache_key_includes_controller_and_port():
    first = delay_stage_module.connect_delay_stage(stage_config(port="COM6", controller="FAKE_STAGE"))
    second = delay_stage_module.connect_delay_stage(stage_config(port="COM7", controller="FAKE_STAGE"))
    third = delay_stage_module.connect_delay_stage(stage_config(port="COM6", controller="OTHER_FAKE_STAGE"))

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


def test_disconnect_all_delay_stages_closes_each_cached_handle_once():
    first = delay_stage_module.connect_delay_stage(stage_config(port="COM6"))
    second = delay_stage_module.connect_delay_stage(stage_config(port="COM7"))

    delay_stage_module.disconnect_delay_stage()

    assert first.controller.close_count == 1
    assert second.controller.close_count == 1
    assert delay_stage_module._DELAY_STAGE_CONNECTIONS == {}
