from __future__ import annotations

from kohdalab.api.devices import actuator_pos_unit
from kohdalab.api.scan_plan import normalize_coordinate, normalize_scanner_coordinate


def delay_stage_unit_for_coordinate(coordinate: str | None) -> str:
    coordinate = normalize_coordinate(coordinate)
    if coordinate == "interface":
        return "mm"
    if coordinate == "instrument":
        return "pulse"
    return "ps"


def scanner_unit_for_coordinate(
    coordinate: str | None,
    *,
    actuator: str | None = None,
    connected_unit: str | None = None,
) -> str:
    coordinate = normalize_scanner_coordinate(coordinate)
    if coordinate == "interface":
        return connected_unit or actuator_pos_unit(actuator)
    return "um"


def scanner_label_for_coordinate(
    axis: str,
    coordinate: str | None,
    *,
    actuator: str | None = None,
    connected_unit: str | None = None,
) -> str:
    axis = axis.strip().lower()
    coordinate = normalize_scanner_coordinate(coordinate)
    unit = scanner_unit_for_coordinate(coordinate, actuator=actuator, connected_unit=connected_unit)
    if coordinate == "measurement":
        return f"{axis} ({unit})"
    return f"scanner_{axis} ({unit})"


def scanner_scale_label_for_actuator(actuator: str | None) -> str:
    return f"sample um / {actuator_pos_unit(actuator)}"


def scanner_axis_spin_value(value: object) -> int:
    normalized = str(value).strip().upper()
    if normalized == "U":
        return 1
    if normalized == "V":
        return 2
    return int(value)


def delay_stage_label_for_coordinate(coordinate: str | None) -> str:
    coordinate = normalize_coordinate(coordinate)
    unit = delay_stage_unit_for_coordinate(coordinate)
    if coordinate == "measurement":
        return f"t ({unit})"
    return f"delay_stage ({unit})"


def coordinate_correction_enabled(coordinate: str | None) -> bool:
    return normalize_coordinate(coordinate) == "measurement"
