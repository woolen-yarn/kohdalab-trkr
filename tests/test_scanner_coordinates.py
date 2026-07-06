from __future__ import annotations

import pytest

from kohdalab.api.devices.scanner import move_scanner_abs
from kohdalab.api.scan_plan import normalize_scanner_coordinate


class FakeScanner:
    def __init__(self, *, unit: str = "mm"):
        self.config = {
            "controller": "CONEXCC",
            "origin_pos": 0.0,
            "sample_um_per_unit": 100.0,
        }
        self.unit = unit
        self.pos = 0.0
        self.moves: list[float] = []

    def get_pos_unit(self):
        return self.unit

    def get_pos_mm(self):
        return self.pos

    def move_pos_mm(self, value):
        self.moves.append(float(value))
        self.pos = float(value)

    def get_pos_deg(self):
        return self.pos

    def move_pos_deg(self, value):
        self.moves.append(float(value))
        self.pos = float(value)


@pytest.mark.parametrize(
    ("coordinate", "expected"),
    [
        (None, "measurement"),
        ("measurement", "measurement"),
        ("interface", "interface"),
        ("control", "interface"),
        ("instrument", "interface"),
        ("device", "interface"),
    ],
)
def test_normalize_scanner_coordinate_prefers_interface(coordinate, expected):
    assert normalize_scanner_coordinate(coordinate) == expected


def test_move_scanner_abs_converts_measurement_um_to_actuator_unit():
    scanner = FakeScanner()

    row = move_scanner_abs(
        scanner_config={},
        axis="x",
        coordinate="measurement",
        value=250.0,
        scanner=scanner,
    )

    assert scanner.moves == [2.5]
    assert row["coordinate"] == "measurement"
    assert row["actual"] == 250.0
    assert row["x_mm"] == 2.5


def test_move_scanner_abs_applies_negative_software_hysteresis_in_measurement_um():
    scanner = FakeScanner()
    scanner.config["software_hysteresis"] = {
        "enabled": True,
        "distance_um": 20.0,
        "direction": "negative",
    }

    row = move_scanner_abs(
        scanner_config={},
        axis="x",
        coordinate="measurement",
        value=250.0,
        scanner=scanner,
    )

    assert scanner.moves == [2.3, 2.5]
    assert row["actual"] == 250.0


def test_move_scanner_abs_reports_software_hysteresis_status():
    scanner = FakeScanner()
    scanner.config["software_hysteresis"] = {
        "enabled": True,
        "distance_um": 20.0,
        "direction": "negative",
    }
    statuses: list[str] = []

    move_scanner_abs(
        scanner_config={},
        axis="x",
        coordinate="measurement",
        value=250.0,
        scanner=scanner,
        on_status=statuses.append,
    )

    assert statuses == ["moving scanner x software hysteresis", "moving scanner x"]


def test_move_scanner_abs_ignores_software_hysteresis_for_conexagap():
    scanner = FakeScanner()
    scanner.config["controller"] = "CONEXAGAP"
    scanner.config["software_hysteresis"] = {
        "enabled": True,
        "distance_um": 20.0,
        "direction": "negative",
    }

    row = move_scanner_abs(
        scanner_config={},
        axis="x",
        coordinate="measurement",
        value=250.0,
        scanner=scanner,
    )

    assert scanner.moves == [2.5]
    assert row["actual"] == 250.0


def test_move_scanner_abs_applies_positive_software_hysteresis_to_interface_target():
    scanner = FakeScanner()
    scanner.config["software_hysteresis"] = {
        "enabled": True,
        "distance_um": 20.0,
        "direction": "positive",
    }

    row = move_scanner_abs(
        scanner_config={},
        axis="x",
        coordinate="interface",
        value=2.5,
        scanner=scanner,
    )

    assert scanner.moves == [2.7, 2.5]
    assert row["coordinate"] == "interface"
    assert row["actual"] == 250.0


def test_move_scanner_abs_can_skip_configured_software_hysteresis():
    scanner = FakeScanner()
    scanner.config["software_hysteresis"] = {
        "enabled": True,
        "distance_um": 20.0,
        "direction": "negative",
    }

    row = move_scanner_abs(
        scanner_config={},
        axis="x",
        coordinate="measurement",
        value=250.0,
        scanner=scanner,
        apply_software_hysteresis=False,
    )

    assert scanner.moves == [2.5]
    assert row["actual"] == 250.0


@pytest.mark.parametrize("coordinate", ["interface", "control", "instrument", "device", "mm", "pos_mm"])
def test_move_scanner_abs_treats_raw_scanner_aliases_as_interface(coordinate):
    scanner = FakeScanner()

    row = move_scanner_abs(
        scanner_config={},
        axis="x",
        coordinate=coordinate,
        value=2.5,
        scanner=scanner,
    )

    assert scanner.moves == [2.5]
    assert row["coordinate"] == "interface"
    assert row["actual"] == 250.0
    assert row["x_mm"] == 2.5


def test_move_scanner_abs_uses_connected_deg_unit_for_interface_alias():
    scanner = FakeScanner(unit="deg")

    row = move_scanner_abs(
        scanner_config={},
        axis="y",
        coordinate="instrument",
        value=1.5,
        scanner=scanner,
    )

    assert scanner.moves == [1.5]
    assert row["coordinate"] == "interface"
    assert row["actual"] == 150.0
    assert row["y_deg"] == 1.5
