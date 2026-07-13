from __future__ import annotations

import math

import pytest

from kohdalab.api.conversion import (
    actuator_pos_to_sample_um,
    sample_um_to_actuator_pos,
    scanner_origin_pos,
)


def test_scanner_origin_prefers_explicit_origin():
    assert (
        scanner_origin_pos({"min_pos": 0.0, "max_pos": 12.0, "origin_pos": 4.0}) == 4.0
    )


def test_scanner_origin_uses_min_max_midpoint_without_explicit_origin():
    assert scanner_origin_pos({"min_pos": 0.0, "max_pos": 12.0}) == 6.0


def test_scanner_origin_uses_configured_origin_when_limits_are_absent():
    assert scanner_origin_pos({"origin_pos": -2.5}) == -2.5


@pytest.mark.parametrize("config", [{}, {"min_pos": 0.0}, {"max_pos": 12.0}])
def test_scanner_origin_defaults_to_zero_without_a_complete_origin_definition(config):
    assert scanner_origin_pos(config) == 0.0


def test_sample_um_and_actuator_position_round_trip_with_new_scale():
    config = {"min_pos": 0.0, "max_pos": 12.0, "sample_um_per_unit": 582.0}

    actuator_pos = sample_um_to_actuator_pos(config, "mm", 291.0)

    assert actuator_pos == pytest.approx(6.5)
    assert actuator_pos_to_sample_um(config, "mm", actuator_pos) == pytest.approx(291.0)


def test_legacy_unit_specific_scale_is_still_accepted():
    config = {"origin_pos": 10.0, "sample_um_per_actuator_deg": 2.0}

    assert sample_um_to_actuator_pos(config, "deg", 8.0) == pytest.approx(14.0)
    assert actuator_pos_to_sample_um(config, "deg", 14.0) == pytest.approx(8.0)


def test_sample_um_to_actuator_pos_rejects_zero_scale():
    with pytest.raises(ValueError, match="non-zero"):
        sample_um_to_actuator_pos({"sample_um_per_unit": 0.0}, "mm", 1.0)


@pytest.mark.parametrize("value", [math.nan, math.inf, -math.inf, True])
def test_scanner_conversion_rejects_non_finite_positions(value):
    config = {"origin_pos": 0.0, "sample_um_per_unit": 1.0}

    with pytest.raises(ValueError, match="must be finite"):
        sample_um_to_actuator_pos(config, "mm", value)
    with pytest.raises(ValueError, match="must be finite"):
        actuator_pos_to_sample_um(config, "mm", value)


@pytest.mark.parametrize(
    "field", ["origin_pos", "min_pos", "max_pos", "sample_um_per_unit"]
)
def test_scanner_conversion_rejects_non_finite_config(field: str):
    config = {"min_pos": 0.0, "max_pos": 10.0, "sample_um_per_unit": 1.0}
    if field == "origin_pos":
        config = {"origin_pos": math.nan, "sample_um_per_unit": 1.0}
    else:
        config[field] = math.nan

    with pytest.raises(ValueError, match="must be finite"):
        actuator_pos_to_sample_um(config, "mm", 1.0)
