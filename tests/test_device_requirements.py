from __future__ import annotations

import pytest

from kohdalab.api import Experiment, missing_devices, required_devices


def config_with_devices() -> dict:
    return {
        "instruments": {
            "lockin": {"main": {}},
            "delay_stage": {"t": {}},
            "scanner": {"x": {}, "y": {}},
        },
        "measurements": {
            "signal_monitor": {},
            "trkr": {},
            "srkr": {},
            "strkr": {"scan": {"fast_axis": "t", "slow_axis": "x"}},
            "srkr_2d": {"scan": {"fast_axis": "x", "slow_axis": "y"}},
        },
    }


def test_required_devices_for_each_measurement():
    config = config_with_devices()

    assert required_devices(config, "signal_monitor") == ["lockin.main"]
    assert required_devices(config, "trkr") == ["lockin.main", "delay_stage.t"]
    assert required_devices(config, "srkr", axis="x") == ["lockin.main", "scanner.x"]
    assert required_devices(config, "srkr", axis="y") == ["lockin.main", "scanner.y"]
    assert required_devices(config, "strkr", fast_axis="t", slow_axis="y") == [
        "lockin.main",
        "delay_stage.t",
        "scanner.y",
    ]
    assert required_devices(config, "srkr_2d") == ["lockin.main", "scanner.x", "scanner.y"]


def test_required_devices_respects_measurement_device_keys():
    config = {
        "instruments": {
            "lockin": {"probe": {}, "pump": {}},
            "delay_stage": {"delay": {}, "aux": {}},
            "scanner": {"fast_x": {}, "slow_y": {}},
        },
        "measurements": {
            "signal_monitor": {"lockin_key": "probe"},
            "trkr": {"lockin_key": "probe", "delay_stage_key": "delay"},
            "srkr": {
                "lockin_key": "pump",
                "scanner_keys": {"x": "fast_x", "y": "slow_y"},
            },
            "strkr": {
                "lockin_key": "pump",
                "delay_stage_key": "delay",
                "scanner_keys": {"x": "fast_x", "y": "slow_y"},
                "scan": {"fast_axis": "x", "slow_axis": "t"},
            },
            "srkr_2d": {
                "lockin_key": "pump",
                "scanner_keys": {"x": "fast_x", "y": "slow_y"},
            },
        },
    }

    assert required_devices(config, "signal_monitor") == ["lockin.probe"]
    assert required_devices(config, "trkr") == ["lockin.probe", "delay_stage.delay"]
    assert required_devices(config, "srkr", axis="x") == ["lockin.pump", "scanner.fast_x"]
    assert required_devices(config, "srkr", axis="y") == ["lockin.pump", "scanner.slow_y"]
    assert required_devices(config, "strkr") == ["lockin.pump", "delay_stage.delay", "scanner.fast_x"]
    assert required_devices(config, "srkr_2d") == ["lockin.pump", "scanner.fast_x", "scanner.slow_y"]


def test_missing_devices_compares_against_connected_map():
    config = config_with_devices()

    assert missing_devices(
        config,
        {"lockin.main": True, "delay_stage.t": False},
        "trkr",
    ) == ["delay_stage.t"]


def test_experiment_missing_devices_uses_session_state():
    experiment = Experiment(config_with_devices())
    experiment.session.lockins["main"] = object()

    assert experiment.missing_devices("signal_monitor") == []
    assert experiment.missing_devices("trkr") == ["delay_stage.t"]
    assert experiment.missing_devices("srkr", axis="x") == ["scanner.x"]
    assert experiment.missing_devices("srkr_2d") == ["scanner.x", "scanner.y"]


def test_required_devices_rejects_invalid_measurement_or_axis():
    with pytest.raises(ValueError, match="Unsupported measurement"):
        required_devices(config_with_devices(), "unknown")
    with pytest.raises(ValueError, match="SRKR axis"):
        required_devices(config_with_devices(), "srkr", axis="z")
