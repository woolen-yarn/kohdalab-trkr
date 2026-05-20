from __future__ import annotations

from kohdalab.api import (
    signal_monitor_plan_from_config,
    srkr_plan_from_config,
    trkr_plan_from_config,
)


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
        }
    }


def test_signal_monitor_plan_from_config_uses_measurement_settings():
    plan = signal_monitor_plan_from_config(config())

    assert plan.interval_s == 0.5
    assert plan.n_points == 12
    assert plan.summary == "12 points, dt=0.5 s"


def test_trkr_plan_from_config_uses_scan_and_zero():
    plan = trkr_plan_from_config(config())

    assert plan.coordinate == "measurement"
    assert plan.target_points == [-50.0, 0.0, 50.0]
    assert plan.scan_points == [-172.0, -122.0, -72.0]
    assert plan.t_zero_ps == -122.0


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
