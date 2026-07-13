from __future__ import annotations

from typing import Callable

StatusCallback = Callable[[str], None]

STATUS_RUNNING = "running"
STATUS_STOPPED = "stopped"
STATUS_WAITING = "waiting"
STATUS_READING_LOCKIN = "reading lock-in"
STATUS_SLOW_AXIS_READY = "slow axis ready"
STATUS_MOVING_DELAY_STAGE = "moving delay stage"

_STATUS_MOVING_SCANNER_PREFIX = "moving scanner "


def moving_scanner_status(axis: str) -> str:
    axis = axis.strip().lower()
    if axis not in {"x", "y"}:
        raise ValueError("scanner axis must be 'x' or 'y'.")
    return f"{_STATUS_MOVING_SCANNER_PREFIX}{axis}"


def moving_axis_status(axis: str) -> str:
    axis = axis.strip().lower()
    if axis == "t":
        return STATUS_MOVING_DELAY_STAGE
    return moving_scanner_status(axis)


def moving_axis_from_status(status: str) -> str | None:
    normalized = status.strip().lower()
    if normalized == STATUS_MOVING_DELAY_STAGE:
        return "t"
    if normalized.startswith(_STATUS_MOVING_SCANNER_PREFIX):
        axis = (
            normalized.removeprefix(_STATUS_MOVING_SCANNER_PREFIX)
            .strip()
            .split(maxsplit=1)[0]
        )
        return axis if axis in {"x", "y"} else None
    return None


__all__ = [
    "STATUS_MOVING_DELAY_STAGE",
    "STATUS_READING_LOCKIN",
    "STATUS_RUNNING",
    "STATUS_SLOW_AXIS_READY",
    "STATUS_STOPPED",
    "STATUS_WAITING",
    "StatusCallback",
    "moving_axis_from_status",
    "moving_axis_status",
    "moving_scanner_status",
]
