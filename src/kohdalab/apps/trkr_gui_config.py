from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from kohdalab.api.config import instrument_key
from kohdalab.apps.trkr_gui_output import output_config_for_measurement


def zero_um_from_config(
    zero_config: dict[str, Any], axis: str, default: float
) -> float:
    mm_key = f"{axis}_mm"
    um_key = f"{axis}_um"
    if um_key in zero_config:
        return float(zero_config[um_key])
    if mm_key in zero_config:
        return float(zero_config[mm_key]) * 1000.0
    return float(default)


def first_number(*values: Any, default: float = 0.0) -> float:
    for value in values:
        if value is None or isinstance(value, dict):
            continue
        try:
            return float(value)
        except (TypeError, ValueError):
            continue
    return float(default)


def measurement_map(config: dict[str, Any]) -> dict[str, Any]:
    measurements = config.get("measurements", config.get("measurement", {}))
    return measurements if isinstance(measurements, dict) else {}


def output_settings_from_measurement(
    settings: dict[str, Any], fallback: dict[str, Any], default_dir: str
) -> dict[str, Any]:
    output = settings.get("output", {})
    if isinstance(output, dict):
        return {
            "output_dir": output.get(
                "dir",
                output.get(
                    "output_dir",
                    settings.get("output_dir", fallback.get("output_dir", default_dir)),
                ),
            ),
            "filename": output.get(
                "filename",
                settings.get("filename", fallback.get("filename", "trkr_run")),
            ),
            "auto_timestamp_suffix": bool(
                output.get(
                    "auto_timestamp_suffix",
                    settings.get(
                        "auto_timestamp_suffix",
                        fallback.get("auto_timestamp_suffix", True),
                    ),
                )
            ),
        }
    return {
        "output_dir": settings.get(
            "output_dir", fallback.get("output_dir", default_dir)
        ),
        "filename": settings.get("filename", fallback.get("filename", "trkr_run")),
        "auto_timestamp_suffix": bool(
            settings.get(
                "auto_timestamp_suffix", fallback.get("auto_timestamp_suffix", True)
            )
        ),
    }


def scanner_scale_key(config: dict[str, Any]) -> str:
    return "sample_um_per_unit"


def scanner_scale_value(config: dict[str, Any], default: float) -> float:
    if "sample_um_per_unit" in config:
        return float(config["sample_um_per_unit"])
    unit = str(
        config.get(
            "pos_unit",
            "deg"
            if str(config.get("actuator", "")).upper().startswith("AG-")
            else "mm",
        )
    )
    legacy_key = f"sample_um_per_actuator_{unit.strip().lower().replace('/', '_')}"
    if legacy_key in config:
        return float(config[legacy_key])
    return float(
        config.get(
            "sample_um_per_actuator_mm",
            config.get("sample_um_per_actuator_deg", default),
        )
    )


def default_instrument_refs() -> dict[str, dict[str, str]]:
    return {
        "lockin": {"signal_monitor": "main", "TRKR": "main", "SRKR": "main"},
        "scanner": {"x": "x", "y": "y"},
        "delay_stage": {"TRKR": "t", "move_abs_t": "t"},
    }


@dataclass(frozen=True)
class LoadedGuiConfig:
    instrument_refs: dict[str, dict[str, str]]
    lockin_config: dict[str, Any]
    x_config: dict[str, Any]
    y_config: dict[str, Any]
    t_config: dict[str, Any]
    trkr_config: dict[str, Any]
    signal_monitor_config: dict[str, Any]
    srkr_config: dict[str, Any]
    move_abs_config: dict[str, Any]

    @property
    def shared_xy_port(self) -> bool:
        x_port = self.x_config.get("port")
        y_port = self.y_config.get("port")
        return bool(x_port and y_port and x_port == y_port)


def extract_loaded_gui_config(data: dict[str, Any]) -> LoadedGuiConfig:
    instruments = data.get("instruments", {})
    if instruments:
        return _extract_api_config(data, instruments)
    return _extract_legacy_config(data)


def _extract_api_config(
    data: dict[str, Any], instruments: dict[str, Any]
) -> LoadedGuiConfig:
    refs = default_instrument_refs()
    measurement = measurement_map(data)
    lockins = instruments.get("lockin", {})
    scanners = instruments.get("scanner", {})
    delay_stages = instruments.get("delay_stage", {})
    trkr_measurement = measurement.get("trkr", {})
    signal_monitor_measurement = measurement.get("signal_monitor", {})
    srkr_measurement = measurement.get("srkr", {})
    move_abs_measurement = measurement.get("move_abs", {})

    refs["lockin"]["TRKR"] = instrument_key(
        data,
        "lockin",
        trkr_measurement.get("lockin_key", trkr_measurement.get("lockin")),
    )
    refs["lockin"]["signal_monitor"] = instrument_key(
        data,
        "lockin",
        signal_monitor_measurement.get(
            "lockin_key",
            signal_monitor_measurement.get("lockin", refs["lockin"]["TRKR"]),
        ),
    )
    refs["lockin"]["SRKR"] = instrument_key(
        data,
        "lockin",
        srkr_measurement.get(
            "lockin_key", srkr_measurement.get("lockin", refs["lockin"]["TRKR"])
        ),
    )

    srkr_scanners = srkr_measurement.get(
        "scanner_keys", srkr_measurement.get("scanners", {})
    )
    move_abs_scanners = move_abs_measurement.get(
        "scanner_keys", move_abs_measurement.get("scanners", {})
    )
    srkr_scanners = srkr_scanners if isinstance(srkr_scanners, dict) else {}
    move_abs_scanners = move_abs_scanners if isinstance(move_abs_scanners, dict) else {}
    refs["scanner"]["x"] = _scanner_ref(
        data, scanners, "x", srkr_scanners, move_abs_scanners
    )
    refs["scanner"]["y"] = _scanner_ref(
        data, scanners, "y", srkr_scanners, move_abs_scanners
    )

    refs["delay_stage"]["TRKR"] = instrument_key(
        data,
        "delay_stage",
        trkr_measurement.get("delay_stage_key", trkr_measurement.get("delay_stage")),
    )
    refs["delay_stage"]["move_abs_t"] = instrument_key(
        data,
        "delay_stage",
        move_abs_measurement.get(
            "delay_stage_key",
            move_abs_measurement.get("delay_stage", refs["delay_stage"]["TRKR"]),
        ),
    )

    return LoadedGuiConfig(
        instrument_refs=refs,
        lockin_config=dict(
            lockins.get(refs["lockin"]["TRKR"], next(iter(lockins.values()), {}))
        ),
        x_config=dict(
            scanners.get(refs["scanner"]["x"], next(iter(scanners.values()), {}))
        ),
        y_config=dict(
            scanners.get(refs["scanner"]["y"], next(iter(scanners.values()), {}))
        ),
        t_config=dict(
            delay_stages.get(
                refs["delay_stage"]["TRKR"], next(iter(delay_stages.values()), {})
            )
        ),
        trkr_config=dict(trkr_measurement),
        signal_monitor_config=dict(signal_monitor_measurement),
        srkr_config=dict(srkr_measurement),
        move_abs_config=dict(move_abs_measurement),
    )


def _scanner_ref(
    data: dict[str, Any],
    scanners: dict[str, Any],
    axis: str,
    srkr_scanners: dict[str, Any],
    move_abs_scanners: dict[str, Any],
) -> str:
    if axis in srkr_scanners:
        return str(srkr_scanners[axis])
    if axis in move_abs_scanners:
        return str(move_abs_scanners[axis])
    if axis in scanners:
        return axis
    return instrument_key(data, "scanner")


def _extract_legacy_config(data: dict[str, Any]) -> LoadedGuiConfig:
    xy_config = data.get("xy_scanner", {})
    return LoadedGuiConfig(
        instrument_refs=default_instrument_refs(),
        lockin_config=dict(data.get("lockin", {})),
        x_config=dict(
            xy_config.get("x") or data.get("x_scanner") or data.get("scanner1", {})
        ),
        y_config=dict(
            xy_config.get("y") or data.get("y_scanner") or data.get("scanner2", {})
        ),
        t_config=dict(data.get("delay_stage") or data.get("delayline", {})),
        trkr_config=dict(data.get("trkr", {})),
        signal_monitor_config=dict(
            data.get("signal_monitor", data.get("lab_time", {}))
        ),
        srkr_config=dict(data.get("srkr", {})),
        move_abs_config={},
    )


@dataclass(frozen=True)
class GuiConfigSnapshot:
    instrument_refs: dict[str, dict[str, str]]
    lockin_config: dict[str, Any]
    scanner_configs: dict[str, dict[str, Any]]
    delay_stage_config: dict[str, Any]
    move_t_coordinate: str
    trkr_coordinate: str
    srkr_coordinate: str
    t_zero_ps: float
    x_zero_um: float
    y_zero_um: float
    trkr_scan: dict[str, float]
    trkr_wait_s: float
    trkr_return_to_zero: bool
    signal_monitor_interval_s: float
    signal_monitor_n_points: int
    srkr_axis: str
    srkr_scan: dict[str, float]
    srkr_wait_s: float
    srkr_return_to_zero: bool
    output_settings: dict[str, dict[str, Any]]


def build_saved_config(snapshot: GuiConfigSnapshot) -> dict[str, Any]:
    lockin_name = _lockin_ref(snapshot, "TRKR")
    signal_lockin_name = _lockin_ref(snapshot, "signal_monitor")
    srkr_lockin_name = _lockin_ref(snapshot, "SRKR")
    scanner_x_name = snapshot.instrument_refs["scanner"]["x"]
    scanner_y_name = snapshot.instrument_refs["scanner"]["y"]
    delay_stage_name = snapshot.instrument_refs["delay_stage"]["TRKR"]

    move_abs_settings = {
        "coordinate": snapshot.move_t_coordinate,
        "zero": _zero_settings(snapshot),
    }
    trkr_settings = {
        "coordinate": snapshot.trkr_coordinate,
        "scan": dict(snapshot.trkr_scan),
        "wait_s": snapshot.trkr_wait_s,
        "return_to_zero": snapshot.trkr_return_to_zero,
        "output": _output_config(snapshot, "TRKR"),
    }
    signal_monitor_settings = {
        "interval_s": snapshot.signal_monitor_interval_s,
        "n_points": snapshot.signal_monitor_n_points,
        "output": _output_config(snapshot, "signal_monitor"),
    }
    srkr_settings = {
        "coordinate": snapshot.srkr_coordinate,
        "scan": {"axis": snapshot.srkr_axis, **snapshot.srkr_scan},
        "wait_s": snapshot.srkr_wait_s,
        "return_to_zero": snapshot.srkr_return_to_zero,
        "output": _output_config(snapshot, "SRKR"),
    }

    if lockin_name != "main":
        trkr_settings["lockin_key"] = lockin_name
    if signal_lockin_name != "main":
        signal_monitor_settings["lockin_key"] = signal_lockin_name
    if srkr_lockin_name != "main":
        srkr_settings["lockin_key"] = srkr_lockin_name
    if delay_stage_name != "t":
        trkr_settings["delay_stage_key"] = delay_stage_name
        move_abs_settings["delay_stage_key"] = snapshot.instrument_refs["delay_stage"][
            "move_abs_t"
        ]
    if scanner_x_name != "x" or scanner_y_name != "y":
        scanner_keys = {"x": scanner_x_name, "y": scanner_y_name}
        move_abs_settings["scanner_keys"] = scanner_keys
        srkr_settings["scanner_keys"] = scanner_keys

    return {
        "instruments": {
            "lockin": {
                lockin_name: dict(snapshot.lockin_config),
                signal_lockin_name: dict(snapshot.lockin_config),
                srkr_lockin_name: dict(snapshot.lockin_config),
            },
            "scanner": {
                scanner_x_name: dict(snapshot.scanner_configs["x"]),
                scanner_y_name: dict(snapshot.scanner_configs["y"]),
            },
            "delay_stage": {
                delay_stage_name: dict(snapshot.delay_stage_config),
            },
        },
        "measurements": {
            "move_abs": move_abs_settings,
            "signal_monitor": signal_monitor_settings,
            "trkr": trkr_settings,
            "srkr": srkr_settings,
        },
    }


def build_measurement_config(
    snapshot: GuiConfigSnapshot, measurement_name: str
) -> dict[str, Any]:
    lockin_name = _lockin_ref(snapshot, measurement_name)
    config: dict[str, Any] = {
        "instruments": {
            "lockin": {
                lockin_name: dict(snapshot.lockin_config),
            },
        },
        "measurements": {
            "move_abs": {
                "zero": _zero_settings(snapshot),
            },
        },
    }

    if measurement_name == "signal_monitor":
        settings: dict[str, Any] = {
            "interval_s": snapshot.signal_monitor_interval_s,
            "n_points": snapshot.signal_monitor_n_points,
        }
        if lockin_name != "main":
            settings["lockin_key"] = lockin_name
        config["measurements"]["signal_monitor"] = settings
        return config

    if measurement_name == "TRKR":
        delay_stage_name = snapshot.instrument_refs["delay_stage"]["TRKR"]
        config["instruments"]["delay_stage"] = {
            delay_stage_name: dict(snapshot.delay_stage_config),
        }
        settings = {
            "coordinate": snapshot.trkr_coordinate,
            "wait_s": snapshot.trkr_wait_s,
            "return_to_zero": snapshot.trkr_return_to_zero,
        }
        if lockin_name != "main":
            settings["lockin_key"] = lockin_name
        if delay_stage_name != "t":
            settings["delay_stage_key"] = delay_stage_name
        config["measurements"]["trkr"] = settings
        return config

    if measurement_name == "SRKR":
        scanner_x_name = snapshot.instrument_refs["scanner"]["x"]
        scanner_y_name = snapshot.instrument_refs["scanner"]["y"]
        scanner_keys = {"x": scanner_x_name, "y": scanner_y_name}
        config["instruments"]["scanner"] = {
            scanner_x_name: dict(snapshot.scanner_configs["x"]),
            scanner_y_name: dict(snapshot.scanner_configs["y"]),
        }
        settings = {
            "coordinate": snapshot.srkr_coordinate,
            "scan": {"axis": snapshot.srkr_axis},
            "wait_s": snapshot.srkr_wait_s,
            "return_to_zero": snapshot.srkr_return_to_zero,
            "scanner_keys": scanner_keys,
        }
        if lockin_name != "main":
            settings["lockin_key"] = lockin_name
        config["measurements"]["srkr"] = settings
        config["measurements"]["move_abs"]["scanner_keys"] = scanner_keys
        return config

    raise ValueError(f"Unsupported measurement: {measurement_name}")


def _lockin_ref(snapshot: GuiConfigSnapshot, measurement_name: str) -> str:
    return str(snapshot.instrument_refs["lockin"].get(measurement_name, "main"))


def _zero_settings(snapshot: GuiConfigSnapshot) -> dict[str, float]:
    return {
        "t_ps": snapshot.t_zero_ps,
        "x_um": snapshot.x_zero_um,
        "y_um": snapshot.y_zero_um,
    }


def _output_config(
    snapshot: GuiConfigSnapshot, measurement_name: str
) -> dict[str, Any]:
    return output_config_for_measurement(snapshot.output_settings[measurement_name])
