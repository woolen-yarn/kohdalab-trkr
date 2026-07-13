from __future__ import annotations

import logging
import time
from typing import Any

from pyvisa import constants

from ._validation import (
    finite_float,
    integer_response,
    parse_float_response,
    resolve_index_from_table,
    visa_read,
    visa_write,
    wait_time,
)


_SENSITIVITY_TABLE = {
    0: 2e-9,
    1: 5e-9,
    2: 10e-9,
    3: 20e-9,
    4: 50e-9,
    5: 100e-9,
    6: 200e-9,
    7: 500e-9,
    8: 1e-6,
    9: 2e-6,
    10: 5e-6,
    11: 10e-6,
    12: 20e-6,
    13: 50e-6,
    14: 100e-6,
    15: 200e-6,
    16: 500e-6,
    17: 1e-3,
    18: 2e-3,
    19: 5e-3,
    20: 10e-3,
    21: 20e-3,
    22: 50e-3,
    23: 100e-3,
    24: 200e-3,
    25: 500e-3,
    26: 1.0,
}

_TIME_CONSTANT_TABLE = {
    0: 10e-6,
    1: 30e-6,
    2: 100e-6,
    3: 300e-6,
    4: 1e-3,
    5: 3e-3,
    6: 10e-3,
    7: 30e-3,
    8: 100e-3,
    9: 300e-3,
    10: 1.0,
    11: 3.0,
    12: 10.0,
    13: 30.0,
    14: 100.0,
    15: 300.0,
    16: 1e3,
    17: 3e3,
    18: 10e3,
    19: 30e3,
}

_COUPLING_MAP = {
    0: "AC",
    1: "DC",
}

_COUPLING_MAP_INV = {value: key for key, value in _COUPLING_MAP.items()}

_SLOPE_MAP = {
    0: 6,
    1: 12,
    2: 18,
    3: 24,
}

_SLOPE_MAP_INV = {value: key for key, value in _SLOPE_MAP.items()}
LOGGER = logging.getLogger(__name__)


class LI5640:
    def __init__(self, inst: Any) -> None:
        self.inst = inst
        self.inst.write_termination = "\n"
        self.inst.read_termination = "\n"
        self.display_settle_s = 0.02

    def configure(self) -> None:
        return None

    def ask(self, cmd: str, delay: float = 0.001) -> str:
        try:
            self.inst.clear()
        except Exception as error:
            LOGGER.debug("LI5640 VISA clear failed before %s", cmd, exc_info=error)
        visa_write(self.inst, "LI5640", cmd)
        time.sleep(delay)
        response = visa_read(self.inst, "LI5640", cmd)
        if not response:
            raise TimeoutError(f"LI5640 timed out waiting for {cmd} response")
        return response

    def ask_float(self, cmd: str, delay: float = 0.001) -> float:
        return finite_float(
            self.ask(cmd, delay=delay), context=f"LI5640 {cmd} response"
        )

    @staticmethod
    def parse_float_response(
        response: str, *, expected_count: int, cmd: str
    ) -> list[float]:
        return parse_float_response(response, expected_count=expected_count, cmd=cmd)

    def ask_floats(
        self, cmd: str, expected_count: int, delay: float = 0.001
    ) -> list[float]:
        return self.parse_float_response(
            self.ask(cmd, delay=delay),
            expected_count=expected_count,
            cmd=cmd,
        )

    @staticmethod
    def _resolve_index_from_table(
        value: float, table: dict[int, float], label: str
    ) -> int:
        return resolve_index_from_table(value, table, label)

    @staticmethod
    def _table_value(index: int, table: dict[int, float], label: str) -> float:
        try:
            return table[index]
        except KeyError as e:
            raise RuntimeError(f"Unexpected {label} index: {index}") from e

    def _restore_output_types(self, saved: str | None) -> None:
        if saved:
            visa_write(self.inst, "LI5640", f"OTYP {saved}")

    def _read_output_values(
        self, output_types: list[int], expected_count: int
    ) -> list[float]:
        saved = None
        try:
            saved = self.ask("OTYP?")
        except (RuntimeError, TimeoutError, ValueError):
            saved = None
        visa_write(
            self.inst,
            "LI5640",
            "OTYP " + ",".join(str(value) for value in output_types),
        )
        try:
            return self.ask_floats("DOUT?", expected_count=expected_count)
        finally:
            self._restore_output_types(saved)

    def _read_display_parameters(self) -> tuple[int, int] | None:
        try:
            return (
                integer_response(
                    self.ask_float("DDEF? 1"), context="LI5640 DDEF? 1 response"
                ),
                integer_response(
                    self.ask_float("DDEF? 2"), context="LI5640 DDEF? 2 response"
                ),
            )
        except (RuntimeError, TimeoutError, ValueError):
            return None

    def _restore_display_parameters(self, saved: tuple[int, int] | None) -> None:
        if saved is None:
            return
        visa_write(self.inst, "LI5640", f"DDEF 1,{saved[0]}")
        visa_write(self.inst, "LI5640", f"DDEF 2,{saved[1]}")

    def _read_display_pair(
        self, data1_parameter: int, data2_parameter: int
    ) -> tuple[float, float]:
        visa_write(self.inst, "LI5640", f"DDEF 1,{data1_parameter}")
        visa_write(self.inst, "LI5640", f"DDEF 2,{data2_parameter}")
        time.sleep(self.display_settle_s)
        first, second = self._read_output_values([1, 2], expected_count=2)
        return first, second

    def get_live_data_raw(self) -> dict[str, Any]:
        saved_display = self._read_display_parameters()
        try:
            x, y = self._read_display_pair(0, 0)
            r, theta = self._read_display_pair(1, 1)
            return {"X": x, "Y": y, "R": r, "Theta": theta}
        finally:
            self._restore_display_parameters(saved_display)

    def get_time_constant(self) -> float:
        index = integer_response(
            self.ask_float("TCON?"), context="LI5640 TCON? response"
        )
        return self._table_value(index, _TIME_CONSTANT_TABLE, "time constant")

    def get_ac_gain(self) -> None:
        return None

    def get_sensitivity(self) -> float:
        index = integer_response(
            self.ask_float("VSEN?"), context="LI5640 VSEN? response"
        )
        return self._table_value(index, _SENSITIVITY_TABLE, "sensitivity")

    def get_ref_freq(self) -> float:
        return self._read_output_values([3], expected_count=1)[0]

    def get_available_couplings(self) -> list[str]:
        return list(_COUPLING_MAP.values())

    def get_available_slopes(self) -> list[int]:
        return list(_SLOPE_MAP.values())

    def get_available_time_constants(self) -> list[float]:
        return list(_TIME_CONSTANT_TABLE.values())

    def get_available_sensitivities(self) -> list[float]:
        return list(_SENSITIVITY_TABLE.values())

    def get_available_ac_gains(self) -> list[float]:
        return []

    def get_coupling(self) -> str:
        value = integer_response(
            self.ask_float("ICPL?"), context="LI5640 ICPL? response"
        )
        try:
            return _COUPLING_MAP[value]
        except KeyError as e:
            raise RuntimeError(f"Unexpected coupling mode: {value}") from e

    def get_slope(self) -> int:
        value = integer_response(
            self.ask_float("SLOP?"), context="LI5640 SLOP? response"
        )
        try:
            return _SLOPE_MAP[value]
        except KeyError as e:
            raise RuntimeError(f"Unexpected slope mode: {value}") from e

    def get_overload_status(self) -> dict[str, Any]:
        value = integer_response(
            self.ask_float("OVCR?"), context="LI5640 OVCR? response"
        )
        raw_input_overload = bool(value & (1 << 0))
        middle_stage_overload = bool(value & (1 << 1))
        data1_display_overload = bool(value & (1 << 2))
        data2_display_overload = bool(value & (1 << 3))
        data1_ratio_display_overload = bool(value & (1 << 4))
        data2_ratio_display_overload = bool(value & (1 << 5))
        any_overload = any(
            (
                raw_input_overload,
                middle_stage_overload,
                data1_display_overload,
                data2_display_overload,
                data1_ratio_display_overload,
                data2_ratio_display_overload,
            )
        )
        return {
            "overload": any_overload,
            "input_overload": any_overload,
            "raw_input_overload": raw_input_overload,
            "middle_stage_overload": middle_stage_overload,
            "data1_display_overload": data1_display_overload,
            "data2_display_overload": data2_display_overload,
            "data1_ratio_display_overload": data1_ratio_display_overload,
            "data2_ratio_display_overload": data2_ratio_display_overload,
            "overload_byte": value,
        }

    def get_wait_time(self, multiplier: float = 4.0) -> float:
        return wait_time(multiplier, self.get_time_constant())

    def auto_phase(self) -> None:
        visa_write(self.inst, "LI5640", "APHS")

    def auto_sensitivity(self) -> None:
        visa_write(self.inst, "LI5640", "ASEN")

    def auto_measure(self) -> None:
        visa_write(self.inst, "LI5640", "ASEN")

    def set_sensitivity(self, value: float) -> None:
        index = self._resolve_index_from_table(value, _SENSITIVITY_TABLE, "sensitivity")
        visa_write(self.inst, "LI5640", f"VSEN {index}")

    def set_time_constant(self, value: float) -> None:
        index = self._resolve_index_from_table(
            value, _TIME_CONSTANT_TABLE, "time constant"
        )
        visa_write(self.inst, "LI5640", f"TCON {index}")

    def set_ac_gain(self, value: float) -> None:
        raise NotImplementedError("LI5640 does not expose SR7265-style AC gain.")

    def set_coupling(self, value: str) -> None:
        if not isinstance(value, str):
            raise ValueError("coupling must be AC or DC.")
        normalized = value.strip().upper()
        try:
            index = _COUPLING_MAP_INV[normalized]
        except KeyError as e:
            raise ValueError(f"Unsupported coupling: {value}. Use AC or DC.") from e
        visa_write(self.inst, "LI5640", f"ICPL {index}")

    def set_slope(self, value: int) -> None:
        if isinstance(value, bool) or not isinstance(value, int):
            raise ValueError("slope must be one of 6, 12, 18, 24 dB/oct.")
        try:
            index = _SLOPE_MAP_INV[value]
        except KeyError as e:
            raise ValueError(
                f"Unsupported slope: {value}. Use one of 6, 12, 18, 24 dB/oct."
            ) from e
        visa_write(self.inst, "LI5640", f"SLOP {index}")

    def release_remote(self) -> None:
        if hasattr(self.inst, "control_ren"):
            self.inst.control_ren(constants.RENLineOperation.address_gtl)

    def close(self) -> None:
        try:
            self.release_remote()
        finally:
            self.inst.close()

    def is_connected(self) -> bool:
        return getattr(self.inst, "session", None) is not None
