from __future__ import annotations

from kohdalab.apps.trkr_gui_devices import (
    corrected_target,
    delay_stage_key,
    device_ref,
    lockin_key,
    scanner_key,
    scanner_keys,
    single_instrument_config,
    xy_scanner_config,
)


def refs():
    return {
        "lockin": {"TRKR": "main", "signal_monitor": "aux"},
        "scanner": {"x": "sample_x", "y": "sample_y"},
        "delay_stage": {"TRKR": "delay", "move_abs_t": "move_delay"},
    }


def test_device_ref():
    assert device_ref("lockin", "main") == "lockin.main"
    assert device_ref("delay_stage", "delay") == "delay_stage.delay"


def test_instrument_ref_helpers():
    assert lockin_key(refs(), "TRKR") == "main"
    assert lockin_key(refs(), "SRKR") == "main"
    assert scanner_key(refs(), "X") == "sample_x"
    assert scanner_keys(refs()) == ("sample_x", "sample_y")
    assert delay_stage_key(refs()) == "delay"
    assert delay_stage_key(refs(), "move_abs_t") == "move_delay"


def test_single_instrument_config_copies_config():
    source = {"port": "COM6"}
    config = single_instrument_config("delay_stage", "delay", source)

    source["port"] = "changed"

    assert config == {"instruments": {"delay_stage": {"delay": {"port": "COM6"}}}}


def test_xy_scanner_config_uses_refs_and_copies_configs():
    x_config = {"port": "COM5"}
    y_config = {"port": "COM4"}
    config = xy_scanner_config(refs(), x_config, y_config)

    x_config["port"] = "changed"

    assert config == {
        "instruments": {
            "scanner": {
                "sample_x": {"port": "COM5"},
                "sample_y": {"port": "COM4"},
            }
        }
    }


def test_corrected_target():
    assert corrected_target(10.0, -2.5) == 7.5
