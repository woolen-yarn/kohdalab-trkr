from __future__ import annotations

import sys
from types import ModuleType
from types import SimpleNamespace

import pytest

import kohdalab.api.notebook as notebook_module
from kohdalab.api.models import MeasurementPoint, Position
from kohdalab.api.notebook import (
    _axis_meta_from_key,
    _build_axis_label,
    _voltage_scale_from_magnitude,
    format_move_abs_row,
    format_point,
    make_srkr_2d_live_update,
    make_srkr_live_update,
    make_strkr_live_update,
    make_trkr_live_update,
    move_abs_row_from_position,
)


def test_srkr_live_update_accepts_fast_axis_alias(monkeypatch):
    calls = []
    monkeypatch.setattr(
        "kohdalab.api.notebook.make_trkr_live_update",
        lambda **kwargs: calls.append(kwargs) or (lambda _point: None),
    )

    make_srkr_live_update(fast_axis="y", y_key="R_V")

    assert calls == [
        {
            "x_key": "y_cor_um",
            "y_key": "R_V",
            "xlabel": None,
            "ylabel": None,
            "title": None,
        }
    ]


def test_strkr_live_update_uses_t_or_spatial_fast_axis(monkeypatch):
    calls = []
    monkeypatch.setattr(
        "kohdalab.api.notebook.make_trkr_live_update",
        lambda **kwargs: calls.append(kwargs) or (lambda _point: None),
    )

    make_strkr_live_update(fast_axis="t")
    make_strkr_live_update(fast_axis="x")

    assert [call["x_key"] for call in calls] == ["t_cor_ps", "x_cor_um"]


def test_srkr_2d_live_update_rejects_t_fast_axis():
    with pytest.raises(ValueError, match="fast_axis"):
        make_srkr_2d_live_update(fast_axis="t")


def test_srkr_live_update_rejects_conflicting_axis_aliases():
    with pytest.raises(ValueError, match="same axis"):
        make_srkr_live_update(fast_axis="x", scan_axis_name="y")


def test_format_point_requires_valid_progress_and_exact_schema():
    point = MeasurementPoint(
        index=1,
        total_points=2,
        row={
            "elapsed_s": 0.5,
            "X_V": 1e-3,
            "Y_V": 2e-3,
            "R_V": 3e-3,
            "Theta_deg": 45.0,
        },
    )

    assert format_point(point, axis_key="elapsed_s").startswith("[1/2] elapsed_s=0.5")
    with pytest.raises(KeyError, match="typo"):
        format_point(point, axis_key="typo")

    point.index = 0
    with pytest.raises(ValueError, match="progress"):
        format_point(point, axis_key="elapsed_s")


def test_formatters_reject_nonfinite_values():
    point = MeasurementPoint(
        index=1,
        total_points=1,
        row={
            "elapsed_s": 0.0,
            "X_V": 0.0,
            "Y_V": 0.0,
            "R_V": 0.0,
            "Theta_deg": 0.0,
        },
    )
    point.row["X_V"] = float("nan")

    with pytest.raises(ValueError, match="finite"):
        format_point(point, axis_key="elapsed_s")
    with pytest.raises(ValueError, match="finite"):
        format_move_abs_row(
            {"axis": "t", "target": float("inf"), "coordinate": "measurement"}
        )


def test_move_abs_row_validates_axis_position_and_scanner_unit():
    position = Position(
        x_um=12.0,
        scanner_x_value=0.12,
        scanner_x_unit="mm",
    )

    row = move_abs_row_from_position(
        "x",
        position,
        target=12.0,
        coordinate="control",
        zero={"x_um": 2.0},
    )

    assert row == {
        "axis": "x",
        "target": 12.0,
        "coordinate": "interface",
        "x_um": 12.0,
        "x_cor_um": 10.0,
        "x_scanner_mm": 0.12,
    }
    with pytest.raises(ValueError, match="axis"):
        move_abs_row_from_position("z", position, target=0.0, coordinate="measurement")

    position.scanner_x_unit = "inch"
    with pytest.raises(RuntimeError, match="scanner_x_unit"):
        move_abs_row_from_position("x", position, target=12.0, coordinate="measurement")


def test_move_abs_row_rejects_ambiguous_scanner_units():
    row = {
        "axis": "x",
        "target": 1.0,
        "coordinate": "measurement",
        "x_scanner_mm": 0.1,
        "x_scanner_deg": 0.2,
    }

    with pytest.raises(ValueError, match="multiple x scanner units"):
        format_move_abs_row(row)


def test_move_abs_row_from_position_requires_scanner_value_and_unit_together():
    position = SimpleNamespace(x_um=12.0, scanner_x_value=0.12, scanner_x_unit=None)

    with pytest.raises(RuntimeError, match="requires a matching scanner unit"):
        move_abs_row_from_position("x", position, target=12.0, coordinate="measurement")

    position.scanner_x_value = None
    position.scanner_x_unit = "mm"
    with pytest.raises(RuntimeError, match="scanner_x_value must be a finite number"):
        move_abs_row_from_position("x", position, target=12.0, coordinate="measurement")


@pytest.mark.parametrize("invalid_pulse", [True, 1.5, "100"])
def test_move_abs_row_from_position_rejects_noninteger_delay_stage_pulse(
    invalid_pulse,
):
    position = SimpleNamespace(
        t_ps=1.0,
        delay_stage_mm=None,
        delay_stage_pulse=invalid_pulse,
    )

    with pytest.raises(RuntimeError, match="delay_stage_pulse must be an integer"):
        move_abs_row_from_position("t", position, target=1.0, coordinate="measurement")


@pytest.mark.parametrize("value", [-1.0, float("nan"), float("inf"), True])
def test_voltage_scale_rejects_invalid_magnitude(value):
    with pytest.raises(ValueError):
        _voltage_scale_from_magnitude(value)


@pytest.mark.parametrize(
    ("key", "expected"),
    [
        ("t_cor_ps", ("t_cor", "ps")),
        ("x_scanner_mm", ("x scanner", "mm")),
        ("y_um", ("y", "um")),
        ("Theta_deg", ("Theta", "deg")),
        ("X_V", ("X", "V")),
        ("elapsed_s", ("elapsed time", "s")),
        ("custom", ("custom", None)),
    ],
)
def test_axis_metadata_detects_units_and_humanizes_known_keys(key, expected):
    assert _axis_meta_from_key(key) == expected


def test_axis_label_prefers_explicit_override_and_formats_detected_unit():
    assert _build_axis_label("x_um", None) == "x (um)"
    assert _build_axis_label("custom", None) == "custom"
    assert _build_axis_label("x_um", "position") == "position"


def test_live_plot_rejects_invalid_points_before_mutating_history(monkeypatch):
    pyplot = pytest.importorskip("matplotlib.pyplot", exc_type=ImportError)
    display_module = pytest.importorskip("IPython.display", exc_type=ImportError)

    class FakeLine:
        def __init__(self):
            self.xs = []
            self.ys = []

        def set_data(self, xs, ys):
            self.xs = list(xs)
            self.ys = list(ys)

    class FakeAxes:
        def __init__(self):
            self.line = FakeLine()

        def plot(self, *_args, **_kwargs):
            return (self.line,)

        def set_xlabel(self, _value):
            return None

        def set_ylabel(self, _value):
            return None

        def set_title(self, _value):
            return None

        def grid(self, *_args, **_kwargs):
            return None

        def relim(self):
            return None

        def autoscale_view(self):
            return None

    axes = FakeAxes()
    monkeypatch.setattr(pyplot, "subplots", lambda **_kwargs: (object(), axes))
    monkeypatch.setattr(
        display_module,
        "display",
        lambda *_args, **_kwargs: SimpleNamespace(update=lambda _figure: None),
    )
    update = make_trkr_live_update(x_key="t_cor_ps", y_key="X_V")

    update(SimpleNamespace(row={"t_cor_ps": 1.0, "X_V": 1e-3}))
    with pytest.raises(ValueError, match="finite"):
        update(SimpleNamespace(row={"t_cor_ps": 2.0, "X_V": float("nan")}))
    update(SimpleNamespace(row={"t_cor_ps": 3.0, "X_V": 3e-3}))

    assert axes.line.xs == [1.0, 3.0]
    assert axes.line.ys == [1.0, 3.0]


def test_format_move_abs_row_formats_delay_stage_fields_and_missing_values():
    text = format_move_abs_row(
        {
            "axis": "T",
            "target": "2.5",
            "coordinate": "measurement",
            "t_ps": 2.5,
            "delay_stage_pulse": 125,
        },
        index=2,
        total=3,
    )

    assert text == (
        "[2/3] t_ps=2.5 t_cor_ps=- delay_stage_mm=- "
        "delay_stage_pulse=125 target=2.5 coordinate=measurement"
    )


def test_format_move_abs_row_formats_spatial_axis_without_control_unit():
    assert (
        format_move_abs_row(
            {
                "axis": "y",
                "target": 4.0,
                "coordinate": "interface",
                "y_um": 4.0,
                "y_cor_um": 1.5,
            }
        )
        == "[1/1] y_um=4 y_cor_um=1.5 target=4 coordinate=interface"
    )


def test_move_abs_row_from_position_supports_delay_stage_legacy_optional_fields():
    position = SimpleNamespace(
        t_ps=3.0,
        delay_stage_mm=None,
        delay_stage_pulse=None,
    )

    assert move_abs_row_from_position(
        " t ",
        position,
        target="3",  # type: ignore[arg-type]
        coordinate="physical",
        zero={"t_ps": 1.0},
    ) == {
        "axis": "t",
        "target": 3.0,
        "coordinate": "physical",
        "t_ps": 3.0,
        "t_cor_ps": 2.0,
        "delay_stage_mm": None,
        "delay_stage_pulse": None,
    }


def test_signal_monitor_live_update_uses_default_keys_when_none(monkeypatch):
    calls = []
    monkeypatch.setattr(
        notebook_module,
        "_make_live_update",
        lambda **kwargs: calls.append(kwargs) or (lambda _point: None),
    )

    notebook_module.make_signal_monitor_live_update(x_key=None, y_key=None)

    assert calls[0]["x_key"] == "elapsed_s"
    assert calls[0]["y_key"] == "R_V"


def test_format_point_rejects_nonmapping_row_and_empty_axis_key():
    point = SimpleNamespace(index=1, total_points=1, row=[])

    with pytest.raises(TypeError, match="point.row must be a mapping"):
        format_point(point, axis_key="elapsed_s")
    with pytest.raises(ValueError, match="axis_key must be a non-empty string"):
        format_point(point, axis_key=" ")


@pytest.mark.parametrize(
    ("row", "error", "message"),
    [
        ([], TypeError, "row must be a mapping"),
        (
            {"axis": "z", "target": 1, "coordinate": "measurement"},
            ValueError,
            "row axis",
        ),
        ({"axis": "x", "coordinate": "measurement"}, KeyError, "target and coordinate"),
        ({"axis": "x", "target": 1}, KeyError, "target and coordinate"),
    ],
)
def test_format_move_abs_row_rejects_malformed_public_rows(row, error, message):
    with pytest.raises(error, match=message):
        format_move_abs_row(row)  # type: ignore[arg-type]


def test_move_abs_row_from_position_rejects_nonfinite_zero_and_missing_axis_value():
    with pytest.raises(ValueError, match="zero.x_um must be finite"):
        move_abs_row_from_position(
            "x",
            Position(x_um=1.0),
            target=1.0,
            coordinate="measurement",
            zero={"x_um": float("inf")},
        )

    with pytest.raises(RuntimeError, match="position.y_um must be a finite number"):
        move_abs_row_from_position(
            "y",
            SimpleNamespace(
                y_um=None,
                scanner_y_value=None,
                scanner_y_unit=None,
            ),
            target=1.0,
            coordinate="measurement",
        )


def test_srkr_live_update_accepts_legacy_scan_axis_name(monkeypatch):
    calls = []
    monkeypatch.setattr(
        notebook_module,
        "make_trkr_live_update",
        lambda **kwargs: calls.append(kwargs) or (lambda _point: None),
    )

    make_srkr_live_update(scan_axis_name=" Y ")

    assert calls[0]["x_key"] == "y_cor_um"


@pytest.mark.parametrize("fast_axis", ["", "z"])
def test_strkr_live_update_rejects_invalid_fast_axis(fast_axis):
    with pytest.raises(ValueError, match="fast_axis"):
        make_strkr_live_update(fast_axis=fast_axis)


@pytest.mark.parametrize(
    ("magnitude", "expected"),
    [
        (0.0, (1e9, "nV")),
        (1e-6, (1e6, "uV")),
        (1e-3, (1e3, "mV")),
        (1.0, (1.0, "V")),
    ],
)
def test_voltage_scale_uses_stable_unit_boundaries(magnitude, expected):
    assert _voltage_scale_from_magnitude(magnitude) == expected


@pytest.mark.parametrize(
    ("key", "expected"),
    [
        ("t_cor_ps", ("t_cor", "ps")),
        ("x_scanner_mm", ("x scanner", "mm")),
        ("Theta_deg", ("Theta", "deg")),
        ("custom", ("custom", None)),
    ],
)
def test_axis_metadata_maps_known_and_custom_row_keys(key, expected):
    assert notebook_module._axis_meta_from_key(key) == expected


def test_axis_label_honors_override_and_infers_units():
    assert notebook_module._build_axis_label("elapsed_s", None) == "elapsed time (s)"
    assert notebook_module._build_axis_label("X_V", "Signal") == "Signal"


def test_live_update_public_wrappers_forward_defaults_without_matplotlib(monkeypatch):
    calls = []
    monkeypatch.setattr(
        notebook_module,
        "_make_live_update",
        lambda **kwargs: calls.append(kwargs) or (lambda _point: None),
    )

    notebook_module.make_trkr_live_update(x_key=None, y_key=None)
    notebook_module.make_signal_monitor_live_update(x_key=None, y_key=None)

    assert calls[0]["x_key"] == "t_cor_ps"
    assert calls[0]["y_key"] == "X_V"
    assert calls[1]["x_key"] == "elapsed_s"
    assert calls[1]["y_key"] == "R_V"


def test_srkr_matching_legacy_and_current_axis_names_are_accepted(monkeypatch):
    calls = []
    monkeypatch.setattr(
        notebook_module,
        "make_trkr_live_update",
        lambda **kwargs: calls.append(kwargs) or (lambda _point: None),
    )

    make_srkr_live_update(fast_axis=" X ", scan_axis_name="x")

    assert calls[0]["x_key"] == "x_cor_um"


def test_srkr_2d_y_axis_selects_y_corrected_coordinate(monkeypatch):
    calls = []
    monkeypatch.setattr(
        notebook_module,
        "make_trkr_live_update",
        lambda **kwargs: calls.append(kwargs) or (lambda _point: None),
    )

    make_srkr_2d_live_update(fast_axis=" Y ")

    assert calls[0]["x_key"] == "y_cor_um"


def test_live_update_validates_rows_scales_voltage_and_updates_display(monkeypatch):
    class FakeLine:
        def __init__(self):
            self.data = None

        def set_data(self, xs, ys):
            self.data = (list(xs), list(ys))

    class FakeAxes:
        def __init__(self):
            self.line = FakeLine()
            self.xlabel = None
            self.ylabel = None
            self.title = None
            self.relim_calls = 0
            self.autoscale_calls = 0

        def plot(self, *_args, **_kwargs):
            return (self.line,)

        def set_xlabel(self, value):
            self.xlabel = value

        def set_ylabel(self, value):
            self.ylabel = value

        def set_title(self, value):
            self.title = value

        def grid(self, *_args, **_kwargs):
            return None

        def relim(self):
            self.relim_calls += 1

        def autoscale_view(self):
            self.autoscale_calls += 1

    class FakeDisplayHandle:
        def __init__(self):
            self.updates = []

        def update(self, figure):
            self.updates.append(figure)

    axes = FakeAxes()
    figure = object()
    handle = FakeDisplayHandle()
    display_calls = []

    pyplot = ModuleType("matplotlib.pyplot")
    pyplot.subplots = lambda **_kwargs: (figure, axes)  # type: ignore[attr-defined]
    matplotlib = ModuleType("matplotlib")
    matplotlib.pyplot = pyplot  # type: ignore[attr-defined]
    display_module = ModuleType("IPython.display")

    def display(value, **kwargs):
        display_calls.append((value, kwargs))
        return handle

    display_module.display = display  # type: ignore[attr-defined]
    monkeypatch.setitem(sys.modules, "matplotlib", matplotlib)
    monkeypatch.setitem(sys.modules, "matplotlib.pyplot", pyplot)
    monkeypatch.setitem(sys.modules, "IPython.display", display_module)

    update = notebook_module.make_trkr_live_update(
        x_key="X_V", y_key="R_V", title="Live signal"
    )

    with pytest.raises(TypeError, match="point.row must be a mapping"):
        update(SimpleNamespace(row=[]))
    with pytest.raises(KeyError, match="R_V"):
        update(SimpleNamespace(row={"X_V": 1e-9}))

    update(SimpleNamespace(row={"X_V": 1e-9, "R_V": 2e-9}))
    update(SimpleNamespace(row={"X_V": 1e-3, "R_V": 2e-3}))

    assert axes.xlabel == "X (mV)"
    assert axes.ylabel == "R (mV)"
    assert axes.title == "Live signal"
    assert axes.line.data[0] == pytest.approx([1e-6, 1.0])
    assert axes.line.data[1] == pytest.approx([2e-6, 2.0])
    assert axes.relim_calls == axes.autoscale_calls == 2
    assert display_calls == [(figure, {"display_id": True})]
    assert handle.updates == [figure]

    partial_update = notebook_module.make_trkr_live_update(
        x_key="t_cor_ps",
        y_key="X_V",
        xlabel="Delay",
        ylabel="Signal",
        title=None,
    )
    partial_update(SimpleNamespace(row={"t_cor_ps": 1.0, "X_V": 1e-6}))

    assert axes.xlabel == "Delay"
    assert axes.ylabel == "Signal"

    inverse_partial_update = notebook_module.make_trkr_live_update(
        x_key="X_V",
        y_key="t_cor_ps",
        xlabel="Signal",
    )
    inverse_partial_update(SimpleNamespace(row={"X_V": 1e-6, "t_cor_ps": 1.0}))

    assert axes.xlabel == "Signal"
    assert axes.ylabel == "t_cor (ps)"


def test_notebook_remaining_validation_boundaries():
    with pytest.raises(ValueError, match="display values must be finite"):
        notebook_module._format_value(float("nan"))

    position = SimpleNamespace(
        x_um=1.0,
        scanner_x_value=None,
        scanner_x_unit=None,
    )
    with pytest.raises(TypeError, match="zero must be a dictionary or None"):
        move_abs_row_from_position(
            "x",
            position,
            target=1.0,
            coordinate="measurement",
            zero=[],  # type: ignore[arg-type]
        )

    with pytest.raises(ValueError, match="fast_axis must be 'x' or 'y'"):
        make_srkr_live_update(fast_axis="z")


def test_move_abs_rows_cover_optional_present_and_absent_control_fields():
    delay_row = move_abs_row_from_position(
        "t",
        SimpleNamespace(t_ps=3.0, delay_stage_mm=0.25, delay_stage_pulse=10),
        target=3.0,
        coordinate="measurement",
    )
    scanner_row = move_abs_row_from_position(
        "y",
        SimpleNamespace(
            y_um=5.0,
            scanner_y_value=0.5,
            scanner_y_unit="deg",
        ),
        target=5.0,
        coordinate="measurement",
    )
    no_control_row = move_abs_row_from_position(
        "x",
        SimpleNamespace(x_um=1.0, scanner_x_value=None, scanner_x_unit=None),
        target=1.0,
        coordinate="measurement",
    )

    assert "delay_stage_mm=0.25" in format_move_abs_row(delay_row)
    assert "y_scanner_deg=0.5" in format_move_abs_row(scanner_row)
    assert no_control_row == {
        "axis": "x",
        "target": 1.0,
        "coordinate": "measurement",
        "x_um": 1.0,
        "x_cor_um": 1.0,
    }
