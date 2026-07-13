from __future__ import annotations

import pytest

from kohdalab.apps.trkr_gui_signal import (
    lockin_display_from_settings,
    overload_display_from_status,
    signal_view_config,
    time_constant_display,
    voltage_scale_from_sensitivity,
)


@pytest.mark.parametrize(
    ("sensitivity", "scale", "unit"),
    [
        (5e-9, 1e9, "nV"),
        (5e-6, 1e6, "uV"),
        (5e-3, 1e3, "mV"),
        (5.0, 1.0, "V"),
    ],
)
def test_voltage_scale_from_sensitivity(sensitivity, scale, unit):
    assert voltage_scale_from_sensitivity(sensitivity) == (scale, unit)


def test_signal_view_config_for_xy_mode():
    view = signal_view_config("X / Y", "mV")

    assert view.signal1_key == "X_V"
    assert view.signal2_key == "Y_V"
    assert view.title1 == "X"
    assert view.title2 == "Y"
    assert view.unit1 == "mV"
    assert view.unit2 == "mV"


def test_signal_view_config_for_r_theta_mode():
    view = signal_view_config("R / Theta", "uV")

    assert view.signal1_key == "R_V"
    assert view.signal2_key == "Theta_deg"
    assert view.title1 == "R"
    assert view.title2 == "Theta"
    assert view.unit1 == "uV"
    assert view.unit2 == "deg"


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (0.0003, "0.3 ms"),
        (0.5, "500 ms"),
        (2.0, "2 s"),
    ],
)
def test_time_constant_display(value, expected):
    assert time_constant_display(value) == expected


def test_lockin_display_from_settings_formats_values():
    display = lockin_display_from_settings(
        {
            "Sensitivity": 5e-6,
            "Time Constant": 0.3,
            "Ref. Freq": 1234.567,
        }
    )

    assert display.sensitivity == "5 uV"
    assert display.time_constant == "300 ms"
    assert display.ref_freq == "1234.57 Hz"
    assert display.voltage_scale == 1e6
    assert display.voltage_unit == "uV"
    assert display.x_title == "X (uV)"
    assert display.theta_title == "Theta (deg)"


def test_lockin_display_from_settings_handles_missing_optional_values():
    display = lockin_display_from_settings({"Sensitivity": 2.0})

    assert display.sensitivity == "2 V"
    assert display.time_constant == "-"
    assert display.ref_freq == "-"


def test_overload_display_from_status_ok_and_missing():
    assert overload_display_from_status(None) == "-"
    assert overload_display_from_status({"overload_byte": 0}) == "-"
    assert (
        overload_display_from_status({"overload": False, "input_overload": True})
        == "OVERLOAD"
    )
    assert overload_display_from_status({"overload": True}) == "OVERLOAD"


def test_overload_display_from_status_ignores_output_flags():
    assert (
        overload_display_from_status({"x_output_overload": True, "overload_byte": 16})
        == "-"
    )
