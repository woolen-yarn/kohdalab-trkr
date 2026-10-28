"""Thin JSON-lines client for the Rust portable TRKR controller.

All measurement, hardware, and safety logic stays in the native process.
"""

from __future__ import annotations

import json
import math
import os
import queue
import shutil
import socket
import subprocess
import sys
import threading
import time
import uuid
import weakref
from pathlib import Path
from typing import Any, Iterator


_PLATFORM = sys.platform

# Profiles and JSON-lines remain revision 1. Recording revision 5 separates
# analysis CSV, raw CSV, and run metadata with UUIDs and file checksums.
# Legacy CSV/JSONL recordings remain readable.
PROFILE_COMPATIBILITY_REVISION = 1
JSON_LINES_API_REVISION = 1
RECORDING_COMPATIBILITY_REVISION = 5


class GuiError(RuntimeError):
    """GUI API failure with a stable code and optional last job receipt."""
    def __init__(self, message: str, code: str | None = None,
                 receipt: dict[str, Any] | None = None) -> None:
        super().__init__(message)
        self.code = code
        self.receipt = receipt
        self.stop_error: Exception | None = None


class GuiTransportError(GuiError):
    """Communication failed; the outcome of an action may be unknown."""


class GuiOwnershipError(GuiError):
    """Exclusive control is absent, expired, or revoked."""


class GuiDeviceError(GuiError):
    """A device operation failed."""


class GuiProtectionError(GuiDeviceError):
    """An explicit device protection condition prevented an operation."""


class GuiSaveError(GuiError):
    """Recording creation, writing, or finalization failed."""


class GuiInitializationError(GuiDeviceError):
    """Device initialization failed."""


class GuiRunStopped(GuiError):
    """Raised when a GUI-managed run was stopped before completion."""


class GuiTimeoutError(GuiError, TimeoutError):
    """The wait expired; receipt describes the last observed state only."""


_ERROR_TYPES = {
    "transport": GuiTransportError, "protocol": GuiTransportError,
    "ownership_lost": GuiOwnershipError,
    "device_error": GuiDeviceError, "protection": GuiProtectionError,
    "save_failed": GuiSaveError, "initialization_failed": GuiInitializationError,
    "stopped": GuiRunStopped, "timeout": GuiTimeoutError,
}


def _gui_error(message: str, code: str | None = None,
               receipt: dict[str, Any] | None = None) -> GuiError:
    return _ERROR_TYPES.get(code, GuiError)(message, code, receipt)


class GuiClient:
    """Control one GUI-owned TRKR session through its loopback API.

    Hardware, profiles, and measurements remain owned by the GUI.  A short
    lease prevents a notebook and a person from changing that shared state at
    the same time. By default closing releases control and lets work continue.
    Optional stop conditions never close the GUI.
    """

    _RUN_STATES = {"running", "completed", "stopped", "error"}
    _MAX_RESPONSE_BYTES = 1_048_576

    def __init__(
        self,
        session: str | Path | None = None,
        *,
        timeout: float = 5.0,
        stop_on_disconnect: bool = False,
        stop_on_error: bool = False,
        stop_on_timeout: bool = False,
    ) -> None:
        if not math.isfinite(timeout) or timeout <= 0:
            raise ValueError("timeout must be greater than zero")
        self._stop_policy = {
            "stop_on_disconnect": stop_on_disconnect,
            "stop_on_error": stop_on_error,
            "stop_on_timeout": stop_on_timeout,
        }
        for name, value in self._stop_policy.items():
            if not isinstance(value, bool):
                raise TypeError(f"{name} must be bool")
        self._session_path = None if session is None else Path(session).expanduser()
        self._timeout = float(timeout)
        self._descriptor: dict[str, Any] | None = None
        self._closed = False
        self._client_id = str(uuid.uuid4())
        self._owns = False
        self._acquire_attempted = False
        self._ownership_lost = False
        self._heartbeat_stop = threading.Event()
        self._heartbeat_thread: threading.Thread | None = None
        self._heartbeat_finalizer = weakref.finalize(
            self, threading.Event.set, self._heartbeat_stop
        )

    @staticmethod
    def _validate_descriptor(data: object, path: Path) -> dict[str, Any]:
        if not isinstance(data, dict):
            raise ValueError(f"Invalid GUI session descriptor: {path}")
        if data.get("revision") != 1:
            raise ValueError(f"Unsupported GUI session revision in {path}")
        host = data.get("host")
        port = data.get("port")
        token = data.get("token")
        instance_id = data.get("instance_id")
        pid = data.get("pid")
        if host != "127.0.0.1":
            raise ValueError("GUI session host must be 127.0.0.1")
        if not isinstance(port, int) or isinstance(port, bool) or not 1 <= port <= 65535:
            raise ValueError("GUI session port must be between 1 and 65535")
        if not all(isinstance(value, str) and value for value in (token, instance_id)):
            raise ValueError("GUI session token and instance_id must be non-empty strings")
        if not isinstance(pid, int) or isinstance(pid, bool) or pid <= 0:
            raise ValueError("GUI session pid must be a positive integer")
        return data

    @classmethod
    def _load_descriptor(cls, path: Path) -> dict[str, Any]:
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            raise ValueError(f"Could not read GUI session descriptor: {path}") from exc
        return cls._validate_descriptor(data, path)

    def _request_to(self, descriptor: dict[str, Any], operation: str, **data: Any) -> Any:
        payload = {
            "token": descriptor["token"], "action": operation,
            "client_id": self._client_id, **data,
        }
        encoded = (json.dumps(payload, separators=(",", ":")) + "\n").encode("utf-8")
        try:
            with socket.create_connection(
                (descriptor["host"], descriptor["port"]), timeout=self._timeout
            ) as connection:
                connection.settimeout(self._timeout)
                connection.sendall(encoded)
                response = bytearray()
                while b"\n" not in response:
                    chunk = connection.recv(4096)
                    if not chunk:
                        raise GuiTransportError("GUI API closed before returning a response", "transport")
                    response.extend(chunk)
                    if len(response) > self._MAX_RESPONSE_BYTES:
                        raise GuiTransportError("GUI API response is too large", "protocol")
        except OSError as exc:
            raise GuiTransportError("Could not connect to the running TRKR GUI", "transport") from exc
        try:
            decoded = json.loads(response.split(b"\n", 1)[0].decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise GuiTransportError("Invalid JSON response from TRKR GUI", "protocol") from exc
        if not isinstance(decoded, dict) or not isinstance(decoded.get("ok"), bool):
            raise GuiTransportError("Invalid response envelope from TRKR GUI", "protocol")
        if not decoded["ok"]:
            raise _gui_error(str(decoded.get("error", "TRKR GUI rejected request")), decoded.get("error_code"))
        if "result" not in decoded:
            raise GuiTransportError("TRKR GUI response omitted result", "protocol")
        return decoded["result"]

    def _discover(self) -> dict[str, Any]:
        if self._session_path is not None:
            return self._load_descriptor(self._session_path)
        directory = Path.home() / ".kohdalab" / "run"
        candidates: list[dict[str, Any]] = []
        for path in sorted(directory.glob("trkr-gui-*.json")) if directory.is_dir() else []:
            try:
                descriptor = self._load_descriptor(path)
                self._request_to(descriptor, "status")
            except (RuntimeError, ValueError):
                # A GUI can remove its descriptor between glob and ping.  It is stale,
                # not a reason to select another running GUI arbitrarily.
                continue
            candidates.append(descriptor)
        if not candidates:
            raise GuiTransportError("No running TRKR GUI API session was found", "transport")
        if len(candidates) != 1:
            raise RuntimeError("Multiple running TRKR GUI API sessions were found; pass session=")
        return candidates[0]

    def _request(self, operation: str, **data: Any) -> Any:
        if self._closed:
            raise RuntimeError("GuiClient is closed")
        if self._descriptor is None:
            self._descriptor = self._discover()
        return self._request_to(self._descriptor, operation, **data)

    @classmethod
    def _receipt(cls, value: Any) -> dict[str, Any]:
        if not isinstance(value, dict) or not isinstance(value.get("id"), str):
            raise RuntimeError("Invalid job receipt from TRKR GUI")
        status = value.get("status")
        if status not in cls._RUN_STATES:
            raise RuntimeError("Invalid job status from TRKR GUI")
        if not {"rows", "output", "error", "kind"} <= value.keys():
            raise RuntimeError("Incomplete job receipt from TRKR GUI")
        return value

    @staticmethod
    def _control(value: Any) -> dict[str, Any]:
        if not isinstance(value, dict):
            raise RuntimeError("Invalid control result from TRKR GUI")
        if not isinstance(value.get("owned"), bool):
            raise RuntimeError("Invalid control ownership from TRKR GUI")
        if value.get("status") not in {"idle", "python", "detached"}:
            raise RuntimeError("Invalid control status from TRKR GUI")
        if value.get("client_id") is not None and not isinstance(value["client_id"], str):
            raise RuntimeError("Invalid control client_id from TRKR GUI")
        if value.get("active_run") is not None:
            GuiClient._receipt(value["active_run"])
        return value

    @staticmethod
    def _uuid(value: str | None, name: str = "request_id") -> str:
        if value is None:
            return str(uuid.uuid4())
        if not isinstance(value, str):
            raise ValueError(f"{name} must be a UUID string or None")
        try:
            uuid.UUID(value)
        except ValueError as exc:
            raise ValueError(f"{name} must be a UUID string or None") from exc
        return value

    def _start_heartbeat(self) -> None:
        if self._heartbeat_thread is not None and self._heartbeat_thread.is_alive():
            return
        self._heartbeat_stop.clear()
        client_ref = weakref.ref(self)
        stop_event = self._heartbeat_stop

        def keep_alive() -> None:
            while not stop_event.wait(1.0):
                client = client_ref()
                if client is None:
                    return
                try:
                    result = client._control(client._request("heartbeat"))
                    if not result["owned"]:
                        client._ownership_lost = True
                        client._owns = False
                        return
                except RuntimeError:
                    client._ownership_lost = True
                    client._owns = False
                    return
                finally:
                    del client

        self._heartbeat_thread = threading.Thread(
            target=keep_alive, name="trkr-gui-heartbeat", daemon=True
        )
        self._heartbeat_thread.start()

    def _stop_heartbeat(self) -> None:
        self._heartbeat_stop.set()
        thread = self._heartbeat_thread
        if thread is not None and thread is not threading.current_thread():
            thread.join(timeout=1.5)
        self._heartbeat_thread = None

    def acquire(self, label: str | None = None) -> dict[str, Any]:
        """Acquire the six-second GUI control lease and keep it alive."""
        if label is not None and (not isinstance(label, str) or len(label) > 48):
            raise ValueError("label must be a string of at most 48 characters or None")
        if self._ownership_lost:
            raise GuiOwnershipError("GUI control was revoked; create a new GuiClient to retry", "ownership_lost")
        payload = {} if label is None else {"label": label}
        result = self._control(self._request("acquire", **payload, **self._stop_policy))
        self._acquire_attempted = True
        self._owns = result["owned"]
        if self._owns and any(self._stop_policy.values()) and result.get("stop_policy") != self._stop_policy:
            try:
                self.release()
            finally:
                raise GuiError("This GUI does not confirm the requested stop policy; update the GUI before starting", "unsupported_policy")
        if self._owns:
            self._start_heartbeat()
        return result

    def heartbeat(self) -> dict[str, Any]:
        """Refresh a lease immediately; normal use refreshes it every second."""
        try:
            result = self._control(self._request("heartbeat"))
        except RuntimeError:
            self._ownership_lost = True
            self._owns = False
            raise
        self._owns = result["owned"]
        if not self._owns:
            self._ownership_lost = True
        return result

    def release(self, *, _reason: str = "disconnect") -> dict[str, Any] | None:
        """Release control; apply the explicitly selected stop conditions."""
        if not self._owns:
            return None
        self._stop_heartbeat()
        try:
            result = self._control(self._request("release", reason=_reason))
        finally:
            self._owns = False
        return result

    def _ensure_control(self) -> None:
        if self._ownership_lost:
            raise GuiOwnershipError("GUI control was revoked; refusing to change shared state", "ownership_lost")
        if self._owns:
            return
        if self._acquire_attempted:
            raise GuiOwnershipError("GUI control is not held", "ownership_lost")
        result = self.acquire()
        if not result["owned"]:
            raise GuiOwnershipError("TRKR GUI is controlled by another client", "ownership_lost")

    def _job(self, operation: str, **payload: Any) -> dict[str, Any]:
        self._ensure_control()
        payload["request_id"] = self._uuid(payload.get("request_id"))
        return self._receipt(self._request(operation, **payload))

    def status(self) -> dict[str, Any]:
        """Return the GUI snapshot and its active run, if any."""
        result = self._request("status")
        if not isinstance(result, dict) or "snapshot" not in result or "active_run" not in result:
            raise RuntimeError("Invalid status result from TRKR GUI")
        active = result["active_run"]
        if active is not None:
            self._receipt(active)
        return result

    def plan(self, workflow: str | None = None) -> dict[str, Any]:
        payload = {} if workflow is None else {"workflow": workflow}
        result = self._request("plan", **payload)
        if not isinstance(result, dict) or not {"plan", "workflow", "source"} <= result.keys():
            raise RuntimeError("Invalid plan result from TRKR GUI")
        return result

    def config(self) -> dict[str, Any]:
        result = self._request("config")
        if not isinstance(result, dict) or not {"profile", "workflow", "source", "config_path"} <= result.keys():
            raise RuntimeError("Invalid config result from TRKR GUI")
        return result

    def load_config(self, path: str | Path) -> dict[str, Any]:
        self._ensure_control()
        return self._request("load_config", path=str(Path(path).expanduser().resolve()))

    def configure(self, profile: dict[str, Any] | None = None, *, source: str | None = None) -> dict[str, Any]:
        if profile is not None and not isinstance(profile, dict):
            raise TypeError("profile must be a dictionary or None")
        if source is not None and source not in {"demo", "hardware"}:
            raise ValueError("source must be 'demo', 'hardware', or None")
        payload: dict[str, Any] = {}
        if profile is not None:
            payload["profile"] = profile
        if source is not None:
            payload["source"] = source
        self._ensure_control()
        return self._request("configure", **payload)

    def set_plan(self, plan: dict[str, Any], workflow: str | None = None) -> dict[str, Any]:
        if not isinstance(plan, dict):
            raise TypeError("plan must be a dictionary")
        payload: dict[str, Any] = {"plan": plan}
        if workflow is not None:
            payload["workflow"] = workflow
        self._ensure_control()
        return self._request("set_plan", **payload)

    def output(self, workflow: str | None = None) -> dict[str, Any]:
        payload = {} if workflow is None else {"workflow": workflow}
        result = self._request("output", **payload)
        if not isinstance(result, dict) or not {"dir", "filename", "auto_timestamp_suffix"} <= result.keys():
            raise RuntimeError("Invalid output result from TRKR GUI")
        return result

    def set_output(self, *, directory: str | Path | None = None, filename: str | None = None,
                   auto_timestamp_suffix: bool | None = None, workflow: str | None = None) -> dict[str, Any]:
        payload: dict[str, Any] = {}
        if directory is not None:
            payload["dir"] = str(Path(directory).expanduser().resolve())
        if filename is not None:
            if not isinstance(filename, str):
                raise TypeError("filename must be a string or None")
            payload["filename"] = filename
        if auto_timestamp_suffix is not None:
            if not isinstance(auto_timestamp_suffix, bool):
                raise TypeError("auto_timestamp_suffix must be a bool or None")
            payload["auto_timestamp_suffix"] = auto_timestamp_suffix
        if workflow is not None:
            payload["workflow"] = workflow
        self._ensure_control()
        return self._request("set_output", **payload)

    def start(
        self,
        workflow: str | None = None,
        *,
        plan: dict[str, Any] | None = None,
        output: str | Path | None = None,
        request_id: str | None = None,
    ) -> dict[str, Any]:
        if workflow is not None and not isinstance(workflow, str):
            raise TypeError("workflow must be a string or None")
        if plan is not None and not isinstance(plan, dict):
            raise TypeError("plan must be a dictionary or None")
        payload: dict[str, Any] = {"request_id": request_id}
        if workflow is not None:
            payload["workflow"] = workflow
        if plan is not None:
            payload["plan"] = plan
        if output is not None:
            payload["output"] = str(Path(output).expanduser().resolve())
        return self._job("start", **payload)

    def result(self, run_id: str) -> dict[str, Any]:
        if not isinstance(run_id, str) or not run_id:
            raise ValueError("run_id must be a non-empty string")
        return self._receipt(self._request("result", run_id=run_id))

    def stop(self, run_id: str) -> dict[str, Any]:
        """Request Stop; use stop_and_wait to verify the terminal outcome."""
        if not isinstance(run_id, str) or not run_id:
            raise ValueError("run_id must be a non-empty string")
        return self._job("stop", run_id=run_id)

    def move(self, targets: list[float | None], *, coordinate: str = "measurement",
             request_id: str | None = None) -> dict[str, Any]:
        if not isinstance(targets, list) or len(targets) != 3 or any(
            value is not None and (isinstance(value, bool) or not isinstance(value, (int, float)))
            for value in targets
        ):
            raise ValueError("targets must be [t_ps, x_um, y_um] with numbers or None")
        if not isinstance(coordinate, str):
            raise TypeError("coordinate must be a string")
        return self._job("move", targets=targets, coordinate=coordinate, request_id=request_id)

    def initialize(self, axis: int, *, home: bool = False, move_to_origin: bool = True,
                   request_id: str | None = None) -> dict[str, Any]:
        if isinstance(axis, bool) or not isinstance(axis, int) or axis not in {0, 1, 2}:
            raise ValueError("axis must be 0 (t), 1 (x), or 2 (y)")
        if not isinstance(home, bool) or not isinstance(move_to_origin, bool):
            raise TypeError("home and move_to_origin must be bool")
        return self._job("initialize", axis=axis, home=home, move_to_origin=move_to_origin,
                         request_id=request_id)

    def connect(self, devices: list[str] | None = None, *, request_id: str | None = None) -> dict[str, Any]:
        if devices is not None and (not isinstance(devices, list) or not all(isinstance(x, str) for x in devices)):
            raise TypeError("devices must be a list of strings or None")
        payload: dict[str, Any] = {"request_id": request_id}
        if devices is not None:
            payload["devices"] = devices
        return self._job("connect", **payload)

    def disconnect(self, devices: list[str] | None = None, *, request_id: str | None = None) -> dict[str, Any]:
        if devices is not None and (not isinstance(devices, list) or not all(isinstance(x, str) for x in devices)):
            raise TypeError("devices must be a list of strings or None")
        payload: dict[str, Any] = {"request_id": request_id}
        if devices is not None:
            payload["devices"] = devices
        return self._job("disconnect", **payload)

    def read(self, *, request_id: str | None = None) -> dict[str, Any]:
        return self._job("read", request_id=request_id)

    def settings(self, *, request_id: str | None = None) -> dict[str, Any]:
        """Read lock-in settings through the GUI-owned device connection."""
        return self._job("lockin_settings", request_id=request_id)

    def set_lockin(self, setting: str, value: Any, *, request_id: str | None = None) -> dict[str, Any]:
        if not isinstance(setting, str) or not setting:
            raise ValueError("setting must be a non-empty string")
        return self._job("set_lockin", setting=setting, value=value, request_id=request_id)

    def auto_lockin(self, action: str, *, request_id: str | None = None) -> dict[str, Any]:
        if not isinstance(action, str) or not action:
            raise ValueError("action must be a non-empty string")
        return self._job("auto_lockin", auto_action=action, request_id=request_id)

    @staticmethod
    def _validate_wait(timeout: float | None, poll_interval: float) -> None:
        if timeout is not None and (not math.isfinite(timeout) or timeout < 0):
            raise ValueError("timeout must be non-negative and finite or None")
        if not math.isfinite(poll_interval) or poll_interval <= 0:
            raise ValueError("poll_interval must be positive and finite")

    def _wait(self, run_id: str, *, timeout: float | None, poll_interval: float,
              accept_stopped: bool, stop_on_timeout: bool) -> dict[str, Any]:
        self._validate_wait(timeout, poll_interval)
        deadline = None if timeout is None else time.monotonic() + timeout
        while True:
            record = self.result(run_id)
            if record["status"] == "completed" or (accept_stopped and record["status"] == "stopped"):
                return record
            if record["status"] == "stopped":
                raise GuiRunStopped(f"GUI run {run_id} was stopped", "stopped", record)
            if record["status"] == "error":
                raise _gui_error(str(record.get("error") or f"GUI run {run_id} failed"),
                                 record.get("error_code") or "job_failed", record)
            if deadline is not None and time.monotonic() >= deadline:
                error = GuiTimeoutError(f"GUI run {run_id} did not finish before timeout; stop is not confirmed",
                                        "timeout", record)
                if stop_on_timeout:
                    try:
                        self.stop(run_id)
                    except Exception as stop_error:
                        error.stop_error = stop_error
                raise error
            sleep_for = poll_interval if deadline is None else min(poll_interval, deadline - time.monotonic())
            if sleep_for > 0:
                time.sleep(sleep_for)

    def wait(self, run_id: str, *, timeout: float | None = None,
             poll_interval: float = 0.1) -> dict[str, Any]:
        """Wait for completion. A timeout requests Stop only when opted in."""
        return self._wait(run_id, timeout=timeout, poll_interval=poll_interval,
                          accept_stopped=False, stop_on_timeout=self._stop_policy["stop_on_timeout"])

    def stop_and_wait(self, run_id: str, *, timeout: float | None = 40.0,
                      poll_interval: float = 0.1) -> dict[str, Any]:
        """Request Stop and return a completed/stopped receipt after cleanup.

        A timeout is not proof of stopped hardware. Errors preserve their code.
        """
        self._validate_wait(timeout, poll_interval)
        record = self.result(run_id)
        if record["status"] == "running":
            self.stop(run_id)
        return self._wait(run_id, timeout=timeout, poll_interval=poll_interval,
                          accept_stopped=True, stop_on_timeout=False)

    def run(self, workflow: str | None = None, **kwargs: Any) -> dict[str, Any]:
        """Start one GUI measurement and wait for its terminal record."""
        timeout = kwargs.pop("timeout", None)
        poll_interval = kwargs.pop("poll_interval", 0.1)
        self._validate_wait(timeout, poll_interval)
        record = self.start(workflow, **kwargs)
        return self.wait(record["id"], timeout=timeout, poll_interval=poll_interval)

    def close(self, *, _reason: str = "disconnect") -> None:
        """Release this client, applying its policy; never close the GUI."""
        if self._closed:
            return
        try:
            self.release(_reason=_reason)
        finally:
            self._stop_heartbeat()
            self._closed = True

    def __enter__(self) -> "GuiClient":
        self.acquire()
        return self

    def __exit__(self, exc_type: object, exc: object, traceback: object) -> None:
        try:
            self.close(_reason="python_error" if exc_type is not None else "disconnect")
        except Exception as cleanup_error:
            if exc is None:
                raise
            # Keep the experiment's original Python exception visible.
            if isinstance(exc, BaseException):
                if hasattr(exc, "add_note"):
                    exc.add_note(f"GUI control release failed: {cleanup_error}")
                else:
                    exc.gui_cleanup_error = cleanup_error

    @classmethod
    def launch(cls, profile: str | Path | None = None, *, binary: str | Path | None = None,
               timeout: float = 15.0, stop_on_disconnect: bool = False,
               stop_on_error: bool = False, stop_on_timeout: bool = False) -> "GuiClient":
        """Launch a GUI API process and attach only to that process's descriptor."""
        if not math.isfinite(timeout) or timeout <= 0:
            raise ValueError("timeout must be greater than zero and finite")
        policy = dict(stop_on_disconnect=stop_on_disconnect, stop_on_error=stop_on_error,
                      stop_on_timeout=stop_on_timeout)
        for name, value in policy.items():
            if not isinstance(value, bool):
                raise TypeError(f"{name} must be bool")
        executable = _explicit_file(binary, "TRKR binary", command=True) if binary else find_binary()
        command = [str(executable), "--gui-api"]
        if profile is not None:
            command.extend(["--config", str(Path(profile).expanduser().resolve())])
        options: dict[str, Any] = {"stdin": subprocess.DEVNULL, "stdout": subprocess.DEVNULL,
                                   "stderr": subprocess.DEVNULL}
        if os.name == "nt":
            options["creationflags"] = getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0)
        else:
            options["start_new_session"] = True
        process = subprocess.Popen(command, **options)
        deadline = time.monotonic() + timeout
        directory = Path.home() / ".kohdalab" / "run"
        while time.monotonic() < deadline:
            exit_code = process.poll()
            if exit_code is not None:
                raise RuntimeError(f"Launched TRKR GUI exited before its API was ready (exit {exit_code})")
            for path in directory.glob("trkr-gui-*.json") if directory.is_dir() else []:
                try:
                    found = cls._load_descriptor(path)
                except ValueError:
                    continue
                if found["pid"] == process.pid:
                    client = cls(path, timeout=min(5.0, timeout), **policy)
                    client._descriptor = found
                    try:
                        client.status()
                    except RuntimeError:
                        continue
                    return client
            time.sleep(0.05)
        raise TimeoutError("Launched TRKR GUI did not publish its API session")


def _roots(search_from: str | Path | None = None) -> list[Path]:
    root = Path.cwd() if search_from is None else Path(search_from).expanduser()
    root = root.resolve()
    if root.is_file():
        root = root.parent
    return [root, *list(root.parents)[:3]]


def _first_file(paths: Iterator[Path] | list[Path]) -> Path | None:
    return next((path.resolve() for path in paths if path.is_file()), None)


def _explicit_file(value: str | Path, name: str, *, command: bool = False) -> Path:
    path = Path(value).expanduser()
    if path.is_file():
        return path.resolve()
    if command:
        found = shutil.which(str(value))
        if found:
            return Path(found).resolve()
    raise FileNotFoundError(f"{name} does not name a file: {value}")


def _windows_install_directories() -> list[Path]:
    directories: list[Path] = []
    try:
        import winreg
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r"Software\KohdaLab\TRKR") as key:
            location, kind = winreg.QueryValueEx(key, "InstallLocation")
            if isinstance(location, str) and location:
                if kind == winreg.REG_EXPAND_SZ:
                    location = winreg.ExpandEnvironmentStrings(location)
                directories.append(Path(location))
    except (ImportError, OSError):
        pass
    local = os.environ.get("LOCALAPPDATA")
    if local:
        directories.append(Path(local) / "KohdaLab" / "TRKR")
    return directories


def find_binary(
    binary: str | Path | None = None, *, search_from: str | Path | None = None
) -> Path:
    """Find the Rust controller without launching it or opening hardware.

    Explicit argument / TRKR_BINARY, nearby portable/source builds, PATH,
    then standard installed locations. An invalid explicit path never falls back.
    """
    override = binary if binary is not None else (os.environ.get("TRKR_BINARY") or None)
    if override is not None:
        return _explicit_file(override, "binary / TRKR_BINARY", command=True)
    executable = "kohdalab-trkr.exe" if _PLATFORM == "win32" else "kohdalab-trkr"
    candidates: list[Path] = []
    for root in _roots(search_from):
        candidates.extend((
            root / executable,
            root / "target/release" / executable,
            root / "target/debug" / executable,
            root / "target/x86_64-pc-windows-msvc/release" / executable,
            root / "next/target/release" / executable,
            root / "next/target/debug" / executable,
            root / "next/target/x86_64-pc-windows-msvc/release" / executable,
            root / "Contents/MacOS" / executable,
            root / "KohdaLab TRKR.app/Contents/MacOS" / executable,
        ))
    found = _first_file(candidates)
    if found:
        return found
    on_path = shutil.which("kohdalab-trkr")
    if on_path:
        return Path(on_path).resolve()
    if _PLATFORM == "win32":
        candidates = [directory / executable for directory in _windows_install_directories()]
    elif _PLATFORM == "darwin":
        candidates = [directory / "KohdaLab TRKR.app/Contents/MacOS/kohdalab-trkr"
                      for directory in (Path.home() / "Applications", Path("/Applications"))]
    else:
        candidates = []
    found = _first_file(candidates)
    if found:
        return found
    raise FileNotFoundError(
        "Could not find kohdalab-trkr. Install the Rust application, open this "
        "notebook beside the portable package, or set TRKR_BINARY to its executable."
    )


def find_profile(
    profile: str | Path | None = None, *, binary: str | Path | None = None,
    search_from: str | Path | None = None,
) -> Path:
    """Find a profile without changing it; TRKR_PROFILE overrides auto discovery."""
    override = profile if profile is not None else (os.environ.get("TRKR_PROFILE") or None)
    if override is not None:
        return _explicit_file(override, "profile / TRKR_PROFILE")
    candidates: list[Path] = []
    roots = _roots(search_from)
    for root in roots:
        candidates.extend((
            root / "config/default.json",
        ))
    candidates.extend(Path.home() / ".kohdalab/config" / name
                      for name in ("trkr-next.json", "default.json"))
    candidates.extend(root / "crates/trkr-desktop/assets/default.json" for root in roots)
    candidates.extend(root / "next/crates/trkr-desktop/assets/default.json" for root in roots)
    found = _first_file(candidates)
    if found:
        return found
    executable = find_binary(binary, search_from=search_from)
    found = _first_file([
        executable.parent / "config/default.json",
        executable.parent / "../Resources/config/default.json",
    ])
    if found:
        return found
    raise FileNotFoundError(
        "Could not find a TRKR profile. Save one in the GUI or set TRKR_PROFILE "
        "to your reviewed JSON profile."
    )


def prepare_profile(
    profile: str | Path | None = None, *, binary: str | Path | None = None,
    search_from: str | Path | None = None,
) -> dict[str, Any]:
    """Ask Rust to validate a profile and prepare plans; never connects hardware."""
    executable = find_binary(binary, search_from=search_from)
    path = find_profile(profile, binary=executable, search_from=search_from)
    return json.loads(subprocess.check_output(
        [str(executable), "--profile-json", str(path)], encoding="utf-8",
        **_process_options(),
    ))


def _process_options() -> dict[str, Any]:
    return {"creationflags": subprocess.CREATE_NO_WINDOW} if os.name == "nt" else {}


class Client:
    def __init__(self, binary: str | Path | None = None) -> None:
        self._process = subprocess.Popen(  # noqa: S603 - explicitly selected native executable
            [str(find_binary(binary)), "--stdio"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            encoding="utf-8",
            bufsize=1,
            **_process_options(),
        )
        if self._process.stdin is None or self._process.stdout is None:
            raise RuntimeError("Could not start portable TRKR controller")
        self._write_lock = threading.Lock()
        self._job_lock = threading.Lock()
        self._events: queue.Queue[dict[str, Any] | None] = queue.Queue()
        self._reader = threading.Thread(target=self._read_events, daemon=True)
        self._reader.start()

    def _read_events(self) -> None:
        if self._process.stdout is None:
            self._events.put(None)
            return
        for line in self._process.stdout:
            try:
                self._events.put(json.loads(line))
            except json.JSONDecodeError:
                self._events.put(
                    {"event": "error", "data": "Invalid JSON from native controller"}
                )
        self._events.put(None)

    def send(self, command: dict[str, Any]) -> None:
        """Send one native Command. Commands are serialized by this client."""
        if self._process.poll() is not None:
            raise RuntimeError("Portable TRKR controller has exited")
        with self._write_lock:
            if self._process.stdin is None:
                raise RuntimeError("Native controller stdin is unavailable")
            self._process.stdin.write(json.dumps(command, separators=(",", ":")) + "\n")
            self._process.stdin.flush()

    def events(self) -> Iterator[dict[str, Any]]:
        while (event := self._events.get()) is not None:
            yield event

    def wait_for_finished(self) -> list[dict[str, Any]]:
        """Drain one native job through its terminal ``finished`` event.

        ``stop()`` remains safe to call from another notebook cell or thread
        while this method is waiting.  Native errors are drained through the
        terminal event before they are raised, so they cannot leak into the
        next command's event stream.
        """
        result: list[dict[str, Any]] = []
        failure: str | None = None
        for event in self.events():
            result.append(event)
            if event.get("event") == "finished":
                if failure is not None:
                    raise RuntimeError(failure)
                return result
            if event.get("event") == "rejected":
                raise RuntimeError(
                    str(event.get("data", "native controller rejected command"))
                )
            if event.get("event") == "error":
                failure = str(event.get("data", "native controller error"))
        raise RuntimeError("Portable TRKR controller closed before finishing")

    def connect(self, config: dict[str, Any], devices: list[str]) -> None:
        self.send({"command": "connect", "config": config, "devices": devices})

    def move(
        self,
        config: dict[str, Any],
        targets: list[float | None],
        coordinate: str = "measurement",
        zero: list[float] | None = None,
        approach: list[bool] | None = None,
    ) -> None:
        """Position selected axes, using configured X/Y compensation by default.

        Pass an explicit approach mask only to override normal positioning.
        """
        self.send(
            {
                "command": "move",
                "config": config,
                "targets": targets,
                "coordinate": coordinate,
                "zero": zero or [0.0] * 3,
                "approach": approach if approach is not None else [False, True, True],
            }
        )

    def read(self) -> None:
        self.send({"command": "read"})

    def settings(self) -> None:
        self.send({"command": "lockin_settings"})

    def stop(self) -> None:
        self.send({"command": "stop"})

    def run(
        self,
        config: dict[str, Any],
        plan: dict[str, Any],
        source: dict[str, Any],
        output: str | Path | None = None,
    ) -> list[dict[str, Any]]:
        """Run one job synchronously and return its native event stream.

        The native protocol has no request identifiers, so this lock prevents
        overlapping jobs from different Python threads.
        """
        with self._job_lock:
            self.send(
                {
                    "command": "run",
                    "config": config,
                    "plan": plan,
                    "source": source,
                    "output": None if output is None else str(output),
                }
            )
            return self.wait_for_finished()

    def close(self) -> None:
        if self._process.poll() is None:
            try:
                self.stop()
            except (BrokenPipeError, RuntimeError):
                pass
            if self._process.stdin:
                self._process.stdin.close()
            try:
                self._process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self._process.terminate()
                self._process.wait(timeout=5)

    def __enter__(self) -> "Client":
        return self

    def __exit__(self, *_: object) -> None:
        self.close()
