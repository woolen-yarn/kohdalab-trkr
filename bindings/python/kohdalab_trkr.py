"""Thin JSON-lines client for the Rust portable TRKR controller.

All measurement, hardware, and safety logic stays in the native process.
"""

from __future__ import annotations

import json
import os
import queue
import shutil
import subprocess
import sys
import threading
from pathlib import Path
from typing import Any, Iterator


_PLATFORM = sys.platform


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
