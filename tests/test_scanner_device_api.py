from __future__ import annotations

import math

import pytest

import kohdalab.api.devices.scanner as scanner_module


class FakeScanner:
    def __init__(self, *, unit: str = "mm", position: float = 0.0) -> None:
        self.config = {
            "controller": "CONEXCC",
            "origin_pos": 0.0,
            "sample_um_per_unit": 100.0,
        }
        self.unit = unit
        self.position = position
        self.origin_pos = 1.5
        self.moves: list[tuple[str, float]] = []
        self.initialize_calls: list[bool] = []

    def get_pos_unit(self) -> str:
        return self.unit

    def get_pos_mm(self) -> float:
        return self.position

    def get_pos_deg(self) -> float:
        return self.position

    def move_pos_mm(self, value: float, *, on_position=None) -> float:
        self.position = value
        self.moves.append(("mm", value))
        if on_position is not None:
            on_position(value)
        return value

    def move_pos_deg(self, value: float, *, on_position=None) -> float:
        self.position = value
        self.moves.append(("deg", value))
        if on_position is not None:
            on_position(value)
        return value

    def initialize(self, *, home: bool = True) -> dict:
        self.initialize_calls.append(home)
        return {"homed": home}

    def get_state(self) -> str:
        return "ready"

    def is_moving(self) -> bool:
        return False


def test_read_scanner_reports_raw_and_corrected_measurement_position():
    scanner = FakeScanner(position=2.5)

    row = scanner_module.read_scanner("X", scanner=scanner, zero_um=50.0)

    assert row == {
        "axis": "x",
        "x_um": 250.0,
        "x_mm": 2.5,
        "unit": "um",
        "zero_um": 50.0,
        "x_cor_um": 200.0,
    }


def test_move_scanner_progress_rows_use_normalized_coordinates():
    scanner = FakeScanner(unit="deg")
    progress: list[dict] = []

    row = scanner_module.move_scanner_abs(
        scanner_config={},
        axis="y",
        coordinate="sample_um",
        value=200.0,
        scanner=scanner,
        on_position=progress.append,
    )

    assert scanner.moves == [("deg", 2.0)]
    assert row["coordinate"] == "measurement"
    assert row["target"] == 200.0
    assert row["y_um"] == 200.0
    assert progress[0]["coordinate"] == "measurement"
    assert progress[0]["target"] == 200.0
    assert progress[0]["actual"] == 200.0
    assert progress[0]["y_deg"] == 2.0
    assert progress[0]["timestamp"].endswith("Z")


def test_move_scanner_interface_alias_preserves_control_target_and_callbacks():
    scanner = FakeScanner(position=0.25)
    statuses: list[str] = []
    progress: list[dict] = []

    row = scanner_module.move_scanner_abs(
        scanner_config={},
        axis="X",
        coordinate="pos_mm",
        value=1.25,
        scanner=scanner,
        on_status=statuses.append,
        on_position=progress.append,
    )

    assert scanner.moves == [("mm", 1.25)]
    assert statuses == ["moving scanner x"]
    assert row["coordinate"] == "interface"
    assert row["target"] == 1.25
    assert row["actual"] == 125.0
    assert row["x_mm"] == 1.25
    assert progress[0]["coordinate"] == "interface"
    assert progress[0]["target"] == 1.25
    assert progress[0]["actual"] == 125.0


def test_move_scanner_software_hysteresis_reports_both_moves():
    scanner = FakeScanner()
    scanner.config["software_hysteresis"] = {
        "enabled": True,
        "distance_um": 10.0,
        "direction": "negative",
    }
    statuses: list[str] = []
    progress: list[dict] = []

    scanner_module.move_scanner_abs(
        scanner_config={},
        axis="x",
        coordinate="measurement",
        value=100.0,
        scanner=scanner,
        on_status=statuses.append,
        on_position=progress.append,
    )

    assert scanner.moves == [("mm", 0.9), ("mm", 1.0)]
    assert statuses == [
        "moving scanner x software hysteresis",
        "moving scanner x",
    ]
    assert [item["target"] for item in progress] == [90.0, 100.0]
    assert [item["actual"] for item in progress] == [90.0, 100.0]


@pytest.mark.parametrize("value", [math.nan, math.inf, -math.inf, True])
def test_move_scanner_rejects_invalid_target_before_connect(monkeypatch, value):
    monkeypatch.setattr(
        scanner_module,
        "connect_scanner",
        lambda _config: pytest.fail("scanner connected before target validation"),
    )

    with pytest.raises(ValueError, match="scanner target must be finite"):
        scanner_module.move_scanner_abs(
            scanner_config={},
            axis="x",
            coordinate="measurement",
            value=value,
        )


@pytest.mark.parametrize("axis", ["", "z", "xy"])
def test_scanner_rejects_invalid_axis_before_connect(monkeypatch, axis: str):
    monkeypatch.setattr(
        scanner_module,
        "connect_scanner",
        lambda _config: pytest.fail("scanner connected before axis validation"),
    )

    with pytest.raises(ValueError, match="scanner axis must be 'x' or 'y'"):
        scanner_module.move_scanner_abs(
            scanner_config={},
            axis=axis,
            coordinate="measurement",
            value=1.0,
        )
    with pytest.raises(ValueError, match="scanner axis must be 'x' or 'y'"):
        scanner_module.read_scanner(axis, config={})


def test_move_scanner_hardware_failure_does_not_emit_position_or_change_state():
    class FailingScanner(FakeScanner):
        def move_pos_mm(self, value: float, *, on_position=None) -> float:
            raise OSError("scanner move failed")

    scanner = FailingScanner(position=0.5)
    statuses: list[str] = []
    progress: list[dict] = []

    with pytest.raises(OSError, match="scanner move failed"):
        scanner_module.move_scanner_abs(
            scanner_config={},
            axis="x",
            coordinate="measurement",
            value=100.0,
            scanner=scanner,
            on_status=statuses.append,
            on_position=progress.append,
        )

    assert scanner.position == 0.5
    assert scanner.moves == []
    assert statuses == ["moving scanner x"]
    assert progress == []


def test_scanner_rejects_non_finite_hardware_reading_and_zero():
    with pytest.raises(ValueError, match="scanner position must be finite"):
        scanner_module.read_scanner("x", scanner=FakeScanner(position=math.nan))
    with pytest.raises(ValueError, match="scanner zero_um must be finite"):
        scanner_module.read_scanner("x", scanner=FakeScanner(), zero_um=math.inf)


@pytest.mark.parametrize("unit", ["pulse", "unknown"])
def test_scanner_rejects_unknown_control_unit(unit: str):
    scanner = FakeScanner(unit=unit)

    with pytest.raises(ValueError, match="Unsupported scanner control unit"):
        scanner_module.read_scanner("x", scanner=scanner)


def test_software_hysteresis_rejects_invalid_distance_and_direction():
    scanner = FakeScanner()
    scanner.config["software_hysteresis"] = {
        "enabled": True,
        "distance_um": math.nan,
        "direction": "negative",
    }
    with pytest.raises(ValueError, match="distance_um.*finite"):
        scanner_module.move_scanner_abs(
            scanner_config={},
            axis="x",
            coordinate="measurement",
            value=100.0,
            scanner=scanner,
        )

    scanner.config["software_hysteresis"] = {
        "enabled": True,
        "distance_um": 10.0,
        "direction": "sideways",
    }
    with pytest.raises(ValueError, match="direction"):
        scanner_module.move_scanner_abs(
            scanner_config={},
            axis="x",
            coordinate="measurement",
            value=100.0,
            scanner=scanner,
        )


@pytest.mark.parametrize(("unit", "move"), [("mm", ("mm", 1.5)), ("deg", ("deg", 1.5))])
def test_initialize_scanner_moves_to_finite_origin(unit: str, move: tuple[str, float]):
    scanner = FakeScanner(unit=unit)
    statuses: list[str] = []

    info = scanner_module.initialize_scanner(
        "x", {}, scanner=scanner, on_status=statuses.append
    )

    assert scanner.initialize_calls == [True]
    assert scanner.moves == [move]
    assert statuses == ["x scanner initializing", "x scanner moving to origin"]
    assert info[f"pos_{unit}"] == 1.5
    assert info["pos_um"] == 150.0
    assert info["state"] == "ready"
    assert info["moving"] is False


def test_initialize_scanner_can_skip_home_and_origin_move():
    scanner = FakeScanner()

    info = scanner_module.initialize_scanner(
        "y",
        {},
        scanner=scanner,
        home=False,
        move_to_origin=False,
    )

    assert scanner.initialize_calls == [False]
    assert scanner.moves == []
    assert info == {"homed": False}


@pytest.mark.parametrize("origin", [math.nan, math.inf, -math.inf, True])
def test_initialize_scanner_rejects_invalid_origin_before_home(origin):
    scanner = FakeScanner()
    scanner.origin_pos = origin

    with pytest.raises(ValueError, match="origin_pos must be finite"):
        scanner_module.initialize_scanner("x", {}, scanner=scanner)

    assert scanner.initialize_calls == []


def test_initialize_scanner_rejects_out_of_range_origin_before_home():
    scanner = FakeScanner()
    scanner.config.update({"min_pos": 0.0, "max_pos": 1.0})
    scanner.origin_pos = 1.5

    with pytest.raises(ValueError, match="above scanner max_pos"):
        scanner_module.initialize_scanner("x", {}, scanner=scanner)

    assert scanner.initialize_calls == []


def test_connect_disconnect_and_actuator_catalog_services(monkeypatch):
    scanner = FakeScanner()
    connected: list[dict] = []
    disconnected: list[dict | None] = []
    monkeypatch.setattr(
        scanner_module,
        "_connect_scanner",
        lambda config: connected.append(config) or scanner,
    )
    monkeypatch.setattr(scanner_module, "_disconnect_scanner", disconnected.append)
    monkeypatch.setattr(
        scanner_module, "ACTUATOR_NAMES", ["ANY", "TRA12CC", "AG-M100D"]
    )
    monkeypatch.setattr(
        scanner_module,
        "ACTUATORS",
        {
            "ANY": {},
            "TRA12CC": {"controllers": ["CONEXCC"], "pos_unit": "mm"},
            "AGM100D": {"controllers": ["CONEXAGAP"], "pos_unit": "deg"},
        },
    )

    assert scanner_module.connect_scanner({"port": "COM1"}) is scanner
    scanner_module.disconnect_scanner({"port": "COM1"})
    scanner_module.disconnect_scanner()
    assert connected == [{"port": "COM1"}]
    assert disconnected == [{"port": "COM1"}, None]
    assert scanner_module.actuator_pos_unit(None) == "mm"
    assert scanner_module.actuator_pos_unit("AG-M100D") == "deg"
    assert scanner_module.list_actuators() == ["AG-M100D", "ANY", "TRA12CC"]
    assert scanner_module.list_actuators("conexcc") == ["ANY", "TRA12CC"]


@pytest.mark.parametrize("coordinate", ["instrument", "device", "mm"])
def test_move_scanner_legacy_control_aliases_normalize_to_interface(coordinate):
    scanner = FakeScanner()

    row = scanner_module.move_scanner_abs(
        scanner_config={},
        axis="x",
        coordinate=coordinate,
        value=1.25,
        scanner=scanner,
    )

    assert row["coordinate"] == "interface"
    assert scanner.moves == [("mm", 1.25)]


def test_software_hysteresis_positive_alias_and_policy_disable():
    scanner = FakeScanner()
    scanner.config["software_hysteresis"] = {
        "enabled": True,
        "approach_distance_um": 20.0,
        "approach": "+",
    }

    scanner_module.move_scanner_abs(
        scanner_config={},
        axis="x",
        coordinate="measurement",
        value=100.0,
        scanner=scanner,
    )
    assert scanner.moves == [("mm", 1.2), ("mm", 1.0)]

    scanner.moves.clear()
    scanner_module.move_scanner_abs(
        scanner_config={},
        axis="x",
        coordinate="measurement",
        value=200.0,
        scanner=scanner,
        apply_software_hysteresis=False,
    )
    assert scanner.moves == [("mm", 2.0)]


def test_non_dictionary_hysteresis_configuration_is_safely_disabled():
    scanner = FakeScanner()
    scanner.config["software_hysteresis"] = "enabled"  # type: ignore[assignment]

    scanner_module.move_scanner_abs(
        scanner_config={},
        axis="x",
        coordinate="measurement",
        value=100.0,
        scanner=scanner,
    )

    assert scanner.moves == [("mm", 1.0)]


def test_initialize_scanner_rejects_below_range_origin_before_home():
    scanner = FakeScanner()
    scanner.config.update({"min_pos": 0.0, "max_pos": 2.0})
    scanner.origin_pos = -0.1

    with pytest.raises(ValueError, match="below scanner min_pos"):
        scanner_module.initialize_scanner("x", {}, scanner=scanner)

    assert scanner.initialize_calls == []


def test_initialize_failure_does_not_attempt_origin_move_or_emit_move_status():
    class FailingInitializeScanner(FakeScanner):
        def initialize(self, *, home: bool = True) -> dict:
            self.initialize_calls.append(home)
            raise OSError("home failed")

    scanner = FailingInitializeScanner()
    statuses: list[str] = []

    with pytest.raises(OSError, match="home failed"):
        scanner_module.initialize_scanner(
            "x", {}, scanner=scanner, on_status=statuses.append
        )

    assert statuses == ["x scanner initializing"]
    assert scanner.moves == []


def test_read_scanner_auto_connects_and_omits_zero_fields_when_unspecified(monkeypatch):
    scanner = FakeScanner(position=0.5)
    configs: list[dict] = []
    monkeypatch.setattr(
        scanner_module,
        "_connect_scanner",
        lambda config: configs.append(config) or scanner,
    )

    row = scanner_module.read_scanner(" y ", {"port": "COM1"})

    assert configs == [{"port": "COM1"}]
    assert row == {"axis": "y", "y_um": 50.0, "y_mm": 0.5, "unit": "um"}


def test_hysteresis_zero_distance_and_non_conex_controller_skip_pre_move():
    scanner = FakeScanner()
    scanner.config["software_hysteresis"] = {"enabled": True, "distance_um": 0.0}

    scanner_module.move_scanner_abs(
        scanner_config={},
        axis="x",
        coordinate="measurement",
        value=100.0,
        scanner=scanner,
    )
    assert scanner.moves == [("mm", 1.0)]

    scanner.moves.clear()
    scanner.config["controller"] = "CONEXAGAP"
    scanner.config["software_hysteresis"] = {"enabled": True, "distance_um": 10.0}
    scanner_module.move_scanner_abs(
        scanner_config={},
        axis="x",
        coordinate="measurement",
        value=200.0,
        scanner=scanner,
    )
    assert scanner.moves == [("mm", 2.0)]


def test_hysteresis_rejects_negative_distance_before_move():
    scanner = FakeScanner()
    scanner.config["software_hysteresis"] = {"enabled": True, "pre_move_um": -1.0}

    with pytest.raises(ValueError, match="pre_move_um must be non-negative"):
        scanner_module.move_scanner_abs(
            scanner_config={},
            axis="x",
            coordinate="measurement",
            value=100.0,
            scanner=scanner,
        )

    assert scanner.moves == []


def test_progress_callback_rejects_nonfinite_control_position_before_consumer():
    scanner = FakeScanner()
    observed: list[dict] = []
    callback = scanner_module._scanner_progress_callback(
        scanner=scanner,
        axis="x",
        unit="mm",
        coordinate="measurement",
        target=1.0,
        on_position=observed.append,
    )
    assert callback is not None

    with pytest.raises(ValueError, match="progress position must be finite"):
        callback(math.nan)

    assert observed == []


def test_initialize_rejects_invalid_axis_and_control_unit_before_homing():
    scanner = FakeScanner(unit="pulse")

    with pytest.raises(ValueError, match="scanner axis must be"):
        scanner_module.initialize_scanner("z", {}, scanner=scanner)
    with pytest.raises(ValueError, match="Unsupported scanner control unit"):
        scanner_module.initialize_scanner("x", {}, scanner=scanner)

    assert scanner.initialize_calls == []


def test_degree_control_move_without_callback_uses_plain_driver_signature():
    scanner = FakeScanner(unit="deg")

    row = scanner_module.move_scanner_abs(
        scanner_config={},
        axis="y",
        coordinate="deg",
        value=1.5,
        scanner=scanner,
    )

    assert scanner.moves == [("deg", 1.5)]
    assert row["coordinate"] == "interface"
    assert row["y_deg"] == 1.5


def test_enabled_hysteresis_without_distance_skips_pre_move():
    scanner = FakeScanner()
    scanner.config["software_hysteresis"] = {"enabled": True}

    scanner_module.move_scanner_abs(
        scanner_config={},
        axis="x",
        coordinate="measurement",
        value=100.0,
        scanner=scanner,
    )

    assert scanner.moves == [("mm", 1.0)]


def test_move_scanner_rejects_unknown_coordinate_before_motion():
    scanner = FakeScanner()

    with pytest.raises(ValueError, match="scanner coordinate must be"):
        scanner_module.move_scanner_abs(
            scanner_config={},
            axis="x",
            coordinate="volts",
            value=1.0,
            scanner=scanner,
        )

    assert scanner.moves == []


def test_low_level_control_move_rejects_unknown_driver_unit():
    scanner = FakeScanner(unit="pulse")

    with pytest.raises(ValueError, match="Unsupported scanner control unit"):
        scanner_module._move_control(scanner, 1.0)

    assert scanner.moves == []
