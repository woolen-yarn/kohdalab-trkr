from __future__ import annotations

import pytest

from kohdalab.apps.trkr_gui_measurement import (
    signal_monitor_plan,
    srkr_2d_plan,
    srkr_plan,
    strkr_plan,
    trkr_plan,
)


def test_signal_monitor_plan_summary_and_values():
    plan = signal_monitor_plan(interval_s=0.5, n_points=12)

    assert plan.interval_s == 0.5
    assert plan.n_points == 12
    assert plan.summary == "12 points, dt=0.5 s"


def test_trkr_plan_offsets_measurement_scan_points_by_zero():
    plan = trkr_plan(
        minimum_ps=-50.0,
        maximum_ps=50.0,
        step_ps=50.0,
        t_zero_ps=-122.0,
        coordinate="measurement",
    )

    assert plan.coordinate == "measurement"
    assert plan.scan_points == [-172.0, -122.0, -72.0]
    assert plan.target_points == [-50.0, 0.0, 50.0]
    assert plan.t_zero_ps == -122.0
    assert plan.summary == "3 points (measurement)"


def test_trkr_plan_keeps_interface_scan_points_unoffset():
    plan = trkr_plan(
        minimum_ps=0.0,
        maximum_ps=1.0,
        step_ps=0.5,
        t_zero_ps=-122.0,
        coordinate="control",
    )

    assert plan.coordinate == "interface"
    assert plan.scan_points == [0.0, 0.5, 1.0]
    assert plan.target_points == [0.0, 0.5, 1.0]
    assert plan.summary == "3 points (interface)"


def test_srkr_plan_offsets_measurement_scan_points_by_active_axis_zero():
    plan = srkr_plan(
        axis="x",
        minimum_um=-10.0,
        maximum_um=10.0,
        step_um=10.0,
        zero_by_axis={"x": 61.5, "y": 477.0},
        coordinate="measurement",
    )

    assert plan.axis == "x"
    assert plan.coordinate == "measurement"
    assert plan.scan_points == [51.5, 61.5, 71.5]
    assert plan.target_points == [-10.0, 0.0, 10.0]
    assert plan.zero == {"x": 61.5, "y": 477.0}
    assert plan.summary == "X measurement, 3 points"


def test_srkr_plan_treats_instrument_alias_as_interface_points():
    plan = srkr_plan(
        axis="Y",
        minimum_um=0.0,
        maximum_um=2.0,
        step_um=1.0,
        zero_by_axis={"x": 61.5, "y": 477.0},
        coordinate="device",
    )

    assert plan.axis == "y"
    assert plan.coordinate == "interface"
    assert plan.scan_points == [0.0, 1.0, 2.0]
    assert plan.target_points == [0.0, 1.0, 2.0]
    assert plan.summary == "Y interface, 3 points"


def test_srkr_plan_rejects_invalid_axis():
    with pytest.raises(ValueError, match="SRKR axis"):
        srkr_plan(
            axis="z",
            minimum_um=0.0,
            maximum_um=1.0,
            step_um=1.0,
            zero_by_axis={"x": 0.0, "y": 0.0},
            coordinate="measurement",
        )


def test_strkr_plan_requires_t_and_one_spatial_axis():
    plan = strkr_plan(
        fast_axis="x",
        slow_axis="t",
        ranges={
            "t": {"min": 0.0, "max": 10.0, "step": 10.0},
            "x": {"min": -1.0, "max": 1.0, "step": 1.0},
            "y": {"min": -2.0, "max": 2.0, "step": 1.0},
        },
        zero_by_axis={"t_ps": 10.0, "x_um": 1.0, "y_um": 2.0},
    )

    assert plan.measurement == "strkr"
    assert plan.fast_axis == "x"
    assert plan.slow_axis == "t"
    assert plan.fast_target_points == [-1.0, 0.0, 1.0]
    assert plan.slow_target_points == [0.0, 10.0]
    assert plan.ranges["y"] == {"min": -2.0, "max": 2.0, "step": 1.0}
    assert plan.total_points == 6


def test_strkr_plan_rejects_spatial_spatial_axes():
    with pytest.raises(ValueError, match="STRKR axes"):
        strkr_plan(
            fast_axis="x",
            slow_axis="y",
            ranges={"x": {"min": 0.0, "max": 1.0, "step": 1.0}, "y": {"min": 0.0, "max": 1.0, "step": 1.0}},
        )


def test_srkr_2d_plan_uses_x_y_axes():
    plan = srkr_2d_plan(
        fast_axis="y",
        slow_axis="x",
        ranges={
            "x": {"min": 0.0, "max": 1.0, "step": 1.0},
            "y": {"min": 10.0, "max": 20.0, "step": 10.0},
        },
    )

    assert plan.measurement == "srkr_2d"
    assert plan.fast_axis == "y"
    assert plan.slow_axis == "x"
    assert plan.fast_target_points == [10.0, 20.0]
    assert plan.slow_target_points == [0.0, 1.0]
