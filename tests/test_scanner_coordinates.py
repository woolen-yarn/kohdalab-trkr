from __future__ import annotations

import pytest

from kohdalab.api.devices.scanner import move_scanner_abs
from kohdalab.api.scan_plan import normalize_scanner_coordinate


class FakeScanner:
    def __init__(self, *, unit: str = "mm"):
        self.config = {
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
