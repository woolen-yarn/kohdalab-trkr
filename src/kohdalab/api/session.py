from __future__ import annotations

from collections.abc import Iterator
from contextlib import AbstractContextManager, contextmanager
from copy import deepcopy
from threading import RLock
from types import TracebackType
from typing import Any, Callable, Literal, Self, cast

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
from kohdalab.api.status import (
    STATUS_MOVING_DELAY_STAGE,
    StatusCallback,
    moving_scanner_status,
)


class DeviceSession:
    """Connection and device-operation layer behind the public Experiment API."""

    _ownership_lock = RLock()
    _shared_owners: dict[
        int, tuple[Any, set[tuple[object, str, str]], tuple[str, ...]]
    ] = {}
    _shared_targets: dict[tuple[str, ...], tuple[dict[str, Any], int]] = {}

    def __init__(self, config: dict[str, Any], *, auto_connect: bool = True):
        self.config = config
        self.auto_connect = bool(auto_connect)
        self.lockins: dict[str, Any] = {}
        self.delay_stages: dict[str, Any] = {}
        self.scanners: dict[str, Any] = {}
        self._connected_configs: dict[str, dict[str, dict[str, Any]]] = {
            "lockin": {},
            "delay_stage": {},
            "scanner": {},
        }
        self._owner_token = object()
        self._state_lock = RLock()
        self._io_locks: dict[str, dict[str, Any]] = {
            "lockin": {},
            "delay_stage": {},
            "scanner": {},
        }

    def set_config(self, config: dict[str, Any]) -> None:
        with self._state_lock:
            changed_refs: list[str] = []
            old_instruments = self.config.get("instruments", {})
            new_instruments = config.get("instruments", {})
            for kind in ("lockin", "delay_stage", "scanner"):
                old_devices = (
                    old_instruments.get(kind, {})
                    if isinstance(old_instruments, dict)
                    else {}
                )
                new_devices = (
                    new_instruments.get(kind, {})
                    if isinstance(new_instruments, dict)
                    else {}
                )
                for key in self._connected_map(kind):
                    pinned = self._connected_configs.get(kind, {}).get(key)
                    old_device = pinned
                    if old_device is None:
                        old_device = (
                            old_devices.get(key)
                            if isinstance(old_devices, dict)
                            else None
                        )
                    new_device = (
                        new_devices.get(key) if isinstance(new_devices, dict) else None
                    )
                    if old_device != new_device:
                        changed_refs.append(f"{kind}.{key}")
            if changed_refs:
                refs = ", ".join(changed_refs)
                raise RuntimeError(
                    f"Disconnect devices before changing their config: {refs}"
                )
            self.config = config

    def __enter__(self) -> Self:
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> Literal[False]:
        del exc_type, traceback
        try:
            self.close()
        except Exception as cleanup_exc:
            if exc is None:
                raise
            exc.add_note(f"DeviceSession cleanup also failed: {cleanup_exc}")
        return False

    def close(self) -> None:
        """Release every device lease held by this session."""
        self.disconnect_all()

    def connect_all(self) -> None:
        refs = [
            *(f"lockin.{key}" for key in self._instrument_keys("lockin")),
            *(f"delay_stage.{key}" for key in self._instrument_keys("delay_stage")),
            *(f"scanner.{key}" for key in self._instrument_keys("scanner")),
        ]
        acquired: list[str] = []
        try:
            for ref in refs:
                kind, key = self.resolve_ref(ref)
                already_connected = self._connected_handle(kind, key) is not None
                self.connect_device(ref)
                if not already_connected:
                    acquired.append(ref)
        except BaseException as error:
            rollback_failures: list[tuple[str, BaseException]] = []
            for ref in reversed(acquired):
                try:
                    self.disconnect_device(ref)
                except BaseException as rollback_error:
                    rollback_failures.append((ref, rollback_error))
            if rollback_failures:
                details = "; ".join(
                    f"{ref}: {rollback_error}"
                    for ref, rollback_error in rollback_failures
                )
                error.add_note(f"connect_all rollback also failed: {details}")
            raise

    def connect_device(self, ref: str) -> Any:
        kind, key = self.resolve_ref(ref)
        config = self._instrument_config(kind, key)
        return self._connect_owned(kind, key, config)

    def disconnect_all(self) -> None:
        failures: list[tuple[str, Exception]] = []
        refs = [
            *(f"lockin.{key}" for key in self._connected_keys("lockin")),
            *(f"delay_stage.{key}" for key in self._connected_keys("delay_stage")),
            *(f"scanner.{key}" for key in self._connected_keys("scanner")),
        ]
        for ref in refs:
            try:
                self.disconnect_device(ref)
            except Exception as error:
                failures.append((ref, error))
        if failures:
            details = "; ".join(f"{ref}: {error}" for ref, error in failures)
            raise RuntimeError(
                f"Failed to disconnect one or more devices: {details}"
            ) from failures[0][1]

    def disconnect_device(self, ref: str) -> None:
        kind, key = self.resolve_ref(ref)
        handle = self._connected_handle(kind, key)
        if handle is None:
            return
        lock = self._device_lock(kind, key)
        with self._ownership_lock:
            with lock:
                handle = self._connected_handle(kind, key)
                if handle is None:
                    return
                config = self._pinned_instrument_config(kind, key)
                if config is None:
                    config = self._instrument_config(kind, key)
                owner = (self._owner_token, kind, key)
                anchor = self._ownership_anchor(handle)
                anchor_id = id(anchor)
                entry = self._shared_owners.get(anchor_id)
                if entry is not None and owner in entry[1] and len(entry[1]) > 1:
                    entry[1].remove(owner)
                    self._pop_connected_handle(kind, key)
                    return

                self._disconnect_handle(kind, config)
                if entry is not None:
                    entry[1].discard(owner)
                    # A multi-owner entry returns above. Reaching this point with an
                    # entry therefore means its final registered owner is releasing
                    # the handle, and the matching target claim can be removed too.
                    self._shared_owners.pop(anchor_id, None)
                    self._shared_targets.pop(entry[2], None)
                self._pop_connected_handle(kind, key)

    def connected_devices(self) -> dict[str, bool]:
        with self._state_lock:
            refs: set[tuple[str, str]] = set()
            for kind, devices in self.config.get("instruments", {}).items():
                if not isinstance(devices, dict):
                    continue
                for key in devices:
                    refs.add((kind, key))
            handles: dict[tuple[str, str], Any] = {}
            for kind in ("lockin", "delay_stage", "scanner"):
                for key, handle in self._connected_map(kind).items():
                    refs.add((kind, key))
                    handles[(kind, key)] = handle
        return {
            f"{kind}.{key}": self._handle_is_connected(handles.get((kind, key)))
            for kind, key in sorted(refs)
        }

    @contextmanager
    def _position_read_lock(
        self, kind: str, key: str, *, skip_busy: bool
    ) -> Iterator[bool]:
        lock = self._device_lock(kind, key)
        if not skip_busy:
            with lock:
                yield True
            return

        lock_with_acquire = cast(Any, lock)
        acquired = bool(lock_with_acquire.acquire(False))
        try:
            yield acquired
        finally:
            if acquired:
                lock_with_acquire.release()

    def read_position(self, *, skip_busy: bool = False) -> Position:
        rows: list[dict[str, Any]] = []
        for key in self._connected_keys("delay_stage"):
            config = self._instrument_config("delay_stage", key)
            with self._position_read_lock(
                "delay_stage", key, skip_busy=skip_busy
            ) as acquired:
                if not acquired:
                    continue
                delay_stage = self._connected_handle("delay_stage", key)
                if delay_stage is None:
                    continue
                self._ensure_handle_connected("delay_stage", key, delay_stage)
                rows.append(read_delay_stage(config, delay_stage=delay_stage))
        for key in self._connected_keys("scanner"):
            axis = self._scanner_axis_from_key(key)
            if axis is None:
                continue
            config = self._instrument_config("scanner", key)
            with self._position_read_lock(
                "scanner", key, skip_busy=skip_busy
            ) as acquired:
                if not acquired:
                    continue
                scanner = self._connected_handle("scanner", key)
                if scanner is None:
                    continue
                self._ensure_handle_connected("scanner", key, scanner)
                rows.append(read_scanner(axis, config, scanner=scanner))
        return Position.from_rows(*rows)

    def read_lockin_signal(self, ref: str = "signal") -> dict[str, Any]:
        key = self.resolve_ref(ref, default_kind="lockin")[1]
        config = self._instrument_config("lockin", key)
        lockin = self._connected_handle("lockin", key)
        if lockin is None:
            lockin = self._require_or_auto_connect("lockin", key, config)
        with self._device_lock("lockin", key):
            self._ensure_handle_connected("lockin", key, lockin)
            return read_lockin_signal(config, lockin=lockin)

    def read_lockin_settings(self, ref: str = "signal") -> dict[str, Any]:
        key = self.resolve_ref(ref, default_kind="lockin")[1]
        config = self._instrument_config("lockin", key)
        lockin = self._connected_handle("lockin", key)
        if lockin is None:
            lockin = self._require_or_auto_connect("lockin", key, config)
        with self._device_lock("lockin", key):
            self._ensure_handle_connected("lockin", key, lockin)
            return read_lockin_settings(config, lockin=lockin)

    def read_lockin_overload(self, ref: str = "signal") -> dict[str, Any]:
        key = self.resolve_ref(ref, default_kind="lockin")[1]
        config = self._instrument_config("lockin", key)
        lockin = self._connected_handle("lockin", key)
        if lockin is None:
            lockin = self._require_or_auto_connect("lockin", key, config)
        with self._device_lock("lockin", key):
            self._ensure_handle_connected("lockin", key, lockin)
            return read_lockin_overload(config, lockin=lockin)

    def lockin_wait_time(
        self, ref: str = "signal", *, multiplier: float = 4.0
    ) -> float:
        key = self.resolve_ref(ref, default_kind="lockin")[1]
        config = self._instrument_config("lockin", key)
        lockin = self._connected_handle("lockin", key)
        if lockin is None:
            lockin = self._require_or_auto_connect("lockin", key, config)
        with self._device_lock("lockin", key):
            self._ensure_handle_connected("lockin", key, lockin)
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
        lockin = self._connected_handle("lockin", key)
        if lockin is None:
            lockin = self._require_or_auto_connect("lockin", key, config)
        with self._device_lock("lockin", key):
            self._ensure_handle_connected("lockin", key, lockin)
            return service_set_lockin_settings(
                config,
                lockin=lockin,
                sensitivity=sensitivity,
                time_constant=time_constant,
                ac_gain=ac_gain,
                coupling=coupling,
                slope=slope,
            )

    def read_live_status(self, *, skip_busy_positions: bool = False) -> LiveStatus:
        signal = None
        settings = None
        overload = None
        lockin_keys = self._connected_keys("lockin")
        if lockin_keys:
            key = lockin_keys[0]
            signal = self.read_lockin_signal(f"lockin.{key}")
            settings = self.read_lockin_settings(f"lockin.{key}")
            overload = self.read_lockin_overload(f"lockin.{key}")
        position = (
            self.read_position(skip_busy=True)
            if skip_busy_positions
            else self.read_position()
        )
        return LiveStatus(
            connected=self.connected_devices(),
            position=position,
            signal=signal,
            lockin_settings=settings,
            lockin_overload=overload,
        )

    def initialize_delay_stage(
        self, ref: str = "delay_stage", *, on_status: StatusCallback | None = None
    ) -> dict[str, Any]:
        key = self.resolve_ref(ref, default_kind="delay_stage")[1]
        config = self._instrument_config("delay_stage", key)
        already_connected = self._connected_handle("delay_stage", key) is not None
        delay_stage = self._connect_owned("delay_stage", key, config)
        try:
            with self._device_lock("delay_stage", key):
                return service_initialize_delay_stage(
                    config,
                    delay_stage=delay_stage,
                    on_status=on_status,
                )
        except BaseException as error:
            if not already_connected:
                self._rollback_initialize_connection(f"delay_stage.{key}", error)
            raise

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
        already_connected = self._connected_handle("scanner", key) is not None
        scanner = self._connect_owned("scanner", key, config)
        try:
            with self._device_lock("scanner", key):
                return service_initialize_scanner(
                    axis,
                    config,
                    scanner=scanner,
                    on_status=on_status,
                )
        except BaseException as error:
            if not already_connected:
                self._rollback_initialize_connection(f"scanner.{key}", error)
            raise

    def initialize_xy(
        self, *, on_status: StatusCallback | None = None
    ) -> dict[str, Any]:
        emit = on_status or (lambda _status: None)
        emit("xy initializing")
        acquired: list[str] = []
        try:
            results: dict[str, dict[str, Any]] = {}
            for axis in ("x", "y"):
                kind, key = self.resolve_ref(f"scanner.{axis}")
                already_connected = self._connected_handle(kind, key) is not None
                results[axis] = self.initialize_scanner(axis, on_status=emit)
                if not already_connected:
                    acquired.append(f"scanner.{key}")
            return results
        except BaseException as error:
            for ref in reversed(acquired):
                self._rollback_initialize_connection(ref, error)
            raise

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
        delay_stage = self._connected_handle("delay_stage", key)
        if delay_stage is None:
            delay_stage = self._require_or_auto_connect("delay_stage", key, config)
        with self._device_lock("delay_stage", key):
            self._ensure_handle_connected("delay_stage", key, delay_stage)
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
        scanner = self._connected_handle("scanner", key)
        if scanner is None:
            scanner = self._require_or_auto_connect("scanner", key, config)
        with self._device_lock("scanner", key):
            self._ensure_handle_connected("scanner", key, scanner)
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

    def _device_lock(self, kind: str, key: str) -> AbstractContextManager[Any]:
        with self._state_lock:
            handle = self._connected_map(kind).get(key)
            if handle is not None:
                anchor = self._ownership_anchor(handle)
                shared_lock = getattr(anchor, "_io_lock", None)
                if shared_lock is not None:
                    return cast(AbstractContextManager[Any], shared_lock)
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
            pinned = self._connected_configs.get(kind, {}).get(key)
            try:
                current = instrument_config(self.config, kind, key)
            except ValueError as exc:
                if pinned is not None:
                    raise RuntimeError(
                        f"Connected device config was removed in place: {kind}.{key}. "
                        "Disconnect uses the pinned connection config."
                    ) from exc
                raise
            if pinned is not None and current != pinned:
                fields = ", ".join(self._changed_config_fields(pinned, current))
                raise RuntimeError(
                    f"Connected device config was mutated in place: {kind}.{key} "
                    f"({fields or 'unknown fields'}). Disconnect before changing it."
                )
            return deepcopy(current)

    def _pinned_instrument_config(self, kind: str, key: str) -> dict[str, Any] | None:
        with self._state_lock:
            pinned = self._connected_configs.get(kind, {}).get(key)
            return None if pinned is None else deepcopy(pinned)

    def _connected_keys(self, kind: str) -> list[str]:
        with self._state_lock:
            return list(self._connected_map(kind))

    def _connected_handle(self, kind: str, key: str) -> Any | None:
        with self._state_lock:
            return self._connected_map(kind).get(key)

    def _set_connected_handle(
        self, kind: str, key: str, device: Any, config: dict[str, Any]
    ) -> None:
        with self._state_lock:
            self._connected_map(kind)[key] = device
            self._connected_configs.setdefault(kind, {})[key] = deepcopy(config)

    def _pop_connected_handle(self, kind: str, key: str) -> None:
        with self._state_lock:
            self._connected_map(kind).pop(key, None)
            self._connected_configs.get(kind, {}).pop(key, None)

    def _connected_map(self, kind: str) -> dict[str, Any]:
        if kind == "lockin":
            return self.lockins
        if kind == "delay_stage":
            return self.delay_stages
        if kind == "scanner":
            return self.scanners
        return {}

    def _require_or_auto_connect(
        self, kind: str, key: str, config: dict[str, Any]
    ) -> Any:
        if not self.auto_connect:
            raise RuntimeError(f"Device not connected: {kind}.{key}")
        return self._connect_owned(kind, key, config)

    def _rollback_initialize_connection(
        self, ref: str, original_error: BaseException
    ) -> None:
        try:
            self.disconnect_device(ref)
        except BaseException as rollback_error:
            original_error.add_note(
                f"initialize connection rollback also failed: {ref}: {rollback_error}"
            )

    def _connect_owned(self, kind: str, key: str, config: dict[str, Any]) -> Any:
        with self._ownership_lock:
            target = self._ownership_target(kind, config)
            claim = self._shared_targets.get(target)
            if claim is not None and claim[0] != config:
                changed = sorted(
                    name
                    for name in set(claim[0]) | set(config)
                    if claim[0].get(name) != config.get(name)
                )
                fields = ", ".join(changed) or "unknown fields"
                raise RuntimeError(
                    f"Shared hardware {'.'.join(target)} is already connected with "
                    f"different instrument config fields: {fields}"
                )
            existing = self._connected_handle(kind, key)
            if existing is not None:
                self._ensure_handle_connected(kind, key, existing)
                return existing
            device = self._connect_handle(kind, config)
            anchor = self._ownership_anchor(device)
            anchor_id = id(anchor)
            entry = self._shared_owners.get(anchor_id)
            if entry is None or entry[0] is not anchor:
                entry = (anchor, set(), target)
                self._shared_owners[anchor_id] = entry
            self._shared_targets.setdefault(target, (deepcopy(config), anchor_id))
            entry[1].add((self._owner_token, kind, key))
            self._set_connected_handle(kind, key, device, config)
            return device

    @staticmethod
    def _changed_config_fields(
        original: dict[str, Any], current: dict[str, Any]
    ) -> list[str]:
        return sorted(
            name
            for name in set(original) | set(current)
            if original.get(name) != current.get(name)
        )

    @staticmethod
    def _handle_is_connected(handle: Any | None) -> bool:
        if handle is None:
            return False
        checker = getattr(handle, "is_connected", None)
        if checker is None:
            return True
        try:
            return bool(checker())
        except Exception:
            return False

    @classmethod
    def _ensure_handle_connected(cls, kind: str, key: str, handle: Any) -> None:
        if not cls._handle_is_connected(handle):
            raise RuntimeError(
                f"Device connection is stale: {kind}.{key}. "
                "Disconnect it before reconnecting."
            )

    @staticmethod
    def _ownership_anchor(device: Any) -> Any:
        return getattr(device, "_stage", device)

    @staticmethod
    def _ownership_target(kind: str, config: dict[str, Any]) -> tuple[str, ...]:
        if kind == "lockin":
            model = str(config.get("lockin_model", config.get("model", "SR7265")))
            return kind, model.strip().upper(), str(config["resource"])
        if kind == "delay_stage":
            controller = str(
                config.get(
                    "delay_stage_controller", config.get("controller", "SHOT302GS")
                )
            )
            return kind, controller.strip().upper(), str(config["port"])
        if kind == "scanner":
            controller = (
                str(
                    config.get(
                        "scanner_controller", config.get("controller", "CONEXCC")
                    )
                )
                .strip()
                .upper()
            )
            axis_value = config.get("axis", 1)
            axis = str(axis_value).strip().upper()
            if controller == "CONEXAGAP":
                axis = {"1": "U", "2": "V"}.get(axis, axis)
            else:
                try:
                    axis = str(int(axis_value))
                except (TypeError, ValueError):
                    pass
            return kind, controller, str(config["port"]), axis
        raise ValueError(f"Unsupported device kind: {kind}")

    @staticmethod
    def _connect_handle(kind: str, config: dict[str, Any]) -> Any:
        if kind == "lockin":
            return connect_lockin(config)
        if kind == "delay_stage":
            return connect_delay_stage(config)
        if kind == "scanner":
            return connect_scanner(config)
        raise ValueError(f"Unsupported device kind: {kind}")

    @staticmethod
    def _disconnect_handle(kind: str, config: dict[str, Any]) -> None:
        if kind == "lockin":
            disconnect_lockin(config)
            return
        if kind == "delay_stage":
            disconnect_delay_stage(config)
            return
        if kind == "scanner":
            disconnect_scanner(config)
            return
        raise ValueError(f"Unsupported device kind: {kind}")

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
