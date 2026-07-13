from __future__ import annotations

import math
import sys
from types import SimpleNamespace

import numpy as np
import pytest
from PySide6 import QtWidgets

from kohdalab.api.models import MeasurementPoint
from kohdalab.apps.trkr_gui import (
    TRKRGui,
    _axis_ticks,
    _finite_row_value,
    _normalized_by_abs_max,
    _replace_combo_items,
    _set_combo_text,
    _valid_scan2d_axes,
    _validated_measurement_point,
)


def _point(row: dict[str, object]) -> MeasurementPoint:
    return MeasurementPoint(index=1, total_points=1, row=row)


@pytest.mark.parametrize("value", [True, math.nan, math.inf, -math.inf])
def test_finite_row_value_rejects_boolean_and_nonfinite_values(value):
    with pytest.raises(ValueError, match="finite"):
        _finite_row_value({"value": value}, "value")


def test_finite_row_value_uses_first_available_key_and_rejects_missing_values():
    assert _finite_row_value({"fallback": "1.5"}, "primary", "fallback") == 1.5
    with pytest.raises(ValueError, match="Missing required value"):
        _finite_row_value({}, "primary", "fallback")


def test_measurement_point_validation_rejects_wrong_payload_and_progress():
    with pytest.raises(TypeError, match="MeasurementPoint"):
        _validated_measurement_point({}, "trkr")

    point = _point(
        {
            "X_V": 1.0,
            "Y_V": 2.0,
            "R_V": 3.0,
            "Theta_deg": 4.0,
            "target_t_cor_ps": 0.0,
        }
    )
    point.index = 2
    with pytest.raises(ValueError, match="point index"):
        _validated_measurement_point(point, "trkr")


def test_measurement_point_validation_rejects_measurement_mismatch_and_unknown_type():
    row = {
        "measurement": "srkr",
        "X_V": 1.0,
        "Y_V": 2.0,
        "R_V": 3.0,
        "Theta_deg": 4.0,
        "target_t_cor_ps": 0.0,
    }
    with pytest.raises(ValueError, match="does not match"):
        _validated_measurement_point(_point(row), "trkr")

    row.pop("measurement")
    with pytest.raises(ValueError, match="Unsupported measurement"):
        _validated_measurement_point(_point(row), "unknown")


def test_axis_ticks_handles_empty_short_and_decimated_series():
    assert _axis_ticks([], []) == []
    assert _axis_ticks([0.0, 1.0], [10.0, 20.0]) == [
        (0.0, "10"),
        (1.0, "20"),
    ]

    ticks = _axis_ticks(
        [float(value) for value in range(10)],
        [float(value) for value in range(10)],
        max_ticks=4,
    )
    assert ticks[0] == (0.0, "0")
    assert ticks[-1] == (9.0, "9")

    unaligned = _axis_ticks(
        [float(value) for value in range(11)],
        [float(value) for value in range(11)],
        max_ticks=4,
    )
    assert unaligned[-2:] == [(9.0, "9"), (10.0, "10")]


def test_image_normalization_preserves_empty_finite_sets_and_zero_images():
    nonfinite = np.array([[math.nan, math.inf]])
    zero = np.zeros((2, 2))

    assert _normalized_by_abs_max(nonfinite) is nonfinite
    assert _normalized_by_abs_max(zero) is zero


@pytest.mark.parametrize(
    ("mode", "fast", "slow", "expected"),
    [
        ("strkr", "t", "y", ("t", "y")),
        ("strkr", "t", "t", ("t", "x")),
        ("strkr", "y", "y", ("y", "t")),
        ("strkr", "z", "t", ("x", "t")),
        ("strkr", "z", "y", ("t", "y")),
        ("strkr", "z", "z", ("t", "x")),
        ("srkr_2d", "x", "y", ("x", "y")),
        ("srkr_2d", "x", "x", ("x", "y")),
        ("srkr_2d", "y", "y", ("y", "x")),
        ("srkr_2d", "z", "x", ("y", "x")),
        ("srkr_2d", "z", "y", ("x", "y")),
        ("srkr_2d", "z", "z", ("x", "y")),
    ],
)
def test_scan2d_axis_validation_uses_deterministic_safe_fallbacks(
    mode, fast, slow, expected
):
    assert _valid_scan2d_axes(mode, fast, slow) == expected


def test_combo_helpers_select_existing_and_insert_custom_values(monkeypatch):
    monkeypatch.setenv("QT_QPA_PLATFORM", "offscreen")
    app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    combo = QtWidgets.QComboBox()
    combo.addItems(["alpha", "beta"])

    _set_combo_text(combo, "beta")
    assert combo.currentText() == "beta"

    _set_combo_text(combo, "custom")
    assert combo.currentIndex() == 0
    assert combo.currentText() == "custom"

    combo.deleteLater()
    app.processEvents()


def test_replace_combo_items_handles_restricted_and_empty_candidates(monkeypatch):
    monkeypatch.setenv("QT_QPA_PLATFORM", "offscreen")
    app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    combo = QtWidgets.QComboBox()
    combo.addItem("old")

    _replace_combo_items(combo, ["x", "y"], current="missing", allow_custom=False)
    assert combo.currentText() == "x"
    assert not combo.signalsBlocked()

    _replace_combo_items(combo, [], current="missing", allow_custom=False)
    assert combo.count() == 0
    assert not combo.signalsBlocked()

    combo.deleteLater()
    app.processEvents()


def test_scan2d_widget_helpers_return_safe_defaults_for_unknown_mode():
    gui = SimpleNamespace()

    assert TRKRGui._scan2d_axis_range_widgets(gui, "unknown") == {}
    assert TRKRGui._scan2d_role_spin_widgets(gui, "unknown") == {}
    assert TRKRGui._scan2d_role_label_widgets(gui, "unknown") == {}
    assert TRKRGui._scan2d_role_hint_widgets(gui, "unknown") == {}
    assert TRKRGui._scan2d_control_axes(gui, "unknown") == ("x", "y")

    TRKRGui._normalize_2d_axis_controls(gui, "unknown")
    TRKRGui._load_scan2d_role_ranges(gui, "unknown")


def test_panel_size_helper_is_safe_before_ui_panels_exist():
    TRKRGui._apply_panel_sizes(SimpleNamespace())


def test_gui_log_stream_installation_forwards_lines_and_restores_process_streams():
    original_stdout = sys.stdout
    original_stderr = sys.stderr
    logged: list[str] = []
    gui = SimpleNamespace(append_log=logged.append)

    try:
        TRKRGui._install_log_streams(gui)
        assert sys.stdout is gui._stdout_stream
        assert sys.stderr is gui._stderr_stream

        sys.stdout.write("stdout line\n")
        sys.stderr.write("stderr line\n")
    finally:
        TRKRGui._restore_log_streams(gui)

    assert sys.stdout is original_stdout
    assert sys.stderr is original_stderr
    assert logged == ["stdout line", "stderr: stderr line"]

    TRKRGui._restore_log_streams(gui)


@pytest.mark.parametrize("index", [-1, 0, 2])
def test_output_run_attachment_is_safe_before_ui_or_for_invalid_tab(index):
    if index == -1:
        gui = SimpleNamespace(measurement_side_layouts=[])
    else:
        gui = SimpleNamespace(output_run_widget=object(), measurement_side_layouts=[])

    TRKRGui._attach_output_run_to_tab(gui, index)


def test_optional_gui_state_helpers_are_safe_before_widgets_exist():
    gui = SimpleNamespace()

    TRKRGui._store_current_output_settings(gui)
    TRKRGui._refresh_scan2d_role_hints(gui, "unknown")
