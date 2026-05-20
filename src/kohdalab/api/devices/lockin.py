from __future__ import annotations

from typing import Any

from kohdalab.interfaces import connect_lockin as _connect_lockin
from kohdalab.interfaces import disconnect_lockin as _disconnect_lockin


def connect_lockin(config: dict[str, Any]):
    return _connect_lockin(config)


def disconnect_lockin(config: dict[str, Any] | None = None) -> None:
    _disconnect_lockin(config)


def read_lockin_signal(config: dict[str, Any] | None = None, *, lockin=None) -> dict[str, Any]:
    instrument = lockin or _connect_lockin(config or {})
    return dict(instrument.get_live_data_raw())


def read_lockin_settings(config: dict[str, Any] | None = None, *, lockin=None) -> dict[str, Any]:
    instrument = lockin or _connect_lockin(config or {})
    return {
        "Sensitivity": instrument.get_sensitivity(),
        "Time Constant": instrument.get_time_constant(),
        "Ref. Freq": instrument.get_ref_freq(),
    }


def set_lockin_settings(
    config: dict[str, Any] | None = None,
    *,
    lockin=None,
    sensitivity: float | None = None,
    time_constant: float | None = None,
    ac_gain: float | None = None,
    coupling: str | None = None,
    slope: int | None = None,
) -> dict[str, Any]:
    instrument = lockin or _connect_lockin(config or {})
    applied: dict[str, Any] = {}
    if sensitivity is not None:
        instrument.set_sensitivity(float(sensitivity))
        applied["Sensitivity"] = instrument.get_sensitivity()
    if time_constant is not None:
        instrument.set_time_constant(float(time_constant))
        applied["Time Constant"] = instrument.get_time_constant()
    if ac_gain is not None:
        instrument.set_ac_gain(float(ac_gain))
        applied["AC Gain"] = instrument.get_ac_gain()
    if coupling is not None:
        instrument.set_coupling(str(coupling))
        applied["Coupling"] = instrument.get_coupling()
    if slope is not None:
        instrument.set_slope(int(slope))
        applied["Slope"] = instrument.get_slope()
    return applied


def read_lockin_overload(config: dict[str, Any] | None = None, *, lockin=None) -> dict[str, Any]:
    instrument = lockin or _connect_lockin(config or {})
    return dict(instrument.get_overload_status())


def get_lockin_wait_time(
    config: dict[str, Any] | None = None,
    *,
    lockin=None,
    multiplier: float = 4.0,
) -> float:
    instrument = lockin or _connect_lockin(config or {})
    return float(instrument.get_wait_time(multiplier=multiplier))
