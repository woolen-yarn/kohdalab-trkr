from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


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

    @classmethod
    def from_rows(cls, *rows: dict[str, Any] | None) -> "Position":
        position = cls()
        for row in rows:
            if not row:
                continue
            if row.get("t_ps") is not None:
                position.t_ps = float(row["t_ps"])
            if row.get("stage_mm") is not None:
                position.delay_stage_mm = float(row["stage_mm"])
            if row.get("delay_stage_mm") is not None:
                position.delay_stage_mm = float(row["delay_stage_mm"])
            if row.get("stage_pulse") is not None:
                position.delay_stage_pulse = int(row["stage_pulse"])
            if row.get("delay_stage_pulse") is not None:
                position.delay_stage_pulse = int(row["delay_stage_pulse"])
            for axis in ("x", "y"):
                if row.get(f"{axis}_um") is not None:
                    setattr(position, f"{axis}_um", float(row[f"{axis}_um"]))
                for unit in ("mm", "deg"):
                    key = f"{axis}_{unit}"
                    scanner_key = f"{axis}_scanner_{unit}"
                    value = row.get(scanner_key, row.get(key))
                    if value is not None:
                        setattr(position, f"scanner_{axis}_value", float(value))
                        setattr(position, f"scanner_{axis}_unit", unit)
        return position


@dataclass
class LiveStatus:
    connected: dict[str, bool] = field(default_factory=dict)
    position: Position = field(default_factory=Position)
    signal: dict[str, Any] | None = None
    lockin_settings: dict[str, Any] | None = None
    lockin_overload: dict[str, Any] | None = None


@dataclass
class LockinSignal:
    x_v: float
    y_v: float
    r_v: float
    theta_deg: float

    @classmethod
    def from_mapping(cls, data: dict[str, Any]) -> "LockinSignal":
        return cls(
            x_v=float(data["X"]),
            y_v=float(data["Y"]),
            r_v=float(data["R"]),
            theta_deg=float(data["Theta"]),
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

    @classmethod
    def from_mapping(cls, data: dict[str, Any]) -> "LockinSettings":
        def maybe_float(key: str) -> float | None:
            value = data.get(key)
            return None if value is None else float(value)

        return cls(
            sensitivity_v=float(data["Sensitivity"]),
            time_constant_s=float(data["Time Constant"]),
            ref_freq_hz=maybe_float("Ref. Freq"),
        )


@dataclass
class MeasurementPoint:
    index: int
    total_points: int
    row: dict[str, Any]
