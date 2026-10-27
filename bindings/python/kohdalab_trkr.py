"""Thin JSON-lines client for the Rust portable TRKR controller.

All measurement, hardware, and safety logic stays in the native process.
"""

from __future__ import annotations

import json
import os
import queue
import shutil
import socket
import subprocess
import sys
import threading
import time
import uuid
from pathlib import Path
from typing import Any, Iterator


_PLATFORM = sys.platform

# Profiles and JSON-lines remain revision 1. Recording revision 5 separates
# analysis CSV, raw CSV, and run metadata with UUIDs and file checksums.
# Legacy CSV/JSONL recordings remain readable.
PROFILE_COMPATIBILITY_REVISION = 1
JSON_LINES_API_REVISION = 1
RECORDING_COMPATIBILITY_REVISION = 5


class GuiRunStopped(RuntimeError):
    """Raised when a GUI-managed run was stopped before completion."""


class GuiClient:
    """Control an already running TRKR GUI through its local API.

    The client never starts the GUI or talks to an instrument.  Each request
    opens a new authenticated loopback connection, so closing this object
    cannot stop an acquisition owned by the GUI.
    """

    _RUN_STATES = {"running", "completed", "stopped", "error"}
    _MAX_RESPONSE_BYTES = 1_048_576

    def __init__(
        self,
        session: str | Path | None = None,
        *,
        timeout: float = 5.0,
    ) -> None:
        if timeout <= 0:
            raise ValueError("timeout must be greater than zero")
        self._session_path = None if session is None else Path(session).expanduser()
        self._timeout = float(timeout)
        self._descriptor: dict[str, Any] | None = None
        self._closed = False

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

    def _request_to(self, descriptor: dict[str, Any], action: str, **data: Any) -> Any:
        payload = {"token": descriptor["token"], "action": action, **data}
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
                        raise RuntimeError("GUI API closed before returning a response")
                    response.extend(chunk)
                    if len(response) > self._MAX_RESPONSE_BYTES:
                        raise RuntimeError("GUI API response is too large")
        except OSError as exc:
            raise RuntimeError("Could not connect to the running TRKR GUI") from exc
        try:
            decoded = json.loads(response.split(b"\n", 1)[0].decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise RuntimeError("Invalid JSON response from TRKR GUI") from exc
        if not isinstance(decoded, dict) or not isinstance(decoded.get("ok"), bool):
            raise RuntimeError("Invalid response envelope from TRKR GUI")
        if not decoded["ok"]:
            raise RuntimeError(str(decoded.get("error", "TRKR GUI rejected request")))
        if "result" not in decoded:
            raise RuntimeError("TRKR GUI response omitted result")
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
            raise RuntimeError("No running TRKR GUI API session was found")
        if len(candidates) != 1:
            raise RuntimeError("Multiple running TRKR GUI API sessions were found; pass session=")
        return candidates[0]

    def _request(self, action: str, **data: Any) -> Any:
        if self._closed:
            raise RuntimeError("GuiClient is closed")
        if self._descriptor is None:
            self._descriptor = self._discover()
        return self._request_to(self._descriptor, action, **data)

    @classmethod
    def _run_record(cls, value: Any) -> dict[str, Any]:
        if not isinstance(value, dict) or not isinstance(value.get("id"), str):
            raise RuntimeError("Invalid run record from TRKR GUI")
        status = value.get("status")
        if status not in cls._RUN_STATES:
            raise RuntimeError("Invalid run status from TRKR GUI")
        return value

    def status(self) -> dict[str, Any]:
        """Return the GUI snapshot and its active run, if any."""
        result = self._request("status")
        if not isinstance(result, dict) or "snapshot" not in result or "active_run" not in result:
            raise RuntimeError("Invalid status result from TRKR GUI")
        active = result["active_run"]
        if active is not None:
            self._run_record(active)
        return result

    def plan(self, workflow: str | None = None) -> dict[str, Any]:
        payload = {} if workflow is None else {"workflow": workflow}
        result = self._request("plan", **payload)
        if not isinstance(result, dict) or not {"plan", "workflow", "source"} <= result.keys():
            raise RuntimeError("Invalid plan result from TRKR GUI")
        return result

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
        if request_id is not None:
            if not isinstance(request_id, str):
                raise ValueError("request_id must be a UUID string or None")
            try:
                uuid.UUID(request_id)
            except ValueError as exc:
                raise ValueError("request_id must be a UUID string or None") from exc
        payload: dict[str, Any] = {"request_id": request_id or str(uuid.uuid4())}
        if workflow is not None:
            payload["workflow"] = workflow
        if plan is not None:
            payload["plan"] = plan
        if output is not None:
            payload["output"] = str(Path(output).expanduser().resolve())
        return self._run_record(self._request("start", **payload))

    def result(self, run_id: str) -> dict[str, Any]:
        if not isinstance(run_id, str) or not run_id:
            raise ValueError("run_id must be a non-empty string")
        return self._run_record(self._request("result", run_id=run_id))

    def stop(self, run_id: str) -> dict[str, Any]:
        """Ask the GUI to stop its matching active run; this is never implicit."""
        if not isinstance(run_id, str) or not run_id:
            raise ValueError("run_id must be a non-empty string")
        return self._run_record(self._request("stop", run_id=run_id))

    def wait(
        self,
        run_id: str,
        *,
        timeout: float | None = None,
        poll_interval: float = 0.1,
    ) -> dict[str, Any]:
        if timeout is not None and timeout < 0:
            raise ValueError("timeout must be non-negative or None")
        if poll_interval <= 0:
            raise ValueError("poll_interval must be greater than zero")
        deadline = None if timeout is None else time.monotonic() + timeout
        while True:
            record = self.result(run_id)
            if record["status"] == "completed":
                return record
            if record["status"] == "stopped":
                raise GuiRunStopped(f"GUI run {run_id} was stopped")
            if record["status"] == "error":
                raise RuntimeError(str(record.get("error") or f"GUI run {run_id} failed"))
            if deadline is not None and time.monotonic() >= deadline:
                raise TimeoutError(f"GUI run {run_id} did not finish before timeout")
            sleep_for = poll_interval if deadline is None else min(poll_interval, deadline - time.monotonic())
            if sleep_for > 0:
                time.sleep(sleep_for)

    def run(self, workflow: str | None = None, **kwargs: Any) -> dict[str, Any]:
        """Start one GUI measurement and wait for its terminal record."""
        timeout = kwargs.pop("timeout", None)
        poll_interval = kwargs.pop("poll_interval", 0.1)
        record = self.start(workflow, **kwargs)
        return self.wait(record["id"], timeout=timeout, poll_interval=poll_interval)

    def close(self) -> None:
        """Release this client only; it never sends Stop or closes the GUI."""
        self._closed = True

    def __enter__(self) -> "GuiClient":
        return self

    def __exit__(self, *_: object) -> None:
        self.close()


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
