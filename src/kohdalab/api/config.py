from __future__ import annotations

import json
import os
from copy import deepcopy
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any

PROJECT_ROOT = Path(__file__).resolve().parents[3]
DEFAULT_CONFIG_PATH = PROJECT_ROOT / "config" / "default.json"
CONFIG_PATH_ENV = "KOHDALAB_CONFIG"
DEFAULT_CONFIG_PATH_ENV = "KOHDALAB_DEFAULT_CONFIG"
CONFIG_STATE_DIR_ENV = "KOHDALAB_STATE_DIR"
LAST_CONFIG_STATE_PATH_ENV = "KOHDALAB_LAST_CONFIG_STATE_PATH"


@dataclass(frozen=True)
class ConfigPathResolution:
    path: Path | None
    source: str
    candidates: list[dict[str, str]]


def config_state_dir() -> Path:
    configured = os.environ.get(CONFIG_STATE_DIR_ENV)
    if configured:
        return Path(configured)
    return Path.home() / ".kohdalab"


def last_config_state_path() -> Path:
    configured = os.environ.get(LAST_CONFIG_STATE_PATH_ENV)
    if configured:
        return Path(configured)
    return config_state_dir() / "last_config.json"


def read_last_config_path(path: str | Path | None = None) -> Path | None:
    state_path = Path(path) if path is not None else last_config_state_path()
    if not state_path.exists():
        return None
    try:
        data = json.loads(state_path.read_text(encoding="utf-8"))
        value = data.get("path") if isinstance(data, dict) else data
    except json.JSONDecodeError:
        value = state_path.read_text(encoding="utf-8").strip()
    if not value:
        return None
    return Path(str(value))


def write_last_config_path(config_path: str | Path, path: str | Path | None = None) -> Path:
    state_path = Path(path) if path is not None else last_config_state_path()
    state_path.parent.mkdir(parents=True, exist_ok=True)
    resolved = Path(config_path)
    state_path.write_text(json.dumps({"path": str(resolved)}, indent=2), encoding="utf-8")
    return state_path


def _record_candidate(candidates: list[dict[str, str]], source: str, path: Path) -> None:
    candidates.append({"source": source, "path": str(path), "exists": str(path.exists())})


def resolve_config_path(
    explicit_path: str | Path | None = None,
    *,
    env_var: str = CONFIG_PATH_ENV,
    last_state_path: str | Path | None = None,
    lab_default_path: str | Path | None = None,
) -> ConfigPathResolution:
    candidates: list[dict[str, str]] = []
    if explicit_path:
        path = Path(explicit_path)
        _record_candidate(candidates, "explicit", path)
        return ConfigPathResolution(path=path, source="explicit", candidates=candidates)

    env_path = os.environ.get(env_var)
    if env_path:
        path = Path(env_path)
        _record_candidate(candidates, env_var, path)
        return ConfigPathResolution(path=path, source=env_var, candidates=candidates)

    last_path = read_last_config_path(last_state_path)
    if last_path is not None:
        _record_candidate(candidates, "last", last_path)
        if last_path.exists():
            return ConfigPathResolution(path=last_path, source="last", candidates=candidates)

    default_from_env = os.environ.get(DEFAULT_CONFIG_PATH_ENV)
    default_path = Path(default_from_env) if default_from_env else Path(lab_default_path or DEFAULT_CONFIG_PATH)
    _record_candidate(candidates, "lab_default", default_path)
    if default_path.exists():
        return ConfigPathResolution(path=default_path, source="lab_default", candidates=candidates)

    return ConfigPathResolution(path=None, source="none", candidates=candidates)


def with_auto_suffix(filename: str) -> str:
    path = Path(filename.strip() or "run.csv")
    stamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    return f"{path.stem}_{stamp}{path.suffix or '.csv'}"


def with_csv_suffix(filename: str) -> str:
    path = Path(filename.strip() or "run.csv")
    return str(path) if path.suffix.lower() == ".csv" else f"{path}.csv"


def normalize_delay_stage_name(stage_name: str | None) -> str | None:
    if not stage_name:
        return None
    normalized = stage_name.strip().upper().replace("_", "-")
    if normalized.startswith("SGSP-"):
        normalized = "SGSP" + normalized[len("SGSP-") :]
    return normalized


def build_range_points(start: float, stop: float, step: float) -> list[float]:
    if step == 0:
        raise ValueError("Step must be non-zero.")
    points: list[float] = []
    current = start
    if step > 0:
        while current <= stop + abs(step) * 1e-9:
            points.append(round(current, 9))
            current += step
    else:
        while current >= stop - abs(step) * 1e-9:
            points.append(round(current, 9))
            current += step
    if not points:
        raise ValueError("No scan points generated. Check min/max/step.")
    return points


DEFAULT_MEASUREMENTS: dict[str, Any] = {
    "move_abs": {
        "coordinate": "measurement",
        "zero": {},
        "targets": {},
    },
    "signal_monitor": {
        "interval_s": 1.0,
        "n_points": 360,
        "output": {
            "dir": str(Path.cwd()),
            "filename": "signal_monitor_run",
            "auto_timestamp_suffix": True,
        },
    },
    "trkr": {
        "coordinate": "measurement",
        "scan": {
            "min": -50.0,
            "max": 300.0,
            "step": 5.0,
        },
        "wait_s": 2.0,
        "return_to_zero": True,
        "output": {
            "dir": str(Path.cwd()),
            "filename": "trkr_run",
            "auto_timestamp_suffix": True,
        },
    },
    "srkr": {
        "coordinate": "measurement",
        "scan": {
            "axis": "x",
            "min": -30.0,
            "max": 30.0,
            "step": 1.0,
        },
        "wait_s": 2.0,
        "return_to_zero": True,
        "output": {
            "dir": str(Path.cwd()),
            "filename": "srkr_run",
            "auto_timestamp_suffix": True,
        },
    },
    "strkr": {
        "scan": {
            "fast_axis": "t",
            "slow_axis": "x",
            "ranges": {
                "t": {"min": -50.0, "max": 300.0, "step": 5.0},
                "x": {"min": -30.0, "max": 30.0, "step": 1.0},
                "y": {"min": -30.0, "max": 30.0, "step": 1.0},
            },
        },
        "wait_s": 2.0,
        "return_to_zero": {"fast_axis": True, "slow_axis": True},
        "output": {
            "dir": str(Path.cwd()),
            "filename": "strkr_run",
            "auto_timestamp_suffix": True,
        },
    },
    "srkr_2d": {
        "scan": {
            "fast_axis": "x",
            "slow_axis": "y",
            "ranges": {
                "x": {"min": -30.0, "max": 30.0, "step": 1.0},
                "y": {"min": -30.0, "max": 30.0, "step": 1.0},
            },
        },
        "wait_s": 2.0,
        "return_to_zero": {"fast_axis": True, "slow_axis": True},
        "output": {
            "dir": str(Path.cwd()),
            "filename": "srkr_2d_run",
            "auto_timestamp_suffix": True,
        },
    },
}


def _deep_defaults(value: dict[str, Any], defaults: dict[str, Any]) -> dict[str, Any]:
    merged = deepcopy(value)
    for key, default_value in defaults.items():
        if key not in merged:
            merged[key] = deepcopy(default_value)
        elif isinstance(merged[key], dict) and isinstance(default_value, dict):
            merged[key] = _deep_defaults(merged[key], default_value)
    return merged


def _infer_profile_name(source: str | Path | None) -> str:
    if source is None:
        return "default"
    return Path(source).stem


def _legacy_scanner_scale(scanner: dict[str, Any]) -> Any:
    unit = str(scanner.get("pos_unit", "")).strip().lower().replace("/", "_")
    if unit:
        key = f"sample_um_per_actuator_{unit}"
        if key in scanner:
            return scanner[key]
    for key in ("sample_um_per_actuator_mm", "sample_um_per_actuator_deg"):
        if key in scanner:
            return scanner[key]
    for key, value in scanner.items():
        if key.startswith("sample_um_per_actuator_"):
            return value
    return None


def normalize_config(config: dict[str, Any], *, source: str | Path | None = None) -> dict[str, Any]:
    normalized = deepcopy(config)
    profile = normalized.setdefault("profile", {})
    if isinstance(profile, dict):
        profile.setdefault("name", _infer_profile_name(source))
    normalized.setdefault("instruments", {})
    normalized.setdefault("measurements", normalized.pop("measurement", {}))

    scanners = normalized.get("instruments", {}).get("scanner", {})
    if isinstance(scanners, dict):
        for scanner in scanners.values():
            if not isinstance(scanner, dict):
                continue
            if "sample_um_per_unit" not in scanner:
                legacy_scale = _legacy_scanner_scale(scanner)
                if legacy_scale is not None:
                    scanner["sample_um_per_unit"] = legacy_scale

    measurements = normalized.setdefault("measurements", {})
    for name, defaults in DEFAULT_MEASUREMENTS.items():
        current = measurements.get(name, {})
        measurements[name] = _deep_defaults(current if isinstance(current, dict) else {}, defaults)
    return normalized


def validate_config(config: dict[str, Any]) -> None:
    instruments = config.get("instruments")
    if not isinstance(instruments, dict):
        raise ValueError("config must contain an 'instruments' object.")
    for kind in ("lockin", "delay_stage", "scanner"):
        if kind not in instruments or not isinstance(instruments[kind], dict) or not instruments[kind]:
            raise ValueError(f"config must contain instruments.{kind}.")
    measurements = config.get("measurements")
    if not isinstance(measurements, dict):
        raise ValueError("config must contain a 'measurements' object.")
    for name in ("move_abs", "signal_monitor", "trkr", "srkr", "strkr", "srkr_2d"):
        if name not in measurements:
            raise ValueError(f"config must contain measurements.{name}.")
    for key, lockin in instruments["lockin"].items():
        for field in ("model", "resource"):
            if field not in lockin:
                raise ValueError(f"config must contain instruments.lockin.{key}.{field}.")
    for key, stage in instruments["delay_stage"].items():
        for field in ("controller", "stage", "port", "direction"):
            if field not in stage:
                raise ValueError(f"config must contain instruments.delay_stage.{key}.{field}.")
    for key, scanner in instruments["scanner"].items():
        for field in ("controller", "actuator", "port", "axis", "sample_um_per_unit"):
            if field not in scanner:
                raise ValueError(f"config must contain instruments.scanner.{key}.{field}.")


def load_config(path: str | Path = DEFAULT_CONFIG_PATH, *, validate: bool = False) -> dict[str, Any]:
    config_path = Path(path)
    with config_path.open("r", encoding="utf-8") as f:
        config = normalize_config(json.load(f), source=config_path)
    if validate:
        validate_config(config)
    return config


def save_config(config: dict[str, Any], path: str | Path, *, validate: bool = False) -> Path:
    config = normalize_config(config)
    if validate:
        validate_config(config)
    output = Path(path)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(config, indent=2), encoding="utf-8")
    return output


def measurement_settings(config: dict[str, Any], name: str) -> dict[str, Any]:
    return config.get("measurements", config.get("measurement", {})).get(name, {})


def measurement_output_settings(config: dict[str, Any], name: str) -> dict[str, Any]:
    settings = measurement_settings(config, name)
    output = settings.get("output", {})
    if isinstance(output, dict):
        return {
            "output_dir": output.get("dir", output.get("output_dir", settings.get("output_dir"))),
            "filename": output.get("filename", settings.get("filename")),
            "auto_timestamp_suffix": output.get("auto_timestamp_suffix", settings.get("auto_timestamp_suffix", True)),
        }
    return {
        "output_dir": settings.get("output_dir"),
        "filename": settings.get("filename"),
        "auto_timestamp_suffix": settings.get("auto_timestamp_suffix", True),
    }


def scan_settings(config: dict[str, Any], name: str) -> dict[str, Any]:
    scan = measurement_settings(config, name).get("scan", {})
    return scan if isinstance(scan, dict) else {}


def move_abs_settings(config: dict[str, Any]) -> dict[str, Any]:
    return measurement_settings(config, "move_abs")


def move_abs_zero(config: dict[str, Any]) -> dict[str, Any]:
    zero = move_abs_settings(config).get("zero", {})
    return zero if isinstance(zero, dict) else {}


def zero_for(config: dict[str, Any], axis: str, default: float = 0.0) -> float:
    axis = axis.strip().lower()
    key = "t_ps" if axis == "t" else f"{axis}_um"
    return float(move_abs_zero(config).get(key, default))


def instrument_config(config: dict[str, Any], kind: str, key: str) -> dict[str, Any]:
    try:
        return config["instruments"][kind][key]
    except KeyError as e:
        raise ValueError(f"Missing instruments.{kind}.{key!r} in config.") from e


def instrument_key(config: dict[str, Any], kind: str, preferred_key: str | None = None) -> str:
    instruments = config.get("instruments", {}).get(kind, {})
    if not isinstance(instruments, dict) or not instruments:
        raise ValueError(f"Missing instruments.{kind} in config.")
    if preferred_key is not None:
        key = str(preferred_key)
        if key in instruments:
            return key
        raise ValueError(f"Missing instruments.{kind}.{key!r} in config.")
    if len(instruments) == 1:
        return next(iter(instruments))
    raise ValueError(f"Multiple instruments.{kind} entries found; specify the key.")


def lockin_config_for(config: dict[str, Any], measurement_name: str, default_key: str = "main") -> dict[str, Any]:
    settings = measurement_settings(config, measurement_name)
    preferred_key = settings.get("lockin_key", settings.get("lockin"))
    if preferred_key is None and default_key in config.get("instruments", {}).get("lockin", {}):
        preferred_key = default_key
    return instrument_config(config, "lockin", instrument_key(config, "lockin", preferred_key))


def delay_stage_config_for(config: dict[str, Any], measurement_name: str, default_key: str = "t") -> dict[str, Any]:
    settings = measurement_settings(config, measurement_name)
    preferred_key = settings.get("delay_stage_key", settings.get("delay_stage"))
    if preferred_key is None and default_key in config.get("instruments", {}).get("delay_stage", {}):
        preferred_key = default_key
    return instrument_config(config, "delay_stage", instrument_key(config, "delay_stage", preferred_key))


def scanner_config_for(config: dict[str, Any], axis: str, measurement_name: str = "srkr") -> dict[str, Any]:
    axis = axis.strip().lower()
    settings = measurement_settings(config, measurement_name)
    scanner_keys = settings.get("scanner_keys", settings.get("scanners", {}))
    preferred_key = scanner_keys.get(axis) if isinstance(scanner_keys, dict) else None
    if preferred_key is None and axis in config.get("instruments", {}).get("scanner", {}):
        preferred_key = axis
    return instrument_config(config, "scanner", instrument_key(config, "scanner", preferred_key))


def output_path(settings: dict[str, Any], default_name: str) -> Path:
    output = settings.get("output", {})
    output = output if isinstance(output, dict) else {}
    output_dir = Path(str(output.get("dir") or output.get("output_dir") or settings.get("output_dir") or Path.cwd()))
    base_name = str(output.get("filename") or settings.get("filename") or default_name)
    base_name = with_csv_suffix(base_name)
    filename = with_auto_suffix(base_name) if bool(output.get("auto_timestamp_suffix", settings.get("auto_timestamp_suffix", True))) else base_name
    return output_dir / filename
