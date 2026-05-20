from __future__ import annotations

import argparse
import csv
import json
import mimetypes
import threading
import traceback
import webbrowser
from collections import deque
from dataclasses import asdict, is_dataclass
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path, PurePosixPath
from typing import Any, Callable
from urllib.parse import urlparse

from kohdalab.api.config import (
    ConfigPathResolution,
    load_config as load_config_file,
    normalize_config,
    resolve_config_path,
    save_config,
    write_last_config_path,
)
from kohdalab.api.experiment import Experiment
from kohdalab.api.measurement_rows import fields_for_rows, output_rows
from kohdalab.api.scan_plan import srkr_2d_plan_from_config, srkr_plan_from_config, strkr_plan_from_config, trkr_plan_from_config
from kohdalab.apps.trkr_gui_output import build_output_path, output_settings_from_fields

DEFAULT_HOST = "127.0.0.1"
DEFAULT_PORT = 8765
MEASUREMENTS = ("signal_monitor", "trkr", "srkr", "strkr", "srkr_2d")
STATIC_ROOT = Path(__file__).with_name("web_static")


def _json_safe(value: Any) -> Any:
    if is_dataclass(value):
        return _json_safe(asdict(value))
    if isinstance(value, Path):
        return str(value)
    if isinstance(value, dict):
        return {str(key): _json_safe(item) for key, item in value.items()}
    if isinstance(value, (list, tuple)):
        return [_json_safe(item) for item in value]
    return value


def _float(value: Any, default: float = 0.0) -> float:
    if value is None or value == "":
        return float(default)
    return float(value)


def _int(value: Any, default: int = 0) -> int:
    if value is None or value == "":
        return int(default)
    return int(value)


def _measurement_settings(config: dict[str, Any], measurement: str) -> dict[str, Any]:
    settings = config.get("measurements", {}).get(measurement, {})
    return settings if isinstance(settings, dict) else {}


def _scan_settings(config: dict[str, Any], measurement: str) -> dict[str, Any]:
    scan = _measurement_settings(config, measurement).get("scan", {})
    return scan if isinstance(scan, dict) else {}


def _output_settings(config: dict[str, Any], measurement: str) -> dict[str, Any]:
    settings = _measurement_settings(config, measurement)
    output = settings.get("output", {})
    output = output if isinstance(output, dict) else {}
    return output_settings_from_fields(
        output_dir=output.get("dir", output.get("output_dir")),
        filename=output.get("filename"),
        auto_timestamp_suffix=bool(output.get("auto_timestamp_suffix", True)),
        default_dir=Path.cwd(),
        default_filename=_default_output_filename(measurement),
    )


def _default_output_filename(measurement: str) -> str:
    return {
        "signal_monitor": "signal_monitor_run",
        "trkr": "trkr_run",
        "srkr": "srkr_run",
        "strkr": "strkr_run",
        "srkr_2d": "srkr_2d_run",
    }[measurement]


def _output_from_payload(config: dict[str, Any], measurement: str, payload: dict[str, Any]) -> dict[str, Any]:
    output = payload.get("output") if isinstance(payload.get("output"), dict) else {}
    fallback = _output_settings(config, measurement)
    return output_settings_from_fields(
        output_dir=output.get("output_dir", output.get("dir", fallback["output_dir"])),
        filename=output.get("filename", fallback["filename"]),
        auto_timestamp_suffix=bool(output.get("auto_timestamp_suffix", fallback["auto_timestamp_suffix"])),
        default_dir=Path.cwd(),
        default_filename=_default_output_filename(measurement),
    )


def _ranges_from_payload(default_ranges: dict[str, Any], payload_ranges: dict[str, Any], axes: tuple[str, ...]) -> dict[str, Any]:
    ranges: dict[str, Any] = {}
    for axis in axes:
        fallback = default_ranges.get(axis, {}) if isinstance(default_ranges.get(axis), dict) else {}
        supplied = payload_ranges.get(axis, {}) if isinstance(payload_ranges.get(axis), dict) else {}
        ranges[axis] = {
            "min": _float(supplied.get("min", fallback.get("min", 0.0))),
            "max": _float(supplied.get("max", fallback.get("max", 0.0))),
            "step": _float(supplied.get("step", fallback.get("step", 1.0))),
        }
    return ranges


def _connected_from_config(config: dict[str, Any]) -> dict[str, bool]:
    connected: dict[str, bool] = {}
    for kind, devices in config.get("instruments", {}).items():
        if not isinstance(devices, dict):
            continue
        for key in devices:
            connected[f"{kind}.{key}"] = False
    return connected


def _config_defaults(config: dict[str, Any]) -> dict[str, Any]:
    measurements = config.get("measurements", {})
    signal = measurements.get("signal_monitor", {})
    trkr = measurements.get("trkr", {})
    srkr = measurements.get("srkr", {})
    strkr = measurements.get("strkr", {})
    srkr_2d = measurements.get("srkr_2d", {})
    strkr_scan = strkr.get("scan", {}) if isinstance(strkr.get("scan", {}), dict) else {}
    srkr_2d_scan = srkr_2d.get("scan", {}) if isinstance(srkr_2d.get("scan", {}), dict) else {}
    return {
        "profile": config.get("profile", {}),
        "instruments": config.get("instruments", {}),
        "measurements": {
            "signal_monitor": {
                "interval_s": signal.get("interval_s", 1.0),
                "n_points": signal.get("n_points", 360),
                "output": _output_settings(config, "signal_monitor"),
            },
            "trkr": {
                "coordinate": trkr.get("coordinate", "measurement"),
                "scan": _scan_settings(config, "trkr"),
                "wait_s": trkr.get("wait_s", 2.0),
                "return_to_zero": bool(trkr.get("return_to_zero", True)),
                "output": _output_settings(config, "trkr"),
            },
            "srkr": {
                "coordinate": srkr.get("coordinate", "measurement"),
                "scan": _scan_settings(config, "srkr"),
                "wait_s": srkr.get("wait_s", 2.0),
                "return_to_zero": bool(srkr.get("return_to_zero", True)),
                "output": _output_settings(config, "srkr"),
            },
            "strkr": {
                "scan": {
                    "fast_axis": strkr_scan.get("fast_axis", "t"),
                    "slow_axis": strkr_scan.get("slow_axis", "x"),
                    "ranges": strkr_scan.get("ranges", {}),
                },
                "wait_s": strkr.get("wait_s", 2.0),
                "return_to_zero": strkr.get("return_to_zero", {"fast_axis": True, "slow_axis": True}),
                "output": _output_settings(config, "strkr"),
            },
            "srkr_2d": {
                "scan": {
                    "fast_axis": srkr_2d_scan.get("fast_axis", "x"),
                    "slow_axis": srkr_2d_scan.get("slow_axis", "y"),
                    "ranges": srkr_2d_scan.get("ranges", {}),
                },
                "wait_s": srkr_2d.get("wait_s", 2.0),
                "return_to_zero": srkr_2d.get("return_to_zero", {"fast_axis": True, "slow_axis": True}),
                "output": _output_settings(config, "srkr_2d"),
            },
        },
    }


class WebExperimentController:
    """Single-session controller used by the remote Web UI."""

    def __init__(
        self,
        config_path: str | Path | None = None,
        *,
        experiment_factory: Callable[..., Experiment] = Experiment,
        last_config_state_path: str | Path | None = None,
        lab_default_path: str | Path | None = None,
    ) -> None:
        self._experiment_factory = experiment_factory
        self._last_config_state_path = Path(last_config_state_path) if last_config_state_path is not None else None
        self._lab_default_path = Path(lab_default_path) if lab_default_path is not None else None
        self._state_lock = threading.RLock()
        self._operation_lock = threading.Lock()
        self._stop_event = threading.Event()
        self._logs: deque[str] = deque(maxlen=500)
        self._rows: dict[str, list[dict[str, Any]]] = {name: [] for name in MEASUREMENTS}
        self._last_output_path: dict[str, str | None] = {name: None for name in MEASUREMENTS}
        self._live_status: Any = None
        self._active_operation: str | None = None
        self._job_thread: threading.Thread | None = None
        self._job: dict[str, Any] = self._empty_job()
        self.config_path: Path | None = None
        self.config_resolution = ConfigPathResolution(path=None, source="none", candidates=[])
        self.config = normalize_config({})
        self.experiment = self._experiment_factory(self.config, auto_connect=False)
        self.load_config(config_path)

    def _empty_job(self) -> dict[str, Any]:
        return {
            "running": False,
            "measurement": None,
            "status": "idle",
            "point": "-",
            "index": 0,
            "total_points": 0,
            "summary": "",
            "output_path": None,
            "error": None,
        }

    def state(self) -> dict[str, Any]:
        with self._state_lock:
            connected = self._safe_connected_devices()
            return _json_safe(
                {
                    "config_path": self.config_path,
                    "config_source": self.config_resolution.source,
                    "config_candidates": self.config_resolution.candidates,
                    "has_config": self.config_path is not None,
                    "config": self.config,
                    "defaults": _config_defaults(self.config),
                    "connected": connected,
                    "live_status": self._live_status,
                    "job": dict(self._job),
                    "active_operation": self._active_operation,
                    "rows": {name: rows[-500:] for name, rows in self._rows.items()},
                    "row_counts": {name: len(rows) for name, rows in self._rows.items()},
                    "last_output_path": dict(self._last_output_path),
                    "logs": list(self._logs),
                }
            )

    def load_config(self, path: str | Path | None = None) -> dict[str, Any]:
        with self._state_lock:
            self._ensure_idle()
            self._disconnect_quietly()
            self.config_resolution = resolve_config_path(
                path,
                last_state_path=self._last_config_state_path,
                lab_default_path=self._lab_default_path,
            )
            if self.config_resolution.path is None:
                self.config_path = None
                self.config = normalize_config({})
                self.experiment = self._experiment_factory(self.config, auto_connect=False)
                self._live_status = None
                self._log("No config loaded. Choose a config path in Session and click Load.")
                return self.state()
            self.config_path = self.config_resolution.path
            self.config = load_config_file(self.config_path)
            self.experiment = self._experiment_factory(self.config, auto_connect=False)
            self._remember_config_path(self.config_path)
            self._live_status = None
            self._log(f"Loaded config ({self.config_resolution.source}): {self.config_path}")
            return self.state()

    def save_config(self, path: str | Path | None = None, config: dict[str, Any] | None = None) -> dict[str, Any]:
        with self._state_lock:
            self._ensure_idle()
            target = Path(path) if path else self.config_path
            if target is None:
                raise ValueError("Choose a config path before saving.")
            if config is not None:
                self._disconnect_quietly()
                self.config = normalize_config(config, source=target)
                self.experiment = self._experiment_factory(self.config, auto_connect=False)
            save_config(self.config, target)
            self.config_path = target
            self._remember_config_path(target)
            self._log(f"Saved config: {target}")
            return self.state()

    def connect_all(self) -> dict[str, Any]:
        return self._run_exclusive("connect_all", self.experiment.connect_all)

    def disconnect_all(self) -> dict[str, Any]:
        return self._run_exclusive("disconnect_all", self.experiment.disconnect_all)

    def connect_device(self, ref: str) -> dict[str, Any]:
        return self._run_exclusive(f"connect {ref}", lambda: self.experiment.connect_device(ref))

    def disconnect_device(self, ref: str) -> dict[str, Any]:
        return self._run_exclusive(f"disconnect {ref}", lambda: self.experiment.disconnect_device(ref))

    def initialize_delay_stage(self, ref: str = "delay_stage.t") -> dict[str, Any]:
        return self._run_exclusive(
            f"initialize {ref}",
            lambda: self.experiment.initialize_delay_stage(ref, on_status=self._status_callback),
        )

    def initialize_scanner(self, axis: str, ref: str | None = None) -> dict[str, Any]:
        axis = axis.strip().lower()
        return self._run_exclusive(
            f"initialize scanner {axis}",
            lambda: self.experiment.initialize_scanner(axis, ref or f"scanner.{axis}", on_status=self._status_callback),
        )

    def read_live_status(self) -> dict[str, Any]:
        def read() -> None:
            status = self.experiment.read_live_status()
            with self._state_lock:
                self._live_status = status

        return self._run_exclusive("read live", read)

    def move_abs(self, axis: str, value: float, coordinate: str = "measurement") -> dict[str, Any]:
        axis = axis.strip().lower()
        coordinate = str(coordinate or "measurement")

        def move() -> None:
            if axis == "t":
                position = self.experiment.move_delay_stage(value, coordinate=coordinate, on_status=self._status_callback)
            elif axis in {"x", "y"}:
                position = self.experiment.move_scanner(axis, value, coordinate=coordinate, on_status=self._status_callback)
            else:
                raise ValueError("axis must be one of 't', 'x', or 'y'.")
            with self._state_lock:
                self._live_status = {"connected": self._safe_connected_devices(), "position": position}

        return self._run_exclusive(f"move {axis}", move)

    def start_measurement(self, payload: dict[str, Any]) -> dict[str, Any]:
        measurement = str(payload.get("measurement", "")).strip().lower()
        if measurement not in MEASUREMENTS:
            raise ValueError(f"Unsupported measurement: {measurement}")
        with self._state_lock:
            self._ensure_idle()
            if not self._operation_lock.acquire(blocking=False):
                raise RuntimeError("Wait for the active device operation to finish first.")
            try:
                run_kwargs, missing, summary, output_path = self._measurement_run_kwargs(measurement, payload)
                if missing:
                    raise RuntimeError("Connect required devices before starting: " + ", ".join(missing))
                self._rows[measurement] = []
                self._stop_event.clear()
                self._job = {
                    "running": True,
                    "measurement": measurement,
                    "status": "starting",
                    "point": "-",
                    "index": 0,
                    "total_points": 0,
                    "summary": summary,
                    "output_path": str(output_path),
                    "error": None,
                }
                self._active_operation = f"run {measurement}"
                self._log(f"Started {summary} -> {output_path}")
                thread = threading.Thread(
                    target=self._measurement_thread,
                    args=(measurement, run_kwargs, output_path),
                    name=f"kohdalab-web-{measurement}",
                    daemon=True,
                )
                self._job_thread = thread
                thread.start()
            except Exception:
                self._operation_lock.release()
                self._active_operation = None
                raise
            return self.state()

    def stop_measurement(self) -> dict[str, Any]:
        with self._state_lock:
            if self._job.get("running"):
                self._stop_event.set()
                self._job["status"] = "stopping"
                self._log("Stop requested.")
            return self.state()

    def save_rows(self, measurement: str, output_payload: dict[str, Any] | None = None) -> dict[str, Any]:
        measurement = measurement.strip().lower()
        if measurement not in MEASUREMENTS:
            raise ValueError(f"Unsupported measurement: {measurement}")
        with self._state_lock:
            rows = list(self._rows[measurement])
        if not rows:
            raise RuntimeError("No rows to save.")
        output = _output_from_payload(self.config, measurement, {"output": output_payload or {}})
        path = build_output_path(output)
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open("w", newline="", encoding="utf-8") as f:
            writer = csv.DictWriter(f, fieldnames=fields_for_rows(rows))
            writer.writeheader()
            writer.writerows(output_rows(rows))
        with self._state_lock:
            self._last_output_path[measurement] = str(path)
            self._log(f"Saved {len(rows)} rows to {path}")
            return self.state()

    def wait_for_idle(self, timeout_s: float = 5.0) -> bool:
        thread = self._job_thread
        if thread is None:
            return True
        thread.join(timeout_s)
        return not thread.is_alive()

    def _measurement_run_kwargs(
        self,
        measurement: str,
        payload: dict[str, Any],
    ) -> tuple[dict[str, Any], list[str], str, Path]:
        output = _output_from_payload(self.config, measurement, payload)
        output_path = build_output_path(output)
        if measurement == "signal_monitor":
            settings = payload.get("settings") if isinstance(payload.get("settings"), dict) else {}
            interval_s = _float(settings.get("interval_s", _measurement_settings(self.config, "signal_monitor").get("interval_s", 1.0)), 1.0)
            n_points = _int(settings.get("n_points", _measurement_settings(self.config, "signal_monitor").get("n_points", 360)), 360)
            return (
                {
                    "interval_s": interval_s,
                    "n_points": n_points,
                    "output": output_path,
                    "on_status": self._status_callback,
                    "on_point": self._point_callback(measurement),
                    "should_continue": self._should_continue,
                },
                self.experiment.missing_devices("signal_monitor"),
                f"Signal Monitor: {n_points} points, dt={interval_s:g} s",
                output_path,
            )
        if measurement == "trkr":
            settings = payload.get("settings") if isinstance(payload.get("settings"), dict) else {}
            scan = settings.get("scan", {}) if isinstance(settings.get("scan", {}), dict) else {}
            default_scan = _scan_settings(self.config, "trkr")
            plan = trkr_plan_from_config(
                self.config,
                minimum_ps=_float(scan.get("min", default_scan.get("min", -50.0))),
                maximum_ps=_float(scan.get("max", default_scan.get("max", 300.0))),
                step_ps=_float(scan.get("step", default_scan.get("step", 5.0))),
                coordinate=str(settings.get("coordinate", _measurement_settings(self.config, "trkr").get("coordinate", "measurement"))),
            )
            return (
                self._run_kwargs_for_scan(
                    measurement,
                    {
                        "plan": plan,
                        "wait_s": _float(settings.get("wait_s", _measurement_settings(self.config, "trkr").get("wait_s", 2.0)), 2.0),
                        "return_to_zero": bool(settings.get("return_to_zero", _measurement_settings(self.config, "trkr").get("return_to_zero", True))),
                        "output": output_path,
                    },
                ),
                self.experiment.missing_devices("trkr"),
                f"TRKR: {plan.summary}",
                output_path,
            )
        if measurement == "srkr":
            settings = payload.get("settings") if isinstance(payload.get("settings"), dict) else {}
            scan = settings.get("scan", {}) if isinstance(settings.get("scan", {}), dict) else {}
            default_scan = _scan_settings(self.config, "srkr")
            axis = str(scan.get("axis", default_scan.get("axis", "x"))).lower()
            plan = srkr_plan_from_config(
                self.config,
                axis=axis,
                minimum_um=_float(scan.get("min", default_scan.get("min", -30.0))),
                maximum_um=_float(scan.get("max", default_scan.get("max", 30.0))),
                step_um=_float(scan.get("step", default_scan.get("step", 1.0))),
                coordinate=str(settings.get("coordinate", _measurement_settings(self.config, "srkr").get("coordinate", "measurement"))),
            )
            return (
                self._run_kwargs_for_scan(
                    measurement,
                    {
                        "plan": plan,
                        "axis": axis,
                        "wait_s": _float(settings.get("wait_s", _measurement_settings(self.config, "srkr").get("wait_s", 2.0)), 2.0),
                        "return_to_zero": bool(settings.get("return_to_zero", _measurement_settings(self.config, "srkr").get("return_to_zero", True))),
                        "output": output_path,
                    },
                ),
                self.experiment.missing_devices("srkr", axis=axis),
                f"SRKR: {plan.summary}",
                output_path,
            )
        if measurement == "strkr":
            settings = payload.get("settings") if isinstance(payload.get("settings"), dict) else {}
            scan = settings.get("scan", {}) if isinstance(settings.get("scan", {}), dict) else {}
            default_scan = _scan_settings(self.config, "strkr")
            default_ranges = default_scan.get("ranges", {}) if isinstance(default_scan.get("ranges", {}), dict) else {}
            fast_axis = str(scan.get("fast_axis", default_scan.get("fast_axis", "t"))).lower()
            slow_axis = str(scan.get("slow_axis", default_scan.get("slow_axis", "x"))).lower()
            return_to_zero = settings.get("return_to_zero", _measurement_settings(self.config, "strkr").get("return_to_zero"))
            plan = strkr_plan_from_config(
                self.config,
                fast_axis=fast_axis,
                slow_axis=slow_axis,
                ranges=_ranges_from_payload(default_ranges, scan.get("ranges", {}) if isinstance(scan.get("ranges", {}), dict) else {}, ("t", "x", "y")),
                return_to_zero=return_to_zero,
            )
            return (
                self._run_kwargs_for_scan(
                    measurement,
                    {
                        "plan": plan,
                        "wait_s": _float(settings.get("wait_s", _measurement_settings(self.config, "strkr").get("wait_s", 2.0)), 2.0),
                        "output": output_path,
                    },
                ),
                self.experiment.missing_devices("strkr", fast_axis=fast_axis, slow_axis=slow_axis),
                plan.summary,
                output_path,
            )
        settings = payload.get("settings") if isinstance(payload.get("settings"), dict) else {}
        scan = settings.get("scan", {}) if isinstance(settings.get("scan", {}), dict) else {}
        default_scan = _scan_settings(self.config, "srkr_2d")
        default_ranges = default_scan.get("ranges", {}) if isinstance(default_scan.get("ranges", {}), dict) else {}
        fast_axis = str(scan.get("fast_axis", default_scan.get("fast_axis", "x"))).lower()
        slow_axis = str(scan.get("slow_axis", default_scan.get("slow_axis", "y"))).lower()
        return_to_zero = settings.get("return_to_zero", _measurement_settings(self.config, "srkr_2d").get("return_to_zero"))
        plan = srkr_2d_plan_from_config(
            self.config,
            fast_axis=fast_axis,
            slow_axis=slow_axis,
            ranges=_ranges_from_payload(default_ranges, scan.get("ranges", {}) if isinstance(scan.get("ranges", {}), dict) else {}, ("x", "y")),
            return_to_zero=return_to_zero,
        )
        return (
            self._run_kwargs_for_scan(
                measurement,
                {
                    "plan": plan,
                    "wait_s": _float(settings.get("wait_s", _measurement_settings(self.config, "srkr_2d").get("wait_s", 2.0)), 2.0),
                    "output": output_path,
                },
            ),
            self.experiment.missing_devices("srkr_2d", fast_axis=fast_axis, slow_axis=slow_axis),
            plan.summary,
            output_path,
        )

    def _run_kwargs_for_scan(self, measurement: str, kwargs: dict[str, Any]) -> dict[str, Any]:
        kwargs.update(
            {
                "on_status": self._status_callback,
                "on_point": self._point_callback(measurement),
                "should_continue": self._should_continue,
            }
        )
        return kwargs

    def _measurement_thread(self, measurement: str, kwargs: dict[str, Any], output_path: Path) -> None:
        try:
            if measurement == "signal_monitor":
                self.experiment.run_signal_monitor(**kwargs)
            elif measurement == "trkr":
                self.experiment.run_trkr(**kwargs)
            elif measurement == "srkr":
                self.experiment.run_srkr(**kwargs)
            elif measurement == "strkr":
                self.experiment.run_strkr(**kwargs)
            elif measurement == "srkr_2d":
                self.experiment.run_srkr_2d(**kwargs)
            with self._state_lock:
                self._last_output_path[measurement] = str(output_path)
                self._job["status"] = "stopped" if self._stop_event.is_set() else "completed"
                self._log(f"{measurement} finished -> {output_path}")
        except Exception as e:
            with self._state_lock:
                self._job["status"] = "error"
                self._job["error"] = str(e)
                self._log(f"{measurement} error: {e}")
                self._log(traceback.format_exc().strip())
        finally:
            with self._state_lock:
                self._job["running"] = False
                self._active_operation = None
                self._stop_event.clear()
            self._operation_lock.release()

    def _point_callback(self, measurement: str):
        def callback(point: Any) -> None:
            row = dict(getattr(point, "row", point))
            index = int(getattr(point, "index", 0) or 0)
            total = int(getattr(point, "total_points", 0) or 0)
            with self._state_lock:
                self._rows[measurement].append(row)
                self._job["index"] = index
                self._job["total_points"] = total
                self._job["point"] = f"{index}/{total}" if total else str(index)

        return callback

    def _status_callback(self, status: str) -> None:
        with self._state_lock:
            if self._job.get("running"):
                self._job["status"] = status
            self._log(f"status: {status}")

    def _should_continue(self) -> bool:
        return not self._stop_event.is_set()

    def _run_exclusive(self, label: str, func: Callable[[], Any]) -> dict[str, Any]:
        with self._state_lock:
            self._ensure_no_job()
        if not self._operation_lock.acquire(blocking=False):
            raise RuntimeError("Wait for the active device operation to finish first.")
        with self._state_lock:
            self._active_operation = label
            self._log(f"Started {label}.")
        try:
            func()
            with self._state_lock:
                self._log(f"Finished {label}.")
                return self.state()
        except Exception as e:
            with self._state_lock:
                self._log(f"{label} error: {e}")
            raise
        finally:
            with self._state_lock:
                self._active_operation = None
            self._operation_lock.release()

    def _ensure_no_job(self) -> None:
        if self._job.get("running"):
            raise RuntimeError("Wait for the active measurement to finish first.")

    def _ensure_idle(self) -> None:
        self._ensure_no_job()
        if self._operation_lock.locked():
            raise RuntimeError("Wait for the active device operation to finish first.")

    def _safe_connected_devices(self) -> dict[str, bool]:
        try:
            return self.experiment.connected_devices()
        except Exception:
            return _connected_from_config(self.config)

    def _disconnect_quietly(self) -> None:
        try:
            self.experiment.disconnect_all()
        except Exception:
            pass

    def _remember_config_path(self, path: Path) -> None:
        try:
            write_last_config_path(path, self._last_config_state_path)
        except OSError as e:
            self._log(f"Could not save last config path: {e}")

    def _log(self, message: str) -> None:
        self._logs.append(message)


class WebUiServer(ThreadingHTTPServer):
    def __init__(self, server_address: tuple[str, int], controller: WebExperimentController):
        super().__init__(server_address, WebUiRequestHandler)
        self.controller = controller


class WebUiRequestHandler(BaseHTTPRequestHandler):
    server: WebUiServer

    def do_GET(self) -> None:
        parsed = urlparse(self.path)
        if parsed.path == "/":
            self._send_static("index.html")
            return
        if parsed.path.startswith("/assets/"):
            self._send_static(parsed.path.removeprefix("/assets/"))
            return
        if parsed.path == "/api/state":
            self._send_json(self.server.controller.state())
            return
        self.send_error(HTTPStatus.NOT_FOUND)

    def do_POST(self) -> None:
        parsed = urlparse(self.path)
        payload = self._read_json()
        try:
            if parsed.path == "/api/config/load":
                result = self.server.controller.load_config(payload.get("path") or None)
            elif parsed.path == "/api/config/save":
                result = self.server.controller.save_config(payload.get("path"), payload.get("config"))
            elif parsed.path == "/api/devices/connect-all":
                result = self.server.controller.connect_all()
            elif parsed.path == "/api/devices/disconnect-all":
                result = self.server.controller.disconnect_all()
            elif parsed.path == "/api/devices/connect":
                result = self.server.controller.connect_device(str(payload["ref"]))
            elif parsed.path == "/api/devices/disconnect":
                result = self.server.controller.disconnect_device(str(payload["ref"]))
            elif parsed.path == "/api/devices/initialize-delay-stage":
                result = self.server.controller.initialize_delay_stage(str(payload.get("ref") or "delay_stage.t"))
            elif parsed.path == "/api/devices/initialize-scanner":
                result = self.server.controller.initialize_scanner(str(payload["axis"]), payload.get("ref"))
            elif parsed.path == "/api/live/read":
                result = self.server.controller.read_live_status()
            elif parsed.path == "/api/move":
                result = self.server.controller.move_abs(
                    str(payload["axis"]),
                    _float(payload["value"]),
                    str(payload.get("coordinate") or "measurement"),
                )
            elif parsed.path == "/api/measurements/start":
                result = self.server.controller.start_measurement(payload)
            elif parsed.path == "/api/measurements/stop":
                result = self.server.controller.stop_measurement()
            elif parsed.path == "/api/measurements/save":
                result = self.server.controller.save_rows(str(payload["measurement"]), payload.get("output"))
            else:
                self.send_error(HTTPStatus.NOT_FOUND)
                return
            self._send_json(result)
        except Exception as e:
            self._send_json({"error": str(e), "state": self.server.controller.state()}, status=HTTPStatus.BAD_REQUEST)

    def log_message(self, _format: str, *args: Any) -> None:
        return

    def _read_json(self) -> dict[str, Any]:
        length = int(self.headers.get("Content-Length", "0") or 0)
        if length <= 0:
            return {}
        raw = self.rfile.read(length)
        return json.loads(raw.decode("utf-8"))

    def _send_json(self, data: Any, *, status: HTTPStatus = HTTPStatus.OK) -> None:
        raw = json.dumps(_json_safe(data), ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def _send_static(self, relative: str) -> None:
        path = _safe_static_path(relative)
        if path is None or not path.exists() or not path.is_file():
            self.send_error(HTTPStatus.NOT_FOUND)
            return
        mime = mimetypes.guess_type(path.name)[0] or "application/octet-stream"
        raw = path.read_bytes()
        self.send_response(HTTPStatus.OK)
        self.send_header("Content-Type", mime)
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)


def _safe_static_path(relative: str) -> Path | None:
    parts = PurePosixPath("/" + relative).parts[1:]
    if not parts or any(part in {"", ".", ".."} for part in parts):
        return None
    return STATIC_ROOT.joinpath(*parts)


def run_server(
    *,
    host: str = DEFAULT_HOST,
    port: int = DEFAULT_PORT,
    config_path: str | Path | None = None,
    open_browser: bool = False,
) -> None:
    controller = WebExperimentController(config_path)
    server = WebUiServer((host, int(port)), controller)
    url = f"http://{host}:{port}"
    print(f"KohdaLab Web UI: {url}", flush=True)
    print("Use SSH tunnel/VPN for remote access; avoid exposing this server to the public internet.", flush=True)
    if open_browser:
        webbrowser.open(url)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("Stopping KohdaLab Web UI.", flush=True)
    finally:
        controller._disconnect_quietly()
        server.server_close()


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Run the KohdaLab remote Web UI.")
    parser.add_argument(
        "--config",
        type=Path,
        default=None,
        help=(
            "Config JSON path. If omitted, kohdalab-web uses KOHDALAB_CONFIG, "
            "then the last loaded config, then the lab default if it exists."
        ),
    )
    parser.add_argument("--host", default=DEFAULT_HOST, help="Bind host. Default: 127.0.0.1 for SSH tunnel/local use.")
    parser.add_argument("--port", type=int, default=DEFAULT_PORT, help=f"Bind port. Default: {DEFAULT_PORT}")
    parser.add_argument("--open", action="store_true", help="Open the Web UI in the default browser.")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    run_server(host=args.host, port=args.port, config_path=args.config, open_browser=args.open)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
