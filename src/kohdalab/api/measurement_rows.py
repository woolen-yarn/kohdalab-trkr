from __future__ import annotations

from typing import Any

from kohdalab.api.models import LockinSignal


SIGNAL_VOLTAGE_KEYS = {"X_V", "Y_V", "R_V"}

MEASUREMENT_FIELDS = [
    "timestamp",
    "measurement",
    "fast_axis",
    "slow_axis",
    "target_elapsed_s",
    "target_t_cor_ps",
    "target_x_cor_um",
    "target_y_cor_um",
    "elapsed_s",
    "t_cor_ps",
    "t_ps",
    "x_cor_um",
    "x_um",
    "y_cor_um",
    "y_um",
    "X_V",
    "Y_V",
    "R_V",
    "Theta_deg",
    "coordinate",
    "delay_stage_mm",
    "delay_stage_pulse",
    "x_scanner_mm",
    "x_scanner_deg",
    "y_scanner_mm",
    "y_scanner_deg",
]

SIGNAL_MONITOR_FIELDS = MEASUREMENT_FIELDS
TRKR_FIELDS = MEASUREMENT_FIELDS
STRKR_FIELDS = MEASUREMENT_FIELDS
SRKR_2D_FIELDS = MEASUREMENT_FIELDS
SRKR_FIELDS_BY_AXIS = {"x": MEASUREMENT_FIELDS, "y": MEASUREMENT_FIELDS}


def signal_row(signal: dict[str, Any]) -> dict[str, Any]:
    return LockinSignal.from_mapping(signal).to_row()


def ordered_row(row: dict[str, Any], fields: list[str]) -> dict[str, Any]:
    ordered = {key: row.get(key) for key in fields}
    ordered.update({key: value for key, value in row.items() if key not in ordered})
    return ordered


def fields_for_row(row: dict[str, Any]) -> list[str]:
    measurement = str(row.get("measurement", "")).strip().lower()
    if measurement in {"signal_monitor", "trkr", "srkr", "strkr", "srkr_2d"}:
        return MEASUREMENT_FIELDS
    return list(row.keys())


def fields_for_rows(rows: list[dict[str, Any]]) -> list[str]:
    fields: list[str] = []
    for row in rows:
        for key in fields_for_row(row):
            if key not in fields:
                fields.append(key)
        for key in row:
            if key not in fields:
                fields.append(key)
    return fields


def format_output_value(key: str, value: Any) -> Any:
    if (
        key in SIGNAL_VOLTAGE_KEYS
        and isinstance(value, (int, float))
        and not isinstance(value, bool)
    ):
        return f"{float(value):.6e}"
    return value


def output_row(row: dict[str, Any]) -> dict[str, Any]:
    return {key: format_output_value(key, value) for key, value in row.items()}


def output_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [output_row(row) for row in rows]


def signal_monitor_row(
    *,
    timestamp: str,
    target_elapsed_s: float,
    elapsed_s: float,
    signal: dict[str, Any],
) -> dict[str, Any]:
    return ordered_row(
        {
            "timestamp": timestamp,
            "measurement": "signal_monitor",
            "fast_axis": "elapsed_s",
            "slow_axis": None,
            "target_elapsed_s": target_elapsed_s,
            "elapsed_s": elapsed_s,
            **signal_row(signal),
        },
        MEASUREMENT_FIELDS,
    )


def trkr_row(
    *,
    timestamp: str,
    target_t_cor_ps: float | int,
    t_cor_ps: float | None,
    t_ps: float | None,
    signal: dict[str, Any],
    coordinate: str,
    delay_stage_mm: float | None,
    delay_stage_pulse: int | None,
) -> dict[str, Any]:
    return ordered_row(
        {
            "timestamp": timestamp,
            "measurement": "trkr",
            "fast_axis": "t",
            "slow_axis": None,
            "target_t_cor_ps": target_t_cor_ps,
            "t_cor_ps": t_cor_ps,
            "t_ps": t_ps,
            **signal_row(signal),
            "coordinate": coordinate,
            "delay_stage_mm": delay_stage_mm,
            "delay_stage_pulse": delay_stage_pulse,
        },
        TRKR_FIELDS,
    )


def srkr_row(
    *,
    timestamp: str,
    fast_axis: str,
    target_cor_um: float | int,
    cor_um: float | None,
    position_um: float | None,
    signal: dict[str, Any],
    coordinate: str,
    scanner_unit: str | None,
    scanner_value: float | None,
) -> dict[str, Any]:
    axis = fast_axis.strip().lower()
    unit_key = "mm" if scanner_unit == "mm" else "deg"
    return ordered_row(
        {
            "timestamp": timestamp,
            "measurement": "srkr",
            "fast_axis": axis,
            "slow_axis": None,
            f"target_{axis}_cor_um": target_cor_um,
            f"{axis}_cor_um": cor_um,
            f"{axis}_um": position_um,
            **signal_row(signal),
            "coordinate": coordinate,
            f"{axis}_scanner_{unit_key}": scanner_value,
        },
        SRKR_FIELDS_BY_AXIS[axis],
    )


def axis_target_key(axis: str) -> str:
    axis = axis.strip().lower()
    if axis == "elapsed_s":
        return "target_elapsed_s"
    if axis == "t":
        return "target_t_cor_ps"
    if axis in {"x", "y"}:
        return f"target_{axis}_cor_um"
    raise ValueError(f"Unsupported axis: {axis}")


def _position_fields(position: Any, zero: dict[str, float]) -> dict[str, Any]:
    t_ps = getattr(position, "t_ps", None)
    x_um = getattr(position, "x_um", None)
    y_um = getattr(position, "y_um", None)
    values = {
        "t_ps": t_ps,
        "t_cor_ps": None
        if t_ps is None
        else float(t_ps) - float(zero.get("t_ps", 0.0)),
        "delay_stage_mm": getattr(position, "delay_stage_mm", None),
        "delay_stage_pulse": getattr(position, "delay_stage_pulse", None),
        "x_um": x_um,
        "x_cor_um": None
        if x_um is None
        else float(x_um) - float(zero.get("x_um", 0.0)),
        "y_um": y_um,
        "y_cor_um": None
        if y_um is None
        else float(y_um) - float(zero.get("y_um", 0.0)),
    }
    for axis in ("x", "y"):
        scanner_unit = getattr(position, f"scanner_{axis}_unit", None)
        scanner_value = getattr(position, f"scanner_{axis}_value", None)
        if scanner_unit in {"mm", "deg"}:
            values[f"{axis}_scanner_{scanner_unit}"] = scanner_value
    return values


def scan2d_row(
    *,
    timestamp: str,
    measurement: str,
    fast_axis: str,
    slow_axis: str,
    targets: dict[str, float],
    position: Any,
    zero: dict[str, float],
    signal: dict[str, Any],
    coordinate: str = "measurement",
) -> dict[str, Any]:
    row: dict[str, Any] = {
        "timestamp": timestamp,
        "measurement": measurement,
        "fast_axis": fast_axis.strip().lower(),
        "slow_axis": slow_axis.strip().lower(),
        "coordinate": coordinate,
        **signal_row(signal),
        **_position_fields(position, zero),
    }
    for axis, target in targets.items():
        row[axis_target_key(axis)] = target
    return ordered_row(row, MEASUREMENT_FIELDS)
