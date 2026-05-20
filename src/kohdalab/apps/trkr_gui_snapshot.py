from __future__ import annotations

from typing import Any


SIGNAL_VOLTAGE_KEYS = {"X_V", "Y_V", "R_V"}
SCANNER_CONTROL_KEYS = {
    "delay_stage_mm",
    "x_scanner_mm",
    "x_scanner_deg",
    "y_scanner_mm",
    "y_scanner_deg",
}
SAMPLE_UM_KEYS = {"x_um", "x_cor_um", "y_um", "y_cor_um"}


def format_ps(value: float) -> str:
    return f"{round(value, 3):.3f}"


def format_snapshot_value(key: str, value: Any, *, voltage_scale: float = 1.0) -> str:
    if isinstance(value, float):
        if key in SIGNAL_VOLTAGE_KEYS:
            return f"{value:.6e}"
        if key == "Theta_deg":
            return f"{value:.3f}"
        if key in {"t_ps", "t_cor_ps"}:
            return format_ps(value)
        if key in SCANNER_CONTROL_KEYS:
            return f"{value:.6f}"
        if key in SAMPLE_UM_KEYS:
            return f"{value:.3f}"
        if key == "elapsed_s":
            return f"{value:.3f}"
        if key == "scan_axis":
            return str(value)
        return f"{value:.6f}"
    return str(value)
