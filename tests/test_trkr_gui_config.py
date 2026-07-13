from __future__ import annotations

import os
from copy import deepcopy
from pathlib import Path
from types import SimpleNamespace

import pytest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PySide6 import QtWidgets

import kohdalab.apps.trkr_gui as gui_module
from kohdalab.api.config import ConfigPathResolution
from kohdalab.apps.trkr_gui import TRKRGui
from kohdalab.apps.trkr_gui_config import (
    GuiConfigSnapshot,
    build_measurement_config,
    build_saved_config,
    extract_loaded_gui_config,
    first_number,
    output_settings_from_measurement,
    scanner_scale_key,
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
            "TRKR": {
                "output_dir": ".",
                "filename": "trkr.csv",
                "auto_timestamp_suffix": False,
            },
            "signal_monitor": {
                "output_dir": ".",
                "filename": "signal.csv",
                "auto_timestamp_suffix": False,
            },
            "SRKR": {
                "output_dir": ".",
                "filename": "srkr.csv",
                "auto_timestamp_suffix": True,
            },
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
            "srkr": {
                "lockin_key": "aux",
                "scanner_keys": {"x": "sample_x", "y": "sample_y"},
            },
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
    assert config["measurements"]["srkr"]["scanner_keys"] == {
        "x": "sample_x",
        "y": "sample_y",
    }
    assert config["measurements"]["srkr"]["output"]["auto_timestamp_suffix"] is True


def test_saved_and_measurement_configs_omit_keys_for_default_refs():
    snap = snapshot()
    snap.instrument_refs["lockin"] = {
        "TRKR": "main",
        "signal_monitor": "main",
        "SRKR": "main",
    }
    snap.instrument_refs["scanner"] = {"x": "x", "y": "y"}
    snap.instrument_refs["delay_stage"] = {"TRKR": "t", "move_abs_t": "t"}

    saved = build_saved_config(snap)
    assert saved["instruments"]["lockin"] == {
        "main": {"model": "SR7265", "resource": "GPIB0::12::INSTR"}
    }
    assert "lockin_key" not in saved["measurements"]["trkr"]
    assert "lockin_key" not in saved["measurements"]["signal_monitor"]
    assert "lockin_key" not in saved["measurements"]["srkr"]
    assert "delay_stage_key" not in saved["measurements"]["trkr"]
    assert "delay_stage_key" not in saved["measurements"]["move_abs"]
    assert "scanner_keys" not in saved["measurements"]["srkr"]
    assert "scanner_keys" not in saved["measurements"]["move_abs"]

    signal = build_measurement_config(snap, "signal_monitor")
    trkr = build_measurement_config(snap, "TRKR")
    srkr = build_measurement_config(snap, "SRKR")
    assert "lockin_key" not in signal["measurements"]["signal_monitor"]
    assert "lockin_key" not in trkr["measurements"]["trkr"]
    assert "delay_stage_key" not in trkr["measurements"]["trkr"]
    assert "lockin_key" not in srkr["measurements"]["srkr"]


def test_saved_and_trkr_measurement_configs_include_custom_lockin_ref():
    snap = snapshot()
    snap.instrument_refs["lockin"]["TRKR"] = "pump_probe"

    saved = build_saved_config(snap)
    trkr = build_measurement_config(snap, "TRKR")

    assert saved["measurements"]["trkr"]["lockin_key"] == "pump_probe"
    assert saved["instruments"]["lockin"]["pump_probe"] == snap.lockin_config
    assert trkr["measurements"]["trkr"]["lockin_key"] == "pump_probe"
    assert trkr["instruments"]["lockin"] == {
        "pump_probe": snap.lockin_config,
    }


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
    assert trkr["instruments"]["delay_stage"] == {
        "delay": {"controller": "SHOT302GS", "port": "COM6"}
    }
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
    assert srkr["measurements"]["move_abs"]["scanner_keys"] == {
        "x": "sample_x",
        "y": "sample_y",
    }


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


def test_extract_api_config_falls_back_to_only_scanner_and_measurement_refs():
    config = {
        "instruments": {
            "lockin": {"primary": {"resource": "LOCKIN"}},
            "scanner": {"shared": {"port": "COM8"}},
            "delay_stage": {"delay": {"port": "COM6"}},
        },
        "measurement": {
            "trkr": {"lockin": "primary", "delay_stage": "delay"},
            "srkr": {"scanners": "invalid"},
            "move_abs": {"scanners": []},
        },
    }

    loaded = extract_loaded_gui_config(config)

    assert loaded.instrument_refs == {
        "lockin": {"TRKR": "primary", "signal_monitor": "primary", "SRKR": "primary"},
        "scanner": {"x": "shared", "y": "shared"},
        "delay_stage": {"TRKR": "delay", "move_abs_t": "delay"},
    }
    assert loaded.x_config == {"port": "COM8"}
    assert loaded.y_config == {"port": "COM8"}
    assert loaded.shared_xy_port


def test_extract_api_config_prefers_move_abs_ref_then_axis_named_scanner():
    loaded = extract_loaded_gui_config(
        {
            "instruments": {
                "lockin": {"main": {}},
                "scanner": {
                    "move_x": {"port": "COM1"},
                    "y": {"port": "COM2"},
                },
                "delay_stage": {"t": {}},
            },
            "measurements": {
                "move_abs": {"scanner_keys": {"x": "move_x"}},
            },
        }
    )

    assert loaded.instrument_refs["scanner"] == {"x": "move_x", "y": "y"}
    assert loaded.x_config == {"port": "COM1"}
    assert loaded.y_config == {"port": "COM2"}


def test_extract_legacy_config_uses_individual_scanner_and_primary_fallbacks():
    loaded = extract_loaded_gui_config(
        {
            "x_scanner": {"port": "COM1"},
            "scanner2": {"port": "COM2"},
            "delay_stage": {"port": "COM3"},
            "signal_monitor": {"n_points": 5},
            "lab_time": {"n_points": 99},
        }
    )

    assert loaded.x_config == {"port": "COM1"}
    assert loaded.y_config == {"port": "COM2"}
    assert loaded.t_config == {"port": "COM3"}
    assert loaded.signal_monitor_config == {"n_points": 5}
    assert not loaded.shared_xy_port


def test_small_gui_config_helpers_keep_legacy_behavior(tmp_path):
    assert scanner_scale_key({}) == "sample_um_per_unit"
    assert zero_um_from_config({"x_mm": 1.25}, "x", 0.0) == 1250.0
    assert zero_um_from_config({"x_um": 12.5}, "x", 0.0) == 12.5
    assert first_number(None, {}, "bad", "3.5", default=1.0) == 3.5
    assert scanner_scale_value({"sample_um_per_unit": 582.0}, 1.0) == 582.0
    assert scanner_scale_value({"sample_um_per_actuator_mm": 12.0}, 1.0) == 12.0
    assert (
        scanner_scale_value(
            {"actuator": "AG-M100D", "sample_um_per_actuator_deg": 4.0}, 1.0
        )
        == 4.0
    )

    output = output_settings_from_measurement(
        {
            "output": {
                "dir": str(tmp_path),
                "filename": "run",
                "auto_timestamp_suffix": False,
            }
        },
        {"output_dir": "fallback", "filename": "fallback"},
        "default",
    )

    assert output == {
        "output_dir": str(tmp_path),
        "filename": "run",
        "auto_timestamp_suffix": False,
    }


def test_small_gui_config_helpers_use_explicit_fallbacks_for_missing_values():
    assert zero_um_from_config({}, "x", 12.5) == 12.5
    assert first_number(None, {}, "bad", default=4.5) == 4.5
    assert scanner_scale_value({}, 3.0) == 3.0
    assert output_settings_from_measurement(
        {"output": "invalid", "filename": "direct"},
        {"output_dir": "fallback", "auto_timestamp_suffix": False},
        "default",
    ) == {
        "output_dir": "fallback",
        "filename": "direct",
        "auto_timestamp_suffix": False,
    }


def test_build_measurement_config_rejects_unknown_measurement():
    with pytest.raises(ValueError, match="Unsupported measurement"):
        build_measurement_config(snapshot(), "unknown")


class DummyValue:
    def __init__(self, value):
        self._value = value

    def value(self):
        return self._value

    def setValue(self, value):
        self._value = value

    def isChecked(self):
        return bool(self._value)

    def currentText(self):
        return str(self._value)


class DummyLabel:
    def __init__(self):
        self.text = None

    def setText(self, value):
        self.text = value


def _range_widgets(minimum: float, maximum: float, step: float):
    return {
        "min": DummyValue(minimum),
        "max": DummyValue(maximum),
        "step": DummyValue(step),
    }


def test_gui_axis_range_payload_and_loading_fail_closed_for_broken_shapes():
    widgets = {
        "x": _range_widgets(-1.0, 1.0, 0.5),
        "y": _range_widgets(-2.0, 2.0, 1.0),
    }

    assert TRKRGui._axis_ranges_payload(SimpleNamespace(), widgets) == {
        "x": {"min": -1.0, "max": 1.0, "step": 0.5},
        "y": {"min": -2.0, "max": 2.0, "step": 1.0},
    }

    TRKRGui._load_axis_ranges(SimpleNamespace(), widgets, {"x": "broken"})
    assert widgets["x"]["min"].value() == -1.0
    assert widgets["y"]["max"].value() == 2.0

    TRKRGui._load_axis_ranges(
        SimpleNamespace(), widgets, {"x": {"min": -3.0, "max": 3.0, "step": 0.25}}
    )
    assert TRKRGui._axis_ranges_payload(SimpleNamespace(), widgets)["x"] == {
        "min": -3.0,
        "max": 3.0,
        "step": 0.25,
    }


def test_gui_measurement_output_settings_use_defaults_for_broken_output():
    dummy = SimpleNamespace(
        _default_output_filename=lambda measurement: f"{measurement}-default.csv"
    )

    settings = TRKRGui._settings_from_measurement_output(
        dummy,
        "trkr",
        {"output": "broken"},
    )

    assert settings["filename"] == "trkr-default.csv"
    assert settings["auto_timestamp_suffix"] is True


def test_gui_runtime_config_snapshot_updates_fields_and_preserves_extensions():
    outputs = {
        name: {
            "output_dir": f"/{name}",
            "filename": f"{name}.csv",
            "auto_timestamp_suffix": name != "trkr",
        }
        for name in ("signal_monitor", "trkr", "srkr", "strkr", "srkr_2d")
    }
    dummy = SimpleNamespace(
        config={
            "profile": {"name": "lab"},
            "extension": {"keep": True},
            "measurements": {"trkr": {"custom": "preserved"}},
        },
        _store_current_output_settings=lambda: None,
        _sync_scan2d_role_values_to_axis_ranges=lambda _mode: None,
        _lockin_config=lambda: {"model": "SR7265", "resource": "LOCKIN"},
        _delay_stage_config=lambda: {"controller": "SHOT302GS", "port": "STAGE"},
        _scanner_config=lambda axis: {"controller": "CONEXCC", "port": axis},
        _default_output_filename=lambda name: f"{name}.csv",
        _axis_ranges_payload=lambda widgets: TRKRGui._axis_ranges_payload(
            SimpleNamespace(), widgets
        ),
        _return_roles_payload=lambda: TRKRGui._return_roles_payload(SimpleNamespace()),
        output_settings_by_mode=outputs,
        t_zero_spin=DummyValue(10.0),
        x_zero_spin=DummyValue(20.0),
        y_zero_spin=DummyValue(30.0),
        move_t_spin=DummyValue(1.0),
        move_x_spin=DummyValue(2.0),
        move_y_spin=DummyValue(3.0),
        t_cor_spin=DummyValue(4.0),
        x_cor_spin=DummyValue(5.0),
        y_cor_spin=DummyValue(6.0),
        signal_interval_spin=DummyValue(0.5),
        signal_points_spin=DummyValue(12),
        trkr_min_spin=DummyValue(-10.0),
        trkr_max_spin=DummyValue(10.0),
        trkr_step_spin=DummyValue(5.0),
        trkr_wait_spin=DummyValue(1.5),
        trkr_return_check=DummyValue(False),
        srkr_axis_combo=DummyValue("Y"),
        srkr_min_spin=DummyValue(-2.0),
        srkr_max_spin=DummyValue(2.0),
        srkr_step_spin=DummyValue(1.0),
        srkr_wait_spin=DummyValue(2.5),
        srkr_return_check=DummyValue(True),
        strkr_fast_axis_combo=DummyValue("T"),
        strkr_slow_axis_combo=DummyValue("X"),
        strkr_range_spins={"t": _range_widgets(0.0, 1.0, 1.0)},
        strkr_wait_spin=DummyValue(3.0),
        srkr_2d_fast_axis_combo=DummyValue("X"),
        srkr_2d_slow_axis_combo=DummyValue("Y"),
        srkr_2d_range_spins={"x": _range_widgets(0.0, 2.0, 1.0)},
        srkr_2d_wait_spin=DummyValue(4.0),
    )

    config = TRKRGui._runtime_config(dummy)

    assert config["extension"] == {"keep": True}
    assert config["measurements"]["trkr"]["custom"] == "preserved"
    assert config["measurements"]["move_abs"]["zero"] == {
        "t_ps": 10.0,
        "x_um": 20.0,
        "y_um": 30.0,
    }
    assert config["measurements"]["trkr"]["scan"] == {
        "min": -10.0,
        "max": 10.0,
        "step": 5.0,
    }
    assert config["measurements"]["srkr"]["scan"]["axis"] == "y"
    assert config["measurements"]["strkr"]["return_to_zero"] == {
        "fast_axis": True,
        "slow_axis": True,
    }
    assert config["measurements"]["trkr"]["output"] == {
        "dir": "/trkr",
        "filename": "trkr.csv",
        "auto_timestamp_suffix": False,
    }


def test_gui_device_setting_snapshots_normalize_and_preserve_extensions():
    dummy = SimpleNamespace(
        _selected_text=lambda combo: combo.currentText().strip(),
        lockin_model_combo=DummyValue(""),
        lockin_resource_combo=DummyValue(" GPIB0::8::INSTR "),
        t_controller_combo=DummyValue(""),
        t_stage_combo=DummyValue(" sgsp_46_500 "),
        t_port_combo=DummyValue(" COM6 "),
        t_direction_spin=DummyValue(1),
        x_controller_combo=DummyValue(" CONEXCC "),
        x_actuator_combo=DummyValue(" TRA12CC "),
        x_port_combo=DummyValue(" COM5 "),
        x_axis_spin=DummyValue(1),
        x_scale_spin=DummyValue(582.0),
        y_controller_combo=DummyValue(" CONEXAGAP "),
        y_actuator_combo=DummyValue(" AG-M100D "),
        y_port_combo=DummyValue(" COM4 "),
        y_axis_spin=DummyValue(2),
        y_scale_spin=DummyValue(412.0),
        config={
            "instruments": {
                "scanner": {
                    "x": {"timeout": 2.5, "legacy": "keep"},
                    "y": "broken",
                }
            }
        },
    )

    assert TRKRGui._lockin_config(dummy) == {
        "model": "SR7265",
        "resource": "GPIB0::8::INSTR",
    }
    assert TRKRGui._delay_stage_config(dummy) == {
        "controller": "SHOT302GS",
        "stage": "SGSP46-500",
        "port": "COM6",
        "direction": 1,
    }
    assert TRKRGui._scanner_config(dummy, "x") == {
        "timeout": 2.5,
        "legacy": "keep",
        "controller": "CONEXCC",
        "actuator": "TRA12CC",
        "port": "COM5",
        "axis": 1,
        "sample_um_per_unit": 582.0,
    }
    assert TRKRGui._scanner_config(dummy, "y") == {
        "controller": "CONEXAGAP",
        "actuator": "AG-M100D",
        "port": "COM4",
        "axis": 2,
        "sample_um_per_unit": 412.0,
    }


def test_gui_lockin_readback_applies_full_settings_and_time_constant_only():
    refreshes: list[bool] = []
    dummy = SimpleNamespace(
        _voltage_scale=1.0,
        _voltage_unit="V",
        sensitivity_label=DummyLabel(),
        tc_label=DummyLabel(),
        freq_label=DummyLabel(),
        _refresh_plot_labels=lambda: refreshes.append(True),
    )

    TRKRGui._apply_lockin_settings(
        dummy,
        {"Sensitivity": 1e-3, "Time Constant": 0.5, "Ref. Freq": 1000.0},
    )

    assert dummy._voltage_scale == 1000.0
    assert dummy._voltage_unit == "mV"
    assert dummy.sensitivity_label.text == "1 mV"
    assert dummy.tc_label.text == "500 ms"
    assert dummy.freq_label.text == "1000 Hz"

    TRKRGui._apply_lockin_settings(dummy, {"Time Constant": 2.0})

    assert dummy.tc_label.text == "2 s"
    assert refreshes == [True, True]


def _new_config_gui(monkeypatch) -> TRKRGui:
    monkeypatch.setattr(TRKRGui, "refresh_all_ports", lambda self: None)
    monkeypatch.setattr(TRKRGui, "_install_log_streams", lambda self: None)
    QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    gui = TRKRGui()
    gui.live_timer.stop()
    return gui


def _close_config_gui(gui: TRKRGui) -> None:
    gui._shutdown_complete = True
    gui.close()


def test_gui_load_fields_applies_devices_and_falls_back_for_missing_values(monkeypatch):
    gui = _new_config_gui(monkeypatch)
    refreshes: list[bool] = []
    monkeypatch.setattr(
        gui, "_refresh_scanner_scale_labels", lambda: refreshes.append(True)
    )
    config = {
        "instruments": {
            "lockin": {"main": {"model": "LI5640", "resource": "GPIB0::9::INSTR"}},
            "delay_stage": {
                "t": {
                    "controller": "GSC01",
                    "stage": "SGSP46-500",
                    "port": "COM6",
                    "direction": 1,
                }
            },
            "scanner": {
                "x": {
                    "controller": "CONEXAGAP",
                    "actuator": "AG-M100D",
                    "port": "COM5",
                    "axis": "V",
                    "sample_um_per_unit": 412.0,
                }
            },
        }
    }

    gui._load_config_into_fields(config)

    assert gui.lockin_model_combo.currentText() == "LI5640"
    assert gui.lockin_resource_combo.currentText() == "GPIB0::9::INSTR"
    assert gui.t_controller_combo.currentText() == "GSC01"
    assert gui.t_port_combo.currentText() == "COM6"
    assert gui.t_direction_spin.value() == 1
    assert gui.x_controller_combo.currentText() == "CONEXAGAP"
    assert gui.x_axis_spin.value() == 2
    assert gui.x_scale_spin.value() == 412.0
    assert gui.y_port_combo.currentText() == "COM5"
    assert refreshes == [True]
    _close_config_gui(gui)


def test_gui_load_config_updates_existing_experiment_and_last_path(
    monkeypatch, tmp_path: Path
):
    gui = _new_config_gui(monkeypatch)
    path = tmp_path / "loaded.json"
    candidate = deepcopy(gui.config)
    candidate["profile"] = {"name": "loaded"}
    assigned: list[dict] = []
    remembered: list[Path] = []

    class ExperimentStub:
        @property
        def config(self):
            return None

        @config.setter
        def config(self, value):
            assigned.append(value)

    gui.experiment = ExperimentStub()  # type: ignore[assignment]
    gui.config_path.setText(str(path))
    monkeypatch.setattr(
        gui_module,
        "resolve_config_path",
        lambda _path: ConfigPathResolution(path, "explicit", [path]),
    )
    monkeypatch.setattr(gui_module, "load_config", lambda _path: candidate)
    monkeypatch.setattr(gui_module, "write_last_config_path", remembered.append)

    gui.load_config_file()

    assert gui.config == candidate
    assert assigned == [gui._runtime_config()]
    assert remembered == [path]
    assert "Loaded config (explicit)" in gui.log.toPlainText()
    gui.experiment = None
    _close_config_gui(gui)


def test_gui_save_config_without_path_warns_without_serializing(monkeypatch):
    gui = _new_config_gui(monkeypatch)
    warnings: list[tuple[str, str]] = []
    gui.config_path.setText("")
    monkeypatch.setattr(
        gui_module,
        "save_config",
        lambda *_args, **_kwargs: pytest.fail("save attempted without path"),
    )
    monkeypatch.setattr(
        gui_module.QtWidgets.QMessageBox,
        "warning",
        lambda _parent, title, message: warnings.append((title, message)),
    )

    gui.save_config_file()

    assert warnings == [("Config Error", "Choose a config path before saving.")]
    _close_config_gui(gui)
