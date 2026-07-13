from __future__ import annotations

import math

import pytest

import kohdalab.api.scan_limits as scan_limits_module
from kohdalab.api.scan_limits import (
    delay_stage_scan_limits,
    preflight_axis_targets,
    scanner_scan_limits,
)


def config():
    return {
        "instruments": {
            "delay_stage": {
                "t": {
                    "controller": "SHOT302GS",
                    "stage": "SGSP46-500",
                    "port": "fake",
                    "direction": 1,
                }
            },
            "scanner": {
                "x": {
                    "controller": "CONEXAGAP",
                    "actuator": "AG-M100D",
                    "port": "fake",
                    "axis": "U",
                    "sample_um_per_unit": 100.0,
                }
            },
        },
        "measurements": {},
    }


def test_delay_stage_scan_limits_use_stage_toml_and_t_zero_offset():
    limits = delay_stage_scan_limits(stage="SGSP46-500", direction=1, t_zero_ps=-122.0)

    assert limits.unit == "ps"
    assert limits.minimum == pytest.approx(-1545.0, abs=1.0)
    assert limits.maximum == pytest.approx(1789.0, abs=1.0)
    assert limits.minimum_step is None


def test_delay_stage_scan_limits_use_microstep_for_minimum_step_when_available():
    limits = delay_stage_scan_limits(
        stage="SGSP46-500",
        direction=1,
        t_zero_ps=-122.0,
        microstep_division=20,
    )

    assert limits.minimum_step == pytest.approx(0.0066713, rel=1e-3)


def test_delay_stage_scan_limits_support_pulse_defined_stage_specs(monkeypatch):
    monkeypatch.setitem(
        scan_limits_module.STAGES,
        "PULSE-STAGE",
        {
            "min_pulse": 100,
            "max_pulse": 1100,
            "pos_um_per_pulse": 0.5,
        },
    )

    limits = delay_stage_scan_limits(stage="PULSE-STAGE", direction=0, t_zero_ps=0.0)

    assert limits.minimum is not None
    assert limits.maximum is not None
    assert limits.minimum_step == pytest.approx(
        2.0 * 0.0005 / scan_limits_module.LIGHT_SPEED_MM_PER_PS
    )


def test_delay_stage_scan_limits_reject_incomplete_stage_travel_spec(monkeypatch):
    monkeypatch.setitem(scan_limits_module.STAGES, "INCOMPLETE", {"pos_unit": "mm"})

    limits = delay_stage_scan_limits(stage="INCOMPLETE", direction=0, t_zero_ps=0.0)

    assert limits.minimum is limits.maximum is limits.minimum_step is None


def test_scanner_scan_limits_use_actuator_toml_and_sample_offset():
    limits = scanner_scan_limits(
        actuator="TRA12CC", sample_um_per_unit=582.0, zero_um=61.5756
    )

    assert limits.unit == "um"
    assert limits.minimum == pytest.approx(-3553.5756)
    assert limits.maximum == pytest.approx(3430.4244)
    assert limits.minimum_step == pytest.approx(0.1164)


def test_scanner_scan_limits_reject_incomplete_actuator_position_spec(monkeypatch):
    monkeypatch.setitem(
        scan_limits_module.ACTUATORS,
        "INCOMPLETE",
        {"min_pos": 0.0, "pos_unit": "mm"},
    )

    limits = scanner_scan_limits(
        actuator="INCOMPLETE", sample_um_per_unit=1.0, zero_um=0.0
    )

    assert limits.minimum is limits.maximum is limits.minimum_step is None


def test_scan_limits_return_empty_values_for_unknown_specs():
    assert (
        delay_stage_scan_limits(stage="missing", direction=1, t_zero_ps=0.0).minimum
        is None
    )
    assert (
        scanner_scan_limits(
            actuator="missing", sample_um_per_unit=1.0, zero_um=0.0
        ).maximum
        is None
    )


def test_preflight_rejects_target_outside_configured_actuator_limits():
    with pytest.raises(ValueError, match="outside"):
        preflight_axis_targets(
            config(),
            measurement_name="srkr",
            axis="x",
            targets=[76.0],
            coordinate="measurement",
        )


def test_preflight_accepts_targets_inside_configured_limits():
    preflight_axis_targets(
        config(),
        measurement_name="trkr",
        axis="t",
        targets=[-100.0, 0.0, 100.0],
        coordinate="measurement",
    )


@pytest.mark.parametrize("axis", ["", "z", "time"])
def test_preflight_rejects_unknown_axes(axis: str):
    with pytest.raises(ValueError, match="axis must be"):
        preflight_axis_targets(
            config(), measurement_name="scan", axis=axis, targets=[0.0]
        )


def test_preflight_rejects_empty_and_non_finite_targets():
    with pytest.raises(ValueError, match="at least one target"):
        preflight_axis_targets(config(), measurement_name="trkr", axis="t", targets=[])
    for value in (math.nan, math.inf, -math.inf):
        with pytest.raises(ValueError, match="must be finite"):
            preflight_axis_targets(
                config(), measurement_name="trkr", axis="t", targets=[value]
            )


@pytest.mark.parametrize(
    ("axis", "coordinate"),
    [("t", "unknown"), ("x", "pulse")],
)
def test_preflight_rejects_coordinates_unsupported_by_the_axis(
    axis: str, coordinate: str
):
    with pytest.raises(ValueError, match="coordinate must be"):
        preflight_axis_targets(
            config(),
            measurement_name="scan",
            axis=axis,
            targets=[0.0],
            coordinate=coordinate,
        )


def test_preflight_accepts_documented_coordinate_aliases():
    preflight_axis_targets(
        config(),
        measurement_name="trkr",
        axis="t",
        targets=[0.0],
        coordinate="ps",
    )
    preflight_axis_targets(
        config(),
        measurement_name="srkr",
        axis="x",
        targets=[0.0],
        coordinate="sample_um",
    )
    preflight_axis_targets(
        config(),
        measurement_name="trkr",
        axis="t",
        targets=[0.0, 500.0],
        coordinate="interface",
    )
    preflight_axis_targets(
        config(),
        measurement_name="srkr",
        axis="x",
        targets=[-0.75, 0.75],
        coordinate="interface",
    )


def test_preflight_allows_repeated_targets_before_a_valid_step():
    preflight_axis_targets(
        config(),
        measurement_name="srkr",
        axis="x",
        targets=[0.0, 0.0, 1.0],
        coordinate="measurement",
    )


def test_preflight_rejects_more_than_the_configured_point_limit(monkeypatch):
    monkeypatch.setattr(scan_limits_module, "MAX_SCAN_POINTS_PER_AXIS", 2)

    with pytest.raises(ValueError, match="maximum is 2"):
        preflight_axis_targets(
            config(), measurement_name="trkr", axis="t", targets=[0.0, 1.0, 2.0]
        )


def test_preflight_rejects_devices_without_limits_for_requested_coordinate():
    with pytest.raises(ValueError, match="no complete pulse limits"):
        preflight_axis_targets(
            config(),
            measurement_name="trkr",
            axis="t",
            targets=[0],
            coordinate="instrument",
        )


def test_preflight_rejects_steps_below_scanner_resolution():
    with pytest.raises(ValueError, match="smaller than the device minimum"):
        preflight_axis_targets(
            config(),
            measurement_name="srkr",
            axis="x",
            targets=[0.0, 0.05],
            coordinate="measurement",
        )
