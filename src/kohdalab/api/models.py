from __future__ import annotations

import math
from collections.abc import Mapping
from dataclasses import dataclass, field
from numbers import Real
from typing import Any


def _finite_float(value: object, name: str) -> float:
    if isinstance(value, bool):
        raise ValueError(f"{name} must be a finite number, not boolean.")
    try:
        result = float(value)  # type: ignore[arg-type]
    except (TypeError, ValueError) as exc:
        raise ValueError(f"{name} must be a finite number.") from exc
    if not math.isfinite(result):
        raise ValueError(f"{name} must be finite.")
    return result


def _optional_finite_float(value: object | None, name: str) -> float | None:
    return None if value is None else _finite_float(value, name)


def _integer(value: object, name: str) -> int:
    number = _finite_float(value, name)
    if not number.is_integer():
        raise ValueError(f"{name} must be an integer.")
    return int(number)


def _optional_integer(value: object | None, name: str) -> int | None:
    return None if value is None else _integer(value, name)


def _mapping(value: object, name: str) -> Mapping[str, Any]:
    if not isinstance(value, Mapping):
        raise TypeError(f"{name} must be a mapping.")
    if any(not isinstance(key, str) or not key for key in value):
        raise ValueError(f"{name} keys must be non-empty strings.")
    return value


@dataclass
class Position:
    t_ps: float | None = None
    delay_stage_mm: float | None = None
    delay_stage_pulse: int | None = None
    x_um: float | None = None
    y_um: float | None = None
    scanner_x_value: float | None = None
    scanner_x_unit: str | None = None
    scanner_y_value: float | None = None
    scanner_y_unit: str | None = None

    def __post_init__(self) -> None:
        for name in (
            "t_ps",
            "delay_stage_mm",
            "x_um",
            "y_um",
            "scanner_x_value",
            "scanner_y_value",
        ):
            setattr(
                self,
                name,
                _optional_finite_float(getattr(self, name), f"Position.{name}"),
            )
        self.delay_stage_pulse = _optional_integer(
            self.delay_stage_pulse, "Position.delay_stage_pulse"
        )
        for axis in ("x", "y"):
            unit_name = f"scanner_{axis}_unit"
            value_name = f"scanner_{axis}_value"
            unit = getattr(self, unit_name)
            value = getattr(self, value_name)
            if unit is not None:
                if not isinstance(unit, str) or unit.strip().lower() not in {
                    "mm",
                    "deg",
                }:
                    raise ValueError(
                        f"Position.{unit_name} must be 'mm', 'deg', or None."
                    )
                unit = unit.strip().lower()
                setattr(self, unit_name, unit)
            if (unit is None) != (value is None):
                raise ValueError(
                    f"Position.{unit_name} and Position.{value_name} must be supplied together."
                )

    @classmethod
    def from_rows(cls, *rows: dict[str, Any] | None) -> "Position":
        values: dict[str, Any] = {}

        def assign(name: str, raw: object, source: str) -> None:
            parsed: float | int
            if name == "delay_stage_pulse":
                parsed = _integer(raw, source)
            else:
                parsed = _finite_float(raw, source)
            if name in values and values[name] != parsed:
                raise ValueError(
                    f"Conflicting values for Position.{name}: {values[name]!r} and {parsed!r}."
                )
            values[name] = parsed

        for row_index, raw_row in enumerate(rows, start=1):
            if raw_row is None:
                continue
            row = _mapping(raw_row, f"position row {row_index}")
            if not row:
                continue
            for key, field_name in (
                ("t_ps", "t_ps"),
                ("stage_mm", "delay_stage_mm"),
                ("delay_stage_mm", "delay_stage_mm"),
                ("stage_pulse", "delay_stage_pulse"),
                ("delay_stage_pulse", "delay_stage_pulse"),
            ):
                if row.get(key) is not None:
                    assign(field_name, row[key], f"position row {row_index}.{key}")
            for axis in ("x", "y"):
                if row.get(f"{axis}_um") is not None:
                    assign(
                        f"{axis}_um",
                        row[f"{axis}_um"],
                        f"position row {row_index}.{axis}_um",
                    )
                for unit in ("mm", "deg"):
                    key = f"{axis}_{unit}"
                    scanner_key = f"{axis}_scanner_{unit}"
                    candidates = [
                        (candidate, row[candidate])
                        for candidate in (key, scanner_key)
                        if row.get(candidate) is not None
                    ]
                    for candidate, raw_value in candidates:
                        existing_unit = values.get(f"scanner_{axis}_unit")
                        if existing_unit is not None and existing_unit != unit:
                            raise ValueError(
                                f"Conflicting scanner units for Position.{axis}: "
                                f"{existing_unit!r} and {unit!r}."
                            )
                        values[f"scanner_{axis}_unit"] = unit
                        assign(
                            f"scanner_{axis}_value",
                            raw_value,
                            f"position row {row_index}.{candidate}",
                        )
        return cls(**values)


@dataclass
class LiveStatus:
    connected: dict[str, bool] = field(default_factory=dict)
    position: Position = field(default_factory=Position)
    signal: dict[str, Any] | None = None
    lockin_settings: dict[str, Any] | None = None
    lockin_overload: dict[str, Any] | None = None

    def __post_init__(self) -> None:
        connected = _mapping(self.connected, "LiveStatus.connected")
        if any(not isinstance(value, bool) for value in connected.values()):
            raise ValueError("LiveStatus.connected values must be boolean.")
        self.connected = dict(connected)
        if not isinstance(self.position, Position):
            raise TypeError("LiveStatus.position must be a Position.")
        if self.signal is not None:
            signal = _mapping(self.signal, "LiveStatus.signal")
            parsed_signal = LockinSignal.from_mapping(signal)
            self.signal = {
                "X": parsed_signal.x_v,
                "Y": parsed_signal.y_v,
                "R": parsed_signal.r_v,
                "Theta": parsed_signal.theta_deg,
            }
        if self.lockin_settings is not None:
            settings = _mapping(self.lockin_settings, "LiveStatus.lockin_settings")
            parsed_settings = LockinSettings.from_mapping(settings)
            self.lockin_settings = {
                "Sensitivity": parsed_settings.sensitivity_v,
                "Time Constant": parsed_settings.time_constant_s,
            }
            if "Ref. Freq" in settings:
                self.lockin_settings["Ref. Freq"] = parsed_settings.ref_freq_hz
        if self.lockin_overload is not None:
            overload = _mapping(self.lockin_overload, "LiveStatus.lockin_overload")
            if not isinstance(overload.get("overload"), bool):
                raise ValueError("LiveStatus.lockin_overload.overload must be boolean.")
            for key, value in overload.items():
                if key == "overload_byte":
                    if (
                        isinstance(value, bool)
                        or not isinstance(value, int)
                        or value < 0
                    ):
                        raise ValueError(
                            "LiveStatus.lockin_overload.overload_byte must be a "
                            "non-negative integer."
                        )
                elif not isinstance(value, bool):
                    raise ValueError(
                        f"LiveStatus.lockin_overload.{key} must be boolean."
                    )
            self.lockin_overload = dict(overload)


@dataclass
class LockinSignal:
    x_v: float
    y_v: float
    r_v: float
    theta_deg: float

    def __post_init__(self) -> None:
        self.x_v = _finite_float(self.x_v, "LockinSignal.x_v")
        self.y_v = _finite_float(self.y_v, "LockinSignal.y_v")
        self.r_v = _finite_float(self.r_v, "LockinSignal.r_v")
        self.theta_deg = _finite_float(self.theta_deg, "LockinSignal.theta_deg")
        if self.r_v < 0:
            raise ValueError("LockinSignal.r_v must be non-negative.")

    @classmethod
    def from_mapping(cls, data: Mapping[str, Any]) -> "LockinSignal":
        mapping = _mapping(data, "lock-in signal")
        return cls(
            x_v=mapping["X"],
            y_v=mapping["Y"],
            r_v=mapping["R"],
            theta_deg=mapping["Theta"],
        )

    def to_row(self) -> dict[str, float]:
        return {
            "X_V": self.x_v,
            "Y_V": self.y_v,
            "R_V": self.r_v,
            "Theta_deg": self.theta_deg,
        }


@dataclass
class LockinSettings:
    sensitivity_v: float
    time_constant_s: float
    ref_freq_hz: float | None = None

    def __post_init__(self) -> None:
        self.sensitivity_v = _finite_float(
            self.sensitivity_v, "LockinSettings.sensitivity_v"
        )
        self.time_constant_s = _finite_float(
            self.time_constant_s, "LockinSettings.time_constant_s"
        )
        self.ref_freq_hz = _optional_finite_float(
            self.ref_freq_hz, "LockinSettings.ref_freq_hz"
        )
        if self.sensitivity_v <= 0:
            raise ValueError("LockinSettings.sensitivity_v must be positive.")
        if self.time_constant_s <= 0:
            raise ValueError("LockinSettings.time_constant_s must be positive.")
        if self.ref_freq_hz is not None and self.ref_freq_hz < 0:
            raise ValueError("LockinSettings.ref_freq_hz must be non-negative.")

    @classmethod
    def from_mapping(cls, data: Mapping[str, Any]) -> "LockinSettings":
        mapping = _mapping(data, "lock-in settings")

        def maybe_float(key: str) -> float | None:
            value = mapping.get(key)
            return (
                None
                if value is None
                else _finite_float(value, f"lock-in settings.{key}")
            )

        return cls(
            sensitivity_v=mapping["Sensitivity"],
            time_constant_s=mapping["Time Constant"],
            ref_freq_hz=maybe_float("Ref. Freq"),
        )


@dataclass
class MeasurementPoint:
    index: int
    total_points: int
    row: dict[str, Any]

    def __post_init__(self) -> None:
        if (
            isinstance(self.index, bool)
            or not isinstance(self.index, int)
            or isinstance(self.total_points, bool)
            or not isinstance(self.total_points, int)
            or self.total_points < 1
            or not 1 <= self.index <= self.total_points
        ):
            raise ValueError(
                "MeasurementPoint progress must satisfy 1 <= index <= total_points "
                "with integer values."
            )
        row = _mapping(self.row, "MeasurementPoint.row")
        if not row:
            raise ValueError("MeasurementPoint.row must not be empty.")
        for key, value in row.items():
            if (
                isinstance(value, Real)
                and not isinstance(value, bool)
                and not math.isfinite(value)
            ):
                raise ValueError(f"MeasurementPoint.row[{key!r}] must be finite.")
        self.row = dict(row)
