from __future__ import annotations

from dataclasses import dataclass, field
from threading import RLock
from typing import Any, cast

import pyvisa

from kohdalab.instruments.lockin import LOCKIN_CONTROLLERS
from kohdalab.interfaces.protocols import LockinController


_LOCKIN_CONNECTIONS: dict[tuple[str, str], "Lockin"] = {}
_LOCKIN_CONNECTIONS_LOCK = RLock()


def open_visa(resource: str, timeout: int = 5000) -> Any:
    rm = pyvisa.ResourceManager()
    inst = rm.open_resource(resource)
    inst.timeout = timeout
    return inst


def list_visa_resources() -> tuple[str, ...]:
    rm = pyvisa.ResourceManager()
    try:
        return rm.list_resources()
    finally:
        rm.close()


def _model_name(config: dict[str, Any]) -> str:
    return str(config.get("lockin_model", config.get("model", "SR7265")))


def _build_lockin_config(config: dict[str, Any]) -> dict[str, Any]:
    settings = dict(config)
    settings["model"] = _model_name(settings)
    return settings


def _build_lockin_controller(config: dict[str, Any]) -> LockinController:
    model_name = _model_name(config)
    controller_cls = LOCKIN_CONTROLLERS.get(model_name)
    if controller_cls is None:
        raise ValueError(f"Unsupported lockin model: {model_name}")

    inst = open_visa(config["resource"])
    try:
        return cast(LockinController, controller_cls(inst))
    except Exception as exc:
        try:
            inst.close()
        except Exception as cleanup_exc:
            raise RuntimeError(
                f"Controller construction failed: {exc}; VISA cleanup failed: {cleanup_exc}"
            ) from exc
        raise


@dataclass(slots=True)
class Lockin:
    controller: LockinController
    config: dict[str, Any]
    _io_lock: Any = field(default_factory=RLock, init=False, repr=False)

    def configure(self, config: dict[str, Any]) -> None:
        with self._io_lock:
            if hasattr(self.controller, "configure"):
                self.controller.configure()
            self.config = config

    def close(self) -> None:
        with self._io_lock:
            self.controller.close()

    def is_connected(self) -> bool:
        with self._io_lock:
            return self.controller.is_connected()

    def ask(self, cmd: str, delay: float = 0.001) -> str:
        with self._io_lock:
            return self.controller.ask(cmd, delay=delay)

    def ask_float(self, cmd: str, delay: float = 0.001) -> float:
        with self._io_lock:
            return self.controller.ask_float(cmd, delay=delay)

    def get_live_data_raw(self) -> dict[str, Any]:
        with self._io_lock:
            return self.controller.get_live_data_raw()

    def get_time_constant(self) -> float:
        with self._io_lock:
            return self.controller.get_time_constant()

    def get_ac_gain(self) -> float | None:
        with self._io_lock:
            return self.controller.get_ac_gain()

    def get_sensitivity(self) -> float:
        with self._io_lock:
            return self.controller.get_sensitivity()

    def get_ref_freq(self) -> float:
        with self._io_lock:
            return self.controller.get_ref_freq()

    def get_available_couplings(self) -> list[str]:
        with self._io_lock:
            return self.controller.get_available_couplings()

    def get_available_slopes(self) -> list[int]:
        with self._io_lock:
            return self.controller.get_available_slopes()

    def get_available_time_constants(self) -> list[float]:
        with self._io_lock:
            return self.controller.get_available_time_constants()

    def get_available_sensitivities(self) -> list[float]:
        with self._io_lock:
            return self.controller.get_available_sensitivities()

    def get_available_ac_gains(self) -> list[float]:
        with self._io_lock:
            return self.controller.get_available_ac_gains()

    def get_coupling(self) -> str:
        with self._io_lock:
            return self.controller.get_coupling()

    def get_slope(self) -> int:
        with self._io_lock:
            return self.controller.get_slope()

    def get_overload_status(self) -> dict[str, Any]:
        with self._io_lock:
            return self.controller.get_overload_status()

    def get_wait_time(self, multiplier: float = 4.0) -> float:
        with self._io_lock:
            return self.controller.get_wait_time(multiplier=multiplier)

    def auto_phase(self) -> None:
        with self._io_lock:
            self.controller.auto_phase()

    def auto_sensitivity(self) -> None:
        with self._io_lock:
            self.controller.auto_sensitivity()

    def auto_measure(self) -> None:
        with self._io_lock:
            self.controller.auto_measure()

    def set_sensitivity(self, value: float) -> None:
        with self._io_lock:
            self.controller.set_sensitivity(value)

    def set_time_constant(self, value: float) -> None:
        with self._io_lock:
            self.controller.set_time_constant(value)

    def set_ac_gain(self, value: float) -> None:
        with self._io_lock:
            self.controller.set_ac_gain(value)

    def set_coupling(self, value: str) -> None:
        with self._io_lock:
            self.controller.set_coupling(value)

    def set_slope(self, value: int) -> None:
        with self._io_lock:
            self.controller.set_slope(value)


def connect_lockin(config: dict[str, Any]) -> Lockin:
    with _LOCKIN_CONNECTIONS_LOCK:
        return _connect_lockin(config)


def _connect_lockin(config: dict[str, Any]) -> Lockin:
    model_name = _model_name(config)
    target = config["resource"]
    cache_key = (model_name, target)
    merged = _build_lockin_config(config)

    lockin: Lockin | None = None
    try:
        cached = _LOCKIN_CONNECTIONS.get(cache_key)
        if cached is not None:
            if cached.is_connected():
                previous_config = cached.config
                cached.configure(merged)
                try:
                    data = cached.get_live_data_raw()
                except Exception as probe_exc:
                    try:
                        cached.configure(previous_config)
                    except Exception as rollback_exc:
                        raise RuntimeError(
                            f"Cached lock-in probe failed: {probe_exc}; "
                            f"configuration rollback failed: {rollback_exc}"
                        ) from probe_exc
                    raise
                print(
                    f"[LOCKIN] Already connected: {model_name} @ {target} (X={data['X']:.3e})"
                )
                return cached
            cached.close()
            _LOCKIN_CONNECTIONS.pop(cache_key, None)

        print(f"[LOCKIN] Not connected: {model_name} @ {target}; connecting...")
        controller = _build_lockin_controller(merged)
        lockin = Lockin(controller=controller, config=merged)
        data = lockin.get_live_data_raw()
        _LOCKIN_CONNECTIONS[cache_key] = lockin
        print(f"[LOCKIN] Connected: {model_name} @ {target} (X={data['X']:.3e})")
        return lockin
    except Exception as e:
        cleanup_error: Exception | None = None
        if lockin is not None and _LOCKIN_CONNECTIONS.get(cache_key) is not lockin:
            try:
                lockin.close()
            except Exception as exc:
                cleanup_error = exc
        suffix = "" if cleanup_error is None else f"; cleanup failed: {cleanup_error}"
        raise RuntimeError(
            f"[LOCKIN] Connection failed: {model_name} @ {target} | {e}{suffix}"
        ) from e


def read_lockin_signal(
    config: dict[str, Any] | None = None, *, lockin: Lockin | None = None
) -> dict[str, Any]:
    instrument = lockin or connect_lockin(config or {})
    return dict(instrument.get_live_data_raw())


def get_lockin_wait_time(
    config: dict[str, Any] | None = None,
    *,
    lockin: Lockin | None = None,
    multiplier: float = 4.0,
) -> float:
    instrument = lockin or connect_lockin(config or {})
    return float(instrument.get_wait_time(multiplier=multiplier))


def disconnect_lockin(config: dict[str, Any] | None = None) -> None:
    with _LOCKIN_CONNECTIONS_LOCK:
        _disconnect_lockin(config)


def _disconnect_lockin(config: dict[str, Any] | None = None) -> None:
    if config is None:
        keys = list(_LOCKIN_CONNECTIONS.keys())
    else:
        keys = [(_model_name(config), config["resource"])]

    failures: list[tuple[tuple[str, str], Exception]] = []
    for key in keys:
        lockin = _LOCKIN_CONNECTIONS.get(key)
        if lockin is None:
            continue
        try:
            lockin.close()
        except Exception as exc:
            failures.append((key, exc))
        else:
            _LOCKIN_CONNECTIONS.pop(key, None)

    if failures:
        details = "; ".join(
            f"{model} @ {resource}: {exc}" for (model, resource), exc in failures
        )
        raise RuntimeError(f"[LOCKIN] Disconnect failed: {details}") from failures[0][1]
