from __future__ import annotations

import re
import time


_SENSITIVITY_TABLE = {
    0: {
        1: 2e-9, 2: 5e-9, 3: 10e-9, 4: 20e-9, 5: 50e-9, 6: 100e-9, 7: 200e-9, 8: 500e-9,
        9: 1e-6, 10: 2e-6, 11: 5e-6, 12: 10e-6, 13: 20e-6, 14: 50e-6, 15: 100e-6, 16: 200e-6,
        17: 500e-6, 18: 1e-3, 19: 2e-3, 20: 5e-3, 21: 10e-3, 22: 20e-3, 23: 50e-3, 24: 100e-3,
        25: 200e-3, 26: 500e-3, 27: 1.0,
    },
    1: {
        1: 2e-15, 2: 5e-15, 3: 10e-15, 4: 20e-15, 5: 50e-15, 6: 100e-15, 7: 200e-15, 8: 500e-15,
        9: 1e-12, 10: 2e-12, 11: 5e-12, 12: 10e-12, 13: 20e-12, 14: 50e-12, 15: 100e-12, 16: 200e-12,
        17: 500e-12, 18: 1e-9, 19: 2e-9, 20: 5e-9, 21: 10e-9, 22: 20e-9, 23: 50e-9, 24: 100e-9,
        25: 200e-9, 26: 500e-9, 27: 1e-6,
    },
    2: {
        7: 2e-15, 8: 5e-15, 9: 10e-15, 10: 20e-15, 11: 50e-15, 12: 100e-15, 13: 200e-15, 14: 500e-15,
        15: 1e-12, 16: 2e-12, 17: 5e-12, 18: 10e-12, 19: 20e-12, 20: 50e-12, 21: 100e-12, 22: 200e-12,
        23: 500e-12, 24: 1e-9, 25: 2e-9, 26: 5e-9, 27: 10e-9,
    },
}

_TIME_CONSTANT_TABLE = {
    0: 10e-6, 1: 20e-6, 2: 40e-6, 3: 80e-6, 4: 160e-6, 5: 320e-6, 6: 640e-6,
    7: 5e-3, 8: 10e-3, 9: 20e-3, 10: 50e-3, 11: 100e-3, 12: 200e-3, 13: 500e-3,
    14: 1.0, 15: 2.0, 16: 5.0, 17: 10.0, 18: 20.0, 19: 50.0, 20: 100.0, 21: 200.0,
    22: 500.0, 23: 1e3, 24: 2e3, 25: 5e3, 26: 10e3, 27: 20e3, 28: 50e3, 29: 100e3,
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

_FLOAT_RE = re.compile(r"[-+]?(?:\d+(?:\.\d*)?|\.\d+)(?:[Ee][-+]?\d+)?")


class SR7265:
    def __init__(self, inst):
        self.inst = inst
        self.inst.write_termination = "\r"
        self.inst.read_termination = "\n"

    def configure(self):
        return None

    def ask(self, cmd: str, delay: float = 0.001) -> str:
        try:
            self.inst.clear()
        except Exception:
            pass
        self.inst.write(cmd)
        time.sleep(delay)
        return self.inst.read().strip()

    def ask_responses(self, cmd: str, response_count: int, delay: float = 0.001) -> list[str]:
        try:
            self.inst.clear()
        except Exception:
            pass
        self.inst.write(cmd)
        time.sleep(delay)
        return [self.inst.read().strip() for _ in range(response_count)]

    def ask_float(self, cmd: str, delay: float = 0.001) -> float:
        ans = self.ask(cmd, delay=delay)
        if ans == "":
            raise RuntimeError(f"Empty response for {cmd}")
        return float(ans)

    @staticmethod
    def parse_float_response(response: str, *, expected_count: int, cmd: str) -> list[float]:
        values = [float(match.group(0)) for match in _FLOAT_RE.finditer(response)]
        if len(values) != expected_count:
            raise RuntimeError(
                f"Unexpected response for {cmd}: {response!r} "
                f"(expected {expected_count} values, got {len(values)})"
            )
        return values

    def ask_floats(self, cmd: str, expected_count: int, delay: float = 0.001) -> list[float]:
        return self.parse_float_response(
            self.ask(cmd, delay=delay),
            expected_count=expected_count,
            cmd=cmd,
        )

    @staticmethod
    def _resolve_index_from_table(value: float, table: dict[int, float], label: str) -> int:
        best_index, best_value = min(table.items(), key=lambda item: abs(item[1] - value))
        tolerance = max(1e-15, abs(best_value) * 1e-9)
        if abs(best_value - value) > tolerance:
            available = ", ".join(f"{v:.6g}" for v in table.values())
            raise ValueError(f"Unsupported {label} value: {value}. Available values: {available}")
        return best_index

    def get_live_data_raw(self) -> dict:
        try:
            xy_response, mp_response = self.ask_responses("XY.;MP.", response_count=2)
            x, y = self.parse_float_response(xy_response, expected_count=2, cmd="XY.")
            r, theta = self.parse_float_response(mp_response, expected_count=2, cmd="MP.")
            return {"X": x, "Y": y, "R": r, "Theta": theta}
        except Exception:
            pass

        try:
            x, y = self.ask_floats("XY.", expected_count=2)
            r, theta = self.ask_floats("MP.", expected_count=2)
            return {"X": x, "Y": y, "R": r, "Theta": theta}
        except Exception:
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
        return int(self.ask_float("IMODE"))

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
        value = int(self.ask_float("CP"))
        try:
            return _COUPLING_MAP[value]
        except KeyError as e:
            raise RuntimeError(f"Unexpected coupling mode: {value}") from e

    def get_slope(self) -> int:
        value = int(self.ask_float("SLOPE"))
        try:
            return _SLOPE_MAP[value]
        except KeyError as e:
            raise RuntimeError(f"Unexpected slope mode: {value}") from e

    def get_overload_status(self) -> dict:
        value = int(self.ask_float("N"))
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
        return multiplier * self.get_time_constant()

    def auto_phase(self):
        self.inst.write("AQN")

    def auto_sensitivity(self):
        self.inst.write("AS")

    def auto_measure(self):
        self.inst.write("ASM")

    def set_sensitivity(self, value: float):
        imode = self.get_imode()
        table = _SENSITIVITY_TABLE.get(imode)
        if table is None:
            raise ValueError(f"Unsupported IMODE for sensitivity: {imode}")
        index = self._resolve_index_from_table(float(value), table, "sensitivity")
        self.inst.write(f"SEN {index}")

    def set_time_constant(self, value: float):
        index = self._resolve_index_from_table(float(value), _TIME_CONSTANT_TABLE, "time constant")
        self.inst.write(f"TC {index}")

    def set_ac_gain(self, value: float):
        index = self._resolve_index_from_table(float(value), _AC_GAIN_TABLE, "AC gain")
        self.inst.write(f"ACGAIN {index}")

    def set_coupling(self, value: str):
        normalized = value.strip().upper()
        try:
            index = _COUPLING_MAP_INV[normalized]
        except KeyError as e:
            raise ValueError(f"Unsupported coupling: {value}. Use AC or DC.") from e
        self.inst.write(f"CP {index}")

    def set_slope(self, value: int):
        try:
            index = _SLOPE_MAP_INV[int(value)]
        except KeyError as e:
            raise ValueError(f"Unsupported slope: {value}. Use one of 6, 12, 18, 24 dB/oct.") from e
        self.inst.write(f"SLOPE {index}")

    def close(self):
        self.inst.close()

    def is_connected(self) -> bool:
        return getattr(self.inst, "session", None) is not None
