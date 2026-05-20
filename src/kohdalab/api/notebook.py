from __future__ import annotations

from typing import Any, Callable


def _format_value(value: Any) -> str:
    if value is None:
        return "-"
    if isinstance(value, float):
        return f"{value:.6g}"
    return str(value)


def format_point(point: Any, *, axis_key: str) -> str:
    row = point.row
    x_value = row.get(axis_key, row.get("elapsed_s"))
    values = [
        f"[{point.index}/{point.total_points}]",
        f"{axis_key}={_format_value(x_value)}",
        f"X={_format_value(row.get('X_V'))} V",
        f"Y={_format_value(row.get('Y_V'))} V",
        f"R={_format_value(row.get('R_V'))} V",
        f"Theta={_format_value(row.get('Theta_deg'))} deg",
    ]
    return " ".join(values)


def format_move_abs_row(row: dict[str, Any], *, index: int = 1, total: int = 1) -> str:
    axis = str(row.get("axis", "")).lower()
    if axis == "t":
        values = [
            f"[{index}/{total}]",
            f"t_ps={_format_value(row.get('t_ps'))}",
            f"t_cor_ps={_format_value(row.get('t_cor_ps'))}",
            f"delay_stage_mm={_format_value(row.get('delay_stage_mm'))}",
            f"delay_stage_pulse={_format_value(row.get('delay_stage_pulse'))}",
        ]
    elif axis in {"x", "y"}:
        control_key = next(
            (
                key
                for key in row
                if key.startswith(f"{axis}_scanner_")
            ),
            None,
        )
        values = [
            f"[{index}/{total}]",
            f"{axis}_um={_format_value(row.get(f'{axis}_um'))}",
            f"{axis}_cor_um={_format_value(row.get(f'{axis}_cor_um'))}",
        ]
        if control_key is not None:
            values.append(f"{control_key}={_format_value(row.get(control_key))}")
    else:
        values = [
            f"[{index}/{total}]",
            f"axis={_format_value(row.get('axis'))}",
        ]
    values.extend(
        [
            f"target={_format_value(row.get('target'))}",
            f"coordinate={_format_value(row.get('coordinate'))}",
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
    axis = axis.strip().lower()
    zero = zero or {}
    row: dict[str, Any] = {
        "axis": axis,
        "target": target,
        "coordinate": coordinate,
    }
    if axis == "t":
        row.update(
            {
                "t_ps": position.t_ps,
                "t_cor_ps": None if position.t_ps is None else position.t_ps - float(zero.get("t_ps", 0.0)),
                "delay_stage_mm": position.delay_stage_mm,
                "delay_stage_pulse": position.delay_stage_pulse,
            }
        )
    elif axis in {"x", "y"}:
        value = getattr(position, f"{axis}_um")
        scanner_value = getattr(position, f"scanner_{axis}_value")
        scanner_unit = getattr(position, f"scanner_{axis}_unit")
        row.update(
            {
                f"{axis}_um": value,
                f"{axis}_cor_um": None if value is None else value - float(zero.get(f"{axis}_um", 0.0)),
            }
        )
        if scanner_unit is not None:
            row[f"{axis}_scanner_{scanner_unit}"] = scanner_value
    return row


def _voltage_scale_from_magnitude(max_abs_value: float) -> tuple[float, str]:
    if max_abs_value < 1e-6:
        return 1e9, "nV"
    if max_abs_value < 1e-3:
        return 1e6, "uV"
    if max_abs_value < 1:
        return 1e3, "mV"
    return 1.0, "V"


def _axis_meta_from_key(key: str) -> tuple[str, str | None]:
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
    import matplotlib.pyplot as plt

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
        xs.append(row[x_key])
        ys.append(row[y_key])

        if x_unit == "V":
            x_scale, x_display_unit = _voltage_scale_from_magnitude(max(abs(value) for value in xs))
            if xlabel is None:
                ax.set_xlabel(f"{x_label_name} ({x_display_unit})")
        if y_unit == "V":
            y_scale, y_display_unit = _voltage_scale_from_magnitude(max(abs(value) for value in ys))
            if ylabel is None:
                ax.set_ylabel(f"{y_label_name} ({y_display_unit})")

        line.set_data([value * x_scale for value in xs], [value * y_scale for value in ys])
        ax.relim()
        ax.autoscale_view()

        if display_handle is None:
            display_handle = display(fig, display_id=True)
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
    axis = (fast_axis or scan_axis_name or "x").strip().lower()
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
    axis = axis.strip().lower()
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
    axis = fast_axis.strip().lower()
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
