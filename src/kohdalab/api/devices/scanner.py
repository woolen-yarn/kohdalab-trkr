from __future__ import annotations

from collections.abc import Callable
from datetime import datetime
from typing import Any

from kohdalab.api.conversion import actuator_pos_to_sample_um, sample_um_to_actuator_pos
from kohdalab.api.scan_plan import normalize_scanner_coordinate
from kohdalab.interfaces import connect_scanner as _connect_scanner
from kohdalab.interfaces import disconnect_scanner as _disconnect_scanner
from kohdalab.interfaces.scanner import ACTUATOR_NAMES, ACTUATORS


def connect_scanner(config: dict[str, Any]):
    return _connect_scanner(config)


def disconnect_scanner(config: dict[str, Any] | None = None) -> None:
    _disconnect_scanner(config)


def actuator_pos_unit(actuator: str | None) -> str:
    if not actuator:
        return "mm"
    settings = ACTUATORS.get(actuator.upper().replace("-", ""), {})
    return str(settings.get("pos_unit", "mm"))


def list_actuators(controller: str | None = None) -> list[str]:
    if controller is None:
        return sorted(ACTUATOR_NAMES)
    controller_name = controller.strip().upper()
    names: list[str] = []
    for name in ACTUATOR_NAMES:
        settings = ACTUATORS.get(name.upper().replace("-", ""), {})
        controllers = settings.get("controllers")
        if not controllers or controller_name in {str(item).upper() for item in controllers}:
            names.append(name)
    return sorted(names)


def _control_pos(scanner) -> tuple[str, float]:
    unit = scanner.get_pos_unit().strip().lower()
    if unit == "mm":
        return unit, float(scanner.get_pos_mm())
    if unit == "deg":
        return unit, float(scanner.get_pos_deg())
    raise ValueError(f"Unsupported scanner control unit: {scanner.get_pos_unit()}")


def read_scanner(
    axis: str,
    config: dict[str, Any] | None = None,
    *,
    scanner=None,
    zero_um: float | None = None,
) -> dict[str, Any]:
    axis = axis.strip().lower()
    if axis not in {"x", "y"}:
        raise ValueError("scanner axis must be 'x' or 'y'.")
    scanner = scanner or _connect_scanner(config or {})
    unit, control = _control_pos(scanner)
    sample_um = float(actuator_pos_to_sample_um(scanner.config, unit, control))
    row: dict[str, Any] = {
        "axis": axis,
        f"{axis}_um": sample_um,
        f"{axis}_{unit}": control,
        "unit": "um",
    }
    if zero_um is not None:
        row["zero_um"] = float(zero_um)
        row[f"{axis}_cor_um"] = sample_um - float(zero_um)
    return row


def _scanner_progress_callback(
    *,
    scanner,
    axis: str,
    unit: str,
    coordinate: str,
    target: float,
    on_position: Callable[[dict[str, Any]], None] | None,
) -> Callable[[float], None] | None:
    if on_position is None:
        return None

    def emit(control: float) -> None:
        sample_um = float(actuator_pos_to_sample_um(scanner.config, unit, float(control)))
        on_position(
            {
                "timestamp": datetime.now().isoformat(timespec="milliseconds"),
                "axis": axis,
                "coordinate": coordinate,
                "target": target,
                "actual": sample_um,
                "unit": "um",
                f"{axis}_um": sample_um,
                f"{axis}_{unit}": float(control),
            }
        )

    return emit


def _move_control(
    scanner,
    value: float,
    *,
    on_position: Callable[[float], None] | None = None,
) -> None:
    unit = scanner.get_pos_unit().strip().lower()
    if unit == "mm":
        if on_position is None:
            scanner.move_pos_mm(float(value))
        else:
            scanner.move_pos_mm(float(value), on_position=on_position)
    elif unit == "deg":
        if on_position is None:
            scanner.move_pos_deg(float(value))
        else:
            scanner.move_pos_deg(float(value), on_position=on_position)
    else:
        raise ValueError(f"Unsupported scanner control unit: {scanner.get_pos_unit()}")


def _software_hysteresis_settings(config: dict[str, Any]) -> dict[str, Any]:
    settings = config.get("software_hysteresis", {})
    if not isinstance(settings, dict):
        return {"enabled": False}
    return settings


def _software_hysteresis_enabled(config: dict[str, Any]) -> bool:
    controller = str(config.get("controller", config.get("scanner_controller", ""))).strip().upper()
    if controller != "CONEXCC":
        return False
    settings = _software_hysteresis_settings(config)
    return bool(settings.get("enabled", False))


def _software_hysteresis_distance_um(config: dict[str, Any]) -> float:
    settings = _software_hysteresis_settings(config)
    for key in ("distance_um", "approach_distance_um", "pre_move_um"):
        if settings.get(key) is not None:
            return abs(float(settings[key]))
    return 0.0


def _software_hysteresis_direction(config: dict[str, Any]) -> str:
    settings = _software_hysteresis_settings(config)
    direction = str(settings.get("direction", settings.get("approach", "negative"))).strip().lower()
    aliases = {
        "negative": "negative",
        "negative_to_target": "negative",
        "minus": "negative",
        "-": "negative",
        "positive": "positive",
        "positive_to_target": "positive",
        "plus": "positive",
        "+": "positive",
    }
    if direction not in aliases:
        raise ValueError("software_hysteresis.direction must be 'negative' or 'positive'.")
    return aliases[direction]


def _software_hysteresis_pre_target(
    *,
    scanner_config: dict[str, Any],
    unit: str,
    control_target: float,
) -> float | None:
    if not _software_hysteresis_enabled(scanner_config):
        return None
    distance_um = _software_hysteresis_distance_um(scanner_config)
    if distance_um <= 0:
        return None

    target_sample_um = actuator_pos_to_sample_um(scanner_config, unit, control_target)
    sign = -1.0 if _software_hysteresis_direction(scanner_config) == "negative" else 1.0
    pre_sample_um = target_sample_um + sign * distance_um
    return sample_um_to_actuator_pos(scanner_config, unit, pre_sample_um)


def _move_control_with_software_hysteresis(
    scanner,
    control_target: float,
    *,
    axis: str,
    unit: str,
    coordinate: str,
    target: float,
    apply_software_hysteresis: bool = True,
    on_status: Callable[[str], None] | None = None,
    on_position: Callable[[dict[str, Any]], None] | None = None,
) -> None:
    pre_target = None
    if apply_software_hysteresis:
        pre_target = _software_hysteresis_pre_target(
            scanner_config=scanner.config,
            unit=unit,
            control_target=float(control_target),
    )
    if pre_target is not None:
        if on_status is not None:
            on_status(f"moving scanner {axis} software hysteresis")
        _move_control(
            scanner,
            pre_target,
            on_position=_scanner_progress_callback(
                scanner=scanner,
                axis=axis,
                unit=unit,
                coordinate=coordinate,
                target=actuator_pos_to_sample_um(scanner.config, unit, pre_target)
                if coordinate == "measurement"
                else pre_target,
                on_position=on_position,
            ),
        )
    if on_status is not None:
        on_status(f"moving scanner {axis}")
    _move_control(
        scanner,
        control_target,
        on_position=_scanner_progress_callback(
            scanner=scanner,
            axis=axis,
            unit=unit,
            coordinate=coordinate,
            target=target,
            on_position=on_position,
        ),
    )


def move_scanner_abs(
    *,
    scanner_config: dict[str, Any],
    axis: str,
    coordinate: str,
    value: float,
    scanner=None,
    apply_software_hysteresis: bool = True,
    on_status: Callable[[str], None] | None = None,
    on_position: Callable[[dict[str, Any]], None] | None = None,
) -> dict[str, Any]:
    axis = axis.strip().lower()
    if axis not in {"x", "y"}:
        raise ValueError("scanner axis must be 'x' or 'y'.")
    coordinate = coordinate.strip().lower()
    scanner = scanner or connect_scanner(scanner_config)
    unit, _ = _control_pos(scanner)
    coordinate = _normalize_move_coordinate(coordinate, unit)
    if coordinate == "measurement":
        control_target = sample_um_to_actuator_pos(scanner.config, unit, float(value))
        _move_control_with_software_hysteresis(
            scanner,
            control_target,
            axis=axis,
            unit=unit,
            coordinate=coordinate,
            target=value,
            apply_software_hysteresis=apply_software_hysteresis,
            on_status=on_status,
            on_position=on_position,
        )
    elif coordinate == "interface":
        _move_control_with_software_hysteresis(
            scanner,
            float(value),
            axis=axis,
            unit=unit,
            coordinate=coordinate,
            target=value,
            apply_software_hysteresis=apply_software_hysteresis,
            on_status=on_status,
            on_position=on_position,
        )
    else:
        raise ValueError(
            "scanner coordinate must be measurement or interface "
            "(instrument/device are accepted compatibility aliases)."
        )
    _, control = _control_pos(scanner)
    sample_um = float(actuator_pos_to_sample_um(scanner.config, unit, control))
    return {
        "timestamp": datetime.now().isoformat(timespec="milliseconds"),
        "axis": axis,
        "coordinate": coordinate,
        "target": value,
        "actual": sample_um,
        "unit": "um",
        f"{axis}_um": sample_um,
        f"{axis}_{unit}": control,
    }


def _normalize_move_coordinate(coordinate: str, unit: str) -> str:
    if coordinate in {"um", "sample_um"}:
        return "measurement"
    if coordinate in {unit, f"pos_{unit}"}:
        return "interface"
    return normalize_scanner_coordinate(coordinate)


def initialize_scanner(
    axis: str,
    config: dict[str, Any],
    *,
    scanner=None,
    home: bool = True,
    move_to_origin: bool = True,
    on_status=None,
) -> dict[str, Any]:
    axis = axis.strip().lower()
    if axis not in {"x", "y"}:
        raise ValueError("scanner axis must be 'x' or 'y'.")
    emit = on_status or (lambda _status: None)
    emit(f"{axis} scanner initializing")
    scanner = scanner or _connect_scanner(config)
    info = dict(scanner.initialize(home=home))
    if move_to_origin:
        emit(f"{axis} scanner moving to origin")
        unit = scanner.get_pos_unit().strip().lower()
        if unit == "mm":
            scanner.move_pos_mm(scanner.origin_pos)
        elif unit == "deg":
            scanner.move_pos_deg(scanner.origin_pos)
        else:
            raise ValueError(f"Unsupported scanner control unit: {scanner.get_pos_unit()}")
        control_unit, control = _control_pos(scanner)
        info[f"pos_{control_unit}"] = control
        info["pos_um"] = float(actuator_pos_to_sample_um(scanner.config, control_unit, control))
        info["state"] = scanner.get_state()
        info["moving"] = scanner.is_moving()
    return info
