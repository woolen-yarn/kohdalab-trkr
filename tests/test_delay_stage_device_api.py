from __future__ import annotations

import math

import pytest

import kohdalab.api.devices.delay_stage as delay_module
from kohdalab.api.devices.delay_stage import LIGHT_SPEED_MM_PER_PS, DelayStageDevice


class FakeController:
    def is_ready(self) -> bool:
        return True


class FakeStage:
    def __init__(
        self,
        *,
        position_mm: float = 5.0,
        limits: tuple[float | None, float | None] = (0.0, 10.0),
    ) -> None:
        self.position_mm = position_mm
        self.limits = limits
        self.controller = FakeController()
        self.status = "ready"
        self.initialize_calls: list[bool] = []
        self.moves: list[tuple[str, float | int]] = []
        self.connected = True

    def is_connected(self) -> bool:
        return self.connected

    def get_limits(self):
        return self.limits

    def get_pos_mm(self) -> float:
        return self.position_mm

    def get_pulse(self) -> int:
        return round(self.position_mm * 1000)

    def get_status(self) -> str:
        return self.status

    def pulse_to_pos_mm(self, pulse: int) -> float:
        return pulse / 1000.0

    def move_pos_mm(self, value: float, *, on_position=None) -> float:
        self.position_mm = float(value)
        self.moves.append(("mm", self.position_mm))
        if on_position is not None:
            on_position(self.get_pulse())
        return self.position_mm

    def move_pulse(self, pulse: int, *, on_position=None) -> int:
        self.position_mm = pulse / 1000.0
        self.moves.append(("pulse", pulse))
        if on_position is not None:
            on_position(pulse)
        return pulse

    def initialize(self, *, home: bool = False) -> dict:
        self.initialize_calls.append(home)
        if home:
            self.position_mm = 0.0
        return {"homed": home}


@pytest.mark.parametrize(
    ("direction", "expected_mm"),
    [(0, 5.0 + LIGHT_SPEED_MM_PER_PS), (1, 5.0 - LIGHT_SPEED_MM_PER_PS)],
)
def test_measurement_coordinate_conversion_is_direction_aware(
    direction: int, expected_mm: float
):
    raw = FakeStage()
    stage = DelayStageDevice(raw, {"direction": direction})

    actual = stage.move_coordinate(2.0, "measurement")

    assert raw.position_mm == pytest.approx(expected_mm)
    assert actual == pytest.approx(2.0)


def test_delay_stage_device_exposes_underlying_connection_health():
    raw = FakeStage()
    stage = DelayStageDevice(raw, {"direction": 0})

    assert stage.is_connected()
    raw.connected = False
    assert not stage.is_connected()


def test_measurement_zero_can_be_explicit_for_stage_without_travel_limit():
    raw = FakeStage(position_mm=2.0, limits=(None, None))
    stage = DelayStageDevice(raw, {"direction": 0, "zero_pos_mm": 2.0})

    assert stage.get_delay_ps() == pytest.approx(0.0)
    stage.move_coordinate(10.0, "measurement")
    assert raw.position_mm == pytest.approx(2.0 + 10.0 * LIGHT_SPEED_MM_PER_PS / 2.0)


def test_measurement_coordinate_rejects_unstable_or_invalid_zero():
    no_limit = DelayStageDevice(FakeStage(limits=(None, None)), {"direction": 0})
    reversed_limits = DelayStageDevice(FakeStage(limits=(10.0, 0.0)), {"direction": 0})
    invalid_zero = DelayStageDevice(
        FakeStage(), {"direction": 0, "zero_pos_mm": math.nan}
    )

    with pytest.raises(ValueError, match="zero_pos_mm or a finite maximum"):
        no_limit.get_delay_ps()
    with pytest.raises(ValueError, match="finite and increasing"):
        reversed_limits.get_delay_ps()
    with pytest.raises(ValueError, match="zero_pos_mm must be finite"):
        invalid_zero.get_delay_ps()


def test_interface_and_instrument_coordinates_report_progress():
    raw = FakeStage()
    stage = DelayStageDevice(raw, {"direction": 0})
    progress: list[dict] = []

    assert stage.move_coordinate(6.0, "interface", on_position=progress.append) == 6.0
    assert (
        stage.move_coordinate(7000, "instrument", on_position=progress.append) == 7000
    )

    assert raw.moves == [("mm", 6.0), ("pulse", 7000)]
    assert [row["stage_pulse"] for row in progress] == [6000, 7000]
    assert [row["stage_mm"] for row in progress] == [6.0, 7.0]
    assert all(row["timestamp"].endswith("Z") for row in progress)


@pytest.mark.parametrize(
    ("value", "coordinate", "message"),
    [
        (math.nan, "measurement", "must be finite"),
        (math.inf, "interface", "must be finite"),
        (1.5, "instrument", "integer pulse"),
        (True, "instrument", "must be finite"),
    ],
)
def test_move_coordinate_rejects_invalid_targets(value, coordinate: str, message: str):
    stage = DelayStageDevice(FakeStage(), {"direction": 0})

    with pytest.raises(ValueError, match=message):
        stage.move_coordinate(value, coordinate)


def test_move_coordinate_rejects_unknown_coordinate():
    with pytest.raises(ValueError, match="coordinate must be"):
        DelayStageDevice(FakeStage(), {"direction": 0}).move_coordinate(1.0, "unknown")


@pytest.mark.parametrize(
    ("coordinate", "expected_unit", "expected_actual"),
    [
        ("ps", "ps", 2.0),
        ("mm", "mm", 6.0),
        ("pulse", "pulse", 6000),
    ],
)
def test_move_delay_stage_abs_normalizes_coordinate_aliases(
    coordinate, expected_unit, expected_actual
):
    stage = DelayStageDevice(FakeStage(), {"direction": 0})
    value = {"ps": 2.0, "mm": 6.0, "pulse": 6000.0}[coordinate]

    row = delay_module.move_delay_stage_abs(
        delay_stage_config=stage.config,
        coordinate=coordinate,
        value=value,
        delay_stage=stage,
    )

    assert row["coordinate"] == coordinate
    assert row["unit"] == expected_unit
    assert row["actual"] == pytest.approx(expected_actual)
    assert row["timestamp"].endswith("Z")


def test_delay_stage_service_normalizes_legacy_sgsp_stage_spelling():
    assert delay_module._normalize_stage_name(" sgsp_46-500 ") == "SGSP46-500"
    assert delay_module._normalize_stage_name("osms20-35") == "OSMS20-35"


def test_move_delay_stage_abs_rejects_unknown_coordinate():
    stage = DelayStageDevice(FakeStage(), {"direction": 0})

    with pytest.raises(ValueError, match="coordinate must be"):
        delay_module.move_delay_stage_abs(
            delay_stage_config=stage.config,
            coordinate="unknown",
            value=1.0,
            delay_stage=stage,
        )


def test_connect_disconnect_read_and_initialize_services(monkeypatch):
    raw = FakeStage()
    connected_configs: list[dict] = []
    disconnected_configs: list[dict | None] = []

    def connect(config):
        connected_configs.append(config)
        return raw

    monkeypatch.setattr(delay_module, "_connect_delay_stage", connect)
    monkeypatch.setattr(
        delay_module, "_disconnect_delay_stage", disconnected_configs.append
    )
    config = {"stage": " sgsp_46_500 ", "direction": 0}

    stage = delay_module.connect_delay_stage(config)
    assert connected_configs == [{"stage": "SGSP46-500", "direction": 0}]
    assert delay_module.read_delay_stage(delay_stage=stage) == {
        "axis": "t",
        "t_ps": 0.0,
        "stage_mm": 5.0,
        "stage_pulse": 5000,
    }

    statuses: list[str] = []
    info = delay_module.initialize_delay_stage(
        config,
        delay_stage=stage,
        on_status=statuses.append,
    )
    assert connected_configs == [{"stage": "SGSP46-500", "direction": 0}]
    assert raw.initialize_calls == [True]
    assert raw.moves[-1] == ("mm", 5.0)
    assert statuses == ["delay_stage initializing", "delay_stage moving to t_ps=0"]
    assert info["ready"] is True
    assert info["delay_ps"] == pytest.approx(0.0)

    delay_module.disconnect_delay_stage(config)
    delay_module.disconnect_delay_stage()
    assert disconnected_configs == [{"stage": "SGSP46-500", "direction": 0}, None]


def test_list_stages_filters_controller_compatibility(monkeypatch):
    monkeypatch.setattr(delay_module, "STAGE_NAMES", ["ANY", "SHOT", "GSC"])
    monkeypatch.setattr(
        delay_module,
        "STAGES",
        {
            "ANY": {},
            "SHOT": {"controllers": ["SHOT302GS"]},
            "GSC": {"controllers": ["GSC01"]},
        },
    )

    assert delay_module.list_stages() == ["ANY", "GSC", "SHOT"]
    assert delay_module.list_stages("shot302gs") == ["ANY", "SHOT"]


@pytest.mark.parametrize("coordinate", ["measurement", "interface"])
def test_move_coordinate_rejects_boolean_before_device_io(coordinate: str):
    raw = FakeStage()
    stage = DelayStageDevice(raw, {"direction": 0})

    with pytest.raises(ValueError, match="must be finite"):
        stage.move_coordinate(True, coordinate)

    assert raw.moves == []


@pytest.mark.parametrize(
    ("coordinate", "value", "expected_unit", "expected_actual"),
    [
        (" t_ps ", 2.0, "ps", 2.0),
        ("pos_mm", 6.0, "mm", 6.0),
        ("device", 6000, "pulse", 6000),
    ],
)
def test_move_delay_stage_abs_accepts_all_documented_aliases_and_whitespace(
    coordinate: str, value: float, expected_unit: str, expected_actual: float
):
    stage = DelayStageDevice(FakeStage(), {"direction": 0})

    row = delay_module.move_delay_stage_abs(
        delay_stage_config=stage.config,
        coordinate=coordinate,
        value=value,
        delay_stage=stage,
    )

    assert row["coordinate"] == coordinate.strip().lower()
    assert row["unit"] == expected_unit
    assert row["actual"] == pytest.approx(expected_actual)


def test_progress_callback_failure_propagates_after_reporting_actual_position():
    raw = FakeStage()
    stage = DelayStageDevice(raw, {"direction": 0})
    observed: list[dict] = []

    def fail_after_observing(row: dict) -> None:
        observed.append(row)
        raise RuntimeError("progress consumer failed")

    with pytest.raises(RuntimeError, match="progress consumer failed"):
        stage.move_coordinate(6.0, "interface", on_position=fail_after_observing)

    assert raw.position_mm == 6.0
    assert observed[0]["stage_mm"] == 6.0
    assert observed[0]["stage_pulse"] == 6000


def test_missing_lower_limit_defaults_measurement_zero_to_zero_based_midpoint():
    stage = DelayStageDevice(
        FakeStage(position_mm=5.0, limits=(None, 10.0)), {"direction": 0}
    )

    assert stage.get_delay_ps() == pytest.approx(0.0)


def test_read_service_connects_when_device_is_not_supplied(monkeypatch):
    raw = FakeStage()
    observed_configs: list[dict] = []

    def connect(config):
        observed_configs.append(config)
        return raw

    monkeypatch.setattr(delay_module, "_connect_delay_stage", connect)

    row = delay_module.read_delay_stage({"direction": 0})

    assert observed_configs == [{"direction": 0, "stage": None}]
    assert row["stage_pulse"] == 5000
