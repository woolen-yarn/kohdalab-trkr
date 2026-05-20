from __future__ import annotations


def scanner_origin_pos(config: dict) -> float:
    min_pos = config.get("min_pos")
    max_pos = config.get("max_pos")
    if min_pos is not None and max_pos is not None:
        return (float(min_pos) + float(max_pos)) * 0.5
    configured = config.get("origin_pos")
    if configured is not None:
        return float(configured)
    return 0.0


def actuator_pos_to_sample_um(config: dict, pos_unit: str, actuator_pos: float) -> float:
    unit = pos_unit.strip().lower().replace("/", "_")
    scale = float(config.get("sample_um_per_unit", config.get(f"sample_um_per_actuator_{unit}", 1.0)))
    return (float(actuator_pos) - scanner_origin_pos(config)) * scale


def sample_um_to_actuator_pos(config: dict, pos_unit: str, sample_um: float) -> float:
    unit = pos_unit.strip().lower().replace("/", "_")
    scale = float(config.get("sample_um_per_unit", config.get(f"sample_um_per_actuator_{unit}", 1.0)))
    if scale == 0:
        raise ValueError("sample_um_per_unit scale must be non-zero.")
    return scanner_origin_pos(config) + float(sample_um) / scale
