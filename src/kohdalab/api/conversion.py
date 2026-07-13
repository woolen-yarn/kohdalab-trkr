from __future__ import annotations

import math
from collections.abc import Mapping
from typing import Any


def _finite_float(value: Any, name: str) -> float:
    number = float(value)
    if isinstance(value, bool) or not math.isfinite(number):
        raise ValueError(f"{name} must be finite.")
    return number


def scanner_origin_pos(config: Mapping[str, Any]) -> float:
    configured = config.get("origin_pos")
    if configured is not None:
        return _finite_float(configured, "scanner origin_pos")
    min_pos = config.get("min_pos")
    max_pos = config.get("max_pos")
    if min_pos is not None and max_pos is not None:
        return (
            _finite_float(min_pos, "scanner min_pos")
            + _finite_float(max_pos, "scanner max_pos")
        ) * 0.5
    return 0.0


def actuator_pos_to_sample_um(
    config: Mapping[str, Any], pos_unit: str, actuator_pos: float
) -> float:
    unit = pos_unit.strip().lower().replace("/", "_")
    scale = _finite_float(
        config.get(
            "sample_um_per_unit", config.get(f"sample_um_per_actuator_{unit}", 1.0)
        ),
        "sample_um_per_unit scale",
    )
    position = _finite_float(actuator_pos, "scanner actuator position")
    return (position - scanner_origin_pos(config)) * scale


def sample_um_to_actuator_pos(
    config: Mapping[str, Any], pos_unit: str, sample_um: float
) -> float:
    unit = pos_unit.strip().lower().replace("/", "_")
    scale = _finite_float(
        config.get(
            "sample_um_per_unit", config.get(f"sample_um_per_actuator_{unit}", 1.0)
        ),
        "sample_um_per_unit scale",
    )
    if scale == 0:
        raise ValueError("sample_um_per_unit scale must be non-zero.")
    return (
        scanner_origin_pos(config)
        + _finite_float(sample_um, "scanner sample position") / scale
    )
