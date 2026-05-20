from __future__ import annotations

from kohdalab.apps.trkr_gui_config import (
    GuiConfigSnapshot,
    build_measurement_config,
    build_saved_config,
    extract_loaded_gui_config,
    first_number,
    output_settings_from_measurement,
    scanner_scale_value,
    zero_um_from_config,
)


def snapshot() -> GuiConfigSnapshot:
    return GuiConfigSnapshot(
        instrument_refs={
            "lockin": {"TRKR": "main", "signal_monitor": "aux", "SRKR": "aux"},
            "scanner": {"x": "sample_x", "y": "sample_y"},
            "delay_stage": {"TRKR": "delay", "move_abs_t": "delay"},
        },
        lockin_config={"model": "SR7265", "resource": "GPIB0::12::INSTR"},
        scanner_configs={
            "x": {"controller": "CONEXCC", "port": "COM5"},
            "y": {"controller": "CONEXCC", "port": "COM4"},
        },
        delay_stage_config={"controller": "SHOT302GS", "port": "COM6"},
        move_t_coordinate="measurement",
        trkr_coordinate="measurement",
        srkr_coordinate="interface",
        t_zero_ps=-122.0,
        x_zero_um=61.5,
        y_zero_um=477.0,
        trkr_scan={"min": -50.0, "max": 300.0, "step": 50.0},
        trkr_wait_s=2.0,
        trkr_return_to_zero=True,
        signal_monitor_interval_s=1.0,
        signal_monitor_n_points=10,
        srkr_axis="x",
        srkr_scan={"min": -30.0, "max": 30.0, "step": 10.0},
        srkr_wait_s=3.0,
        srkr_return_to_zero=False,
        output_settings={
            "TRKR": {"output_dir": ".", "filename": "trkr.csv", "auto_timestamp_suffix": False},
            "signal_monitor": {"output_dir": ".", "filename": "signal.csv", "auto_timestamp_suffix": False},
            "SRKR": {"output_dir": ".", "filename": "srkr.csv", "auto_timestamp_suffix": True},
        },
    )


def test_extract_loaded_gui_config_from_api_config_resolves_refs_and_sections():
    config = {
        "instruments": {
            "lockin": {
                "main": {"model": "SR7265", "resource": "GPIB0::12::INSTR"},
                "aux": {"model": "SR7265", "resource": "GPIB0::13::INSTR"},
            },
            "scanner": {
                "sample_x": {"controller": "CONEXCC", "port": "COM5"},
                "sample_y": {"controller": "CONEXCC", "port": "COM4"},
            },
            "delay_stage": {
                "delay": {"controller": "SHOT302GS", "port": "COM6"},
            },
        },
        "measurements": {
            "move_abs": {
                "delay_stage_key": "delay",
                "scanner_keys": {"x": "sample_x", "y": "sample_y"},
                "zero": {"t_ps": -122.0},
            },
            "signal_monitor": {"lockin_key": "aux"},
            "trkr": {"lockin_key": "main", "delay_stage_key": "delay"},
            "srkr": {"lockin_key": "aux", "scanner_keys": {"x": "sample_x", "y": "sample_y"}},
        },
    }

    loaded = extract_loaded_gui_config(config)

    assert loaded.instrument_refs == {
        "lockin": {"TRKR": "main", "signal_monitor": "aux", "SRKR": "aux"},
        "scanner": {"x": "sample_x", "y": "sample_y"},
        "delay_stage": {"TRKR": "delay", "move_abs_t": "delay"},
    }
    assert loaded.lockin_config["resource"] == "GPIB0::12::INSTR"
    assert loaded.x_config["port"] == "COM5"
    assert loaded.y_config["port"] == "COM4"
    assert loaded.t_config["port"] == "COM6"
    assert loaded.move_abs_config["zero"]["t_ps"] == -122.0
    assert not loaded.shared_xy_port


def test_build_saved_config_from_snapshot():
    config = build_saved_config(snapshot())

    assert config["instruments"]["lockin"] == {
        "main": {"model": "SR7265", "resource": "GPIB0::12::INSTR"},
        "aux": {"model": "SR7265", "resource": "GPIB0::12::INSTR"},
    }
    assert config["instruments"]["scanner"]["sample_x"]["port"] == "COM5"
    assert config["instruments"]["delay_stage"]["delay"]["port"] == "COM6"
    assert config["measurements"]["move_abs"] == {
        "coordinate": "measurement",
        "zero": {"t_ps": -122.0, "x_um": 61.5, "y_um": 477.0},
        "delay_stage_key": "delay",
        "scanner_keys": {"x": "sample_x", "y": "sample_y"},
    }
    assert config["measurements"]["signal_monitor"]["lockin_key"] == "aux"
    assert config["measurements"]["trkr"]["delay_stage_key"] == "delay"
    assert config["measurements"]["srkr"]["scanner_keys"] == {"x": "sample_x", "y": "sample_y"}
    assert config["measurements"]["srkr"]["output"]["auto_timestamp_suffix"] is True


def test_build_measurement_config_from_snapshot():
    snap = snapshot()

    signal = build_measurement_config(snap, "signal_monitor")
    assert signal["instruments"] == {
        "lockin": {"aux": {"model": "SR7265", "resource": "GPIB0::12::INSTR"}}
    }
    assert signal["measurements"]["signal_monitor"] == {
        "interval_s": 1.0,
        "n_points": 10,
        "lockin_key": "aux",
    }

    trkr = build_measurement_config(snap, "TRKR")
    assert trkr["instruments"]["delay_stage"] == {"delay": {"controller": "SHOT302GS", "port": "COM6"}}
    assert trkr["measurements"]["trkr"] == {
        "coordinate": "measurement",
        "wait_s": 2.0,
        "return_to_zero": True,
        "delay_stage_key": "delay",
    }

    srkr = build_measurement_config(snap, "SRKR")
    assert srkr["instruments"]["scanner"]["sample_y"]["port"] == "COM4"
    assert srkr["measurements"]["srkr"] == {
        "coordinate": "interface",
        "scan": {"axis": "x"},
        "wait_s": 3.0,
        "return_to_zero": False,
        "scanner_keys": {"x": "sample_x", "y": "sample_y"},
        "lockin_key": "aux",
    }
    assert srkr["measurements"]["move_abs"]["scanner_keys"] == {"x": "sample_x", "y": "sample_y"}


def test_extract_loaded_gui_config_from_legacy_config():
    config = {
        "lockin": {"model": "SR7265", "resource": "GPIB0::12::INSTR"},
        "xy_scanner": {
            "x": {"port": "COM5", "sample_um_per_actuator_mm": 582.0},
            "y": {"port": "COM5", "sample_um_per_actuator_mm": 412.0},
        },
        "delayline": {"port": "COM6"},
        "lab_time": {"n_points": 3},
        "trkr": {"wait_s": 1.0},
        "srkr": {"wait_s": 2.0},
    }

    loaded = extract_loaded_gui_config(config)

    assert loaded.instrument_refs["lockin"]["TRKR"] == "main"
    assert loaded.lockin_config["resource"] == "GPIB0::12::INSTR"
    assert loaded.x_config["port"] == "COM5"
    assert loaded.y_config["port"] == "COM5"
    assert loaded.shared_xy_port
    assert loaded.t_config["port"] == "COM6"
    assert loaded.signal_monitor_config["n_points"] == 3


def test_small_gui_config_helpers_keep_legacy_behavior(tmp_path):
    assert zero_um_from_config({"x_mm": 1.25}, "x", 0.0) == 1250.0
    assert zero_um_from_config({"x_um": 12.5}, "x", 0.0) == 12.5
    assert first_number(None, {}, "bad", "3.5", default=1.0) == 3.5
    assert scanner_scale_value({"sample_um_per_unit": 582.0}, 1.0) == 582.0
    assert scanner_scale_value({"actuator": "AG-M100D", "sample_um_per_actuator_deg": 4.0}, 1.0) == 4.0

    output = output_settings_from_measurement(
        {"output": {"dir": str(tmp_path), "filename": "run", "auto_timestamp_suffix": False}},
        {"output_dir": "fallback", "filename": "fallback"},
        "default",
    )

    assert output == {
        "output_dir": str(tmp_path),
        "filename": "run",
        "auto_timestamp_suffix": False,
    }
