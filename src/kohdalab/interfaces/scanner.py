from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from decimal import Decimal
from pathlib import Path

import serial

from kohdalab.instruments.scanner import SCANNER_CONTROLLERS
from kohdalab.interfaces.common import load_toml, merge_config, midpoint


def _normalize_actuator(actuator: str | None) -> str | None:
    if actuator is None:
        return None
    return actuator.upper().replace("-", "")


_ACTUATORS_PATH = Path(__file__).resolve().parent.parent / "instruments" / "scanner" / "actuator.toml"
_ACTUATORS_RAW = load_toml(_ACTUATORS_PATH)
ACTUATOR_NAMES = list(_ACTUATORS_RAW)

ACTUATORS = {
    _normalize_actuator(name) or name: settings
    for name, settings in _ACTUATORS_RAW.items()
}


_SCANNER_CONNECTIONS: dict[tuple[str, str, str], "Scanner"] = {}
_SCANNER_SERIALS: dict[str, serial.Serial] = {}


def _controller_name(config: dict) -> str:
    return config.get("scanner_controller", config.get("controller", "CONEXCC"))


def _actuator_settings(actuator: str | None) -> dict:
    return dict(ACTUATORS.get(_normalize_actuator(actuator), {}))


def _validate_actuator_controller(config: dict) -> None:
    actuator_name = config.get("actuator")
    if not actuator_name:
        return
    settings = _actuator_settings(actuator_name)
    controllers = settings.get("controllers")
    if not controllers:
        return
    controller_name = _controller_name(config).upper()
    allowed = {str(controller).upper() for controller in controllers}
    if controller_name not in allowed:
        raise ValueError(
            f"Actuator {actuator_name!r} is only supported by {sorted(allowed)}, not {controller_name!r}."
        )


def _axis_key(config: dict) -> str:
    axis = config.get("axis", 1)
    controller_name = _controller_name(config)
    if controller_name == "CONEXAGAP":
        if isinstance(axis, str):
            normalized = axis.strip().upper()
            if normalized in {"U", "V"}:
                return normalized
        axis_int = int(axis)
        if axis_int == 1:
            return "U"
        if axis_int == 2:
            return "V"
        raise ValueError(f"Unsupported CONEXAGAP axis: {axis}")
    return str(int(axis))


def _build_scanner_config(config: dict) -> dict:
    actuator_name = config.get("actuator")
    settings = merge_config(_actuator_settings(actuator_name), config)
    settings["controller"] = _controller_name(settings)
    _validate_actuator_controller(settings)
    return settings


def _build_scanner_controller(config: dict, ser: serial.Serial | None = None):
    controller_name = _controller_name(config)
    controller_cls = SCANNER_CONTROLLERS.get(controller_name)
    if controller_cls is None:
        raise ValueError(f"Unsupported scanner controller: {controller_name}")

    kwargs = {
        "port": config["port"],
        "baudrate": int(config.get("baudrate", 921600)),
        "timeout": float(config.get("timeout", 1.0)),
        "ser": ser,
        "axis": config.get("axis", 1),
        "controller_address": int(config.get("controller_address", 1)),
        "pos_unit": str(config.get("pos_unit", "mm")),
    }
    if controller_name == "CONEXCC" and "ensure_closed_loop_on_move" in config:
        kwargs["ensure_closed_loop_on_move"] = bool(config["ensure_closed_loop_on_move"])
    return controller_cls(**kwargs)


@dataclass(slots=True)
class Scanner:
    controller: object
    config: dict

    def configure(self, config: dict):
        self.config = config
        if hasattr(self.controller, "configure"):
            self.controller.configure(
                axis=config.get("axis", 1),
                controller_address=int(config.get("controller_address", 1)),
                pos_unit=self.pos_unit,
            )

    def close(self):
        self.controller.close()

    def is_connected(self) -> bool:
        return self.controller.is_connected()

    @property
    def port(self) -> str:
        return self.controller.port

    @property
    def ser(self) -> serial.Serial:
        return self.controller.ser

    @property
    def axis(self) -> int:
        axis = self.config.get("axis", 1)
        if isinstance(axis, str) and axis.strip().upper() in {"U", "V"}:
            return 1 if axis.strip().upper() == "U" else 2
        return int(axis)

    @property
    def actuator_name(self) -> str | None:
        return self.config.get("actuator")

    @property
    def pos_unit(self) -> str:
        return str(self.config.get("pos_unit", "mm"))

    def get_pos_unit(self) -> str:
        return self.pos_unit

    @property
    def origin_pos(self) -> float:
        configured = self.config.get("origin_pos")
        if configured is not None:
            return self._round_pos(float(configured))
        return self._round_pos(midpoint(self.min_pos, self.max_pos))

    @property
    def min_pos(self) -> float | None:
        value = self.config.get("min_pos")
        return None if value is None else self._round_pos(float(value))

    @property
    def max_pos(self) -> float | None:
        value = self.config.get("max_pos")
        return None if value is None else self._round_pos(float(value))

    @property
    def pos_digits(self) -> int:
        configured = self.config.get("pos_digits")
        if configured is not None:
            return int(configured)
        resolution = self.config.get("resolution", self.config.get("min_step"))
        if resolution is not None:
            normalized = Decimal(str(resolution)).normalize()
            exponent = normalized.as_tuple().exponent
            return max(0, -exponent)
        return 4

    def _round_pos(self, pos: float) -> float:
        return round(pos, self.pos_digits)

    def normalize_pos(self, pos: float) -> float:
        return self._round_pos(float(pos))

    def _get_position(self) -> float:
        return self._round_pos(self.controller.get_pos_raw())

    def get_pos_raw(self) -> float:
        return self._get_position()

    def _require_pos_unit(self, expected_unit: str):
        if self.pos_unit.lower() != expected_unit:
            raise ValueError(f"Scanner actuator unit is {self.pos_unit!r}, not {expected_unit!r}.")

    def get_pos_mm(self) -> float:
        self._require_pos_unit("mm")
        return self._get_position()

    def get_pos_deg(self) -> float:
        self._require_pos_unit("deg")
        return self._get_position()

    def get_state(self) -> str:
        return self.controller.get_state()

    def is_moving(self) -> bool:
        return self.controller.is_moving()

    def wait_until_stopped(
        self,
        timeout: float = 30.0,
        *,
        on_position: Callable[[float], None] | None = None,
    ):
        if on_position is None:
            self.controller.wait_until_stopped(timeout=timeout)
        else:
            self.controller.wait_until_stopped(timeout=timeout, on_position=on_position)

    def get_pos_limits(self) -> tuple[float | None, float | None]:
        return self.min_pos, self.max_pos

    def _check_pos_range(self, pos: float):
        if self.min_pos is not None and pos < self.min_pos:
            raise ValueError(f"pos={pos} is below limit {self.min_pos}")
        if self.max_pos is not None and pos > self.max_pos:
            raise ValueError(f"pos={pos} is above limit {self.max_pos}")

    def initialize(self, home: bool = False, timeout: float = 30.0) -> dict:
        if home:
            self.home()
            self.wait_until_stopped(timeout=timeout)
        pos = self.get_pos_raw()
        pos_min, pos_max = self.get_pos_limits()
        unit_key = f"pos_{self.pos_unit.lower().replace('/', '_')}"
        return {
            "axis": self.axis,
            "state": self.get_state(),
            "moving": self.is_moving(),
            "actuator": self.actuator_name,
            unit_key: pos,
            "pos_limits": (pos_min, pos_max),
            "origin_pos": self.origin_pos,
            "pos_unit": self.pos_unit,
            "pos_digits": self.pos_digits,
        }

    def _move_position(
        self,
        pos: float,
        timeout: float = 30.0,
        *,
        on_position: Callable[[float], None] | None = None,
    ) -> float:
        target = self._round_pos(float(pos))
        self._check_pos_range(target)
        if on_position is None:
            return self._round_pos(self.controller.move_abs_raw(target, timeout=timeout))
        return self._round_pos(self.controller.move_abs_raw(target, timeout=timeout, on_position=on_position))

    def move_pos_raw(
        self,
        pos_raw: float,
        timeout: float = 30.0,
        *,
        on_position: Callable[[float], None] | None = None,
    ) -> float:
        return self._move_position(pos_raw, timeout=timeout, on_position=on_position)

    def move_pos_mm(
        self,
        pos_mm: float,
        timeout: float = 30.0,
        *,
        on_position: Callable[[float], None] | None = None,
    ) -> float:
        self._require_pos_unit("mm")
        return self._move_position(pos_mm, timeout=timeout, on_position=on_position)

    def move_pos_deg(
        self,
        pos_deg: float,
        timeout: float = 30.0,
        *,
        on_position: Callable[[float], None] | None = None,
    ) -> float:
        self._require_pos_unit("deg")
        return self._move_position(pos_deg, timeout=timeout, on_position=on_position)

    def _move_relative_position(
        self,
        delta: float,
        timeout: float = 30.0,
        *,
        on_position: Callable[[float], None] | None = None,
    ) -> float:
        delta = self._round_pos(float(delta))
        target = self._round_pos(self.get_pos_raw() + delta)
        self._check_pos_range(target)
        if on_position is None:
            return self._round_pos(self.controller.move_rel_raw(delta, timeout=timeout))
        return self._round_pos(self.controller.move_rel_raw(delta, timeout=timeout, on_position=on_position))

    def move_relative_pos_raw(
        self,
        delta_raw: float,
        timeout: float = 30.0,
        *,
        on_position: Callable[[float], None] | None = None,
    ) -> float:
        return self._move_relative_position(delta_raw, timeout=timeout, on_position=on_position)

    def move_relative_pos_mm(
        self,
        delta_mm: float,
        timeout: float = 30.0,
        *,
        on_position: Callable[[float], None] | None = None,
    ) -> float:
        self._require_pos_unit("mm")
        return self._move_relative_position(delta_mm, timeout=timeout, on_position=on_position)

    def move_relative_pos_deg(
        self,
        delta_deg: float,
        timeout: float = 30.0,
        *,
        on_position: Callable[[float], None] | None = None,
    ) -> float:
        self._require_pos_unit("deg")
        return self._move_relative_position(delta_deg, timeout=timeout, on_position=on_position)

    def stop(self):
        self.controller.stop()

    def home(self):
        self.controller.home()


def connect_scanner(config: dict) -> Scanner:
    controller_name = _controller_name(config)
    target = config["port"]
    merged = _build_scanner_config(config)
    axis_key = _axis_key(merged)
    cache_key = (controller_name, target, axis_key)
    label = controller_name if merged.get("actuator") is None else f"{controller_name}/{merged['actuator']}"

    try:
        cached = _SCANNER_CONNECTIONS.get(cache_key)
        if cached is not None and cached.is_connected():
            cached.configure(merged)
            pos = cached.get_pos_raw()
            print(f"[SCANNER] Already connected: {label} @ {target} axis={axis_key} (pos={pos:.4f}{cached.pos_unit})")
            return cached

        print(f"[SCANNER] Not connected: {label} @ {target} axis={axis_key}; connecting...")
        ser = _SCANNER_SERIALS.get(target)
        if ser is None or not ser.is_open:
            ser = serial.Serial(
                port=target,
                baudrate=int(merged.get("baudrate", 921600)),
                bytesize=serial.EIGHTBITS,
                parity=serial.PARITY_NONE,
                stopbits=serial.STOPBITS_ONE,
                timeout=float(merged.get("timeout", 1.0)),
                xonxoff=False,
                rtscts=False,
                dsrdtr=False,
            )
            _SCANNER_SERIALS[target] = ser

        controller = _build_scanner_controller(merged, ser=ser)
        scanner = Scanner(controller=controller, config=merged)
        _SCANNER_CONNECTIONS[cache_key] = scanner
        pos = scanner.get_pos_raw()
        print(f"[SCANNER] Connected: {label} @ {target} axis={axis_key} (pos={pos:.4f}{scanner.pos_unit})")
        return scanner
    except Exception as e:
        raise RuntimeError(f"[SCANNER] Connection failed: {controller_name} @ {target} | {e}")


def disconnect_scanner(config: dict | None = None):
    if config is None:
        keys = list(_SCANNER_CONNECTIONS.keys())
    else:
        keys = [(_controller_name(config), config["port"], _axis_key(config))]

    for key in keys:
        scanner = _SCANNER_CONNECTIONS.pop(key, None)
        if scanner is not None:
            ser = scanner.ser
            try:
                scanner.close()
            except Exception:
                pass
            serial_is_shared = any(inst.ser is ser for inst in _SCANNER_CONNECTIONS.values())
            if ser is not None and not serial_is_shared:
                try:
                    if ser.is_open:
                        ser.close()
                except Exception:
                    pass
                _SCANNER_SERIALS.pop(scanner.port, None)
