from __future__ import annotations

import pytest

from kohdalab.apps.trkr_gui_snapshot import format_ps, format_snapshot_value


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (1.2344, "1.234"),
        (1.2345, "1.234"),
        (1.2346, "1.235"),
        (-0.0001, "-0.000"),
    ],
)
def test_format_ps_matches_existing_rounding(value, expected):
    assert format_ps(value) == expected


@pytest.mark.parametrize(
    ("key", "value", "scale", "expected"),
    [
        ("X_V", 0.00123, 1000.0, "1.230000e-03"),
        ("Y_V", 0.00234, 1000.0, "2.340000e-03"),
        ("R_V", 0.00345, 1000.0, "3.450000e-03"),
        ("Theta_deg", 12.3456, 1.0, "12.346"),
        ("t_ps", -122.1236, 1.0, "-122.124"),
        ("t_cor_ps", 1.2, 1.0, "1.200"),
        ("delay_stage_mm", 1.23456789, 1.0, "1.234568"),
        ("x_scanner_deg", 2.34567891, 1.0, "2.345679"),
        ("x_um", 61.5756, 1.0, "61.576"),
        ("y_cor_um", -0.3333, 1.0, "-0.333"),
        ("elapsed_s", 9.8765, 1.0, "9.877"),
        ("other", 1.23456789, 1.0, "1.234568"),
    ],
)
def test_format_snapshot_float_values(key, value, scale, expected):
    assert format_snapshot_value(key, value, voltage_scale=scale) == expected


def test_format_snapshot_non_float_values():
    assert format_snapshot_value("scan_axis", "x") == "x"
    assert format_snapshot_value("missing", "-") == "-"
    assert format_snapshot_value("count", 3) == "3"
