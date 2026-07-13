from __future__ import annotations

from collections.abc import Callable
from typing import TYPE_CHECKING, Any, Protocol

import serial


class ScannerController(Protocol):
    port: str
    ser: serial.Serial

    def configure(
        self,
        *,
        axis: int | str | None = None,
        controller_address: int | None = None,
        pos_unit: str | None = None,
    ) -> None: ...

    def close(self) -> None: ...

    def is_connected(self) -> bool: ...

    def get_pos_raw(self) -> float: ...

    def get_state(self) -> str: ...

    def is_moving(self) -> bool: ...

    def wait_until_stopped(
        self,
        timeout: float = 30.0,
        poll_interval: float = 0.05,
        on_position: Callable[[float], None] | None = None,
    ) -> None: ...

    def move_abs_raw(
        self,
        pos_raw: float,
        timeout: float = 30.0,
        *,
        on_position: Callable[[float], None] | None = None,
    ) -> float: ...

    def move_rel_raw(
        self,
        delta_raw: float,
        timeout: float = 30.0,
        *,
        on_position: Callable[[float], None] | None = None,
    ) -> float: ...

    def stop(self) -> None: ...

    def home(self) -> None: ...


class DelayStageController(Protocol):
    def configure(
        self,
        *,
        axis_count: int | None = None,
        default_axis: int | None = None,
        pos_unit: str | None = None,
    ) -> None: ...

    def close(self) -> None: ...

    def is_connected(self) -> bool: ...

    def get_microstep_division(self, axis: int | None = None) -> int: ...

    def get_pos_raw(self, axis: int | None = None) -> int: ...

    def get_positions(self) -> list[int]: ...

    def get_status(self) -> str: ...

    def is_ready(self) -> bool: ...

    def execute_drive(self) -> None: ...

    def move_abs_raw(
        self,
        pos_raw: int,
        axis: int | None = None,
        *,
        on_position: Callable[[int], None] | None = None,
    ) -> int: ...

    def move_rel_raw(
        self,
        delta_raw: int,
        axis: int | None = None,
        *,
        on_position: Callable[[int], None] | None = None,
    ) -> int: ...

    def jog(self, positive: bool = True, axis: int | None = None) -> None: ...

    def set_excitation(self, enabled: bool, axis: int | None = None) -> None: ...

    def set_logical_zero(self) -> None: ...

    def query_internal(self, code: str) -> str: ...

    def home(self, axis: int | None = None) -> None: ...

    def stop(self) -> None: ...


class LockinController(Protocol):
    def configure(self) -> None: ...

    def close(self) -> None: ...

    def is_connected(self) -> bool: ...

    def ask(self, cmd: str, delay: float = 0.001) -> str: ...

    def ask_float(self, cmd: str, delay: float = 0.001) -> float: ...

    def get_live_data_raw(self) -> dict[str, Any]: ...

    def get_time_constant(self) -> float: ...

    def get_ac_gain(self) -> float | None: ...

    def get_sensitivity(self) -> float: ...

    def get_ref_freq(self) -> float: ...

    def get_available_couplings(self) -> list[str]: ...

    def get_available_slopes(self) -> list[int]: ...

    def get_available_time_constants(self) -> list[float]: ...

    def get_available_sensitivities(self) -> list[float]: ...

    def get_available_ac_gains(self) -> list[float]: ...

    def get_coupling(self) -> str: ...

    def get_slope(self) -> int: ...

    def get_overload_status(self) -> dict[str, Any]: ...

    def get_wait_time(self, multiplier: float = 4.0) -> float: ...

    def auto_phase(self) -> None: ...

    def auto_sensitivity(self) -> None: ...

    def auto_measure(self) -> None: ...

    def set_sensitivity(self, value: float) -> None: ...

    def set_time_constant(self, value: float) -> None: ...

    def set_ac_gain(self, value: float) -> None: ...

    def set_coupling(self, value: str) -> None: ...

    def set_slope(self, value: int) -> None: ...


if TYPE_CHECKING:
    from kohdalab.instruments.delay_stage import GSC01, GSC01A, Shot302GS
    from kohdalab.instruments.lockin import LI5640, SR5210, SR7265, SR830
    from kohdalab.instruments.scanner import ConexAgap, ConexCC

    def _check_scanner_controller(controller: ConexAgap | ConexCC) -> ScannerController:
        return controller

    def _check_delay_stage_controller(
        controller: GSC01 | GSC01A | Shot302GS,
    ) -> DelayStageController:
        return controller

    def _check_lockin_controller(
        controller: LI5640 | SR5210 | SR7265 | SR830,
    ) -> LockinController:
        return controller
