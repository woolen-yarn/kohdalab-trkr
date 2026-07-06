from __future__ import annotations

from threading import RLock
from typing import Any, Callable

from kohdalab.api.config import instrument_config, instrument_key
from kohdalab.api.devices import (
    connect_delay_stage,
    connect_lockin,
    connect_scanner,
    disconnect_delay_stage,
    disconnect_lockin,
    disconnect_scanner,
    get_lockin_wait_time,
    initialize_delay_stage as service_initialize_delay_stage,
    initialize_scanner as service_initialize_scanner,
    move_delay_stage_abs,
    move_scanner_abs,
    read_delay_stage,
    read_lockin_overload,
    read_lockin_settings,
    read_lockin_signal,
    read_scanner,
    set_lockin_settings as service_set_lockin_settings,
)
from kohdalab.api.models import LiveStatus, Position
from kohdalab.api.status import STATUS_MOVING_DELAY_STAGE, StatusCallback, moving_scanner_status


class DeviceSession:
    """Connection and device-operation layer behind the public Experiment API."""

    def __init__(self, config: dict[str, Any], *, auto_connect: bool = True):
        self.config = config
        self.auto_connect = bool(auto_connect)
        self.lockins: dict[str, Any] = {}
        self.delay_stages: dict[str, Any] = {}
        self.scanners: dict[str, Any] = {}
        self._state_lock = RLock()
        self._io_locks: dict[str, dict[str, Any]] = {
            "lockin": {},
            "delay_stage": {},
            "scanner": {},
        }

    def set_config(self, config: dict[str, Any]) -> None:
        with self._state_lock:
            self.config = config

    def connect_all(self) -> None:
        for key in self._instrument_keys("lockin"):
            self.connect_device(f"lockin.{key}")
        for key in self._instrument_keys("delay_stage"):
            self.connect_device(f"delay_stage.{key}")
        for key in self._instrument_keys("scanner"):
            self.connect_device(f"scanner.{key}")

    def connect_device(self, ref: str) -> Any:
        kind, key = self.resolve_ref(ref)
        config = self._instrument_config(kind, key)
        with self._device_lock(kind, key):
            if kind == "lockin":
                device = connect_lockin(config)
            elif kind == "delay_stage":
                device = connect_delay_stage(config)
            elif kind == "scanner":
                device = connect_scanner(config)
            else:
                raise ValueError(f"Unsupported device kind: {kind}")
            self._set_connected_handle(kind, key, device)
            return device

    def disconnect_all(self) -> None:
        for key in self._connected_keys("lockin"):
            self.disconnect_device(f"lockin.{key}")
        for key in self._connected_keys("delay_stage"):
            self.disconnect_device(f"delay_stage.{key}")
        for key in self._connected_keys("scanner"):
            self.disconnect_device(f"scanner.{key}")

    def disconnect_device(self, ref: str) -> None:
        kind, key = self.resolve_ref(ref)
        config = self._instrument_config(kind, key)
        with self._device_lock(kind, key):
            if kind == "lockin":
                disconnect_lockin(config)
            elif kind == "delay_stage":
                disconnect_delay_stage(config)
            elif kind == "scanner":
                disconnect_scanner(config)
            else:
                raise ValueError(f"Unsupported device kind: {kind}")
            self._pop_connected_handle(kind, key)

    def connected_devices(self) -> dict[str, bool]:
        connected: dict[str, bool] = {}
        with self._state_lock:
            for kind, devices in self.config.get("instruments", {}).items():
                if not isinstance(devices, dict):
                    continue
                for key in devices:
                    connected[f"{kind}.{key}"] = key in self._connected_map(kind)
        return connected

    def read_position(self) -> Position:
        rows: list[dict[str, Any]] = []
        for key in self._connected_keys("delay_stage"):
            config = self._instrument_config("delay_stage", key)
            with self._device_lock("delay_stage", key):
                delay_stage = self._connected_handle("delay_stage", key)
                if delay_stage is None:
                    continue
                rows.append(read_delay_stage(config, delay_stage=delay_stage))
        for key in self._connected_keys("scanner"):
            axis = self._scanner_axis_from_key(key)
            if axis is None:
                continue
            config = self._instrument_config("scanner", key)
            with self._device_lock("scanner", key):
                scanner = self._connected_handle("scanner", key)
                if scanner is None:
                    continue
                rows.append(read_scanner(axis, config, scanner=scanner))
        return Position.from_rows(*rows)

    def read_lockin_signal(self, ref: str = "signal") -> dict[str, Any]:
        key = self.resolve_ref(ref, default_kind="lockin")[1]
        config = self._instrument_config("lockin", key)
        with self._device_lock("lockin", key):
            lockin = self._connected_handle("lockin", key)
            if lockin is None:
                lockin = self._require_or_auto_connect("lockin", key, config)
            return read_lockin_signal(config, lockin=lockin)

    def read_lockin_settings(self, ref: str = "signal") -> dict[str, Any]:
        key = self.resolve_ref(ref, default_kind="lockin")[1]
        config = self._instrument_config("lockin", key)
        with self._device_lock("lockin", key):
            lockin = self._connected_handle("lockin", key)
            if lockin is None:
                lockin = self._require_or_auto_connect("lockin", key, config)
            return read_lockin_settings(config, lockin=lockin)

    def read_lockin_overload(self, ref: str = "signal") -> dict[str, Any]:
        key = self.resolve_ref(ref, default_kind="lockin")[1]
        config = self._instrument_config("lockin", key)
        with self._device_lock("lockin", key):
            lockin = self._connected_handle("lockin", key)
            if lockin is None:
                lockin = self._require_or_auto_connect("lockin", key, config)
            return read_lockin_overload(config, lockin=lockin)

    def lockin_wait_time(self, ref: str = "signal", *, multiplier: float = 4.0) -> float:
        key = self.resolve_ref(ref, default_kind="lockin")[1]
        config = self._instrument_config("lockin", key)
        with self._device_lock("lockin", key):
            lockin = self._connected_handle("lockin", key)
            if lockin is None:
                lockin = self._require_or_auto_connect("lockin", key, config)
            return get_lockin_wait_time(config, lockin=lockin, multiplier=multiplier)

    def set_lockin_settings(
        self,
        ref: str = "signal",
        *,
        sensitivity: float | None = None,
        time_constant: float | None = None,
        ac_gain: float | None = None,
        coupling: str | None = None,
        slope: int | None = None,
    ) -> dict[str, Any]:
        key = self.resolve_ref(ref, default_kind="lockin")[1]
        config = self._instrument_config("lockin", key)
        with self._device_lock("lockin", key):
            lockin = self._connected_handle("lockin", key)
            if lockin is None:
                lockin = self._require_or_auto_connect("lockin", key, config)
            return service_set_lockin_settings(
                config,
                lockin=lockin,
                sensitivity=sensitivity,
                time_constant=time_constant,
                ac_gain=ac_gain,
                coupling=coupling,
                slope=slope,
            )

    def read_live_status(self) -> LiveStatus:
        signal = None
        settings = None
        overload = None
        lockin_keys = self._connected_keys("lockin")
        if lockin_keys:
            key = lockin_keys[0]
            signal = self.read_lockin_signal(f"lockin.{key}")
            settings = self.read_lockin_settings(f"lockin.{key}")
            overload = self.read_lockin_overload(f"lockin.{key}")
        return LiveStatus(
            connected=self.connected_devices(),
            position=self.read_position(),
            signal=signal,
            lockin_settings=settings,
            lockin_overload=overload,
        )

    def initialize_delay_stage(self, ref: str = "delay_stage", *, on_status: StatusCallback | None = None) -> dict[str, Any]:
        key = self.resolve_ref(ref, default_kind="delay_stage")[1]
        config = self._instrument_config("delay_stage", key)
        with self._device_lock("delay_stage", key):
            info = service_initialize_delay_stage(config, on_status=on_status)
            self._set_connected_handle("delay_stage", key, connect_delay_stage(config))
            return info

    def initialize_scanner(
        self,
        axis: str,
        ref: str | None = None,
        *,
        on_status: StatusCallback | None = None,
    ) -> dict[str, Any]:
        axis = axis.strip().lower()
        if axis not in {"x", "y"}:
            raise ValueError("scanner axis must be 'x' or 'y'.")
        key = self.resolve_ref(ref or f"scanner.{axis}", default_kind="scanner")[1]
        config = self._instrument_config("scanner", key)
        with self._device_lock("scanner", key):
            info = service_initialize_scanner(axis, config, on_status=on_status)
            self._set_connected_handle("scanner", key, connect_scanner(config))
            return info

    def initialize_xy(self, *, on_status: StatusCallback | None = None) -> dict[str, Any]:
        emit = on_status or (lambda _status: None)
        emit("xy initializing")
        return {
            "x": self.initialize_scanner("x", on_status=emit),
            "y": self.initialize_scanner("y", on_status=emit),
        }

    def move_delay_stage(
        self,
        value: float,
        *,
        coordinate: str = "measurement",
        ref: str = "delay_stage",
        on_status: StatusCallback | None = None,
        on_position: Callable[[dict[str, Any]], None] | None = None,
    ) -> Position:
        key = self.resolve_ref(ref, default_kind="delay_stage")[1]
        config = self._instrument_config("delay_stage", key)
        with self._device_lock("delay_stage", key):
            delay_stage = self._connected_handle("delay_stage", key)
            if delay_stage is None:
                delay_stage = self._require_or_auto_connect("delay_stage", key, config)
            if on_status is not None:
                on_status(STATUS_MOVING_DELAY_STAGE)
            row = move_delay_stage_abs(
                delay_stage_config=config,
                coordinate=coordinate,
                value=value,
                delay_stage=delay_stage,
                on_position=on_position,
            )
        return Position.from_rows(row)

    def move_scanner(
        self,
        axis: str,
        value: float,
        *,
        coordinate: str = "measurement",
        ref: str | None = None,
        apply_software_hysteresis: bool = True,
        on_status: StatusCallback | None = None,
        on_position: Callable[[dict[str, Any]], None] | None = None,
    ) -> Position:
        axis = axis.strip().lower()
        if axis not in {"x", "y"}:
            raise ValueError("scanner axis must be 'x' or 'y'.")
        key = self.resolve_ref(ref or f"scanner.{axis}", default_kind="scanner")[1]
        config = self._instrument_config("scanner", key)
        with self._device_lock("scanner", key):
            scanner = self._connected_handle("scanner", key)
            if scanner is None:
                scanner = self._require_or_auto_connect("scanner", key, config)
            if on_status is not None:
                on_status(moving_scanner_status(axis))
            row = move_scanner_abs(
                scanner_config=config,
                axis=axis,
                coordinate=coordinate,
                value=value,
                scanner=scanner,
                apply_software_hysteresis=apply_software_hysteresis,
                on_status=on_status,
                on_position=on_position,
            )
        return Position.from_rows(row)

    def resolve_ref(self, ref: str, default_kind: str | None = None) -> tuple[str, str]:
        aliases = {
            "signal": ("lockin", None),
            "lockin": ("lockin", None),
            "delay": ("delay_stage", None),
            "delay_stage": ("delay_stage", None),
            "stage": ("delay_stage", None),
            "t": ("delay_stage", "t"),
            "scanner_x": ("scanner", "x"),
            "scanner_y": ("scanner", "y"),
            "x": ("scanner", "x"),
            "y": ("scanner", "y"),
        }
        normalized = ref.strip()
        with self._state_lock:
            if normalized in aliases:
                kind, key = aliases[normalized]
                return kind, key or instrument_key(self.config, kind)
            if "." in normalized:
                kind, key = normalized.split(".", 1)
                return self._normalize_kind(kind), key
            if default_kind is not None:
                return self._normalize_kind(default_kind), normalized
        raise ValueError(f"Device reference must be '<kind>.<key>': {ref!r}")

    def _device_lock(self, kind: str, key: str):
        with self._state_lock:
            locks = self._io_locks.setdefault(kind, {})
            lock = locks.get(key)
            if lock is None:
                lock = RLock()
                locks[key] = lock
            return lock

    def _instrument_keys(self, kind: str) -> list[str]:
        with self._state_lock:
            devices = self.config.get("instruments", {}).get(kind, {})
            return list(devices) if isinstance(devices, dict) else []

    def _instrument_config(self, kind: str, key: str) -> dict[str, Any]:
        with self._state_lock:
            return instrument_config(self.config, kind, key)

    def _connected_keys(self, kind: str) -> list[str]:
        with self._state_lock:
            return list(self._connected_map(kind))

    def _connected_handle(self, kind: str, key: str) -> Any | None:
        with self._state_lock:
            return self._connected_map(kind).get(key)

    def _set_connected_handle(self, kind: str, key: str, device: Any) -> None:
        with self._state_lock:
            self._connected_map(kind)[key] = device

    def _pop_connected_handle(self, kind: str, key: str) -> None:
        with self._state_lock:
            self._connected_map(kind).pop(key, None)

    def _connected_map(self, kind: str) -> dict[str, Any]:
        if kind == "lockin":
            return self.lockins
        if kind == "delay_stage":
            return self.delay_stages
        if kind == "scanner":
            return self.scanners
        return {}

    def _require_or_auto_connect(self, kind: str, key: str, config: dict[str, Any]) -> Any:
        if not self.auto_connect:
            raise RuntimeError(f"Device not connected: {kind}.{key}")
        if kind == "lockin":
            device = connect_lockin(config)
        elif kind == "delay_stage":
            device = connect_delay_stage(config)
        elif kind == "scanner":
            device = connect_scanner(config)
        else:
            raise ValueError(f"Unsupported device kind: {kind}")
        self._set_connected_handle(kind, key, device)
        return device

    def _normalize_kind(self, kind: str) -> str:
        aliases = {
            "lockins": "lockin",
            "delay_stages": "delay_stage",
            "stages": "delay_stage",
            "scanners": "scanner",
        }
        normalized = kind.strip().lower()
        return aliases.get(normalized, normalized)

    def _scanner_axis_from_key(self, key: str) -> str | None:
        normalized = key.lower()
        if normalized.endswith("x") or normalized == "x":
            return "x"
        if normalized.endswith("y") or normalized == "y":
            return "y"
        return None
