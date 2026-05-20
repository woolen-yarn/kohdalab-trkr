from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path
from threading import RLock
from typing import Optional

from kohdalab.instruments.delay_stage import DELAY_STAGE_CONTROLLERS
from kohdalab.interfaces.common import load_toml, merge_config


_STAGES_PATH = Path(__file__).resolve().parent.parent / "instruments" / "delay_stage" / "stages.toml"
STAGES = load_toml(_STAGES_PATH)
STAGE_NAMES = list(STAGES)


_DELAY_STAGE_CONNECTIONS: dict[tuple[str, str], "DelayStage"] = {}


def _controller_name(config: dict) -> str:
    return config.get("delay_stage_controller", config.get("controller", "SHOT302GS"))


def _validate_stage_controller(config: dict) -> None:
    stage_name = config.get("stage")
    if not stage_name:
        return
    settings = STAGES.get(stage_name, {})
    controllers = settings.get("controllers")
    if not controllers:
        return
    controller_name = _controller_name(config).upper()
    allowed = {str(controller).upper() for controller in controllers}
    if controller_name not in allowed:
        raise ValueError(
            f"Stage {stage_name!r} is only supported by {sorted(allowed)}, not {controller_name!r}."
        )


def _build_delay_stage_config(config: dict) -> dict:
    stage_name = config.get("stage")
    settings = dict(STAGES.get(stage_name, {}))
    settings.setdefault("axis_count", 1)
    settings.setdefault("controller_axis", 1)
    settings.setdefault("min_pulse", 0)
    merged = merge_config(settings, config)
    _validate_stage_controller(merged)
    return merged


def _build_delay_stage_controller(config: dict):
    controller_name = _controller_name(config)
    controller_cls = DELAY_STAGE_CONTROLLERS.get(controller_name)
    if controller_cls is None:
        raise ValueError(f"Unsupported delay stage controller: {controller_name}")

    return controller_cls(
        port=config["port"],
        baudrate=int(config.get("baudrate", 9600)),
        timeout=float(config.get("timeout", 1.0)),
        write_termination=config.get("write_termination", "\r\n"),
        read_termination=config.get("read_termination", "\r\n"),
        axis_count=int(config.get("axis_count", 1)),
        default_axis=int(config.get("controller_axis", config.get("default_axis", 1))),
        pos_unit=str(config.get("pos_unit", "pulse")),
    )


@dataclass(slots=True)
class DelayStage:
    controller: object
    config: dict
    _io_lock: object = field(default_factory=RLock, init=False, repr=False)
    _microstep_divisions: dict[int, int] = field(default_factory=dict, init=False, repr=False)

    def configure(self, config: dict):
        with self._io_lock:
            self.config = config
            self._microstep_divisions.clear()
            if hasattr(self.controller, "configure"):
                self.controller.configure(
                    axis_count=int(config.get("axis_count", 1)),
                    default_axis=self.axis,
                    pos_unit=str(config.get("pos_unit", "pulse")),
                )

    def close(self):
        with self._io_lock:
            self.controller.close()

    def is_connected(self) -> bool:
        with self._io_lock:
            return self.controller.is_connected()

    @property
    def stage_name(self) -> str | None:
        return self.config.get("stage")

    def _axis(self, axis: Optional[int] = None) -> int:
        return axis or int(self.config.get("controller_axis", self.config.get("default_axis", 1)))

    @property
    def axis(self) -> int:
        return self._axis()

    @property
    def pos_unit(self) -> str:
        return str(self.config.get("pos_unit", "pulse"))

    def get_pos_unit(self) -> str:
        return self.pos_unit

    def get_pos_um_per_pulse(self, axis: Optional[int] = None) -> float:
        direct = self.config.get("pos_um_per_pulse")
        if direct is not None:
            return float(direct)

        screw_lead_mm_per_rev = self.config.get("screw_lead_mm_per_rev")
        step_angle_deg = self.config.get("step_angle_deg")
        if screw_lead_mm_per_rev is None or step_angle_deg is None:
            raise ValueError(
                "Delay stage config requires 'pos_um_per_pulse' or both "
                "'screw_lead_mm_per_rev' and 'step_angle_deg'."
            )

        stage_axis = self._axis(axis)
        microstep_division = self.get_microstep_division(stage_axis)
        pulses_per_rev = (360.0 / float(step_angle_deg)) * microstep_division
        return 1000.0 * float(screw_lead_mm_per_rev) / pulses_per_rev

    def get_microstep_division(self, axis: Optional[int] = None) -> int:
        stage_axis = self._axis(axis)
        with self._io_lock:
            cached = self._microstep_divisions.get(stage_axis)
            if cached is not None:
                return cached
            division = int(self.controller.get_microstep_division(axis=stage_axis))
            self._microstep_divisions[stage_axis] = division
            return division

    def get_cached_microstep_division(self, axis: Optional[int] = None) -> int | None:
        with self._io_lock:
            return self._microstep_divisions.get(self._axis(axis))

    def pulse_to_pos_um(self, pulse: int, axis: Optional[int] = None) -> float:
        return pulse * self.get_pos_um_per_pulse(axis)

    def pos_um_to_pulse(self, pos_um: float, axis: Optional[int] = None) -> int:
        return round(pos_um / self.get_pos_um_per_pulse(axis))

    def get_pos_mm(self, axis: Optional[int] = None) -> float:
        with self._io_lock:
            stage_axis = self._axis(axis)
            return self.pulse_to_pos_um(self.controller.get_pos_raw(axis=stage_axis), stage_axis) / 1000.0

    def pulse_to_pos_mm(self, pulse: int, axis: Optional[int] = None) -> float:
        return self.pulse_to_pos_um(pulse, axis) / 1000.0

    def pos_mm_to_pulse(self, pos_mm: float, axis: Optional[int] = None) -> int:
        return self.pos_um_to_pulse(float(pos_mm) * 1000.0, axis)

    def get_pos_raw(self, axis: Optional[int] = None) -> int:
        with self._io_lock:
            return self.controller.get_pos_raw(axis=self._axis(axis))

    def get_pulse(self, axis: Optional[int] = None) -> int:
        return self.get_pos_raw(axis)

    def get_positions(self) -> list[int]:
        with self._io_lock:
            return self.controller.get_positions()

    def get_limits(self, axis: Optional[int] = None) -> tuple[Optional[float], Optional[float]]:
        stage_axis = self._axis(axis)
        pos_min = None
        if self.config.get("min_pulse") is not None:
            pos_min = self.pulse_to_pos_mm(int(self.config["min_pulse"]), stage_axis)

        pos_max = None
        if self.config.get("max_pulse") is not None:
            pos_max = self.pulse_to_pos_mm(int(self.config["max_pulse"]), stage_axis)
        elif self.config.get("travel_mm") is not None:
            pos_max = float(self.config["travel_mm"])

        return pos_min, pos_max

    def _check_range(self, pos_mm: float, axis: Optional[int] = None):
        pos_min, pos_max = self.get_limits(axis)
        if pos_min is not None and pos_mm < pos_min:
            raise ValueError(f"position={pos_mm} mm is below limit {pos_min} mm")
        if pos_max is not None and pos_mm > pos_max:
            raise ValueError(f"position={pos_mm} mm is above limit {pos_max} mm")

    def get_status(self) -> str:
        with self._io_lock:
            return self.controller.get_status()

    def initialize(self, home: bool = False, axis: Optional[int] = None) -> dict:
        with self._io_lock:
            stage_axis = self._axis(axis)
            if home:
                self.home(stage_axis)
            pos_mm = self.get_pos_mm(stage_axis)
            return {
                "ready": self.controller.is_ready(),
                "status": self.controller.get_status(),
                "stage": self.stage_name,
                "axis": stage_axis,
                "pos_raw": self.get_pos_raw(stage_axis),
                "pos_unit": self.get_pos_unit(),
                "pos_mm": pos_mm,
                "pos_limits": self.get_limits(stage_axis),
            }

    def execute_drive(self):
        with self._io_lock:
            self.controller.execute_drive()

    def move_pos_raw(
        self,
        pos_raw: int,
        axis: Optional[int] = None,
        *,
        on_position: Callable[[int], None] | None = None,
    ) -> int:
        with self._io_lock:
            stage_axis = self._axis(axis)
            if on_position is None:
                return self.controller.move_abs_raw(int(pos_raw), axis=stage_axis)
            return self.controller.move_abs_raw(int(pos_raw), axis=stage_axis, on_position=on_position)

    def move_pulse(
        self,
        pulse: int,
        axis: Optional[int] = None,
        *,
        on_position: Callable[[int], None] | None = None,
    ) -> int:
        return self.move_pos_raw(pulse, axis, on_position=on_position)

    def move_relative_pos_raw(
        self,
        delta_raw: int,
        axis: Optional[int] = None,
        *,
        on_position: Callable[[int], None] | None = None,
    ) -> int:
        with self._io_lock:
            stage_axis = self._axis(axis)
            if on_position is None:
                return self.controller.move_rel_raw(int(delta_raw), axis=stage_axis)
            return self.controller.move_rel_raw(int(delta_raw), axis=stage_axis, on_position=on_position)

    def move_pos_mm(
        self,
        pos_mm: float,
        axis: Optional[int] = None,
        *,
        on_position: Callable[[int], None] | None = None,
    ) -> float:
        with self._io_lock:
            stage_axis = self._axis(axis)
            self._check_range(pos_mm, stage_axis)
            pulse = self.pos_mm_to_pulse(pos_mm, stage_axis)
            if on_position is None:
                self.controller.move_abs_raw(pulse, axis=stage_axis)
            else:
                self.controller.move_abs_raw(pulse, axis=stage_axis, on_position=on_position)
            return self.get_pos_mm(stage_axis)

    def move_relative_pos_mm(
        self,
        delta_mm: float,
        axis: Optional[int] = None,
        *,
        on_position: Callable[[int], None] | None = None,
    ) -> float:
        with self._io_lock:
            stage_axis = self._axis(axis)
            target = self.get_pos_mm(stage_axis) + delta_mm
            self._check_range(target, stage_axis)
            delta_pulse = self.pos_mm_to_pulse(delta_mm, stage_axis)
            if on_position is None:
                self.controller.move_rel_raw(delta_pulse, axis=stage_axis)
            else:
                self.controller.move_rel_raw(delta_pulse, axis=stage_axis, on_position=on_position)
            return self.get_pos_mm(stage_axis)

    def jog(self, positive: bool = True, axis: Optional[int] = None):
        with self._io_lock:
            self.controller.jog(positive=positive, axis=self._axis(axis))

    def set_excitation(self, enabled: bool, axis: Optional[int] = None):
        with self._io_lock:
            self.controller.set_excitation(enabled=enabled, axis=self._axis(axis))

    def set_logical_zero(self):
        with self._io_lock:
            self.controller.set_logical_zero()

    def query_internal(self, code: str) -> str:
        with self._io_lock:
            return self.controller.query_internal(code)

    def home(self, axis: Optional[int] = None):
        with self._io_lock:
            self.controller.home(axis=self._axis(axis))

    def stop(self):
        with self._io_lock:
            self.controller.stop()


def connect_delay_stage(config: dict) -> DelayStage:
    controller_name = _controller_name(config)
    target = config["port"]
    cache_key = (controller_name, target)
    merged = _build_delay_stage_config(config)
    label = controller_name if merged.get("stage") is None else f"{controller_name}/{merged['stage']}"

    try:
        cached = _DELAY_STAGE_CONNECTIONS.get(cache_key)
        if cached is not None and cached.is_connected():
            cached.configure(merged)
            pos = cached.get_pos_mm()
            print(f"[DELAY_STAGE] Already connected: {label} @ {target} (pos={pos:.6f}mm)")
            return cached

        print(f"[DELAY_STAGE] Not connected: {label} @ {target}; connecting...")
        controller = _build_delay_stage_controller(merged)
        stage = DelayStage(controller=controller, config=merged)
        _DELAY_STAGE_CONNECTIONS[cache_key] = stage
        pos = stage.get_pos_mm()
        print(f"[DELAY_STAGE] Connected: {label} @ {target} (pos={pos:.6f}mm)")
        return stage
    except Exception as e:
        raise RuntimeError(f"[DELAY_STAGE] Connection failed: {controller_name} @ {target} | {e}")


def disconnect_delay_stage(config: dict | None = None):
    if config is None:
        keys = list(_DELAY_STAGE_CONNECTIONS.keys())
    else:
        controller_name = _controller_name(config)
        keys = [(controller_name, config["port"])]

    for key in keys:
        stage = _DELAY_STAGE_CONNECTIONS.pop(key, None)
        if stage is not None:
            try:
                stage.close()
            except Exception:
                pass
