from __future__ import annotations

from dataclasses import dataclass

import pyvisa

from kohdalab.instruments.lockin import LOCKIN_CONTROLLERS


_LOCKIN_CONNECTIONS: dict[tuple[str, str], "Lockin"] = {}


def open_visa(resource: str, timeout: int = 5000):
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


def _model_name(config: dict) -> str:
    return config.get("lockin_model", config.get("model", "SR7265"))


def _build_lockin_config(config: dict) -> dict:
    settings = dict(config)
    settings["model"] = _model_name(settings)
    return settings


def _build_lockin_controller(config: dict):
    model_name = _model_name(config)
    controller_cls = LOCKIN_CONTROLLERS.get(model_name)
    if controller_cls is None:
        raise ValueError(f"Unsupported lockin model: {model_name}")

    inst = open_visa(config["resource"])
    return controller_cls(inst)


@dataclass(slots=True)
class Lockin:
    controller: object
    config: dict

    def configure(self, config: dict):
        self.config = config
        if hasattr(self.controller, "configure"):
            self.controller.configure()

    def close(self):
        self.controller.close()

    def is_connected(self) -> bool:
        return self.controller.is_connected()

    def ask(self, cmd: str, delay: float = 0.001) -> str:
        return self.controller.ask(cmd, delay=delay)

    def ask_float(self, cmd: str, delay: float = 0.001) -> float:
        return self.controller.ask_float(cmd, delay=delay)

    def get_live_data_raw(self) -> dict:
        return self.controller.get_live_data_raw()

    def get_time_constant(self) -> float:
        return self.controller.get_time_constant()

    def get_ac_gain(self) -> float:
        return self.controller.get_ac_gain()

    def get_sensitivity(self) -> float:
        return self.controller.get_sensitivity()

    def get_ref_freq(self) -> float:
        return self.controller.get_ref_freq()

    def get_available_couplings(self) -> list[str]:
        return self.controller.get_available_couplings()

    def get_available_slopes(self) -> list[int]:
        return self.controller.get_available_slopes()

    def get_available_time_constants(self) -> list[float]:
        return self.controller.get_available_time_constants()

    def get_available_sensitivities(self) -> list[float]:
        return self.controller.get_available_sensitivities()

    def get_available_ac_gains(self) -> list[float]:
        return self.controller.get_available_ac_gains()

    def get_coupling(self) -> str:
        return self.controller.get_coupling()

    def get_slope(self) -> int:
        return self.controller.get_slope()

    def get_overload_status(self) -> dict:
        return self.controller.get_overload_status()

    def get_wait_time(self, multiplier: float = 4.0) -> float:
        return self.controller.get_wait_time(multiplier=multiplier)

    def auto_phase(self):
        self.controller.auto_phase()

    def auto_sensitivity(self):
        self.controller.auto_sensitivity()

    def auto_measure(self):
        self.controller.auto_measure()

    def set_sensitivity(self, value: float):
        self.controller.set_sensitivity(value)

    def set_time_constant(self, value: float):
        self.controller.set_time_constant(value)

    def set_ac_gain(self, value: float):
        self.controller.set_ac_gain(value)

    def set_coupling(self, value: str):
        self.controller.set_coupling(value)

    def set_slope(self, value: int):
        self.controller.set_slope(value)


def connect_lockin(config: dict) -> Lockin:
    model_name = _model_name(config)
    target = config["resource"]
    cache_key = (model_name, target)
    merged = _build_lockin_config(config)

    try:
        cached = _LOCKIN_CONNECTIONS.get(cache_key)
        if cached is not None and cached.is_connected():
            cached.configure(merged)
            data = cached.get_live_data_raw()
            print(f"[LOCKIN] Already connected: {model_name} @ {target} (X={data['X']:.3e})")
            return cached

        print(f"[LOCKIN] Not connected: {model_name} @ {target}; connecting...")
        controller = _build_lockin_controller(merged)
        lockin = Lockin(controller=controller, config=merged)
        _LOCKIN_CONNECTIONS[cache_key] = lockin
        data = lockin.get_live_data_raw()
        print(f"[LOCKIN] Connected: {model_name} @ {target} (X={data['X']:.3e})")
        return lockin
    except Exception as e:
        raise RuntimeError(f"[LOCKIN] Connection failed: {model_name} @ {target} | {e}")


def read_lockin_signal(config: dict | None = None, *, lockin: Lockin | None = None) -> dict:
    instrument = lockin or connect_lockin(config or {})
    return dict(instrument.get_live_data_raw())


def get_lockin_wait_time(
    config: dict | None = None,
    *,
    lockin: Lockin | None = None,
    multiplier: float = 4.0,
) -> float:
    instrument = lockin or connect_lockin(config or {})
    return float(instrument.get_wait_time(multiplier=multiplier))


def disconnect_lockin(config: dict | None = None):
    if config is None:
        keys = list(_LOCKIN_CONNECTIONS.keys())
    else:
        keys = [(_model_name(config), config["resource"])]

    for key in keys:
        lockin = _LOCKIN_CONNECTIONS.pop(key, None)
        if lockin is not None:
            try:
                lockin.close()
            except Exception:
                pass
