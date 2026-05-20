from __future__ import annotations

from dataclasses import dataclass

from kohdalab.api.config import normalize_delay_stage_name
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
    spec = dict(STAGES.get(normalize_delay_stage_name(stage), {}))
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
    lower_ps = sign * 2.0 * (min_mm - zero_mm) / LIGHT_SPEED_MM_PER_PS - float(t_zero_ps)
    upper_ps = sign * 2.0 * (max_mm - zero_mm) / LIGHT_SPEED_MM_PER_PS - float(t_zero_ps)
    low, high = _sorted_limits(lower_ps, upper_ps)

    min_step = None
    if spec.get("pos_um_per_pulse") is not None:
        min_step = abs(2.0 * (float(spec["pos_um_per_pulse"]) / 1000.0) / LIGHT_SPEED_MM_PER_PS)
    elif (
        microstep_division is not None
        and int(microstep_division) > 0
        and spec.get("screw_lead_mm_per_rev") is not None
        and spec.get("step_angle_deg") is not None
    ):
        full_step_mm = float(spec["screw_lead_mm_per_rev"]) * float(spec["step_angle_deg"]) / 360.0
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
    lower_um = actuator_pos_to_sample_um(config, pos_unit, float(min_pos)) - float(zero_um)
    upper_um = actuator_pos_to_sample_um(config, pos_unit, float(max_pos)) - float(zero_um)
    low, high = _sorted_limits(lower_um, upper_um)

    resolution = spec.get("resolution", spec.get("min_step"))
    min_step = None if resolution is None else abs(float(resolution) * float(sample_um_per_unit))
    return ScanLimits(low, high, min_step, "um")
