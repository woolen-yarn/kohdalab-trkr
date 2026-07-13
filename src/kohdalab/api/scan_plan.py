from __future__ import annotations

import math
from dataclasses import dataclass
from typing import Any

from kohdalab.api.config import (
    MAX_SCAN_POINTS_TOTAL,
    build_range_points,
    measurement_settings,
    move_abs_zero,
    scan_settings,
)

AXES = {"t", "x", "y"}
SPATIAL_AXES = {"x", "y"}


def normalize_coordinate(coordinate: str | None) -> str:
    normalized = (coordinate or "measurement").strip().lower()
    aliases = {
        "control": "interface",
        "device": "instrument",
    }
    return aliases.get(normalized, normalized)


def normalize_scanner_coordinate(coordinate: str | None) -> str:
    normalized = normalize_coordinate(coordinate)
    if normalized == "instrument":
        return "interface"
    return normalized


@dataclass(frozen=True)
class SignalMonitorPlan:
    interval_s: float
    n_points: int
    summary: str


@dataclass(frozen=True)
class TrkrPlan:
    coordinate: str
    scan_points: list[float]
    target_points: list[float]
    t_zero_ps: float
    summary: str


@dataclass(frozen=True)
class SrkrPlan:
    axis: str
    coordinate: str
    scan_points: list[float]
    target_points: list[float]
    zero: dict[str, float]
    summary: str


@dataclass(frozen=True)
class Scan2DPlan:
    measurement: str
    fast_axis: str
    slow_axis: str
    ranges: dict[str, dict[str, float]]
    fast_target_points: list[float]
    slow_target_points: list[float]
    zero: dict[str, float]
    return_to_zero: dict[str, bool]
    summary: str

    @property
    def total_points(self) -> int:
        return self.fast_point_count * self.slow_point_count

    @property
    def fast_point_count(self) -> int:
        return len(self.fast_target_points)

    @property
    def slow_point_count(self) -> int:
        return len(self.slow_target_points)

    @property
    def slow_line_count(self) -> int:
        return self.slow_point_count


@dataclass(frozen=True)
class StrkrPlan(Scan2DPlan):
    pass


@dataclass(frozen=True)
class Srkr2DPlan(Scan2DPlan):
    pass


def signal_monitor_plan(*, interval_s: float, n_points: int) -> SignalMonitorPlan:
    interval_s = float(interval_s)
    raw_n_points = n_points
    n_points = int(raw_n_points)
    if not math.isfinite(interval_s) or interval_s < 0:
        raise ValueError("Signal Monitor interval_s must be finite and non-negative.")
    if isinstance(raw_n_points, bool) or float(raw_n_points) != n_points:
        raise ValueError("Signal Monitor n_points must be an integer.")
    if n_points <= 0 or n_points > MAX_SCAN_POINTS_TOTAL:
        raise ValueError(
            f"Signal Monitor n_points must be between 1 and {MAX_SCAN_POINTS_TOTAL}."
        )
    return SignalMonitorPlan(
        interval_s=interval_s,
        n_points=n_points,
        summary=f"{n_points} points, dt={interval_s:.1f} s",
    )


def signal_monitor_plan_from_config(
    config: dict[str, Any],
    *,
    interval_s: float | None = None,
    n_points: int | None = None,
) -> SignalMonitorPlan:
    settings = measurement_settings(config, "signal_monitor")
    return signal_monitor_plan(
        interval_s=float(
            interval_s if interval_s is not None else settings.get("interval_s", 1.0)
        ),
        n_points=n_points if n_points is not None else settings.get("n_points", 360),
    )


def trkr_plan(
    *,
    minimum_ps: float,
    maximum_ps: float,
    step_ps: float,
    t_zero_ps: float,
    coordinate: str,
) -> TrkrPlan:
    coordinate = normalize_coordinate(coordinate)
    if coordinate not in {"measurement", "interface", "instrument"}:
        raise ValueError(
            "TRKR coordinate must be measurement, interface, or instrument."
        )
    if not math.isfinite(float(t_zero_ps)):
        raise ValueError("TRKR t_zero_ps must be finite.")
    target_points = build_range_points(
        float(minimum_ps), float(maximum_ps), float(step_ps)
    )
    scan_points = (
        [float(t_zero_ps) + point for point in target_points]
        if coordinate == "measurement"
        else target_points
    )
    return TrkrPlan(
        coordinate=coordinate,
        scan_points=scan_points,
        target_points=target_points,
        t_zero_ps=float(t_zero_ps),
        summary=f"{len(scan_points)} points ({coordinate})",
    )


def trkr_plan_from_config(
    config: dict[str, Any],
    *,
    minimum_ps: float | None = None,
    maximum_ps: float | None = None,
    step_ps: float | None = None,
    t_zero_ps: float | None = None,
    coordinate: str | None = None,
) -> TrkrPlan:
    settings = measurement_settings(config, "trkr")
    scan = scan_settings(config, "trkr")
    zero = move_abs_zero(config)
    return trkr_plan(
        minimum_ps=float(
            minimum_ps if minimum_ps is not None else scan.get("min", -50.0)
        ),
        maximum_ps=float(
            maximum_ps if maximum_ps is not None else scan.get("max", 300.0)
        ),
        step_ps=float(step_ps if step_ps is not None else scan.get("step", 5.0)),
        t_zero_ps=float(t_zero_ps if t_zero_ps is not None else zero.get("t_ps", 0.0)),
        coordinate=str(
            coordinate
            if coordinate is not None
            else settings.get("coordinate", "measurement")
        ),
    )


def srkr_plan(
    *,
    axis: str,
    minimum_um: float,
    maximum_um: float,
    step_um: float,
    zero_by_axis: dict[str, float],
    coordinate: str,
) -> SrkrPlan:
    axis = axis.strip().lower()
    if axis not in {"x", "y"}:
        raise ValueError("SRKR axis must be 'x' or 'y'.")
    coordinate = normalize_scanner_coordinate(coordinate)
    if coordinate not in {"measurement", "interface"}:
        raise ValueError("SRKR coordinate must be measurement or interface.")
    zero = {
        "x": float(zero_by_axis["x"]),
        "y": float(zero_by_axis["y"]),
    }
    if not all(math.isfinite(value) for value in zero.values()):
        raise ValueError("SRKR zero values must be finite.")
    target_points = build_range_points(
        float(minimum_um), float(maximum_um), float(step_um)
    )
    if coordinate == "measurement":
        axis_zero = zero[axis]
        scan_points = [axis_zero + point for point in target_points]
    else:
        scan_points = target_points
    return SrkrPlan(
        axis=axis,
        coordinate=coordinate,
        scan_points=scan_points,
        target_points=target_points,
        zero=zero,
        summary=f"{axis.upper()} {coordinate}, {len(scan_points)} points",
    )


def srkr_plan_from_config(
    config: dict[str, Any],
    *,
    axis: str | None = None,
    minimum_um: float | None = None,
    maximum_um: float | None = None,
    step_um: float | None = None,
    zero_by_axis: dict[str, float] | None = None,
    coordinate: str | None = None,
) -> SrkrPlan:
    settings = measurement_settings(config, "srkr")
    scan = scan_settings(config, "srkr")
    zero = move_abs_zero(config)
    return srkr_plan(
        axis=str(axis if axis is not None else scan.get("axis", "x")),
        minimum_um=float(
            minimum_um if minimum_um is not None else scan.get("min", -30.0)
        ),
        maximum_um=float(
            maximum_um if maximum_um is not None else scan.get("max", 30.0)
        ),
        step_um=float(step_um if step_um is not None else scan.get("step", 1.0)),
        zero_by_axis=zero_by_axis
        or {
            "x": float(zero.get("x_um", 0.0)),
            "y": float(zero.get("y_um", 0.0)),
        },
        coordinate=str(
            coordinate
            if coordinate is not None
            else settings.get("coordinate", "measurement")
        ),
    )


def _axis_range_from_config(
    ranges: dict[str, Any],
    axis: str,
    *,
    default_min: float,
    default_max: float,
    default_step: float,
) -> dict[str, float]:
    data = ranges.get(axis, {})
    if not isinstance(data, dict):
        data = {}
    return {
        "min": float(data.get("min", default_min)),
        "max": float(data.get("max", default_max)),
        "step": float(data.get("step", default_step)),
    }


def _normalize_axis(axis: str) -> str:
    normalized = axis.strip().lower()
    if normalized not in AXES:
        raise ValueError("axis must be one of 't', 'x', or 'y'.")
    return normalized


def _normalize_2d_axes(fast_axis: str, slow_axis: str) -> tuple[str, str]:
    fast = _normalize_axis(fast_axis)
    slow = _normalize_axis(slow_axis)
    if fast == slow:
        raise ValueError("fast_axis and slow_axis must be different.")
    return fast, slow


def _normalize_return_to_zero(value: Any, *, default: bool = True) -> dict[str, bool]:
    if isinstance(value, bool):
        return {"fast_axis": value, "slow_axis": value}
    if isinstance(value, dict):
        return {
            "fast_axis": bool(value.get("fast_axis", default)),
            "slow_axis": bool(value.get("slow_axis", default)),
        }
    return {"fast_axis": default, "slow_axis": default}


def _zero_from_config(
    config: dict[str, Any], zero_by_axis: dict[str, float] | None = None
) -> dict[str, float]:
    zero = move_abs_zero(config) if zero_by_axis is None else zero_by_axis
    return {
        "t_ps": float(zero.get("t_ps", 0.0)),
        "x_um": float(zero.get("x_um", 0.0)),
        "y_um": float(zero.get("y_um", 0.0)),
    }


def _target_points(ranges: dict[str, dict[str, float]], axis: str) -> list[float]:
    axis_range = ranges[axis]
    return build_range_points(
        float(axis_range["min"]), float(axis_range["max"]), float(axis_range["step"])
    )


def _strkr_ranges(ranges: dict[str, Any]) -> dict[str, dict[str, float]]:
    return {
        "t": _axis_range_from_config(
            ranges, "t", default_min=-50.0, default_max=300.0, default_step=5.0
        ),
        "x": _axis_range_from_config(
            ranges, "x", default_min=-30.0, default_max=30.0, default_step=1.0
        ),
        "y": _axis_range_from_config(
            ranges, "y", default_min=-30.0, default_max=30.0, default_step=1.0
        ),
    }


def _srkr_2d_ranges(ranges: dict[str, Any]) -> dict[str, dict[str, float]]:
    return {
        "x": _axis_range_from_config(
            ranges, "x", default_min=-30.0, default_max=30.0, default_step=1.0
        ),
        "y": _axis_range_from_config(
            ranges, "y", default_min=-30.0, default_max=30.0, default_step=1.0
        ),
    }


def strkr_plan(
    *,
    fast_axis: str,
    slow_axis: str,
    ranges: dict[str, Any],
    zero_by_axis: dict[str, float] | None = None,
    return_to_zero: Any = None,
) -> StrkrPlan:
    fast, slow = _normalize_2d_axes(fast_axis, slow_axis)
    if "t" not in {fast, slow} or not ({fast, slow} & SPATIAL_AXES):
        raise ValueError("STRKR axes must combine t with x or y.")
    scan_ranges = _strkr_ranges(ranges)
    fast_points = _target_points(scan_ranges, fast)
    slow_points = _target_points(scan_ranges, slow)
    if len(fast_points) * len(slow_points) > MAX_SCAN_POINTS_TOTAL:
        raise ValueError(
            f"STRKR scan exceeds the maximum of {MAX_SCAN_POINTS_TOTAL} total points."
        )
    return StrkrPlan(
        measurement="strkr",
        fast_axis=fast,
        slow_axis=slow,
        ranges=scan_ranges,
        fast_target_points=fast_points,
        slow_target_points=slow_points,
        zero=_zero_from_config({}, zero_by_axis),
        return_to_zero=_normalize_return_to_zero(return_to_zero, default=True),
        summary=f"STRKR {fast.upper()} fast / {slow.upper()} slow, {len(fast_points) * len(slow_points)} points",
    )


def strkr_plan_from_config(
    config: dict[str, Any],
    *,
    fast_axis: str | None = None,
    slow_axis: str | None = None,
    ranges: dict[str, Any] | None = None,
    zero_by_axis: dict[str, float] | None = None,
    return_to_zero: Any = None,
) -> StrkrPlan:
    settings = measurement_settings(config, "strkr")
    scan = scan_settings(config, "strkr")
    scan_ranges = ranges if ranges is not None else scan.get("ranges", {})
    return strkr_plan(
        fast_axis=str(
            fast_axis if fast_axis is not None else scan.get("fast_axis", "t")
        ),
        slow_axis=str(
            slow_axis if slow_axis is not None else scan.get("slow_axis", "x")
        ),
        ranges=scan_ranges if isinstance(scan_ranges, dict) else {},
        zero_by_axis=zero_by_axis or _zero_from_config(config),
        return_to_zero=return_to_zero
        if return_to_zero is not None
        else settings.get("return_to_zero"),
    )


def srkr_2d_plan(
    *,
    fast_axis: str,
    slow_axis: str,
    ranges: dict[str, Any],
    zero_by_axis: dict[str, float] | None = None,
    return_to_zero: Any = None,
) -> Srkr2DPlan:
    fast, slow = _normalize_2d_axes(fast_axis, slow_axis)
    if {fast, slow} != SPATIAL_AXES:
        raise ValueError("SRKR_2D axes must be x and y.")
    scan_ranges = _srkr_2d_ranges(ranges)
    fast_points = _target_points(scan_ranges, fast)
    slow_points = _target_points(scan_ranges, slow)
    if len(fast_points) * len(slow_points) > MAX_SCAN_POINTS_TOTAL:
        raise ValueError(
            f"SRKR 2D scan exceeds the maximum of {MAX_SCAN_POINTS_TOTAL} total points."
        )
    return Srkr2DPlan(
        measurement="srkr_2d",
        fast_axis=fast,
        slow_axis=slow,
        ranges=scan_ranges,
        fast_target_points=fast_points,
        slow_target_points=slow_points,
        zero=_zero_from_config({}, zero_by_axis),
        return_to_zero=_normalize_return_to_zero(return_to_zero, default=True),
        summary=f"SRKR 2D {fast.upper()} fast / {slow.upper()} slow, {len(fast_points) * len(slow_points)} points",
    )


def srkr_2d_plan_from_config(
    config: dict[str, Any],
    *,
    fast_axis: str | None = None,
    slow_axis: str | None = None,
    ranges: dict[str, Any] | None = None,
    zero_by_axis: dict[str, float] | None = None,
    return_to_zero: Any = None,
) -> Srkr2DPlan:
    settings = measurement_settings(config, "srkr_2d")
    scan = scan_settings(config, "srkr_2d")
    scan_ranges = ranges if ranges is not None else scan.get("ranges", {})
    return srkr_2d_plan(
        fast_axis=str(
            fast_axis if fast_axis is not None else scan.get("fast_axis", "x")
        ),
        slow_axis=str(
            slow_axis if slow_axis is not None else scan.get("slow_axis", "y")
        ),
        ranges=scan_ranges if isinstance(scan_ranges, dict) else {},
        zero_by_axis=zero_by_axis or _zero_from_config(config),
        return_to_zero=return_to_zero
        if return_to_zero is not None
        else settings.get("return_to_zero"),
    )
