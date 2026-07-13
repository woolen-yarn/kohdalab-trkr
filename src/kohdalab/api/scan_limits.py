from __future__ import annotations

import math
from dataclasses import dataclass
from typing import Any, Iterable

from kohdalab.api.config import (
    MAX_SCAN_POINTS_PER_AXIS,
    delay_stage_config_for,
    normalize_delay_stage_name,
    scanner_config_for,
)
from kohdalab.api.conversion import actuator_pos_to_sample_um
from kohdalab.api.devices.delay_stage import LIGHT_SPEED_MM_PER_PS
from kohdalab.interfaces.delay_stage import STAGES
from kohdalab.interfaces.scanner import ACTUATORS


@dataclass(frozen=True)
class ScanLimits:
    minimum: float | None
    maximum: float | None
    minimum_step: float | None
    unit: str


def _sorted_limits(a: float, b: float) -> tuple[float, float]:
    return (a, b) if a <= b else (b, a)


def delay_stage_scan_limits(
    *,
    stage: str | None,
    direction: int,
    t_zero_ps: float,
    microstep_division: int | None = None,
) -> ScanLimits:
    normalized_stage = normalize_delay_stage_name(stage)
    spec = {} if normalized_stage is None else dict(STAGES.get(normalized_stage, {}))
    if not spec:
        return ScanLimits(None, None, None, "ps")

    min_mm = 0.0
    if spec.get("min_pulse") is not None and spec.get("pos_um_per_pulse") is not None:
        min_mm = int(spec["min_pulse"]) * float(spec["pos_um_per_pulse"]) / 1000.0

    max_mm = spec.get("travel_mm")
    if spec.get("max_pulse") is not None and spec.get("pos_um_per_pulse") is not None:
        max_mm = int(spec["max_pulse"]) * float(spec["pos_um_per_pulse"]) / 1000.0
    if max_mm is None:
        return ScanLimits(None, None, None, "ps")

    max_mm = float(max_mm)
    zero_mm = (min_mm + max_mm) * 0.5
    sign = 1.0 if int(direction) == 0 else -1.0
    lower_ps = sign * 2.0 * (min_mm - zero_mm) / LIGHT_SPEED_MM_PER_PS - float(
        t_zero_ps
    )
    upper_ps = sign * 2.0 * (max_mm - zero_mm) / LIGHT_SPEED_MM_PER_PS - float(
        t_zero_ps
    )
    low, high = _sorted_limits(lower_ps, upper_ps)

    min_step = None
    if spec.get("pos_um_per_pulse") is not None:
        min_step = abs(
            2.0 * (float(spec["pos_um_per_pulse"]) / 1000.0) / LIGHT_SPEED_MM_PER_PS
        )
    elif (
        microstep_division is not None
        and int(microstep_division) > 0
        and spec.get("screw_lead_mm_per_rev") is not None
        and spec.get("step_angle_deg") is not None
    ):
        full_step_mm = (
            float(spec["screw_lead_mm_per_rev"]) * float(spec["step_angle_deg"]) / 360.0
        )
        pulse_mm = full_step_mm / int(microstep_division)
        min_step = abs(2.0 * pulse_mm / LIGHT_SPEED_MM_PER_PS)

    return ScanLimits(low, high, min_step, "ps")


def scanner_scan_limits(
    *,
    actuator: str | None,
    sample_um_per_unit: float,
    zero_um: float,
) -> ScanLimits:
    actuator_key = str(actuator or "").upper().replace("-", "")
    spec = dict(ACTUATORS.get(actuator_key, {}))
    if not spec:
        return ScanLimits(None, None, None, "um")

    min_pos = spec.get("min_pos")
    max_pos = spec.get("max_pos")
    if min_pos is None or max_pos is None:
        return ScanLimits(None, None, None, "um")

    config = dict(spec)
    config["sample_um_per_unit"] = float(sample_um_per_unit)
    pos_unit = str(spec.get("pos_unit", "mm"))
    lower_um = actuator_pos_to_sample_um(config, pos_unit, float(min_pos)) - float(
        zero_um
    )
    upper_um = actuator_pos_to_sample_um(config, pos_unit, float(max_pos)) - float(
        zero_um
    )
    low, high = _sorted_limits(lower_um, upper_um)

    resolution = spec.get("resolution", spec.get("min_step"))
    min_step = (
        None
        if resolution is None
        else abs(float(resolution) * float(sample_um_per_unit))
    )
    return ScanLimits(low, high, min_step, "um")


def _normalized_coordinate(axis: str, coordinate: str) -> str:
    value = coordinate.strip().lower()
    if axis == "t":
        aliases = {
            "t_ps": "measurement",
            "ps": "measurement",
            "pos_mm": "interface",
            "mm": "interface",
            "pulse": "instrument",
            "device": "instrument",
        }
    else:
        aliases = {
            "um": "measurement",
            "sample_um": "measurement",
            "instrument": "interface",
            "device": "interface",
        }
    return aliases.get(value, value)


def _axis_limits(
    config: dict[str, Any],
    *,
    measurement_name: str,
    axis: str,
    coordinate: str,
) -> ScanLimits:
    coordinate = _normalized_coordinate(axis, coordinate)
    if axis == "t":
        stage_config = delay_stage_config_for(config, measurement_name)
        stage_name = normalize_delay_stage_name(stage_config.get("stage"))
        spec = {} if stage_name is None else dict(STAGES.get(stage_name, {}))
        if coordinate == "measurement":
            return delay_stage_scan_limits(
                stage=stage_name,
                direction=int(stage_config.get("direction", 0)),
                t_zero_ps=0.0,
            )
        if coordinate == "interface":
            return ScanLimits(
                0.0,
                float(spec["travel_mm"]) if spec.get("travel_mm") is not None else None,
                None,
                "mm",
            )
        if coordinate == "instrument":
            minimum = spec.get("min_pulse")
            maximum = spec.get("max_pulse")
            return ScanLimits(
                None if minimum is None else float(minimum),
                None if maximum is None else float(maximum),
                1.0,
                "pulse",
            )
        raise ValueError(
            "delay stage coordinate must be measurement, interface, or instrument."
        )

    scanner_config = scanner_config_for(config, axis, measurement_name)
    actuator_key = str(scanner_config.get("actuator", "")).upper().replace("-", "")
    spec = dict(ACTUATORS.get(actuator_key, {}))
    if coordinate == "measurement":
        return scanner_scan_limits(
            actuator=actuator_key,
            sample_um_per_unit=float(scanner_config.get("sample_um_per_unit", 0.0)),
            zero_um=0.0,
        )
    if coordinate == "interface":
        minimum = spec.get("min_pos")
        maximum = spec.get("max_pos")
        resolution = spec.get("resolution", spec.get("min_step"))
        return ScanLimits(
            None if minimum is None else float(minimum),
            None if maximum is None else float(maximum),
            None if resolution is None else abs(float(resolution)),
            str(spec.get("pos_unit", "actuator unit")),
        )
    raise ValueError("scanner coordinate must be measurement or interface.")


def preflight_axis_targets(
    config: dict[str, Any],
    *,
    measurement_name: str,
    axis: str,
    targets: Iterable[float | int],
    coordinate: str = "measurement",
) -> None:
    axis = axis.strip().lower()
    if axis not in {"t", "x", "y"}:
        raise ValueError("axis must be one of 't', 'x', or 'y'.")
    values = [float(value) for value in targets]
    if not values:
        raise ValueError(
            f"{measurement_name} {axis}-axis scan must contain at least one target."
        )
    if len(values) > MAX_SCAN_POINTS_PER_AXIS:
        raise ValueError(
            f"{measurement_name} {axis}-axis scan has {len(values)} targets; "
            f"maximum is {MAX_SCAN_POINTS_PER_AXIS}."
        )
    if not all(math.isfinite(value) for value in values):
        raise ValueError(f"{measurement_name} {axis}-axis targets must be finite.")

    limits = _axis_limits(
        config,
        measurement_name=measurement_name,
        axis=axis,
        coordinate=coordinate,
    )
    if limits.minimum is None or limits.maximum is None:
        raise ValueError(
            f"Cannot preflight {measurement_name} {axis}-axis: configured device has no complete "
            f"{limits.unit} limits."
        )
    tolerance = max(abs(limits.minimum), abs(limits.maximum), 1.0) * 1e-12
    for value in values:
        if value < limits.minimum - tolerance or value > limits.maximum + tolerance:
            raise ValueError(
                f"{measurement_name} {axis}-axis target {value:g} {limits.unit} is outside "
                f"[{limits.minimum:g}, {limits.maximum:g}] {limits.unit}."
            )
    if limits.minimum_step is not None:
        for previous, current in zip(values, values[1:], strict=False):
            distance = abs(current - previous)
            if tolerance < distance < limits.minimum_step - tolerance:
                raise ValueError(
                    f"{measurement_name} {axis}-axis step {distance:g} {limits.unit} is smaller than "
                    f"the device minimum {limits.minimum_step:g} {limits.unit}."
                )
