"""Thin JSON-lines client for the Rust portable TRKR controller.

All measurement, hardware, and safety logic stays in the native process.
"""

from __future__ import annotations

import json
import queue
import subprocess
import threading
from pathlib import Path
from typing import Any, Iterator


class Client:
    def __init__(self, binary: str | Path = "kohdalab-trkr") -> None:
        self._process = subprocess.Popen(  # noqa: S603 - explicitly selected native executable
            [str(binary), "--stdio"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            bufsize=1,
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
        self.send(
            {
                "command": "move",
                "config": config,
                "targets": targets,
                "coordinate": coordinate,
                "zero": zero or [0.0] * 3,
                "approach": approach or [False] * 3,
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
