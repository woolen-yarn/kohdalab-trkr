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
    0: {
        1: 2e-9,
        2: 5e-9,
        3: 10e-9,
        4: 20e-9,
        5: 50e-9,
        6: 100e-9,
        7: 200e-9,
        8: 500e-9,
        9: 1e-6,
        10: 2e-6,
        11: 5e-6,
        12: 10e-6,
        13: 20e-6,
        14: 50e-6,
        15: 100e-6,
        16: 200e-6,
        17: 500e-6,
        18: 1e-3,
        19: 2e-3,
        20: 5e-3,
        21: 10e-3,
        22: 20e-3,
        23: 50e-3,
        24: 100e-3,
        25: 200e-3,
        26: 500e-3,
        27: 1.0,
    },
    1: {
        1: 2e-15,
        2: 5e-15,
        3: 10e-15,
        4: 20e-15,
        5: 50e-15,
        6: 100e-15,
        7: 200e-15,
        8: 500e-15,
        9: 1e-12,
        10: 2e-12,
        11: 5e-12,
        12: 10e-12,
        13: 20e-12,
        14: 50e-12,
        15: 100e-12,
        16: 200e-12,
        17: 500e-12,
        18: 1e-9,
        19: 2e-9,
        20: 5e-9,
        21: 10e-9,
        22: 20e-9,
        23: 50e-9,
        24: 100e-9,
        25: 200e-9,
        26: 500e-9,
        27: 1e-6,
    },
    2: {
        7: 2e-15,
        8: 5e-15,
        9: 10e-15,
        10: 20e-15,
        11: 50e-15,
        12: 100e-15,
        13: 200e-15,
        14: 500e-15,
        15: 1e-12,
        16: 2e-12,
        17: 5e-12,
        18: 10e-12,
        19: 20e-12,
        20: 50e-12,
        21: 100e-12,
        22: 200e-12,
        23: 500e-12,
        24: 1e-9,
        25: 2e-9,
        26: 5e-9,
        27: 10e-9,
    },
}
LOGGER = logging.getLogger(__name__)

_TIME_CONSTANT_TABLE = {
    0: 10e-6,
    1: 20e-6,
    2: 40e-6,
    3: 80e-6,
    4: 160e-6,
    5: 320e-6,
    6: 640e-6,
    7: 5e-3,
    8: 10e-3,
    9: 20e-3,
    10: 50e-3,
    11: 100e-3,
    12: 200e-3,
    13: 500e-3,
    14: 1.0,
    15: 2.0,
    16: 5.0,
    17: 10.0,
    18: 20.0,
    19: 50.0,
    20: 100.0,
    21: 200.0,
    22: 500.0,
    23: 1e3,
    24: 2e3,
    25: 5e3,
    26: 10e3,
    27: 20e3,
    28: 50e3,
    29: 100e3,
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

_AC_GAIN_TABLE = {
    0: 0.0,
    1: 10.0,
    2: 20.0,
    3: 30.0,
    4: 40.0,
    5: 50.0,
    6: 60.0,
    7: 70.0,
    8: 80.0,
    9: 90.0,
}


class SR7265:
    def __init__(self, inst: Any) -> None:
        self.inst = inst
        self.inst.write_termination = "\r"
        self.inst.read_termination = "\n"

    def configure(self) -> None:
        return None

    def ask(self, cmd: str, delay: float = 0.001) -> str:
        try:
            self.inst.clear()
        except Exception as error:
            LOGGER.debug("SR7265 VISA clear failed before %s", cmd, exc_info=error)
        visa_write(self.inst, "SR7265", cmd)
        time.sleep(delay)
        response = visa_read(self.inst, "SR7265", cmd)
        if not response:
            raise TimeoutError(f"SR7265 timed out waiting for {cmd} response")
        return response

    def ask_responses(
        self, cmd: str, response_count: int, delay: float = 0.001
    ) -> list[str]:
        try:
            self.inst.clear()
        except Exception as error:
            LOGGER.debug("SR7265 VISA clear failed before %s", cmd, exc_info=error)
        if (
            isinstance(response_count, bool)
            or not isinstance(response_count, int)
            or response_count < 1
        ):
            raise ValueError("response_count must be a positive integer.")
        visa_write(self.inst, "SR7265", cmd)
        time.sleep(delay)
        responses = []
        for index in range(response_count):
            response = visa_read(self.inst, "SR7265", cmd)
            if not response:
                raise TimeoutError(
                    f"SR7265 timed out waiting for response {index + 1}/{response_count} to {cmd}"
                )
            responses.append(response)
        return responses

    def ask_float(self, cmd: str, delay: float = 0.001) -> float:
        return finite_float(
            self.ask(cmd, delay=delay), context=f"SR7265 {cmd} response"
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

    def get_live_data_raw(self) -> dict[str, Any]:
        try:
            xy_response, mp_response = self.ask_responses("XY.;MP.", response_count=2)
            x, y = self.parse_float_response(xy_response, expected_count=2, cmd="XY.")
            r, theta = self.parse_float_response(
                mp_response, expected_count=2, cmd="MP."
            )
            return {"X": x, "Y": y, "R": r, "Theta": theta}
        except (RuntimeError, TimeoutError, ValueError):
            pass

        try:
            x, y = self.ask_floats("XY.", expected_count=2)
            r, theta = self.ask_floats("MP.", expected_count=2)
            return {"X": x, "Y": y, "R": r, "Theta": theta}
        except (RuntimeError, TimeoutError, ValueError):
            pass

        return {
            "X": self.ask_float("X."),
            "Y": self.ask_float("Y."),
            "R": self.ask_float("MAG."),
            "Theta": self.ask_float("PHA."),
        }

    def get_time_constant(self) -> float:
        return self.ask_float("TC.")

    def get_ac_gain(self) -> float:
        return self.ask_float("ACGAIN") * 10.0

    def get_sensitivity(self) -> float:
        return self.ask_float("SEN.")

    def get_ref_freq(self) -> float:
        return self.ask_float("FRQ.")

    def get_imode(self) -> int:
        return integer_response(
            self.ask_float("IMODE"), context="SR7265 IMODE response"
        )

    def get_available_couplings(self) -> list[str]:
        return list(_COUPLING_MAP.values())

    def get_available_slopes(self) -> list[int]:
        return list(_SLOPE_MAP.values())

    def get_available_time_constants(self) -> list[float]:
        return list(_TIME_CONSTANT_TABLE.values())

    def get_available_sensitivities(self) -> list[float]:
        imode = self.get_imode()
        table = _SENSITIVITY_TABLE.get(imode)
        if table is None:
            raise ValueError(f"Unsupported IMODE for sensitivity: {imode}")
        return list(table.values())

    def get_available_ac_gains(self) -> list[float]:
        return list(_AC_GAIN_TABLE.values())

    def get_coupling(self) -> str:
        value = integer_response(self.ask_float("CP"), context="SR7265 CP response")
        try:
            return _COUPLING_MAP[value]
        except KeyError as e:
            raise RuntimeError(f"Unexpected coupling mode: {value}") from e

    def get_slope(self) -> int:
        value = integer_response(
            self.ask_float("SLOPE"), context="SR7265 SLOPE response"
        )
        try:
            return _SLOPE_MAP[value]
        except KeyError as e:
            raise RuntimeError(f"Unexpected slope mode: {value}") from e

    def get_overload_status(self) -> dict[str, Any]:
        value = integer_response(self.ask_float("N"), context="SR7265 N response")
        status = {
            "ch1_output_overload": bool(value & (1 << 1)),
            "ch2_output_overload": bool(value & (1 << 2)),
            "y_output_overload": bool(value & (1 << 3)),
            "x_output_overload": bool(value & (1 << 4)),
            "input_overload": bool(value & (1 << 6)),
            "reference_unlock": bool(value & (1 << 7)),
            "overload_byte": value,
        }
        status["overload"] = any(
            bool(status[key])
            for key in (
                "ch1_output_overload",
                "ch2_output_overload",
                "y_output_overload",
                "x_output_overload",
                "input_overload",
            )
        )
        return status

    def get_wait_time(self, multiplier: float = 4.0) -> float:
        return wait_time(multiplier, self.get_time_constant())

    def auto_phase(self) -> None:
        visa_write(self.inst, "SR7265", "AQN")

    def auto_sensitivity(self) -> None:
        visa_write(self.inst, "SR7265", "AS")

    def auto_measure(self) -> None:
        visa_write(self.inst, "SR7265", "ASM")

    def set_sensitivity(self, value: float) -> None:
        imode = self.get_imode()
        table = _SENSITIVITY_TABLE.get(imode)
        if table is None:
            raise ValueError(f"Unsupported IMODE for sensitivity: {imode}")
        index = self._resolve_index_from_table(value, table, "sensitivity")
        visa_write(self.inst, "SR7265", f"SEN {index}")

    def set_time_constant(self, value: float) -> None:
        index = self._resolve_index_from_table(
            value, _TIME_CONSTANT_TABLE, "time constant"
        )
        visa_write(self.inst, "SR7265", f"TC {index}")

    def set_ac_gain(self, value: float) -> None:
        index = self._resolve_index_from_table(value, _AC_GAIN_TABLE, "AC gain")
        visa_write(self.inst, "SR7265", f"ACGAIN {index}")

    def set_coupling(self, value: str) -> None:
        if not isinstance(value, str):
            raise ValueError("coupling must be AC or DC.")
        normalized = value.strip().upper()
        try:
            index = _COUPLING_MAP_INV[normalized]
        except KeyError as e:
            raise ValueError(f"Unsupported coupling: {value}. Use AC or DC.") from e
        visa_write(self.inst, "SR7265", f"CP {index}")

    def set_slope(self, value: int) -> None:
        if isinstance(value, bool) or not isinstance(value, int):
            raise ValueError("slope must be one of 6, 12, 18, 24 dB/oct.")
        try:
            index = _SLOPE_MAP_INV[value]
        except KeyError as e:
            raise ValueError(
                f"Unsupported slope: {value}. Use one of 6, 12, 18, 24 dB/oct."
            ) from e
        visa_write(self.inst, "SR7265", f"SLOPE {index}")

    def close(self) -> None:
        self.inst.close()

    def is_connected(self) -> bool:
        return getattr(self.inst, "session", None) is not None
