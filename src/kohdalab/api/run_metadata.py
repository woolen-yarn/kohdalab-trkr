from __future__ import annotations

import hashlib
import json
import os
import platform
import sys
from copy import deepcopy
from datetime import datetime, timezone
from pathlib import Path
from typing import Any
from uuid import uuid4

from kohdalab import __version__


RUN_METADATA_SCHEMA_VERSION = 1
_SENSITIVE_KEY_PARTS = ("password", "secret", "token", "api_key", "apikey")
_TERMINAL_STATUSES = {"completed", "stopped", "failed", "interrupted"}


def utc_now_iso() -> str:
    """Return an unambiguous millisecond-resolution RFC 3339 UTC timestamp."""
    return (
        datetime.now(timezone.utc)
        .isoformat(timespec="milliseconds")
        .replace("+00:00", "Z")
    )


def metadata_path(output_path: str | Path) -> Path:
    path = Path(output_path)
    return path.with_suffix(f"{path.suffix}.meta.json")


def _redacted_config(value: Any) -> Any:
    if isinstance(value, dict):
        return {
            str(key): "<redacted>"
            if any(part in str(key).lower() for part in _SENSITIVE_KEY_PARTS)
            else _redacted_config(item)
            for key, item in value.items()
        }
    if isinstance(value, list):
        return [_redacted_config(item) for item in value]
    if isinstance(value, tuple):
        return [_redacted_config(item) for item in value]
    return deepcopy(value)


def _canonical_json(value: Any) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
        allow_nan=False,
    ).encode("utf-8")


def sha256_file(path: str | Path) -> str:
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return f"sha256:{digest.hexdigest()}"


class RunMetadata:
    """Persist provenance and terminal state beside one measurement CSV."""

    def __init__(
        self,
        *,
        output_path: str | Path,
        measurement: str,
        config: dict[str, Any],
        expected_points: int,
        allow_overwrite: bool = False,
    ) -> None:
        if (
            isinstance(expected_points, bool)
            or not isinstance(expected_points, int)
            or expected_points < 0
        ):
            raise ValueError("expected_points must be a non-negative integer.")
        self.output_path = Path(output_path)
        self.path = metadata_path(self.output_path)
        self.allow_overwrite = allow_overwrite
        self._written = False
        config_snapshot = _redacted_config(config)
        self.data: dict[str, Any] = {
            "schema_version": RUN_METADATA_SCHEMA_VERSION,
            "run_id": str(uuid4()),
            "measurement": measurement,
            "status": "running",
            "started_at": utc_now_iso(),
            "finished_at": None,
            "expected_points": int(expected_points),
            "rows_written": 0,
            "output_file": self.output_path.name,
            "output_sha256": None,
            "config_sha256": f"sha256:{hashlib.sha256(_canonical_json(config_snapshot)).hexdigest()}",
            "config": config_snapshot,
            "software": {
                "kohdalab_version": __version__,
                "python_version": platform.python_version(),
                "python_implementation": platform.python_implementation(),
                "platform": sys.platform,
            },
            "error": None,
        }

    def write(self) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        temporary = self.path.with_name(f".{self.path.name}.{self.data['run_id']}.tmp")
        try:
            with temporary.open("w", encoding="utf-8") as stream:
                json.dump(
                    self.data,
                    stream,
                    ensure_ascii=False,
                    indent=2,
                    sort_keys=True,
                    allow_nan=False,
                )
                stream.write("\n")
                stream.flush()
                os.fsync(stream.fileno())
            if self._written or self.allow_overwrite:
                temporary.replace(self.path)
            else:
                os.link(temporary, self.path)
                temporary.unlink()
            self._written = True
        finally:
            temporary.unlink(missing_ok=True)

    def finish(
        self, *, status: str, rows_written: int, error: BaseException | None = None
    ) -> None:
        if status not in _TERMINAL_STATUSES:
            available = ", ".join(sorted(_TERMINAL_STATUSES))
            raise ValueError(
                f"Unsupported run status: {status!r}. Use one of: {available}."
            )
        if (
            isinstance(rows_written, bool)
            or not isinstance(rows_written, int)
            or not 0 <= rows_written <= self.data["expected_points"]
        ):
            raise ValueError(
                "rows_written must be an integer between zero and expected_points."
            )
        if status == "completed" and rows_written != self.data["expected_points"]:
            raise ValueError("completed runs must write exactly expected_points rows.")
        self.data["status"] = status
        self.data["finished_at"] = utc_now_iso()
        self.data["rows_written"] = int(rows_written)
        if self.output_path.is_file():
            self.data["output_sha256"] = sha256_file(self.output_path)
        if error is not None:
            self.data["error"] = {
                "type": type(error).__name__,
                "message": str(error),
            }
            notes = getattr(error, "__notes__", None)
            if notes:
                self.data["error"]["notes"] = [str(note) for note in notes]
        self.write()
