from __future__ import annotations

from dataclasses import dataclass
from typing import Any


def sample_axis_ticks(
    positions: list[float], labels: list[str], *, max_ticks: int = 12
) -> list[tuple[float, str]]:
    if len(positions) <= max_ticks:
        return list(zip(positions, labels, strict=True))

    step = max(1, (len(positions) - 1) // (max_ticks - 1))
    indexes = list(range(0, len(positions), step))
    if indexes[-1] != len(positions) - 1:
        indexes.append(len(positions) - 1)
    return [(positions[index], labels[index]) for index in indexes]


def signal_scale(signal_key: str, voltage_scale: float) -> float:
    return 1.0 if signal_key == "Theta_deg" else voltage_scale


def scan2d_uses_equal_spatial_units(fast_axis: str, slow_axis: str) -> bool:
    return {fast_axis.strip().lower(), slow_axis.strip().lower()} == {"x", "y"}


@dataclass(frozen=True)
class SeriesData:
    x: list[float]
    y: list[float]


def standard_plot_series(
    rows: list[dict[str, Any]],
    *,
    measurement_name: str,
    signal1_key: str,
    signal2_key: str,
    voltage_scale: float,
) -> tuple[SeriesData, SeriesData]:
    x_key = "elapsed_s" if measurement_name == "signal_monitor" else "t_cor_ps"
    x: list[float] = []
    for row in rows:
        value = row.get(x_key, row.get("t_ps"))
        if value is None:
            raise ValueError(f"Missing plot x value: {x_key}")
        x.append(float(value))
    scale1 = signal_scale(signal1_key, voltage_scale)
    scale2 = signal_scale(signal2_key, voltage_scale)
    return (
        SeriesData(x=x, y=[row[signal1_key] * scale1 for row in rows]),
        SeriesData(x=x, y=[row[signal2_key] * scale2 for row in rows]),
    )


@dataclass(frozen=True)
class SrkrPlotData:
    positions: list[float]
    cor_values: list[float]
    signal_values: list[float]


@dataclass(frozen=True)
class SrkrPlotSeries:
    x_signal1: SrkrPlotData
    x_signal2: SrkrPlotData
    y_signal1: SrkrPlotData
    y_signal2: SrkrPlotData


def srkr_plot_series(
    rows: list[dict[str, Any]],
    *,
    signal1_key: str,
    signal2_key: str,
    voltage_scale: float,
) -> SrkrPlotSeries:
    x_rows = [
        row
        for row in rows
        if row.get("fast_axis", row.get("scan_axis")) == "x"
        and row.get("x_um") is not None
    ]
    y_rows = [
        row
        for row in rows
        if row.get("fast_axis", row.get("scan_axis")) == "y"
        and row.get("y_um") is not None
    ]
    scale1 = signal_scale(signal1_key, voltage_scale)
    scale2 = signal_scale(signal2_key, voltage_scale)

    def data(
        axis: str, signal_key: str, scale: float, axis_rows: list[dict[str, Any]]
    ) -> SrkrPlotData:
        return SrkrPlotData(
            positions=[row[f"{axis}_um"] for row in axis_rows],
            cor_values=[row[f"{axis}_cor_um"] for row in axis_rows],
            signal_values=[row[signal_key] * scale for row in axis_rows],
        )

    return SrkrPlotSeries(
        x_signal1=data("x", signal1_key, scale1, x_rows),
        x_signal2=data("x", signal2_key, scale2, x_rows),
        y_signal1=data("y", signal1_key, scale1, y_rows),
        y_signal2=data("y", signal2_key, scale2, y_rows),
    )


def signal_monitor_top_labels(point_count: int) -> list[str]:
    return [str(i + 1) for i in range(point_count)]


def trkr_top_labels(x_values: list[float], zero_ps: float) -> list[str]:
    return [f"{round(value - zero_ps):.0f}" for value in x_values]
