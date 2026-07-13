from __future__ import annotations

import logging
import time
from typing import Any

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
LOGGER = logging.getLogger(__name__)

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


class SR830:
    def __init__(self, inst: Any) -> None:
        self.inst = inst
        self.inst.write_termination = "\n"
        self.inst.read_termination = "\n"

    def configure(self) -> None:
        return None

    def ask(self, cmd: str, delay: float = 0.001) -> str:
        try:
            self.inst.clear()
        except Exception as error:
            LOGGER.debug("SR830 VISA clear failed before %s", cmd, exc_info=error)
        visa_write(self.inst, "SR830", cmd)
        time.sleep(delay)
        response = visa_read(self.inst, "SR830", cmd)
        if not response:
            raise TimeoutError(f"SR830 timed out waiting for {cmd} response")
        return response

    def ask_float(self, cmd: str, delay: float = 0.001) -> float:
        return finite_float(self.ask(cmd, delay=delay), context=f"SR830 {cmd} response")

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

    def get_live_data_raw(self) -> dict[str, Any]:
        x, y, r, theta = self.ask_floats("SNAP?1,2,3,4", expected_count=4)
        return {"X": x, "Y": y, "R": r, "Theta": theta}

    def get_time_constant(self) -> float:
        index = integer_response(
            self.ask_float("OFLT?"), context="SR830 OFLT? response"
        )
        return self._table_value(index, _TIME_CONSTANT_TABLE, "time constant")

    def get_ac_gain(self) -> None:
        return None

    def get_sensitivity(self) -> float:
        index = integer_response(
            self.ask_float("SENS?"), context="SR830 SENS? response"
        )
        return self._table_value(index, _SENSITIVITY_TABLE, "sensitivity")

    def get_ref_freq(self) -> float:
        return self.ask_float("FREQ?")

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
            self.ask_float("ICPL?"), context="SR830 ICPL? response"
        )
        try:
            return _COUPLING_MAP[value]
        except KeyError as e:
            raise RuntimeError(f"Unexpected coupling mode: {value}") from e

    def get_slope(self) -> int:
        value = integer_response(
            self.ask_float("OFSL?"), context="SR830 OFSL? response"
        )
        try:
            return _SLOPE_MAP[value]
        except KeyError as e:
            raise RuntimeError(f"Unexpected slope mode: {value}") from e

    def get_overload_status(self) -> dict[str, Any]:
        value = integer_response(
            self.ask_float("LIAS?"), context="SR830 LIAS? response"
        )
        input_overload = bool(value & (1 << 0))
        return {
            "overload": input_overload,
            "input_overload": input_overload,
            "filter_overload": bool(value & (1 << 1)),
            "reference_unlock": bool(value & (1 << 3)),
            "range_changed": bool(value & (1 << 4)),
            "time_constant_changed": bool(value & (1 << 5)),
            "data_storage_triggered": bool(value & (1 << 6)),
            "overload_byte": value,
        }

    def get_wait_time(self, multiplier: float = 4.0) -> float:
        return wait_time(multiplier, self.get_time_constant())

    def auto_phase(self) -> None:
        visa_write(self.inst, "SR830", "APHS")

    def auto_sensitivity(self) -> None:
        visa_write(self.inst, "SR830", "AGAN")

    def auto_measure(self) -> None:
        visa_write(self.inst, "SR830", "AGAN")

    def set_sensitivity(self, value: float) -> None:
        index = self._resolve_index_from_table(value, _SENSITIVITY_TABLE, "sensitivity")
        visa_write(self.inst, "SR830", f"SENS {index}")

    def set_time_constant(self, value: float) -> None:
        index = self._resolve_index_from_table(
            value, _TIME_CONSTANT_TABLE, "time constant"
        )
        visa_write(self.inst, "SR830", f"OFLT {index}")

    def set_ac_gain(self, value: float) -> None:
        raise NotImplementedError("SR830 does not expose SR7265-style AC gain.")

    def set_coupling(self, value: str) -> None:
        if not isinstance(value, str):
            raise ValueError("coupling must be AC or DC.")
        normalized = value.strip().upper()
        try:
            index = _COUPLING_MAP_INV[normalized]
        except KeyError as e:
            raise ValueError(f"Unsupported coupling: {value}. Use AC or DC.") from e
        visa_write(self.inst, "SR830", f"ICPL {index}")

    def set_slope(self, value: int) -> None:
        if isinstance(value, bool) or not isinstance(value, int):
            raise ValueError("slope must be one of 6, 12, 18, 24 dB/oct.")
        try:
            index = _SLOPE_MAP_INV[value]
        except KeyError as e:
            raise ValueError(
                f"Unsupported slope: {value}. Use one of 6, 12, 18, 24 dB/oct."
            ) from e
        visa_write(self.inst, "SR830", f"OFSL {index}")

    def close(self) -> None:
        self.inst.close()

    def is_connected(self) -> bool:
        return getattr(self.inst, "session", None) is not None
