from __future__ import annotations

from typing import Any


def device_ref(kind: str, key: str) -> str:
    return f"{kind}.{key}"


def lockin_key(instrument_refs: dict[str, dict[str, str]], measurement_name: str, *, default: str = "main") -> str:
    return str(instrument_refs.get("lockin", {}).get(measurement_name, default))


def scanner_key(instrument_refs: dict[str, dict[str, str]], axis: str) -> str:
    return str(instrument_refs["scanner"][axis.strip().lower()])


def scanner_keys(instrument_refs: dict[str, dict[str, str]]) -> tuple[str, str]:
    return scanner_key(instrument_refs, "x"), scanner_key(instrument_refs, "y")


def delay_stage_key(instrument_refs: dict[str, dict[str, str]], measurement_name: str = "TRKR") -> str:
    return str(instrument_refs["delay_stage"][measurement_name])


def single_instrument_config(kind: str, key: str, config: dict[str, Any]) -> dict[str, Any]:
    return {
        "instruments": {
            kind: {
                key: dict(config),
            },
        },
    }


def xy_scanner_config(
    instrument_refs: dict[str, dict[str, str]],
    x_config: dict[str, Any],
    y_config: dict[str, Any],
) -> dict[str, Any]:
    x_key, y_key = scanner_keys(instrument_refs)
    return {
        "instruments": {
            "scanner": {
                x_key: dict(x_config),
                y_key: dict(y_config),
            },
        },
    }


def corrected_target(origin: float, corrected_value: float) -> float:
    return float(origin) + float(corrected_value)
