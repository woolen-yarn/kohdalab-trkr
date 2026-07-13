from __future__ import annotations

from typing import Any

from kohdalab.api.config import instrument_key, measurement_settings


def _lockin_ref(config: dict[str, Any], measurement_name: str) -> str:
    settings = measurement_settings(config, measurement_name)
    preferred_key = settings.get("lockin_key", settings.get("lockin"))
    if preferred_key is None and "main" in config.get("instruments", {}).get(
        "lockin", {}
    ):
        preferred_key = "main"
    return f"lockin.{instrument_key(config, 'lockin', preferred_key)}"


def _delay_stage_ref(config: dict[str, Any], measurement_name: str) -> str:
    settings = measurement_settings(config, measurement_name)
    preferred_key = settings.get("delay_stage_key", settings.get("delay_stage"))
    if preferred_key is None and "t" in config.get("instruments", {}).get(
        "delay_stage", {}
    ):
        preferred_key = "t"
    return f"delay_stage.{instrument_key(config, 'delay_stage', preferred_key)}"


def _scanner_ref(
    config: dict[str, Any], axis: str, measurement_name: str = "srkr"
) -> str:
    axis = axis.strip().lower()
    if axis not in {"x", "y"}:
        raise ValueError("SRKR axis must be 'x' or 'y'.")
    settings = measurement_settings(config, measurement_name)
    scanner_keys = settings.get("scanner_keys", settings.get("scanners", {}))
    preferred_key = scanner_keys.get(axis) if isinstance(scanner_keys, dict) else None
    if preferred_key is None and axis in config.get("instruments", {}).get(
        "scanner", {}
    ):
        preferred_key = axis
    return f"scanner.{instrument_key(config, 'scanner', preferred_key)}"


def _scan_axes(config: dict[str, Any], measurement_name: str) -> tuple[str, str]:
    scan = measurement_settings(config, measurement_name).get("scan", {})
    scan = scan if isinstance(scan, dict) else {}
    return str(scan.get("fast_axis", "")).lower(), str(
        scan.get("slow_axis", "")
    ).lower()


def required_devices(
    config: dict[str, Any],
    measurement_name: str,
    *,
    axis: str | None = None,
    fast_axis: str | None = None,
    slow_axis: str | None = None,
) -> list[str]:
    measurement = measurement_name.strip().lower()
    if measurement == "signal":
        measurement = "signal_monitor"
    if measurement == "signal_monitor":
        return [_lockin_ref(config, measurement)]
    if measurement == "trkr":
        return [_lockin_ref(config, measurement), _delay_stage_ref(config, measurement)]
    if measurement == "srkr":
        return [
            _lockin_ref(config, measurement),
            _scanner_ref(config, axis or "x", measurement),
        ]
    if measurement == "strkr":
        config_fast, config_slow = _scan_axes(config, measurement)
        axes = {fast_axis or config_fast or "t", slow_axis or config_slow or "x"}
        if "t" not in axes or not (axes & {"x", "y"}):
            raise ValueError("STRKR axes must combine t with x or y.")
        refs = [_lockin_ref(config, measurement), _delay_stage_ref(config, measurement)]
        for scan_axis in sorted(axes & {"x", "y"}):
            refs.append(_scanner_ref(config, scan_axis, measurement))
        return refs
    if measurement == "srkr_2d":
        return [
            _lockin_ref(config, measurement),
            _scanner_ref(config, "x", measurement),
            _scanner_ref(config, "y", measurement),
        ]
    raise ValueError(f"Unsupported measurement: {measurement_name}")


def missing_devices(
    config: dict[str, Any],
    connected: dict[str, bool],
    measurement_name: str,
    *,
    axis: str | None = None,
    fast_axis: str | None = None,
    slow_axis: str | None = None,
) -> list[str]:
    return [
        ref
        for ref in required_devices(
            config,
            measurement_name,
            axis=axis,
            fast_axis=fast_axis,
            slow_axis=slow_axis,
        )
        if not connected.get(ref, False)
    ]
