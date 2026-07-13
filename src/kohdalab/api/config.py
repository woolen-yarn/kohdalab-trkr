from __future__ import annotations

import json
import math
import os
from copy import deepcopy
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any

PACKAGE_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_CONFIG_PATH = PACKAGE_ROOT / "resources" / "default.json"
CONFIG_PATH_ENV = "KOHDALAB_CONFIG"
DEFAULT_CONFIG_PATH_ENV = "KOHDALAB_DEFAULT_CONFIG"
CONFIG_STATE_DIR_ENV = "KOHDALAB_STATE_DIR"
LAST_CONFIG_STATE_PATH_ENV = "KOHDALAB_LAST_CONFIG_STATE_PATH"
MAX_SCAN_POINTS_PER_AXIS = 100_000
MAX_SCAN_POINTS_TOTAL = 1_000_000


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


def write_last_config_path(
    config_path: str | Path, path: str | Path | None = None
) -> Path:
    state_path = Path(path) if path is not None else last_config_state_path()
    state_path.parent.mkdir(parents=True, exist_ok=True)
    resolved = Path(config_path)
    state_path.write_text(
        json.dumps({"path": str(resolved)}, indent=2), encoding="utf-8"
    )
    return state_path


def _record_candidate(
    candidates: list[dict[str, str]], source: str, path: Path
) -> None:
    candidates.append(
        {"source": source, "path": str(path), "exists": str(path.exists())}
    )


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
            return ConfigPathResolution(
                path=last_path, source="last", candidates=candidates
            )

    default_from_env = os.environ.get(DEFAULT_CONFIG_PATH_ENV)
    default_path = (
        Path(default_from_env)
        if default_from_env
        else Path(lab_default_path or DEFAULT_CONFIG_PATH)
    )
    _record_candidate(candidates, "lab_default", default_path)
    if default_path.exists():
        return ConfigPathResolution(
            path=default_path, source="lab_default", candidates=candidates
        )

    return ConfigPathResolution(path=None, source="none", candidates=candidates)


def with_auto_suffix(filename: str) -> str:
    path = Path(filename.strip() or "run.csv")
    stamp = datetime.now().strftime("%Y%m%d_%H%M%S_%f")
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


def build_range_points(
    start: float,
    stop: float,
    step: float,
    *,
    max_points: int = MAX_SCAN_POINTS_PER_AXIS,
) -> list[float]:
    start = float(start)
    stop = float(stop)
    step = float(step)
    if not all(math.isfinite(value) for value in (start, stop, step)):
        raise ValueError("Scan start, stop, and step must be finite numbers.")
    if step == 0:
        raise ValueError("Step must be non-zero.")
    if (stop - start) * step < 0:
        raise ValueError("No scan points generated. Check min/max/step.")
    point_count = math.floor(abs(stop - start) / abs(step) + 1e-9) + 1
    if point_count > int(max_points):
        raise ValueError(
            f"Scan would generate {point_count} points; maximum is {max_points} per axis."
        )
    return [round(start + index * step, 9) for index in range(point_count)]


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


def normalize_config(
    config: dict[str, Any], *, source: str | Path | None = None
) -> dict[str, Any]:
    normalized = deepcopy(config)
    profile = normalized.setdefault("profile", {})
    if isinstance(profile, dict):
        profile.setdefault("name", _infer_profile_name(source))
    normalized.setdefault("instruments", {})
    normalized.setdefault("measurements", normalized.pop("measurement", {}))

    instruments = normalized.get("instruments", {})
    if isinstance(instruments, dict):
        lockins = instruments.get("lockin", {})
        if isinstance(lockins, dict):
            for lockin in lockins.values():
                if isinstance(lockin, dict) and lockin.get("model") is not None:
                    lockin["model"] = str(lockin["model"]).strip().upper()
        delay_stages = instruments.get("delay_stage", {})
        if isinstance(delay_stages, dict):
            for stage in delay_stages.values():
                if not isinstance(stage, dict):
                    continue
                if stage.get("controller") is not None:
                    stage["controller"] = str(stage["controller"]).strip().upper()
                if stage.get("stage") is not None:
                    stage["stage"] = normalize_delay_stage_name(str(stage["stage"]))

    scanners = instruments.get("scanner", {}) if isinstance(instruments, dict) else {}
    if isinstance(scanners, dict):
        for scanner in scanners.values():
            if not isinstance(scanner, dict):
                continue
            if scanner.get("controller") is not None:
                scanner["controller"] = str(scanner["controller"]).strip().upper()
            if scanner.get("actuator") is not None:
                scanner["actuator"] = (
                    str(scanner["actuator"]).strip().upper().replace("_", "-")
                )
            if "sample_um_per_unit" not in scanner:
                legacy_scale = _legacy_scanner_scale(scanner)
                if legacy_scale is not None:
                    scanner["sample_um_per_unit"] = legacy_scale

    measurements = normalized.setdefault("measurements", {})
    for name, defaults in DEFAULT_MEASUREMENTS.items():
        current = measurements.get(name, {})
        measurements[name] = _deep_defaults(
            current if isinstance(current, dict) else {}, defaults
        )
    return normalized


def validate_config(config: dict[str, Any]) -> None:
    from kohdalab.instruments.delay_stage import DELAY_STAGE_CONTROLLERS
    from kohdalab.instruments.lockin import LOCKIN_CONTROLLERS
    from kohdalab.instruments.scanner import SCANNER_CONTROLLERS
    from kohdalab.interfaces.delay_stage import STAGES
    from kohdalab.interfaces.scanner import ACTUATORS

    instruments = config.get("instruments")
    if not isinstance(instruments, dict):
        raise ValueError("config must contain an 'instruments' object.")
    for kind in ("lockin", "delay_stage", "scanner"):
        if (
            kind not in instruments
            or not isinstance(instruments[kind], dict)
            or not instruments[kind]
        ):
            raise ValueError(f"config must contain instruments.{kind}.")
        for key in instruments[kind]:
            if not str(key).strip() or "." in str(key):
                raise ValueError(
                    f"instruments.{kind} keys must be non-empty and must not contain '.'."
                )
    measurements = config.get("measurements")
    if not isinstance(measurements, dict):
        raise ValueError("config must contain a 'measurements' object.")
    for name in ("move_abs", "signal_monitor", "trkr", "srkr", "strkr", "srkr_2d"):
        if name not in measurements:
            raise ValueError(f"config must contain measurements.{name}.")
    for key, lockin in instruments["lockin"].items():
        if not isinstance(lockin, dict):
            raise ValueError(f"instruments.lockin.{key} must be an object.")
        for field in ("model", "resource"):
            if not str(lockin.get(field, "")).strip():
                raise ValueError(
                    f"config must contain instruments.lockin.{key}.{field}."
                )
        model = str(lockin["model"]).strip().upper()
        if model not in LOCKIN_CONTROLLERS:
            raise ValueError(
                f"Unsupported instruments.lockin.{key}.model {model!r}; "
                f"supported: {sorted(LOCKIN_CONTROLLERS)}."
            )
    lockin_endpoints: dict[str, str] = {}
    for key, lockin in instruments["lockin"].items():
        lockin_endpoint = str(lockin["resource"]).strip().casefold()
        if lockin_endpoint in lockin_endpoints:
            raise ValueError(
                f"instruments.lockin.{key} duplicates resource used by "
                f"instruments.lockin.{lockin_endpoints[lockin_endpoint]}."
            )
        lockin_endpoints[lockin_endpoint] = key

    delay_stage_endpoints: dict[tuple[str, str], str] = {}
    for key, stage in instruments["delay_stage"].items():
        if not isinstance(stage, dict):
            raise ValueError(f"instruments.delay_stage.{key} must be an object.")
        for field in ("controller", "stage", "port", "direction"):
            if field not in stage:
                raise ValueError(
                    f"config must contain instruments.delay_stage.{key}.{field}."
                )
        for field in ("controller", "stage", "port"):
            if not str(stage[field]).strip():
                raise ValueError(
                    f"instruments.delay_stage.{key}.{field} must not be empty."
                )
        if int(stage["direction"]) not in {0, 1}:
            raise ValueError(f"instruments.delay_stage.{key}.direction must be 0 or 1.")
        controller = str(stage["controller"]).strip().upper()
        if controller not in DELAY_STAGE_CONTROLLERS:
            raise ValueError(
                f"Unsupported instruments.delay_stage.{key}.controller {controller!r}; "
                f"supported: {sorted(DELAY_STAGE_CONTROLLERS)}."
            )
        stage_name = normalize_delay_stage_name(str(stage["stage"]))
        if stage_name not in STAGES:
            raise ValueError(
                f"Unsupported instruments.delay_stage.{key}.stage {stage_name!r}; supported: {sorted(STAGES)}."
            )
        allowed_stage_controllers = {
            str(item).upper() for item in STAGES[stage_name].get("controllers", [])
        }
        if allowed_stage_controllers and controller not in allowed_stage_controllers:
            raise ValueError(
                f"instruments.delay_stage.{key} stage {stage_name!r} is not compatible with {controller!r}."
            )
        delay_stage_endpoint = (controller, str(stage["port"]).strip().casefold())
        if delay_stage_endpoint in delay_stage_endpoints:
            raise ValueError(
                f"instruments.delay_stage.{key} duplicates controller/port used by "
                f"instruments.delay_stage.{delay_stage_endpoints[delay_stage_endpoint]}."
            )
        delay_stage_endpoints[delay_stage_endpoint] = key
        if stage.get("zero_pos_mm") is not None and not math.isfinite(
            float(stage["zero_pos_mm"])
        ):
            raise ValueError(
                f"instruments.delay_stage.{key}.zero_pos_mm must be finite."
            )

    scanner_endpoints: dict[tuple[str, str, str], str] = {}
    for key, scanner in instruments["scanner"].items():
        if not isinstance(scanner, dict):
            raise ValueError(f"instruments.scanner.{key} must be an object.")
        for field in ("controller", "actuator", "port", "axis", "sample_um_per_unit"):
            if field not in scanner:
                raise ValueError(
                    f"config must contain instruments.scanner.{key}.{field}."
                )
        for field in ("controller", "actuator", "port", "axis"):
            if not str(scanner[field]).strip():
                raise ValueError(
                    f"instruments.scanner.{key}.{field} must not be empty."
                )
        controller = str(scanner["controller"]).strip().upper()
        if controller not in SCANNER_CONTROLLERS:
            raise ValueError(
                f"Unsupported instruments.scanner.{key}.controller {controller!r}; "
                f"supported: {sorted(SCANNER_CONTROLLERS)}."
            )
        actuator_key = (
            str(scanner["actuator"]).strip().upper().replace("-", "").replace("_", "")
        )
        if actuator_key not in ACTUATORS:
            raise ValueError(
                f"Unsupported instruments.scanner.{key}.actuator {scanner['actuator']!r}; "
                f"supported: {sorted(ACTUATORS)}."
            )
        allowed_scanner_controllers = {
            str(item).upper() for item in ACTUATORS[actuator_key].get("controllers", [])
        }
        if (
            allowed_scanner_controllers
            and controller not in allowed_scanner_controllers
        ):
            raise ValueError(
                f"instruments.scanner.{key} actuator {scanner['actuator']!r} "
                f"is not compatible with {controller!r}."
            )
        raw_axis = scanner["axis"]
        if controller == "CONEXAGAP":
            axis_text = str(raw_axis).strip().upper()
            axis_aliases = {"1": "U", "2": "V", "U": "U", "V": "V"}
            if axis_text not in axis_aliases:
                raise ValueError(
                    f"instruments.scanner.{key}.axis must be U/V or 1/2 for CONEXAGAP."
                )
            endpoint_axis = axis_aliases[axis_text]
        else:
            try:
                axis_number = int(raw_axis)
            except (TypeError, ValueError) as error:
                raise ValueError(
                    f"instruments.scanner.{key}.axis must be a positive integer."
                ) from error
            if (
                isinstance(raw_axis, bool)
                or axis_number <= 0
                or str(raw_axis).strip() != str(axis_number)
            ):
                raise ValueError(
                    f"instruments.scanner.{key}.axis must be a positive integer."
                )
            endpoint_axis = str(axis_number)
        scanner_endpoint = (
            controller,
            str(scanner["port"]).strip().casefold(),
            endpoint_axis,
        )
        if scanner_endpoint in scanner_endpoints:
            raise ValueError(
                f"instruments.scanner.{key} duplicates controller/port/axis used by "
                f"instruments.scanner.{scanner_endpoints[scanner_endpoint]}."
            )
        scanner_endpoints[scanner_endpoint] = key
        scale = float(scanner["sample_um_per_unit"])
        if not math.isfinite(scale) or scale == 0:
            raise ValueError(
                f"instruments.scanner.{key}.sample_um_per_unit must be finite and non-zero."
            )
        for field in ("min_pos", "max_pos", "origin_pos"):
            if scanner.get(field) is not None and not math.isfinite(
                float(scanner[field])
            ):
                raise ValueError(f"instruments.scanner.{key}.{field} must be finite.")
        if scanner.get("min_pos") is not None and scanner.get("max_pos") is not None:
            minimum = float(scanner["min_pos"])
            maximum = float(scanner["max_pos"])
            if maximum <= minimum:
                raise ValueError(
                    f"instruments.scanner.{key} max_pos must be greater than min_pos."
                )
            if (
                scanner.get("origin_pos") is not None
                and not minimum <= float(scanner["origin_pos"]) <= maximum
            ):
                raise ValueError(
                    f"instruments.scanner.{key}.origin_pos must be within min_pos/max_pos."
                )
        hysteresis = scanner.get("software_hysteresis", {})
        if not isinstance(hysteresis, dict):
            raise ValueError(
                f"instruments.scanner.{key}.software_hysteresis must be an object."
            )
        if "enabled" in hysteresis and not isinstance(hysteresis["enabled"], bool):
            raise ValueError(
                f"instruments.scanner.{key}.software_hysteresis.enabled must be boolean."
            )
        for field in ("distance_um", "approach_distance_um", "pre_move_um"):
            if hysteresis.get(field) is not None:
                distance = float(hysteresis[field])
                if not math.isfinite(distance) or distance < 0:
                    raise ValueError(
                        f"instruments.scanner.{key}.software_hysteresis.{field} "
                        "must be finite and non-negative."
                    )
        direction = (
            str(hysteresis.get("direction", hysteresis.get("approach", "negative")))
            .strip()
            .lower()
        )
        if direction not in {
            "negative",
            "negative_to_target",
            "minus",
            "-",
            "positive",
            "positive_to_target",
            "plus",
            "+",
        }:
            raise ValueError(
                f"instruments.scanner.{key}.software_hysteresis.direction must be negative or positive."
            )

    zero = measurements["move_abs"].get("zero", {})
    if not isinstance(zero, dict):
        raise ValueError("measurements.move_abs.zero must be an object.")
    for field in ("t_ps", "x_um", "y_um"):
        if field in zero and not math.isfinite(float(zero[field])):
            raise ValueError(f"measurements.move_abs.zero.{field} must be finite.")

    monitor = measurements["signal_monitor"]
    interval = float(monitor.get("interval_s", 1.0))
    raw_point_count = monitor.get("n_points", 360)
    point_count = int(raw_point_count)
    if not math.isfinite(interval) or interval < 0:
        raise ValueError(
            "measurements.signal_monitor.interval_s must be finite and non-negative."
        )
    if isinstance(raw_point_count, bool) or float(raw_point_count) != point_count:
        raise ValueError("measurements.signal_monitor.n_points must be an integer.")
    if point_count <= 0 or point_count > MAX_SCAN_POINTS_TOTAL:
        raise ValueError(
            f"measurements.signal_monitor.n_points must be between 1 and {MAX_SCAN_POINTS_TOTAL}."
        )

    for name in ("trkr", "srkr"):
        settings = measurements[name]
        wait = float(settings.get("wait_s", 1.0))
        if not math.isfinite(wait) or wait < 0:
            raise ValueError(
                f"measurements.{name}.wait_s must be finite and non-negative."
            )
        scan = settings.get("scan", {})
        if not isinstance(scan, dict):
            raise ValueError(f"measurements.{name}.scan must be an object.")
        build_range_points(float(scan["min"]), float(scan["max"]), float(scan["step"]))

    trkr_coordinate = (
        str(measurements["trkr"].get("coordinate", "measurement")).strip().lower()
    )
    if trkr_coordinate not in {
        "measurement",
        "interface",
        "instrument",
        "control",
        "device",
    }:
        raise ValueError(
            "measurements.trkr.coordinate must be measurement, interface, or instrument."
        )

    srkr_axis = str(measurements["srkr"]["scan"].get("axis", "x")).strip().lower()
    if srkr_axis not in {"x", "y"}:
        raise ValueError("measurements.srkr.scan.axis must be 'x' or 'y'.")
    srkr_coordinate = (
        str(measurements["srkr"].get("coordinate", "measurement")).strip().lower()
    )
    if srkr_coordinate not in {
        "measurement",
        "interface",
        "instrument",
        "control",
        "device",
    }:
        raise ValueError(
            "measurements.srkr.coordinate must be measurement or interface."
        )

    for name, allowed_axes in (("strkr", {"t", "x", "y"}), ("srkr_2d", {"x", "y"})):
        settings = measurements[name]
        wait = float(settings.get("wait_s", 1.0))
        if not math.isfinite(wait) or wait < 0:
            raise ValueError(
                f"measurements.{name}.wait_s must be finite and non-negative."
            )
        scan = settings.get("scan", {})
        ranges = scan.get("ranges", {}) if isinstance(scan, dict) else {}
        fast = (
            str(scan.get("fast_axis", "")).strip().lower()
            if isinstance(scan, dict)
            else ""
        )
        slow = (
            str(scan.get("slow_axis", "")).strip().lower()
            if isinstance(scan, dict)
            else ""
        )
        if fast not in allowed_axes or slow not in allowed_axes or fast == slow:
            raise ValueError(
                f"measurements.{name} must define two different supported scan axes."
            )
        if name == "strkr" and "t" not in {fast, slow}:
            raise ValueError("measurements.strkr axes must combine t with x or y.")
        total_points = 1
        for axis in (fast, slow):
            axis_range = ranges.get(axis, {}) if isinstance(ranges, dict) else {}
            if not isinstance(axis_range, dict):
                raise ValueError(
                    f"measurements.{name}.scan.ranges.{axis} must be an object."
                )
            total_points *= len(
                build_range_points(
                    float(axis_range["min"]),
                    float(axis_range["max"]),
                    float(axis_range["step"]),
                )
            )
        if total_points > MAX_SCAN_POINTS_TOTAL:
            raise ValueError(
                f"measurements.{name} would generate {total_points} points; maximum is {MAX_SCAN_POINTS_TOTAL}."
            )

    for name in ("signal_monitor", "trkr", "srkr", "strkr", "srkr_2d"):
        output = measurements[name].get("output", {})
        if not isinstance(output, dict):
            raise ValueError(f"measurements.{name}.output must be an object.")
        directory = str(output.get("dir", output.get("output_dir", ""))).strip()
        if not directory:
            raise ValueError(f"measurements.{name}.output.dir must not be empty.")
        filename = str(output.get("filename", "")).strip()
        if not filename:
            raise ValueError(f"measurements.{name}.output.filename must not be empty.")
        if filename in {".", ".."} or "/" in filename or "\\" in filename:
            raise ValueError(
                f"measurements.{name}.output.filename must be a file name, not a path."
            )
        if not isinstance(output.get("auto_timestamp_suffix", True), bool):
            raise ValueError(
                f"measurements.{name}.output.auto_timestamp_suffix must be boolean."
            )

    from kohdalab.api.device_requirements import required_devices

    for name in ("signal_monitor", "trkr", "srkr", "strkr", "srkr_2d"):
        try:
            if name == "srkr":
                required_devices(config, name, axis=srkr_axis)
            else:
                required_devices(config, name)
        except ValueError as error:
            raise ValueError(
                f"measurements.{name} has an invalid device reference: {error}"
            ) from error


def load_config(
    path: str | Path = DEFAULT_CONFIG_PATH, *, validate: bool = True
) -> dict[str, Any]:
    config_path = Path(path)
    with config_path.open("r", encoding="utf-8") as f:
        config = normalize_config(json.load(f), source=config_path)
    if validate:
        validate_config(config)
    return config


def save_config(
    config: dict[str, Any], path: str | Path, *, validate: bool = False
) -> Path:
    config = normalize_config(config)
    if validate:
        validate_config(config)
    output = Path(path)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(config, indent=2), encoding="utf-8")
    return output


def measurement_settings(config: dict[str, Any], name: str) -> dict[str, Any]:
    measurements = config.get("measurements", config.get("measurement", {}))
    if not isinstance(measurements, dict):
        return {}
    settings = measurements.get(name, {})
    return settings if isinstance(settings, dict) else {}


def measurement_output_settings(config: dict[str, Any], name: str) -> dict[str, Any]:
    settings = measurement_settings(config, name)
    output = settings.get("output", {})
    if isinstance(output, dict):
        return {
            "output_dir": output.get(
                "dir", output.get("output_dir", settings.get("output_dir"))
            ),
            "filename": output.get("filename", settings.get("filename")),
            "auto_timestamp_suffix": output.get(
                "auto_timestamp_suffix", settings.get("auto_timestamp_suffix", True)
            ),
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
        settings = config["instruments"][kind][key]
    except (KeyError, TypeError) as e:
        raise ValueError(f"Missing instruments.{kind}.{key!r} in config.") from e
    if not isinstance(settings, dict):
        raise ValueError(f"Invalid instruments.{kind}.{key!r} config.")
    return settings


def instrument_key(
    config: dict[str, Any], kind: str, preferred_key: str | None = None
) -> str:
    instruments = config.get("instruments", {}).get(kind, {})
    if not isinstance(instruments, dict) or not instruments:
        raise ValueError(f"Missing instruments.{kind} in config.")
    if preferred_key is not None:
        key = str(preferred_key)
        if key in instruments:
            return key
        raise ValueError(f"Missing instruments.{kind}.{key!r} in config.")
    if len(instruments) == 1:
        return str(next(iter(instruments)))
    raise ValueError(f"Multiple instruments.{kind} entries found; specify the key.")


def lockin_config_for(
    config: dict[str, Any], measurement_name: str, default_key: str = "main"
) -> dict[str, Any]:
    settings = measurement_settings(config, measurement_name)
    preferred_key = settings.get("lockin_key", settings.get("lockin"))
    if preferred_key is None and default_key in config.get("instruments", {}).get(
        "lockin", {}
    ):
        preferred_key = default_key
    return instrument_config(
        config, "lockin", instrument_key(config, "lockin", preferred_key)
    )


def delay_stage_config_for(
    config: dict[str, Any], measurement_name: str, default_key: str = "t"
) -> dict[str, Any]:
    settings = measurement_settings(config, measurement_name)
    preferred_key = settings.get("delay_stage_key", settings.get("delay_stage"))
    if preferred_key is None and default_key in config.get("instruments", {}).get(
        "delay_stage", {}
    ):
        preferred_key = default_key
    return instrument_config(
        config, "delay_stage", instrument_key(config, "delay_stage", preferred_key)
    )


def scanner_config_for(
    config: dict[str, Any], axis: str, measurement_name: str = "srkr"
) -> dict[str, Any]:
    axis = axis.strip().lower()
    settings = measurement_settings(config, measurement_name)
    scanner_keys = settings.get("scanner_keys", settings.get("scanners", {}))
    preferred_key = scanner_keys.get(axis) if isinstance(scanner_keys, dict) else None
    if preferred_key is None and axis in config.get("instruments", {}).get(
        "scanner", {}
    ):
        preferred_key = axis
    return instrument_config(
        config, "scanner", instrument_key(config, "scanner", preferred_key)
    )


def output_path(settings: dict[str, Any], default_name: str) -> Path:
    output = settings.get("output", {})
    output = output if isinstance(output, dict) else {}
    output_dir = Path(
        str(
            output.get("dir")
            or output.get("output_dir")
            or settings.get("output_dir")
            or Path.cwd()
        )
    )
    base_name = str(output.get("filename") or settings.get("filename") or default_name)
    base_name = with_csv_suffix(base_name)
    filename = (
        with_auto_suffix(base_name)
        if bool(
            output.get(
                "auto_timestamp_suffix", settings.get("auto_timestamp_suffix", True)
            )
        )
        else base_name
    )
    return output_dir / filename
