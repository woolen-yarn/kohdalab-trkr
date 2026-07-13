from __future__ import annotations

import math

import pytest

from kohdalab.api import (
    signal_monitor_plan_from_config,
    srkr_2d_plan_from_config,
    srkr_plan_from_config,
    strkr_plan_from_config,
    trkr_plan_from_config,
)
from kohdalab.api.config import MAX_SCAN_POINTS_TOTAL
from kohdalab.api.scan_plan import srkr_plan, trkr_plan


def config() -> dict:
    return {
        "measurements": {
            "move_abs": {
                "zero": {
                    "t_ps": -122.0,
                    "x_um": 61.5,
                    "y_um": 477.0,
                }
            },
            "signal_monitor": {
                "interval_s": 0.5,
                "n_points": 12,
            },
            "trkr": {
                "coordinate": "measurement",
                "scan": {"min": -50.0, "max": 50.0, "step": 50.0},
            },
            "srkr": {
                "coordinate": "measurement",
                "scan": {"axis": "y", "min": -10.0, "max": 10.0, "step": 10.0},
            },
            "strkr": {
                "scan": {
                    "fast_axis": "T",
                    "slow_axis": "X",
                    "ranges": {
                        "t": {"min": 0.0, "max": 1.0, "step": 1.0},
                        "x": {"min": -1.0, "max": 1.0, "step": 1.0},
                    },
                },
                "return_to_zero": {"fast_axis": False, "slow_axis": True},
            },
            "srkr_2d": {
                "scan": {
                    "fast_axis": "Y",
                    "slow_axis": "X",
                    "ranges": {
                        "x": {"min": 0.0, "max": 1.0, "step": 1.0},
                        "y": {"min": 0.0, "max": 2.0, "step": 1.0},
                    },
                },
                "return_to_zero": False,
            },
        }
    }


def test_signal_monitor_plan_from_config_uses_measurement_settings():
    plan = signal_monitor_plan_from_config(config())

    assert plan.interval_s == 0.5
    assert plan.n_points == 12
    assert plan.summary == "12 points, dt=0.5 s"


def test_signal_monitor_plan_rejects_fractional_point_count():
    with pytest.raises(ValueError, match="integer"):
        signal_monitor_plan_from_config(config(), n_points=1.5)


@pytest.mark.parametrize("n_points", [True, 0, MAX_SCAN_POINTS_TOTAL + 1])
def test_signal_monitor_plan_rejects_ambiguous_or_out_of_range_point_count(n_points):
    with pytest.raises(ValueError, match="integer|between"):
        signal_monitor_plan_from_config(config(), n_points=n_points)


@pytest.mark.parametrize("interval_s", [math.nan, math.inf, -0.1])
def test_signal_monitor_plan_rejects_unsafe_interval(interval_s):
    with pytest.raises(ValueError, match="finite and non-negative"):
        signal_monitor_plan_from_config(config(), interval_s=interval_s)


def test_signal_monitor_plan_accepts_maximum_point_boundary():
    plan = signal_monitor_plan_from_config(config(), n_points=MAX_SCAN_POINTS_TOTAL)

    assert plan.n_points == MAX_SCAN_POINTS_TOTAL


def test_trkr_plan_from_config_uses_scan_and_zero():
    plan = trkr_plan_from_config(config())

    assert plan.coordinate == "measurement"
    assert plan.target_points == [-50.0, 0.0, 50.0]
    assert plan.scan_points == [-172.0, -122.0, -72.0]
    assert plan.t_zero_ps == -122.0


def test_trkr_plan_normalizes_coordinate_alias():
    plan = trkr_plan_from_config(config(), coordinate=" DEVICE ")

    assert plan.coordinate == "instrument"
    assert plan.scan_points == plan.target_points


@pytest.mark.parametrize("t_zero_ps", [math.nan, math.inf])
def test_trkr_plan_rejects_non_finite_zero(t_zero_ps):
    with pytest.raises(ValueError, match="t_zero_ps must be finite"):
        trkr_plan_from_config(config(), t_zero_ps=t_zero_ps)


def test_direct_trkr_plan_rejects_unknown_coordinate():
    with pytest.raises(ValueError, match="TRKR coordinate"):
        trkr_plan(
            minimum_ps=0.0,
            maximum_ps=1.0,
            step_ps=1.0,
            t_zero_ps=0.0,
            coordinate="unknown",
        )


def test_srkr_plan_from_config_uses_axis_scan_and_zero():
    plan = srkr_plan_from_config(config())

    assert plan.axis == "y"
    assert plan.coordinate == "measurement"
    assert plan.target_points == [-10.0, 0.0, 10.0]
    assert plan.scan_points == [467.0, 477.0, 487.0]
    assert plan.zero == {"x": 61.5, "y": 477.0}


def test_plan_from_config_allows_runtime_overrides():
    plan = srkr_plan_from_config(
        config(),
        axis="x",
        minimum_um=0.0,
        maximum_um=1.0,
        step_um=1.0,
        zero_by_axis={"x": 10.0, "y": 20.0},
        coordinate="instrument",
    )

    assert plan.axis == "x"
    assert plan.coordinate == "interface"
    assert plan.target_points == [0.0, 1.0]
    assert plan.scan_points == [0.0, 1.0]
    assert plan.zero == {"x": 10.0, "y": 20.0}


@pytest.mark.parametrize(
    ("axis", "coordinate", "zero", "message"),
    [
        ("z", "measurement", {"x": 0.0, "y": 0.0}, "axis must"),
        ("x", "pulse", {"x": 0.0, "y": 0.0}, "coordinate must"),
        ("x", "measurement", {"x": math.nan, "y": 0.0}, "zero values"),
    ],
)
def test_direct_srkr_plan_rejects_invalid_axis_coordinate_and_zero(
    axis, coordinate, zero, message
):
    with pytest.raises(ValueError, match=message):
        srkr_plan(
            axis=axis,
            minimum_um=0.0,
            maximum_um=1.0,
            step_um=1.0,
            zero_by_axis=zero,
            coordinate=coordinate,
        )


def test_2d_plans_from_config_normalize_axes_and_return_policy():
    strkr = strkr_plan_from_config(config())
    srkr_2d = srkr_2d_plan_from_config(config())

    assert (strkr.fast_axis, strkr.slow_axis) == ("t", "x")
    assert strkr.total_points == 6
    assert strkr.slow_line_count == 3
    assert strkr.return_to_zero == {"fast_axis": False, "slow_axis": True}
    assert strkr.zero == {"t_ps": -122.0, "x_um": 61.5, "y_um": 477.0}
    assert (srkr_2d.fast_axis, srkr_2d.slow_axis) == ("y", "x")
    assert srkr_2d.total_points == 6
    assert srkr_2d.return_to_zero == {"fast_axis": False, "slow_axis": False}


@pytest.mark.parametrize(
    ("factory", "fast_axis", "slow_axis", "ranges", "message"),
    [
        (
            strkr_plan_from_config,
            "t",
            "x",
            {
                "t": {"min": 0.0, "max": 999.0, "step": 1.0},
                "x": {"min": 0.0, "max": 1000.0, "step": 1.0},
            },
            "STRKR scan exceeds",
        ),
        (
            srkr_2d_plan_from_config,
            "x",
            "y",
            {
                "x": {"min": 0.0, "max": 999.0, "step": 1.0},
                "y": {"min": 0.0, "max": 1000.0, "step": 1.0},
            },
            "SRKR 2D scan exceeds",
        ),
    ],
)
def test_2d_plans_reject_total_point_count_over_limit(
    factory, fast_axis, slow_axis, ranges, message
):
    with pytest.raises(ValueError, match=message):
        factory(
            config(),
            fast_axis=fast_axis,
            slow_axis=slow_axis,
            ranges=ranges,
        )


@pytest.mark.parametrize(
    ("factory", "fast_axis", "slow_axis", "message"),
    [
        (strkr_plan_from_config, "x", "y", "combine t"),
        (srkr_2d_plan_from_config, "t", "x", "must be x and y"),
        (srkr_2d_plan_from_config, "x", "x", "must be different"),
    ],
)
def test_2d_plans_reject_invalid_or_ambiguous_axes(
    factory, fast_axis, slow_axis, message
):
    with pytest.raises(ValueError, match=message):
        factory(config(), fast_axis=fast_axis, slow_axis=slow_axis)


def test_2d_plan_uses_default_axis_range_when_axis_config_is_not_an_object():
    plan = srkr_2d_plan_from_config(
        config(),
        fast_axis="x",
        slow_axis="y",
        ranges={"x": [], "y": {"min": 0.0, "max": 1.0, "step": 1.0}},
    )

    assert plan.fast_target_points == [float(value) for value in range(-30, 31)]


def test_2d_plan_uses_default_return_policy_for_invalid_policy_type():
    data = config()
    data["measurements"]["srkr_2d"]["return_to_zero"] = "invalid"

    plan = srkr_2d_plan_from_config(data)

    assert plan.return_to_zero == {"fast_axis": True, "slow_axis": True}


def test_2d_plan_rejects_unknown_axis_name():
    with pytest.raises(ValueError, match="axis must be"):
        strkr_plan_from_config(config(), fast_axis="z", slow_axis="t")
