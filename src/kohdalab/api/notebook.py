from __future__ import annotations

import math
from collections.abc import Mapping
from typing import Any, Callable

from kohdalab.api.scan_plan import normalize_coordinate


def _nonempty_text(value: object, name: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"{name} must be a non-empty string.")
    return value.strip()


def _finite_number(
    value: object,
    name: str,
    *,
    error_type: type[ValueError] | type[RuntimeError] = ValueError,
) -> float:
    if isinstance(value, bool):
        raise error_type(f"{name} must be a finite number, not boolean.")
    try:
        number = float(value)  # type: ignore[arg-type]
    except (TypeError, ValueError) as exc:
        raise error_type(f"{name} must be a finite number.") from exc
    if not math.isfinite(number):
        raise error_type(f"{name} must be finite.")
    return number


def _validate_progress(index: object, total: object) -> tuple[int, int]:
    if (
        isinstance(index, bool)
        or not isinstance(index, int)
        or isinstance(total, bool)
        or not isinstance(total, int)
        or total < 1
        or not 1 <= index <= total
    ):
        raise ValueError(
            "progress must satisfy 1 <= index <= total with integer values."
        )
    return index, total


def _format_value(value: Any) -> str:
    if value is None:
        return "-"
    if isinstance(value, float):
        if not math.isfinite(value):
            raise ValueError("display values must be finite.")
        return f"{value:.6g}"
    return str(value)


def format_point(point: Any, *, axis_key: str) -> str:
    axis_key = _nonempty_text(axis_key, "axis_key")
    row = point.row
    if not isinstance(row, Mapping):
        raise TypeError("point.row must be a mapping.")
    index, total = _validate_progress(point.index, point.total_points)
    required = (axis_key, "X_V", "Y_V", "R_V", "Theta_deg")
    missing = [key for key in required if key not in row]
    if missing:
        raise KeyError(
            f"Measurement point is missing required fields: {', '.join(missing)}"
        )
    numeric = {
        key: _finite_number(row[key], f"measurement field {key}") for key in required
    }
    x_value = numeric[axis_key]
    values = [
        f"[{index}/{total}]",
        f"{axis_key}={_format_value(x_value)}",
        f"X={_format_value(numeric['X_V'])} V",
        f"Y={_format_value(numeric['Y_V'])} V",
        f"R={_format_value(numeric['R_V'])} V",
        f"Theta={_format_value(numeric['Theta_deg'])} deg",
    ]
    return " ".join(values)


def format_move_abs_row(row: dict[str, Any], *, index: int = 1, total: int = 1) -> str:
    if not isinstance(row, Mapping):
        raise TypeError("row must be a mapping.")
    index, total = _validate_progress(index, total)
    axis = _nonempty_text(row.get("axis"), "row axis").lower()
    if axis not in {"t", "x", "y"}:
        raise ValueError("row axis must be 't', 'x', or 'y'.")
    if "target" not in row or "coordinate" not in row:
        raise KeyError("Move row must contain target and coordinate fields.")
    target = _finite_number(row["target"], "target")
    coordinate = _nonempty_text(row["coordinate"], "coordinate")
    if axis == "t":
        values = [
            f"[{index}/{total}]",
            f"t_ps={_format_value(row.get('t_ps'))}",
            f"t_cor_ps={_format_value(row.get('t_cor_ps'))}",
            f"delay_stage_mm={_format_value(row.get('delay_stage_mm'))}",
            f"delay_stage_pulse={_format_value(row.get('delay_stage_pulse'))}",
        ]
    else:
        control_keys = [
            key for key in (f"{axis}_scanner_mm", f"{axis}_scanner_deg") if key in row
        ]
        if len(control_keys) > 1:
            raise ValueError(f"Move row contains multiple {axis} scanner units.")
        control_key = control_keys[0] if control_keys else None
        values = [
            f"[{index}/{total}]",
            f"{axis}_um={_format_value(row.get(f'{axis}_um'))}",
            f"{axis}_cor_um={_format_value(row.get(f'{axis}_cor_um'))}",
        ]
        if control_key is not None:
            values.append(f"{control_key}={_format_value(row.get(control_key))}")
    values.extend(
        [
            f"target={_format_value(target)}",
            f"coordinate={_format_value(coordinate)}",
        ]
    )
    return " ".join(values)


def move_abs_row_from_position(
    axis: str,
    position: Any,
    *,
    target: float,
    coordinate: str,
    zero: dict[str, float] | None = None,
) -> dict[str, Any]:
    axis = _nonempty_text(axis, "axis").lower()
    if axis not in {"t", "x", "y"}:
        raise ValueError("axis must be 't', 'x', or 'y'.")
    target_value = _finite_number(target, "target")
    coordinate = normalize_coordinate(_nonempty_text(coordinate, "coordinate"))
    if zero is not None and not isinstance(zero, dict):
        raise TypeError("zero must be a dictionary or None.")
    zero = zero or {}
    row: dict[str, Any] = {
        "axis": axis,
        "target": target_value,
        "coordinate": coordinate,
    }
    if axis == "t":
        t_ps = _finite_number(
            getattr(position, "t_ps", None),
            "position.t_ps",
            error_type=RuntimeError,
        )
        zero_t = _finite_number(zero.get("t_ps", 0.0), "zero.t_ps")
        delay_stage_mm = getattr(position, "delay_stage_mm", None)
        if delay_stage_mm is not None:
            delay_stage_mm = _finite_number(
                delay_stage_mm,
                "position.delay_stage_mm",
                error_type=RuntimeError,
            )
        delay_stage_pulse = getattr(position, "delay_stage_pulse", None)
        if delay_stage_pulse is not None and (
            isinstance(delay_stage_pulse, bool)
            or not isinstance(delay_stage_pulse, int)
        ):
            raise RuntimeError("position.delay_stage_pulse must be an integer or None.")
        row.update(
            {
                "t_ps": t_ps,
                "t_cor_ps": t_ps - zero_t,
                "delay_stage_mm": delay_stage_mm,
                "delay_stage_pulse": delay_stage_pulse,
            }
        )
    else:
        value = _finite_number(
            getattr(position, f"{axis}_um", None),
            f"position.{axis}_um",
            error_type=RuntimeError,
        )
        zero_value = _finite_number(zero.get(f"{axis}_um", 0.0), f"zero.{axis}_um")
        scanner_value = getattr(position, f"scanner_{axis}_value", None)
        scanner_unit = getattr(position, f"scanner_{axis}_unit", None)
        row.update(
            {
                f"{axis}_um": value,
                f"{axis}_cor_um": value - zero_value,
            }
        )
        if scanner_unit is None:
            if scanner_value is not None:
                raise RuntimeError(
                    f"position.scanner_{axis}_value requires a matching scanner unit."
                )
        else:
            if scanner_unit not in {"mm", "deg"}:
                raise RuntimeError(
                    f"position.scanner_{axis}_unit must be 'mm', 'deg', or None."
                )
            row[f"{axis}_scanner_{scanner_unit}"] = _finite_number(
                scanner_value,
                f"position.scanner_{axis}_value",
                error_type=RuntimeError,
            )
    return row


def _voltage_scale_from_magnitude(max_abs_value: float) -> tuple[float, str]:
    max_abs_value = _finite_number(max_abs_value, "voltage magnitude")
    if max_abs_value < 0:
        raise ValueError("voltage magnitude must be non-negative.")
    if max_abs_value < 1e-6:
        return 1e9, "nV"
    if max_abs_value < 1e-3:
        return 1e6, "uV"
    if max_abs_value < 1:
        return 1e3, "mV"
    return 1.0, "V"


def _axis_meta_from_key(key: str) -> tuple[str, str | None]:
    key = _nonempty_text(key, "axis key")
    base_key = key
    unit: str | None = None

    for suffix, detected_unit in (
        ("_ps", "ps"),
        ("_mm", "mm"),
        ("_um", "um"),
        ("_deg", "deg"),
        ("_V", "V"),
        ("_s", "s"),
    ):
        if key.endswith(suffix):
            base_key = key[: -len(suffix)]
            unit = detected_unit
            break

    label_map = {
        "t": "t",
        "t_cor": "t_cor",
        "delay_stage": "delay stage",
        "x_scanner": "x scanner",
        "y_scanner": "y scanner",
        "x": "x",
        "x_cor": "x_cor",
        "y": "y",
        "y_cor": "y_cor",
        "elapsed": "elapsed time",
        "X": "X",
        "Y": "Y",
        "R": "R",
        "Theta": "Theta",
    }
    return label_map.get(base_key, base_key), unit


def _build_axis_label(key: str, override: str | None) -> str:
    if override:
        return override
    label, unit = _axis_meta_from_key(key)
    return f"{label} ({unit})" if unit else label


def _make_live_update(
    *,
    x_key: str,
    y_key: str,
    xlabel: str | None = None,
    ylabel: str | None = None,
    title: str | None = None,
) -> Callable[[Any], None]:
    from IPython.display import display

    typed_display: Callable[..., Any] = display
    import matplotlib.pyplot as plt

    x_key = _nonempty_text(x_key, "x_key")
    y_key = _nonempty_text(y_key, "y_key")
    xs: list[float] = []
    ys: list[float] = []
    x_label_name, x_unit = _axis_meta_from_key(x_key)
    y_label_name, y_unit = _axis_meta_from_key(y_key)
    x_scale = 1.0
    y_scale = 1.0

    fig, ax = plt.subplots(figsize=(8, 4))
    (line,) = ax.plot([], [], marker="o", ms=3)
    display_handle = None
    ax.set_xlabel(_build_axis_label(x_key, xlabel))
    ax.set_ylabel(_build_axis_label(y_key, ylabel))
    ax.grid(True, alpha=0.3)
    if title:
        ax.set_title(title)

    def live_update(point: Any) -> None:
        nonlocal display_handle, x_scale, y_scale
        row = point.row
        if not isinstance(row, Mapping):
            raise TypeError("point.row must be a mapping.")
        if x_key not in row or y_key not in row:
            missing = [key for key in (x_key, y_key) if key not in row]
            raise KeyError(
                f"Plot point is missing required fields: {', '.join(missing)}"
            )
        x_value = _finite_number(row[x_key], f"plot field {x_key}")
        y_value = _finite_number(row[y_key], f"plot field {y_key}")
        xs.append(x_value)
        ys.append(y_value)

        if x_unit == "V":
            x_scale, x_display_unit = _voltage_scale_from_magnitude(
                max(abs(value) for value in xs)
            )
            if xlabel is None:
                ax.set_xlabel(f"{x_label_name} ({x_display_unit})")
        if y_unit == "V":
            y_scale, y_display_unit = _voltage_scale_from_magnitude(
                max(abs(value) for value in ys)
            )
            if ylabel is None:
                ax.set_ylabel(f"{y_label_name} ({y_display_unit})")

        line.set_data(
            [value * x_scale for value in xs], [value * y_scale for value in ys]
        )
        ax.relim()
        ax.autoscale_view()

        if display_handle is None:
            display_handle = typed_display(fig, display_id=True)
        else:
            display_handle.update(fig)

    return live_update


def make_trkr_live_update(
    *,
    x_key: str | None = "t_cor_ps",
    y_key: str | None = "X_V",
    xlabel: str | None = None,
    ylabel: str | None = None,
    title: str | None = None,
) -> Callable[[Any], None]:
    return _make_live_update(
        x_key=x_key or "t_cor_ps",
        y_key=y_key or "X_V",
        xlabel=xlabel,
        ylabel=ylabel,
        title=title,
    )


def make_srkr_live_update(
    *,
    fast_axis: str | None = None,
    scan_axis_name: str | None = None,
    x_key: str | None = None,
    y_key: str | None = "X_V",
    xlabel: str | None = None,
    ylabel: str | None = None,
    title: str | None = None,
) -> Callable[[Any], None]:
    if fast_axis is not None and scan_axis_name is not None:
        normalized_fast = _nonempty_text(fast_axis, "fast_axis").lower()
        normalized_alias = _nonempty_text(scan_axis_name, "scan_axis_name").lower()
        if normalized_fast != normalized_alias:
            raise ValueError(
                "fast_axis and scan_axis_name must refer to the same axis."
            )
    axis = _nonempty_text(fast_axis or scan_axis_name or "x", "fast_axis").lower()
    if axis not in {"x", "y"}:
        raise ValueError("fast_axis must be 'x' or 'y'.")
    return make_trkr_live_update(
        x_key=x_key or f"{axis}_cor_um",
        y_key=y_key,
        xlabel=xlabel,
        ylabel=ylabel,
        title=title,
    )


def _fast_axis_key(axis: str) -> str:
    axis = _nonempty_text(axis, "fast_axis").lower()
    if axis == "t":
        return "t_cor_ps"
    if axis in {"x", "y"}:
        return f"{axis}_cor_um"
    raise ValueError("fast_axis must be 't', 'x', or 'y'.")


def make_strkr_live_update(
    *,
    fast_axis: str = "t",
    x_key: str | None = None,
    y_key: str | None = "X_V",
    xlabel: str | None = None,
    ylabel: str | None = None,
    title: str | None = None,
) -> Callable[[Any], None]:
    return make_trkr_live_update(
        x_key=x_key or _fast_axis_key(fast_axis),
        y_key=y_key,
        xlabel=xlabel,
        ylabel=ylabel,
        title=title,
    )


def make_srkr_2d_live_update(
    *,
    fast_axis: str = "x",
    x_key: str | None = None,
    y_key: str | None = "X_V",
    xlabel: str | None = None,
    ylabel: str | None = None,
    title: str | None = None,
) -> Callable[[Any], None]:
    axis = _nonempty_text(fast_axis, "fast_axis").lower()
    if axis not in {"x", "y"}:
        raise ValueError("fast_axis must be 'x' or 'y'.")
    return make_trkr_live_update(
        x_key=x_key or f"{axis}_cor_um",
        y_key=y_key,
        xlabel=xlabel,
        ylabel=ylabel,
        title=title,
    )


def make_signal_monitor_live_update(
    *,
    x_key: str | None = "elapsed_s",
    y_key: str | None = "R_V",
    xlabel: str | None = None,
    ylabel: str | None = None,
    title: str | None = None,
) -> Callable[[Any], None]:
    return _make_live_update(
        x_key=x_key or "elapsed_s",
        y_key=y_key or "R_V",
        xlabel=xlabel,
        ylabel=ylabel,
        title=title,
    )
