from __future__ import annotations

import pytest

from kohdalab.apps.trkr_gui_coordinates import (
    coordinate_correction_enabled,
    delay_stage_label_for_coordinate,
    delay_stage_unit_for_coordinate,
    normalize_coordinate,
    scanner_axis_spin_value,
    scanner_label_for_coordinate,
    scanner_scale_label_for_actuator,
    scanner_unit_for_coordinate,
)


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (None, "measurement"),
        ("measurement", "measurement"),
        (" interface ", "interface"),
        ("instrument", "instrument"),
        ("control", "interface"),
        ("device", "instrument"),
    ],
)
def test_normalize_coordinate(value, expected):
    assert normalize_coordinate(value) == expected


@pytest.mark.parametrize(
    ("coordinate", "unit", "label"),
    [
        ("measurement", "ps", "t (ps)"),
        ("interface", "mm", "delay_stage (mm)"),
        ("instrument", "pulse", "delay_stage (pulse)"),
        ("control", "mm", "delay_stage (mm)"),
        ("device", "pulse", "delay_stage (pulse)"),
    ],
)
def test_delay_stage_unit_and_label(coordinate, unit, label):
    assert delay_stage_unit_for_coordinate(coordinate) == unit
    assert delay_stage_label_for_coordinate(coordinate) == label


def test_scanner_unit_and_label_for_measurement_coordinate():
    assert scanner_unit_for_coordinate("measurement", actuator="TRA12CC") == "um"
    assert scanner_label_for_coordinate("x", "measurement", actuator="TRA12CC") == "x (um)"


def test_scanner_unit_and_label_for_interface_coordinate_use_actuator_unit():
    assert scanner_unit_for_coordinate("interface", actuator="TRA12CC") == "mm"
    assert scanner_label_for_coordinate("x", "interface", actuator="TRA12CC") == "scanner_x (mm)"
    assert scanner_unit_for_coordinate("instrument", actuator="AG-M100D") == "deg"
    assert scanner_label_for_coordinate("y", "instrument", actuator="AG-M100D") == "scanner_y (deg)"


def test_scanner_connected_unit_overrides_actuator_lookup():
    assert scanner_unit_for_coordinate("interface", actuator="TRA12CC", connected_unit="deg") == "deg"
    assert scanner_label_for_coordinate("x", "interface", actuator="TRA12CC", connected_unit="deg") == "scanner_x (deg)"


def test_scanner_scale_label_uses_actuator_unit():
    assert scanner_scale_label_for_actuator("TRA12CC") == "sample um / mm"
    assert scanner_scale_label_for_actuator("AG-M100D") == "sample um / deg"


@pytest.mark.parametrize(("value", "expected"), [("U", 1), ("V", 2), ("1", 1), (2, 2)])
def test_scanner_axis_spin_value_accepts_agap_axis_names(value, expected):
    assert scanner_axis_spin_value(value) == expected


@pytest.mark.parametrize(
    ("coordinate", "expected"),
    [
        ("measurement", True),
        ("interface", False),
        ("instrument", False),
        ("control", False),
        ("device", False),
    ],
)
def test_coordinate_correction_enabled(coordinate, expected):
    assert coordinate_correction_enabled(coordinate) is expected
