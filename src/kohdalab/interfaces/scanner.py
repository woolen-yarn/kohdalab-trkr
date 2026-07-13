from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass, field
from decimal import Decimal
from pathlib import Path
from threading import RLock
from typing import Any, cast

import serial

from kohdalab.instruments.scanner import SCANNER_CONTROLLERS
from kohdalab.interfaces.common import load_toml, merge_config, midpoint
from kohdalab.interfaces.protocols import ScannerController


def _normalize_actuator(actuator: str | None) -> str | None:
    if actuator is None:
        return None
    return actuator.upper().replace("-", "")


_ACTUATORS_PATH = (
    Path(__file__).resolve().parent.parent / "instruments" / "scanner" / "actuator.toml"
)
_ACTUATORS_RAW = load_toml(_ACTUATORS_PATH)
ACTUATOR_NAMES = list(_ACTUATORS_RAW)

ACTUATORS = {
    _normalize_actuator(name) or name: settings
    for name, settings in _ACTUATORS_RAW.items()
}


_SCANNER_CONNECTIONS: dict[tuple[str, str, str], "Scanner"] = {}
_SCANNER_SERIALS: dict[str, serial.Serial] = {}
_SCANNER_SERIAL_LOCKS: dict[str, RLock] = {}
_SCANNER_CONNECTIONS_LOCK = RLock()


def _controller_name(config: dict[str, Any]) -> str:
    return str(config.get("scanner_controller", config.get("controller", "CONEXCC")))


def _actuator_settings(actuator: str | None) -> dict[str, Any]:
    key = _normalize_actuator(actuator)
    return {} if key is None else dict(ACTUATORS.get(key, {}))


def _validate_actuator_controller(config: dict[str, Any]) -> None:
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


def _axis_key(config: dict[str, Any]) -> str:
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


def _build_scanner_config(config: dict[str, Any]) -> dict[str, Any]:
    actuator_name = config.get("actuator")
    settings = merge_config(_actuator_settings(actuator_name), config)
    settings["controller"] = _controller_name(settings)
    _validate_actuator_controller(settings)
    return settings


def _build_scanner_controller(
    config: dict[str, Any], ser: serial.Serial | None = None
) -> ScannerController:
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
        kwargs["ensure_closed_loop_on_move"] = bool(
            config["ensure_closed_loop_on_move"]
        )
    return cast(ScannerController, controller_cls(**kwargs))


@dataclass(slots=True)
class Scanner:
    controller: ScannerController
    config: dict[str, Any]
    _io_lock: Any = field(default_factory=RLock, init=False, repr=False)

    def configure(self, config: dict[str, Any]) -> None:
        with self._io_lock:
            if hasattr(self.controller, "configure"):
                try:
                    self._configure_controller(config)
                except Exception as exc:
                    try:
                        self._configure_controller(self.config)
                    except Exception as rollback_exc:
                        raise RuntimeError(
                            f"Scanner configuration failed: {exc}; "
                            f"rollback failed: {rollback_exc}"
                        ) from exc
                    raise
            self.config = config

    def _configure_controller(self, config: dict[str, Any]) -> None:
        self.controller.configure(
            axis=config.get("axis", 1),
            controller_address=int(config.get("controller_address", 1)),
            pos_unit=str(config.get("pos_unit", "mm")),
        )

    def close(self) -> None:
        with self._io_lock:
            self.controller.close()

    def is_connected(self) -> bool:
        with self._io_lock:
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
            return max(0, -exponent) if isinstance(exponent, int) else 4
        return 4

    def _round_pos(self, pos: float) -> float:
        return round(pos, self.pos_digits)

    def normalize_pos(self, pos: float) -> float:
        return self._round_pos(float(pos))

    def _get_position(self) -> float:
        return self._round_pos(self.controller.get_pos_raw())

    def get_pos_raw(self) -> float:
        with self._io_lock:
            return self._get_position()

    def _require_pos_unit(self, expected_unit: str) -> None:
        if self.pos_unit.lower() != expected_unit:
            raise ValueError(
                f"Scanner actuator unit is {self.pos_unit!r}, not {expected_unit!r}."
            )

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
    ) -> None:
        if on_position is None:
            self.controller.wait_until_stopped(timeout=timeout)
        else:
            self.controller.wait_until_stopped(timeout=timeout, on_position=on_position)

    def get_pos_limits(self) -> tuple[float | None, float | None]:
        return self.min_pos, self.max_pos

    def _check_pos_range(self, pos: float) -> None:
        if self.min_pos is not None and pos < self.min_pos:
            raise ValueError(f"pos={pos} is below limit {self.min_pos}")
        if self.max_pos is not None and pos > self.max_pos:
            raise ValueError(f"pos={pos} is above limit {self.max_pos}")

    def initialize(self, home: bool = False, timeout: float = 30.0) -> dict[str, Any]:
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
            return self._round_pos(
                self.controller.move_abs_raw(target, timeout=timeout)
            )
        return self._round_pos(
            self.controller.move_abs_raw(
                target, timeout=timeout, on_position=on_position
            )
        )

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
        return self._round_pos(
            self.controller.move_rel_raw(
                delta, timeout=timeout, on_position=on_position
            )
        )

    def move_relative_pos_raw(
        self,
        delta_raw: float,
        timeout: float = 30.0,
        *,
        on_position: Callable[[float], None] | None = None,
    ) -> float:
        return self._move_relative_position(
            delta_raw, timeout=timeout, on_position=on_position
        )

    def move_relative_pos_mm(
        self,
        delta_mm: float,
        timeout: float = 30.0,
        *,
        on_position: Callable[[float], None] | None = None,
    ) -> float:
        self._require_pos_unit("mm")
        return self._move_relative_position(
            delta_mm, timeout=timeout, on_position=on_position
        )

    def move_relative_pos_deg(
        self,
        delta_deg: float,
        timeout: float = 30.0,
        *,
        on_position: Callable[[float], None] | None = None,
    ) -> float:
        self._require_pos_unit("deg")
        return self._move_relative_position(
            delta_deg, timeout=timeout, on_position=on_position
        )

    def stop(self) -> None:
        self.controller.stop()

    def home(self) -> None:
        self.controller.home()


def connect_scanner(config: dict[str, Any]) -> Scanner:
    with _SCANNER_CONNECTIONS_LOCK:
        return _connect_scanner(config)


def _connect_scanner(config: dict[str, Any]) -> Scanner:
    controller_name = _controller_name(config)
    target = config["port"]
    merged = _build_scanner_config(config)
    axis_key = _axis_key(merged)
    cache_key = (controller_name, target, axis_key)
    label = (
        controller_name
        if merged.get("actuator") is None
        else f"{controller_name}/{merged['actuator']}"
    )

    scanner: Scanner | None = None
    ser: serial.Serial | None = None
    serial_created = False
    try:
        cached = _SCANNER_CONNECTIONS.get(cache_key)
        if cached is not None:
            if cached.is_connected():
                previous_config = cached.config
                cached.configure(merged)
                try:
                    pos = cached.get_pos_raw()
                except Exception as probe_exc:
                    try:
                        cached.configure(previous_config)
                    except Exception as rollback_exc:
                        raise RuntimeError(
                            f"Cached scanner probe failed: {probe_exc}; "
                            f"configuration rollback failed: {rollback_exc}"
                        ) from probe_exc
                    raise
                print(
                    f"[SCANNER] Already connected: {label} @ {target} axis={axis_key} (pos={pos:.4f}{cached.pos_unit})"
                )
                return cached
            cached.close()
            _SCANNER_CONNECTIONS.pop(cache_key, None)

        print(
            f"[SCANNER] Not connected: {label} @ {target} axis={axis_key}; connecting..."
        )
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
            serial_created = True

        controller = _build_scanner_controller(merged, ser=ser)
        scanner = Scanner(controller=controller, config=merged)
        io_lock = _SCANNER_SERIAL_LOCKS.get(target, RLock())
        scanner._io_lock = io_lock
        pos = scanner.get_pos_raw()
        if serial_created:
            _SCANNER_SERIALS[target] = ser
        _SCANNER_SERIAL_LOCKS[target] = io_lock
        _SCANNER_CONNECTIONS[cache_key] = scanner
        print(
            f"[SCANNER] Connected: {label} @ {target} axis={axis_key} (pos={pos:.4f}{scanner.pos_unit})"
        )
        return scanner
    except Exception as e:
        cleanup_failures: list[str] = []
        if scanner is not None and _SCANNER_CONNECTIONS.get(cache_key) is not scanner:
            try:
                scanner.close()
            except Exception as exc:
                cleanup_failures.append(f"controller cleanup failed: {exc}")
        if (
            serial_created
            and ser is not None
            and _SCANNER_SERIALS.get(target) is not ser
        ):
            try:
                if ser.is_open:
                    ser.close()
            except Exception as exc:
                _SCANNER_SERIALS[target] = ser
                cleanup_failures.append(f"serial cleanup failed: {exc}")
        suffix = "" if not cleanup_failures else "; " + "; ".join(cleanup_failures)
        raise RuntimeError(
            f"[SCANNER] Connection failed: {controller_name} @ {target} | {e}{suffix}"
        ) from e


def disconnect_scanner(config: dict[str, Any] | None = None) -> None:
    with _SCANNER_CONNECTIONS_LOCK:
        _disconnect_scanner(config)


def _disconnect_scanner(config: dict[str, Any] | None = None) -> None:
    if config is None:
        keys = list(_SCANNER_CONNECTIONS.keys())
        serial_ports = list(_SCANNER_SERIALS.keys())
    else:
        keys = [(_controller_name(config), config["port"], _axis_key(config))]
        serial_ports = [config["port"]]

    failures: list[tuple[str, Exception]] = []
    for key in keys:
        scanner = _SCANNER_CONNECTIONS.get(key)
        if scanner is None:
            continue
        try:
            scanner.close()
        except Exception as exc:
            controller, port, axis = key
            failures.append((f"{controller} @ {port} axis={axis}", exc))
            continue

        _SCANNER_CONNECTIONS.pop(key, None)

    for port in serial_ports:
        ser = _SCANNER_SERIALS.get(port)
        if ser is None:
            continue
        serial_is_shared = any(
            inst.ser is ser for inst in _SCANNER_CONNECTIONS.values()
        )
        if serial_is_shared:
            continue
        try:
            if ser.is_open:
                ser.close()
        except Exception as exc:
            failures.append((f"serial @ {port}", exc))
        else:
            _SCANNER_SERIALS.pop(port, None)
            _SCANNER_SERIAL_LOCKS.pop(port, None)

    if failures:
        details = "; ".join(f"{target}: {exc}" for target, exc in failures)
        raise RuntimeError(f"[SCANNER] Disconnect failed: {details}") from failures[0][
            1
        ]
