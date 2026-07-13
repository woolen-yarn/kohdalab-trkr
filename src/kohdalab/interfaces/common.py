from __future__ import annotations

import tomllib
from pathlib import Path
from typing import Any


def load_toml(path: Path) -> dict[str, Any]:
    with path.open("rb") as f:
        return tomllib.load(f)


def merge_config(defaults: dict[str, Any], overrides: dict[str, Any]) -> dict[str, Any]:
    merged = dict(defaults)
    merged.update({key: value for key, value in overrides.items() if value is not None})
    return merged


def midpoint(
    min_value: float | int | None,
    max_value: float | int | None,
    *,
    default: float = 0.0,
) -> float:
    if min_value is None or max_value is None:
        return float(default)
    return (float(min_value) + float(max_value)) / 2.0
