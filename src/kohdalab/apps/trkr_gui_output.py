from __future__ import annotations

from pathlib import Path
from typing import Any

from kohdalab.api.config import with_auto_suffix, with_csv_suffix


def output_settings_from_fields(
    *,
    output_dir: str | Path | None,
    filename: str | None,
    auto_timestamp_suffix: bool,
    default_dir: str | Path | None = None,
    default_filename: str = "trkr_run",
) -> dict[str, Any]:
    directory = str(output_dir or "").strip()
    if not directory:
        directory = str(default_dir or Path.cwd())
    base_name = str(filename or "").strip() or default_filename
    return {
        "output_dir": directory,
        "filename": base_name,
        "auto_timestamp_suffix": bool(auto_timestamp_suffix),
    }


def normalize_output_settings(
    settings: dict[str, Any] | None,
    *,
    default_dir: str | Path | None = None,
    default_filename: str = "trkr_run",
) -> dict[str, Any]:
    settings = settings or {}
    return output_settings_from_fields(
        output_dir=settings.get("output_dir", settings.get("dir")),
        filename=settings.get("filename"),
        auto_timestamp_suffix=bool(settings.get("auto_timestamp_suffix", True)),
        default_dir=default_dir,
        default_filename=default_filename,
    )


def build_output_path(settings: dict[str, Any]) -> Path:
    normalized = normalize_output_settings(settings)
    output_dir = Path(str(normalized["output_dir"]))
    base_name = with_csv_suffix(str(normalized["filename"]))
    filename = with_auto_suffix(base_name) if normalized["auto_timestamp_suffix"] else base_name
    return output_dir / filename


def output_config_for_measurement(settings: dict[str, Any]) -> dict[str, Any]:
    normalized = normalize_output_settings(settings)
    return {
        "dir": normalized["output_dir"],
        "filename": normalized["filename"],
        "auto_timestamp_suffix": normalized["auto_timestamp_suffix"],
    }
