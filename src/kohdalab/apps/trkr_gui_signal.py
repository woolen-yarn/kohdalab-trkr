from __future__ import annotations

from dataclasses import dataclass
from typing import Any


def voltage_scale_from_sensitivity(sensitivity_v: float) -> tuple[float, str]:
    if sensitivity_v < 1e-6:
        return 1e9, "nV"
    if sensitivity_v < 1e-3:
        return 1e6, "uV"
    if sensitivity_v < 1:
        return 1e3, "mV"
    return 1.0, "V"


def time_constant_display(value_s: float) -> str:
    value = float(value_s)
    if value < 1.0:
        return f"{value * 1e3:.3g} ms"
    return f"{value:.3g} s"


@dataclass(frozen=True)
class SignalViewConfig:
    signal1_key: str
    signal2_key: str
    title1: str
    title2: str
    unit1: str
    unit2: str


def signal_view_config(mode: str, voltage_unit: str) -> SignalViewConfig:
    if mode == "R / Theta":
        return SignalViewConfig("R_V", "Theta_deg", "R", "Theta", voltage_unit, "deg")
    return SignalViewConfig("X_V", "Y_V", "X", "Y", voltage_unit, voltage_unit)


@dataclass(frozen=True)
class LockinDisplay:
    sensitivity: str
    time_constant: str
    ref_freq: str
    voltage_scale: float
    voltage_unit: str
    x_title: str
    y_title: str
    r_title: str
    theta_title: str


def lockin_display_from_settings(settings: dict[str, Any]) -> LockinDisplay:
    sensitivity = float(settings["Sensitivity"])
    time_constant = settings.get("Time Constant")
    ref_freq = settings.get("Ref. Freq", settings.get("Frequency"))
    voltage_scale, voltage_unit = voltage_scale_from_sensitivity(sensitivity)
    sensitivity_value = sensitivity * voltage_scale
    return LockinDisplay(
        sensitivity=f"{sensitivity_value:.3g} {voltage_unit}",
        time_constant="-" if time_constant is None else time_constant_display(float(time_constant)),
        ref_freq="-" if ref_freq is None else f"{float(ref_freq):.6g} Hz",
        voltage_scale=voltage_scale,
        voltage_unit=voltage_unit,
        x_title=f"X ({voltage_unit})",
        y_title=f"Y ({voltage_unit})",
        r_title=f"R ({voltage_unit})",
        theta_title="Theta (deg)",
    )


def overload_display_from_status(status: dict[str, Any] | None) -> str:
    if not status:
        return "-"
    overloaded = bool(status.get("input_overload", status.get("overload", False)))
    return "OVERLOAD" if overloaded else "-"
