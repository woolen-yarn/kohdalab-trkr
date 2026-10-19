from __future__ import annotations

import importlib.util
from pathlib import Path


SCRIPT = Path(__file__).parents[1] / "scripts" / "validate_trkr_compact_layout.py"
SPEC = importlib.util.spec_from_file_location("validate_trkr_compact_layout", SCRIPT)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def _rect(x: float, y: float, width: float, height: float) -> list[float]:
    return [x, y, width, height]


def _payload(lockin_height: float = 234.0) -> dict[str, object]:
    geometry: dict[str, list[float]] = {
        "center_panel": _rect(400, 8, 850, 884),
        "measurement_tabs": _rect(409, 17, 832, 216),
        "lockin_group": _rect(19, 123, 350, lockin_height),
        "lockin_device": _rect(30, 150, 71, 24),
        "lockin_name": _rect(121, 150, 140, 24),
        "lockin_live_0": _rect(30, 201, 150, 35),
        "lockin_live_1": _rect(190, 201, 150, 35),
        "trkr_min_spin": _rect(472, 84, 132, 26),
        "trkr_max_spin": _rect(472, 116, 132, 26),
        "trkr_step_spin": _rect(472, 148, 132, 26),
        "trkr_axis_combo": _rect(472, 52, 132, 26),
        "trkr_wait_spin": _rect(472, 180, 72, 26),
        "trkr_tc_button": _rect(550, 180, 76, 26),
        "output_dir_edit": _rect(840, 70, 138, 26),
        "output_name_edit": _rect(840, 102, 138, 26),
        "output_browse_button": _rect(984, 70, 54, 26),
        "auto_suffix_check": _rect(984, 102, 54, 24),
        "start_button": _rect(800, 174, 72, 26),
        "stop_button": _rect(878, 174, 72, 26),
        "save_rows_button": _rect(956, 174, 72, 26),
    }
    for index, axis in enumerate(("t", "x", "y")):
        y = 313 + index * 166
        geometry[f"motion_{axis}_group"] = _rect(19, y, 350, 158)
        geometry[f"{axis}_device"] = _rect(30, y + 27, 71, 24)
        geometry[f"{axis}_device_name"] = _rect(121, y + 27, 140, 24)
        for row in range(3):
            row_y = y + 57 + row * 32
            geometry[f"{axis}_row_{row}_label"] = _rect(30, row_y, 70, 26)
            geometry[f"{axis}_row_{row}_value"] = _rect(105, row_y, 67, 26)
        geometry[f"move_{axis}_spin"] = _rect(178, y + 57, 72, 26)
        geometry[f"move_{axis}_button"] = _rect(277, y + 57, 81, 26)
        geometry[f"use_current_{axis}_button"] = _rect(277, y + 89, 81, 26)
        geometry[f"{axis}_zero_spin"] = _rect(178, y + 89, 72, 26)
        geometry[f"{axis}_cor_spin"] = _rect(178, y + 121, 72, 26)
        geometry[f"{axis}_cor_button"] = _rect(277, y + 121, 81, 26)
    for key in ("min", "max", "step"):
        x,y,w,h = geometry[f"trkr_{key}_spin"]
        geometry[f"trkr_{key}_label"] = _rect(x-60,y,55,h)
        geometry[f"trkr_{key}_hint"] = _rect(x+w+6,y,106,h)
    return {"workflow": "trkr", "geometry": geometry}


def test_compact_layout_accepts_fixed_center_and_expanded_lockin() -> None:
    assert MODULE.validate_geometry(_payload()) == []
    assert MODULE.validate_geometry(_payload(348.0), "devices-expanded") == []


def test_compact_layout_rejects_overflow_and_noncompact_parts() -> None:
    payload = _payload()
    geometry = payload["geometry"]
    assert isinstance(geometry, dict)
    geometry["center_panel"] = _rect(400, 8, 849, 884)
    geometry["trkr_max_spin"] = _rect(1200, 116, 132, 26)
    geometry["x_device"] = _rect(287, 506, 71, 24)
    errors = MODULE.validate_geometry(payload)
    assert any("expected at least 850" in error for error in errors)
    assert any("measurement control outside pane: trkr_max_spin" in error for error in errors)
    assert any("x_device is not left-aligned" in error for error in errors)


def test_compact_layout_checks_2d_numbers_and_ignores_lockin_mode_combo() -> None:
    payload = _payload()
    geometry = payload["geometry"]
    assert isinstance(geometry, dict)
    payload["workflow"] = "strkr"
    for role in ("fast", "slow"):
        geometry[f"strkr_{role}_axis_combo"] = _rect(420, 52, 72, 26)
        for index, key in enumerate(("min", "max", "step")):
            geometry[f"strkr_{role}_{key}"] = _rect(470, 84 + index * 32, 72, 26)
            geometry[f"strkr_{role}_{key}_label"] = _rect(410,84+index*32,55,26)
            geometry[f"strkr_{role}_{key}_hint"] = _rect(548,84+index*32,106,26)
    geometry["strkr_wait_spin"] = _rect(470, 180, 72, 26)
    geometry["strkr_tc_button"] = _rect(548, 180, 76, 26)
    geometry["strkr_fast_theta"] = _rect(470, 52, 72, 26)
    geometry["signal_mode_combo"] = _rect(30, 180, 92, 24)
    assert MODULE.validate_geometry(payload) == []
    geometry["strkr_fast_min"] = _rect(1200, 84, 72, 26)
    errors = MODULE.validate_geometry(payload)
    assert any("measurement control outside pane: strkr_fast_min" in error for error in errors)


def test_signal_monitor_does_not_treat_lockin_mode_as_measurement_input() -> None:
    payload = _payload()
    geometry = payload["geometry"]
    assert isinstance(geometry, dict)
    payload["workflow"] = "signal_monitor"
    geometry["signal_interval_spin"] = _rect(472, 52, 120, 26)
    geometry["signal_points_spin"] = _rect(472, 84, 120, 26)
    geometry["signal_mode_combo"] = _rect(30, 180, 92, 24)
    assert MODULE.validate_geometry(payload) == []


def test_center_can_grow_but_right_panel_stays_fixed() -> None:
    payload = _payload()
    payload["geometry"]["center_panel"][2] = 1258
    payload["geometry"]["right_panel"] = _rect(1666, 8, 300, 884)
    assert MODULE.validate_geometry(payload) == []
    payload["geometry"]["right_panel"][2] = 654
    assert any("right_panel width" in error for error in MODULE.validate_geometry(payload))


def test_config_uses_three_rows() -> None:
    payload = _payload()
    geometry = payload["geometry"]
    geometry["session_group"] = _rect(19, 17, 350, 128)
    for row, names in enumerate((("config_path", "browse_button"), ("load_button", "save_button"), ("connect_button", "disconnect_button"))):
        for col, name in enumerate(names):
            geometry[name] = _rect(30 + col * 164, 44 + row * 32, 156, 26)
    assert MODULE.validate_geometry(payload) == []
    geometry["load_button"][1] = geometry["config_path"][1]
    assert any("Config row 2" in error for error in MODULE.validate_geometry(payload))


def test_position_readout_keeps_room_for_signed_values() -> None:
    payload = _payload()
    payload["geometry"]["t_row_1_value"][2] = 41
    assert any("too narrow for the position readout" in error for error in MODULE.validate_geometry(payload))


def test_output_and_run_frames_have_outer_and_between_margins() -> None:
    payload = _payload()
    geometry = payload["geometry"]
    geometry["measurement_tabs"][3] = 226
    geometry["output_group"] = _rect(975, 55, 250, 96)
    geometry["run_group"] = _rect(975, 163, 250, 64)
    assert MODULE.validate_geometry(payload) == []
    geometry["output_group"][1] -= 12
    assert any("frame margins" in error for error in MODULE.validate_geometry(payload))
