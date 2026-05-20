from __future__ import annotations

import pytest

from kohdalab.api.scan_limits import delay_stage_scan_limits, scanner_scan_limits


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


def test_scanner_scan_limits_use_actuator_toml_and_sample_offset():
    limits = scanner_scan_limits(actuator="TRA12CC", sample_um_per_unit=582.0, zero_um=61.5756)

    assert limits.unit == "um"
    assert limits.minimum == pytest.approx(-3553.5756)
    assert limits.maximum == pytest.approx(3430.4244)
    assert limits.minimum_step == pytest.approx(0.1164)


def test_scan_limits_return_empty_values_for_unknown_specs():
    assert delay_stage_scan_limits(stage="missing", direction=1, t_zero_ps=0.0).minimum is None
    assert scanner_scan_limits(actuator="missing", sample_um_per_unit=1.0, zero_um=0.0).maximum is None
