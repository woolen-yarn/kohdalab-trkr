from __future__ import annotations

import math
from collections.abc import Callable
from typing import Any

from kohdalab.api.run_metadata import utc_now_iso
from kohdalab.interfaces import connect_delay_stage as _connect_delay_stage
from kohdalab.interfaces import disconnect_delay_stage as _disconnect_delay_stage
from kohdalab.interfaces.delay_stage import STAGE_NAMES, STAGES, DelayStage

LIGHT_SPEED_MM_PER_PS = 0.299792458


def _normalize_stage_name(stage_name: str | None) -> str | None:
    if not stage_name:
        return None
    normalized = stage_name.strip().upper().replace("_", "-")
    if normalized.startswith("SGSP-"):
        normalized = "SGSP" + normalized[len("SGSP-") :]
    return normalized


def _zero_pos_mm(config: dict[str, Any], stage: DelayStage) -> float:
    configured_zero = config.get("zero_pos_mm")
    if configured_zero is not None:
        zero = float(configured_zero)
        if not math.isfinite(zero):
            raise ValueError("delay stage zero_pos_mm must be finite.")
        return zero
    pos_min_mm, pos_max_mm = stage.get_limits()
    if pos_max_mm is None:
        raise ValueError(
            "delay stage measurement coordinates require zero_pos_mm or a finite maximum limit."
        )
    if pos_min_mm is None:
        pos_min_mm = 0.0
    minimum = float(pos_min_mm)
    maximum = float(pos_max_mm)
    if not math.isfinite(minimum) or not math.isfinite(maximum) or maximum <= minimum:
        raise ValueError("delay stage limits must be finite and increasing.")
    return minimum + (maximum - minimum) * 0.5


def _finite_float(value: float | int, *, coordinate: str) -> float:
    number = float(value)
    if isinstance(value, bool) or not math.isfinite(number):
        raise ValueError(f"delay stage {coordinate} target must be finite.")
    return number


def _integer_pulse(value: float | int) -> int:
    number = _finite_float(value, coordinate="instrument")
    pulse = int(number)
    if number != pulse:
        raise ValueError("delay stage instrument target must be an integer pulse.")
    return pulse


def _delay_ps(stage: DelayStage, config: dict[str, Any]) -> float:
    sign = 1.0 if int(config.get("direction", 0)) == 0 else -1.0
    return (
        sign
        * 2.0
        * (float(stage.get_pos_mm()) - _zero_pos_mm(config, stage))
        / LIGHT_SPEED_MM_PER_PS
    )


def _delay_ps_from_mm(stage_mm: float, zero_mm: float, config: dict[str, Any]) -> float:
    sign = 1.0 if int(config.get("direction", 0)) == 0 else -1.0
    return sign * 2.0 * (float(stage_mm) - float(zero_mm)) / LIGHT_SPEED_MM_PER_PS


def _delay_ps_to_mm(
    stage: DelayStage, delay_ps: float, config: dict[str, Any]
) -> float:
    sign = 1.0 if int(config.get("direction", 0)) == 0 else -1.0
    return (
        _zero_pos_mm(config, stage)
        + sign * float(delay_ps) * LIGHT_SPEED_MM_PER_PS / 2.0
    )


class DelayStageDevice:
    def __init__(self, stage: DelayStage, config: dict[str, Any]) -> None:
        self._stage = stage
        self.config = config

    def initialize(self, home: bool = False) -> dict[str, Any]:
        info = dict(self._stage.initialize(home=home))
        info["pos_mm"] = self.get_pos_mm()
        info["pulse"] = self.get_pulse()
        info["delay_ps"] = self.get_delay_ps()
        info["direction"] = int(self.config.get("direction", 0))
        return info

    def is_connected(self) -> bool:
        return bool(self._stage.is_connected())

    def get_pos_mm(self) -> float:
        return float(self._stage.get_pos_mm())

    def get_pulse(self) -> int:
        return int(self._stage.get_pulse())

    def get_delay_ps(self) -> float:
        return float(_delay_ps(self._stage, self.config))

    def get_status(self) -> str:
        return self._stage.get_status()

    def is_ready(self) -> bool:
        return bool(self._stage.controller.is_ready())

    def _progress_callback(
        self,
        on_position: Callable[[dict[str, Any]], None] | None,
    ) -> Callable[[int], None] | None:
        if on_position is None:
            return None

        zero_mm = _zero_pos_mm(self.config, self._stage)

        def emit(pulse: int) -> None:
            stage_mm = float(self._stage.pulse_to_pos_mm(int(pulse)))
            on_position(
                {
                    "timestamp": utc_now_iso(),
                    "axis": "t",
                    "t_ps": _delay_ps_from_mm(stage_mm, zero_mm, self.config),
                    "stage_mm": stage_mm,
                    "stage_pulse": int(pulse),
                }
            )

        return emit

    def move_coordinate(
        self,
        value: float | int,
        coordinate: str,
        *,
        on_position: Callable[[dict[str, Any]], None] | None = None,
    ) -> float | int:
        coordinate = coordinate.strip().lower()
        progress = self._progress_callback(on_position)
        if coordinate == "measurement":
            target = _finite_float(value, coordinate=coordinate)
            self._stage.move_pos_mm(
                _delay_ps_to_mm(self._stage, target, self.config),
                on_position=progress,
            )
            return self.get_delay_ps()
        if coordinate == "interface":
            self._stage.move_pos_mm(
                _finite_float(value, coordinate=coordinate), on_position=progress
            )
            return self.get_pos_mm()
        if coordinate == "instrument":
            self._stage.move_pulse(_integer_pulse(value), on_position=progress)
            return self.get_pulse()
        raise ValueError(
            "delay stage coordinate must be measurement, interface, or instrument."
        )


def connect_delay_stage(config: dict[str, Any]) -> DelayStageDevice:
    merged = dict(config)
    merged["stage"] = _normalize_stage_name(merged.get("stage"))
    return DelayStageDevice(_connect_delay_stage(merged), merged)


def disconnect_delay_stage(config: dict[str, Any] | None = None) -> None:
    merged = (
        None
        if config is None
        else {**config, "stage": _normalize_stage_name(config.get("stage"))}
    )
    _disconnect_delay_stage(merged)


def read_delay_stage(
    config: dict[str, Any] | None = None,
    *,
    delay_stage: DelayStageDevice | None = None,
) -> dict[str, Any]:
    stage = delay_stage or connect_delay_stage(config or {})
    return {
        "axis": "t",
        "t_ps": float(stage.get_delay_ps()),
        "stage_mm": float(stage.get_pos_mm()),
        "stage_pulse": int(stage.get_pulse()),
    }


def move_delay_stage_abs(
    *,
    delay_stage_config: dict[str, Any],
    coordinate: str,
    value: float,
    delay_stage: DelayStageDevice | None = None,
    on_position: Callable[[dict[str, Any]], None] | None = None,
) -> dict[str, Any]:
    coordinate = coordinate.strip().lower()
    stage = delay_stage or connect_delay_stage(delay_stage_config)
    if coordinate in {"measurement", "t_ps", "ps"}:
        stage.move_coordinate(value, "measurement", on_position=on_position)
        actual = stage.get_delay_ps()
        unit = "ps"
    elif coordinate in {"interface", "pos_mm", "mm"}:
        stage.move_coordinate(value, "interface", on_position=on_position)
        actual = stage.get_pos_mm()
        unit = "mm"
    elif coordinate in {"instrument", "pulse", "device"}:
        stage.move_coordinate(value, "instrument", on_position=on_position)
        actual = stage.get_pulse()
        unit = "pulse"
    else:
        raise ValueError(
            "delay stage coordinate must be measurement, interface, or instrument."
        )
    return {
        "timestamp": utc_now_iso(),
        "axis": "t",
        "coordinate": coordinate,
        "target": value,
        "actual": actual,
        "unit": unit,
        "t_ps": float(stage.get_delay_ps()),
        "stage_mm": float(stage.get_pos_mm()),
        "stage_pulse": int(stage.get_pulse()),
    }


def initialize_delay_stage(
    config: dict[str, Any],
    *,
    delay_stage: DelayStageDevice | None = None,
    on_status: Callable[[str], None] | None = None,
) -> dict[str, Any]:
    emit = on_status or (lambda _status: None)
    emit("delay_stage initializing")
    stage = delay_stage or connect_delay_stage(config)
    info = dict(stage.initialize(home=True))
    emit("delay_stage moving to t_ps=0")
    stage.move_coordinate(0.0, "measurement")
    info["delay_ps"] = stage.get_delay_ps()
    info["pos_mm"] = stage.get_pos_mm()
    info["pulse"] = stage.get_pulse()
    info["status"] = stage.get_status()
    info["ready"] = stage.is_ready()
    return info


def list_stages(controller: str | None = None) -> list[str]:
    if controller is None:
        return sorted(STAGE_NAMES)
    controller_name = controller.strip().upper()
    names: list[str] = []
    for name in STAGE_NAMES:
        settings = STAGES.get(name, {})
        controllers = settings.get("controllers")
        if not controllers or controller_name in {
            str(item).upper() for item in controllers
        }:
            names.append(name)
    return sorted(names)
