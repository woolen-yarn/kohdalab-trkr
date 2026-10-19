#!/usr/bin/env python3
"""Validate geometry emitted by the compact fixed-width TRKR GUI smoke mode.

Usage: ``python scripts/validate_trkr_compact_layout.py CAPTURE_OR_DIRECTORY ...``.
The check is intentionally independent of the historical Python-parity fixture:
the compact layout is a new, user-approved design rather than a pixel copy.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


Rect = tuple[float, float, float, float]
AXES = ("t", "x", "y")
COMMON_MEASUREMENT_CONTROLS = (
    "output_dir_edit",
    "output_name_edit",
    "output_browse_button",
    "auto_suffix_check",
    "start_button",
    "stop_button",
    "save_rows_button",
)
REQUIRED_MEASUREMENT_CONTROLS = {
    "signal_monitor": ("signal_interval_spin", "signal_points_spin"),
    "trkr": (
        "trkr_axis_combo",
        "trkr_min_spin",
        "trkr_max_spin",
        "trkr_step_spin",
        "trkr_wait_spin",
        "trkr_tc_button",
    ),
    "srkr": (
        "srkr_axis_combo",
        "srkr_min_spin",
        "srkr_max_spin",
        "srkr_step_spin",
        "srkr_wait_spin",
        "srkr_tc_button",
    ),
    "strkr": (
        "strkr_fast_axis_combo",
        "strkr_slow_axis_combo",
        "strkr_fast_min",
        "strkr_fast_max",
        "strkr_fast_step",
        "strkr_slow_min",
        "strkr_slow_max",
        "strkr_slow_step",
        "strkr_wait_spin",
        "strkr_tc_button",
    ),
    "srkr_2d": (
        "srkr_2d_fast_axis_combo",
        "srkr_2d_slow_axis_combo",
        "srkr_2d_fast_min",
        "srkr_2d_fast_max",
        "srkr_2d_fast_step",
        "srkr_2d_slow_min",
        "srkr_2d_slow_max",
        "srkr_2d_slow_step",
        "srkr_2d_wait_spin",
        "srkr_2d_tc_button",
    ),
}


def _rect(geometry: dict[str, Any], name: str, errors: list[str]) -> Rect | None:
    value = geometry.get(name)
    if not isinstance(value, list) or len(value) != 4 or not all(
        isinstance(v, (int, float)) for v in value
    ):
        errors.append(f"missing or invalid geometry: {name}")
        return None
    x, y, width, height = (float(v) for v in value)
    if width < 0 or height < 0:
        errors.append(f"negative geometry: {name}")
    return x, y, width, height


def _inside(inner: Rect, outer: Rect, tolerance: float = 0.01) -> bool:
    ix, iy, iw, ih = inner
    ox, oy, ow, oh = outer
    return (
        ix >= ox - tolerance
        and iy >= oy - tolerance
        and ix + iw <= ox + ow + tolerance
        and iy + ih <= oy + oh + tolerance
    )


def _measurement_controls(workflow: str, geometry: dict[str, Any]) -> list[str]:
    names = set(COMMON_MEASUREMENT_CONTROLS)
    names.update(REQUIRED_MEASUREMENT_CONTROLS.get(workflow, ()))
    # U/V scans expose their theta fields only when selected.  These names do
    # not end in ``_spin`` in the 2D controls, so include both spellings.
    if "spatial_theta_spin" in geometry:
        names.add("spatial_theta_spin")
    names.update(
        name
        for name in geometry
        if name.startswith(f"{workflow}_") and name.endswith("_theta")
    )
    return sorted(names)


def validate_geometry(payload: dict[str, Any], state: str = "") -> list[str]:
    """Return all compact-layout violations for one smoke JSON payload."""
    errors: list[str] = []
    geometry = payload.get("geometry")
    workflow = payload.get("workflow")
    if not isinstance(geometry, dict):
        return ["payload has no geometry mapping"]
    if not isinstance(workflow, str):
        return ["payload has no workflow"]
    if workflow not in REQUIRED_MEASUREMENT_CONTROLS:
        return [f"unknown workflow: {workflow}"]

    center = _rect(geometry, "center_panel", errors)
    measurement = _rect(geometry, "measurement_tabs", errors)
    if center is not None and center[2] < 850.0 - 0.01:
        errors.append(f"center_panel width is {center[2]:g}, expected at least 850")
    if center is not None and measurement is not None and not _inside(measurement, center):
        errors.append("measurement_tabs is outside the centre panel")

    if "right_panel" in geometry:
        right = _rect(geometry, "right_panel", errors)
        expected_right = 27.0 if state == "right-collapsed" else 300.0
        if right is not None and abs(right[2] - expected_right) > 0.01:
            errors.append(f"right_panel width is {right[2]:g}, expected fixed {expected_right:g}")
    if "output_group" in geometry and "run_group" in geometry and measurement is not None:
        output = _rect(geometry, "output_group", errors)
        run = _rect(geometry, "run_group", errors)
        if output is not None and run is not None:
            page_right = measurement[0] + measurement[2] - 4.0
            page_top = measurement[1] + 26.0
            page_bottom = measurement[1] + measurement[3] - 4.0
            margins = (page_right - output[0] - output[2], output[1] - page_top,
                       run[1] - output[1] - output[3], page_bottom - run[1] - run[3])
            if any(abs(margin - 12.0) > 0.01 for margin in margins):
                errors.append("Output/Run frame margins must all be 12 pixels")

    if "session_group" in geometry:
        config = _rect(geometry, "session_group", errors)
        rows = (("config_path", "browse_button"), ("load_button", "save_button"), ("connect_button", "disconnect_button"))
        for row, names in enumerate(rows):
            for name in names:
                control = _rect(geometry, name, errors)
                if config is not None and control is not None:
                    if not _inside(control, config) or abs(control[1] - config[1] - 27.0 - 32.0 * row) > 0.01:
                        errors.append(f"{name} is outside Config row {row + 1}")

    if state != "left-collapsed":
        lockin = _rect(geometry, "lockin_group", errors)
        expected_lockin_height = 348.0 if state in {"devices-expanded", "devices-scrolled"} else 234.0
        if lockin is not None and abs(lockin[3] - expected_lockin_height) > 0.01:
            errors.append(
                f"lockin_group height is {lockin[3]:g}, expected {expected_lockin_height:g}"
            )
        for channel in range(2):
            live = _rect(geometry, f"lockin_live_{channel}", errors)
            if lockin is not None and live is not None and not _inside(live, lockin):
                errors.append(f"lockin_live_{channel} is outside its group")

        for axis in AXES:
            group = _rect(geometry, f"motion_{axis}_group", errors)
            device = _rect(geometry, f"{axis}_device", errors)
            name = _rect(geometry, f"{axis}_device_name", errors)
            if group is not None and device is not None:
                if not _inside(device, group):
                    errors.append(f"{axis}_device is outside its motion group")
                if abs(device[0] - group[0] - 11.0) > 0.01:
                    errors.append(f"{axis}_device is not left-aligned in its motion group")
            if group is not None and name is not None and not _inside(name, group):
                errors.append(f"{axis}_device_name is outside its motion group")
            if name is not None and device is not None and name[0] < device[0] + device[2]:
                errors.append(f"{axis}_device_name overlaps its Device button")
            rows: list[Rect] = []
            for row in range(3):
                for suffix in ("label", "value"):
                    control = _rect(geometry, f"{axis}_row_{row}_{suffix}", errors)
                    if group is not None and control is not None and not _inside(control, group):
                        errors.append(f"{axis}_row_{row}_{suffix} is outside its motion group")
                    if suffix == "value" and control is not None and control[2] < 50:
                        errors.append(f"{axis}_row_{row}_value is too narrow for the position readout")
                row_label = _rect(geometry, f"{axis}_row_{row}_label", errors)
                if row_label is not None:
                    rows.append(row_label)
            if len(rows) == 3 and not (rows[0][1] < rows[1][1] < rows[2][1]):
                errors.append(f"{axis} motion rows are not ordered")
            for name in (
                f"move_{axis}_spin",
                f"move_{axis}_button",
                f"use_current_{axis}_button",
                f"{axis}_zero_spin",
                f"{axis}_cor_spin",
                f"{axis}_cor_button",
            ):
                control = _rect(geometry, name, errors)
                if group is not None and control is not None and not _inside(control, group):
                    errors.append(f"{name} is outside its motion group")

        lockin_device = _rect(geometry, "lockin_device", errors)
        lockin_name = _rect(geometry, "lockin_name", errors)
        if lockin is not None and lockin_device is not None:
            if not _inside(lockin_device, lockin):
                errors.append("lockin_device is outside its group")
            if abs(lockin_device[0] - lockin[0] - 11.0) > 0.01:
                errors.append("lockin_device is not left-aligned in its group")
        if lockin is not None and lockin_name is not None and not _inside(lockin_name, lockin):
            errors.append("lockin_name is outside its group")
        if lockin_name is not None and lockin_device is not None and lockin_name[0] < lockin_device[0] + lockin_device[2]:
            errors.append("lockin_name overlaps its Device button")

    if workflow != "signal_monitor":
        roles = ("",) if workflow in {"trkr", "srkr"} else ("fast_", "slow_")
        for role in roles:
            for key in ("min", "max", "step"):
                prefix = f"{workflow}_{role}{key}"
                label = _rect(geometry, prefix + "_label", errors)
                field = _rect(geometry, prefix + ("_spin" if not role else ""), errors)
                hint = _rect(geometry, prefix + "_hint", errors)
                for name, control in (("label", label),("field",field),("hint",hint)):
                    if control is not None and measurement is not None and not _inside(control,measurement):
                        errors.append(f"range {name} outside pane: {prefix}")
                if label is not None and field is not None and label[0]+label[2] > field[0]:
                    errors.append(f"range label overlaps field: {prefix}")
                if field is not None and hint is not None and field[0]+field[2] > hint[0]:
                    errors.append(f"range field overlaps hint: {prefix}")
    if workflow == "srkr_2d":
        theta = [name for name in geometry if name.startswith("srkr_2d_") and name.endswith("_theta")]
        if len(theta) > 1:
            errors.append("SRKR 2D has duplicate theta editors")
        if state in {"rotated", "rotated-reverse"}:
            expected = "srkr_2d_slow_theta" if state == "rotated-reverse" else "srkr_2d_fast_theta"
            if theta != [expected]:
                errors.append("SRKR 2D theta is not exclusively on the U axis")
    if measurement is not None:
        for name in _measurement_controls(workflow, geometry):
            control = _rect(geometry, name, errors)
            if control is not None and not _inside(control, measurement):
                errors.append(f"measurement control outside pane: {name}")
    return errors


def _state_for(path: Path) -> str:
    return next((state for state in ("devices-expanded", "devices-scrolled", "left-collapsed", "right-collapsed", "rotated-reverse", "rotated") if state in path.as_posix()), "")


def _capture_files(paths: list[Path]) -> list[Path]:
    files: list[Path] = []
    for path in paths:
        if path.is_dir():
            files.extend(sorted(path.rglob("layout_*.json")))
        else:
            files.append(path)
    return files


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("capture", nargs="+", type=Path)
    args = parser.parse_args()
    files = _capture_files(args.capture)
    if not files:
        parser.error("no layout_*.json captures found")
    failed = 0
    for path in files:
        try:
            payload = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            print(f"FAIL {path}: {exc}")
            failed += 1
            continue
        errors = validate_geometry(payload, _state_for(path))
        if errors:
            failed += 1
            print(f"FAIL {path}")
            for error in errors:
                print(f"  - {error}")
        else:
            print(f"PASS {path}")
    return int(bool(failed))


if __name__ == "__main__":
    raise SystemExit(main())
