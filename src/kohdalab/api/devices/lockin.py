from __future__ import annotations

import math
from typing import Any, cast

from kohdalab.interfaces import connect_lockin as _connect_lockin
from kohdalab.interfaces import disconnect_lockin as _disconnect_lockin
from kohdalab.interfaces.lockin import Lockin
from kohdalab.interfaces.protocols import LockinController


def connect_lockin(config: dict[str, Any]) -> Lockin:
    return _connect_lockin(config)


def disconnect_lockin(config: dict[str, Any] | None = None) -> None:
    _disconnect_lockin(config)


def read_lockin_signal(
    config: dict[str, Any] | None = None, *, lockin: LockinController | None = None
) -> dict[str, Any]:
    instrument = lockin or _connect_lockin(config or {})
    return dict(instrument.get_live_data_raw())


def read_lockin_settings(
    config: dict[str, Any] | None = None, *, lockin: LockinController | None = None
) -> dict[str, Any]:
    instrument = lockin or _connect_lockin(config or {})
    return {
        "Sensitivity": instrument.get_sensitivity(),
        "Time Constant": instrument.get_time_constant(),
        "Ref. Freq": instrument.get_ref_freq(),
    }


def _finite_setting(value: object, name: str) -> float:
    if isinstance(value, bool):
        raise ValueError(f"{name} must be a finite number, not boolean.")
    try:
        result = float(cast(Any, value))
    except (TypeError, ValueError) as exc:
        raise ValueError(f"{name} must be a finite number.") from exc
    if not math.isfinite(result):
        raise ValueError(f"{name} must be finite.")
    return result


def _verify_numeric_setting(name: str, requested: float, actual: object) -> float:
    measured = _finite_setting(actual, f"{name} read-back")
    if not math.isclose(measured, requested, rel_tol=1e-9, abs_tol=1e-15):
        raise RuntimeError(
            f"{name} read-back mismatch: requested {requested!r}, received {measured!r}."
        )
    return measured


def set_lockin_settings(
    config: dict[str, Any] | None = None,
    *,
    lockin: LockinController | None = None,
    sensitivity: float | None = None,
    time_constant: float | None = None,
    ac_gain: float | None = None,
    coupling: str | None = None,
    slope: int | None = None,
) -> dict[str, Any]:
    requested_sensitivity = (
        None if sensitivity is None else _finite_setting(sensitivity, "sensitivity")
    )
    requested_time_constant = (
        None
        if time_constant is None
        else _finite_setting(time_constant, "time constant")
    )
    requested_ac_gain = None if ac_gain is None else _finite_setting(ac_gain, "AC gain")
    if coupling is not None and not isinstance(coupling, str):
        raise ValueError("coupling must be a string.")
    requested_coupling = None if coupling is None else coupling.strip().upper()
    if slope is not None and (isinstance(slope, bool) or not isinstance(slope, int)):
        raise ValueError("slope must be an integer.")

    instrument = lockin or _connect_lockin(config or {})
    applied: dict[str, Any] = {}
    if requested_sensitivity is not None:
        instrument.set_sensitivity(requested_sensitivity)
        applied["Sensitivity"] = _verify_numeric_setting(
            "Sensitivity", requested_sensitivity, instrument.get_sensitivity()
        )
    if requested_time_constant is not None:
        instrument.set_time_constant(requested_time_constant)
        applied["Time Constant"] = _verify_numeric_setting(
            "Time Constant", requested_time_constant, instrument.get_time_constant()
        )
    if requested_ac_gain is not None:
        instrument.set_ac_gain(requested_ac_gain)
        applied["AC Gain"] = _verify_numeric_setting(
            "AC Gain", requested_ac_gain, instrument.get_ac_gain()
        )
    if requested_coupling is not None:
        instrument.set_coupling(requested_coupling)
        actual_coupling = instrument.get_coupling()
        if not isinstance(actual_coupling, str):
            raise RuntimeError(
                "Coupling read-back returned an invalid type: "
                f"{type(actual_coupling).__name__}."
            )
        if actual_coupling.strip().upper() != requested_coupling:
            raise RuntimeError(
                "Coupling read-back mismatch: "
                f"requested {requested_coupling!r}, received {actual_coupling!r}."
            )
        applied["Coupling"] = actual_coupling
    if slope is not None:
        instrument.set_slope(slope)
        actual_slope = instrument.get_slope()
        if isinstance(actual_slope, bool) or actual_slope != slope:
            raise RuntimeError(
                f"Slope read-back mismatch: requested {slope!r}, received {actual_slope!r}."
            )
        applied["Slope"] = actual_slope
    return applied


def read_lockin_overload(
    config: dict[str, Any] | None = None, *, lockin: LockinController | None = None
) -> dict[str, Any]:
    instrument = lockin or _connect_lockin(config or {})
    return dict(instrument.get_overload_status())


def get_lockin_wait_time(
    config: dict[str, Any] | None = None,
    *,
    lockin: LockinController | None = None,
    multiplier: float = 4.0,
) -> float:
    instrument = lockin or _connect_lockin(config or {})
    return float(instrument.get_wait_time(multiplier=multiplier))
