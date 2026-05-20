from __future__ import annotations

import re
import time


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

_FLOAT_RE = re.compile(r"[-+]?(?:\d+(?:\.\d*)?|\.\d+)(?:[Ee][-+]?\d+)?")


class SR830:
    def __init__(self, inst):
        self.inst = inst
        self.inst.write_termination = "\n"
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

    @staticmethod
    def _table_value(index: int, table: dict[int, float], label: str) -> float:
        try:
            return table[index]
        except KeyError as e:
            raise RuntimeError(f"Unexpected {label} index: {index}") from e

    def get_live_data_raw(self) -> dict:
        x, y, r, theta = self.ask_floats("SNAP?1,2,3,4", expected_count=4)
        return {"X": x, "Y": y, "R": r, "Theta": theta}

    def get_time_constant(self) -> float:
        index = int(self.ask_float("OFLT?"))
        return self._table_value(index, _TIME_CONSTANT_TABLE, "time constant")

    def get_ac_gain(self) -> None:
        return None

    def get_sensitivity(self) -> float:
        index = int(self.ask_float("SENS?"))
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
        value = int(self.ask_float("ICPL?"))
        try:
            return _COUPLING_MAP[value]
        except KeyError as e:
            raise RuntimeError(f"Unexpected coupling mode: {value}") from e

    def get_slope(self) -> int:
        value = int(self.ask_float("OFSL?"))
        try:
            return _SLOPE_MAP[value]
        except KeyError as e:
            raise RuntimeError(f"Unexpected slope mode: {value}") from e

    def get_overload_status(self) -> dict:
        value = int(self.ask_float("LIAS?"))
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
        return multiplier * self.get_time_constant()

    def auto_phase(self):
        self.inst.write("APHS")

    def auto_sensitivity(self):
        self.inst.write("AGAN")

    def auto_measure(self):
        self.inst.write("AGAN")

    def set_sensitivity(self, value: float):
        index = self._resolve_index_from_table(float(value), _SENSITIVITY_TABLE, "sensitivity")
        self.inst.write(f"SENS {index}")

    def set_time_constant(self, value: float):
        index = self._resolve_index_from_table(float(value), _TIME_CONSTANT_TABLE, "time constant")
        self.inst.write(f"OFLT {index}")

    def set_ac_gain(self, value: float):
        raise NotImplementedError("SR830 does not expose SR7265-style AC gain.")

    def set_coupling(self, value: str):
        normalized = value.strip().upper()
        try:
            index = _COUPLING_MAP_INV[normalized]
        except KeyError as e:
            raise ValueError(f"Unsupported coupling: {value}. Use AC or DC.") from e
        self.inst.write(f"ICPL {index}")

    def set_slope(self, value: int):
        try:
            index = _SLOPE_MAP_INV[int(value)]
        except KeyError as e:
            raise ValueError(f"Unsupported slope: {value}. Use one of 6, 12, 18, 24 dB/oct.") from e
        self.inst.write(f"OFSL {index}")

    def close(self):
        self.inst.close()

    def is_connected(self) -> bool:
        return getattr(self.inst, "session", None) is not None
