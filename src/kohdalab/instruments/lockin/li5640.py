from __future__ import annotations

import re
import time

from pyvisa import constants


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


class LI5640:
    def __init__(self, inst):
        self.inst = inst
        self.inst.write_termination = "\n"
        self.inst.read_termination = "\n"
        self.display_settle_s = 0.02

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

    def _restore_output_types(self, saved: str | None) -> None:
        if saved:
            self.inst.write(f"OTYP {saved}")

    def _read_output_values(self, output_types: list[int], expected_count: int) -> list[float]:
        saved = None
        try:
            saved = self.ask("OTYP?")
        except Exception:
            saved = None
        self.inst.write("OTYP " + ",".join(str(value) for value in output_types))
        try:
            return self.ask_floats("DOUT?", expected_count=expected_count)
        finally:
            self._restore_output_types(saved)

    def _read_display_parameters(self) -> tuple[int, int] | None:
        try:
            return int(self.ask_float("DDEF? 1")), int(self.ask_float("DDEF? 2"))
        except Exception:
            return None

    def _restore_display_parameters(self, saved: tuple[int, int] | None) -> None:
        if saved is None:
            return
        self.inst.write(f"DDEF 1,{saved[0]}")
        self.inst.write(f"DDEF 2,{saved[1]}")

    def _read_display_pair(self, data1_parameter: int, data2_parameter: int) -> tuple[float, float]:
        self.inst.write(f"DDEF 1,{data1_parameter}")
        self.inst.write(f"DDEF 2,{data2_parameter}")
        time.sleep(self.display_settle_s)
        first, second = self._read_output_values([1, 2], expected_count=2)
        return first, second

    def get_live_data_raw(self) -> dict:
        saved_display = self._read_display_parameters()
        try:
            x, y = self._read_display_pair(0, 0)
            r, theta = self._read_display_pair(1, 1)
            return {"X": x, "Y": y, "R": r, "Theta": theta}
        finally:
            self._restore_display_parameters(saved_display)

    def get_time_constant(self) -> float:
        index = int(self.ask_float("TCON?"))
        return self._table_value(index, _TIME_CONSTANT_TABLE, "time constant")

    def get_ac_gain(self) -> None:
        return None

    def get_sensitivity(self) -> float:
        index = int(self.ask_float("VSEN?"))
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
        value = int(self.ask_float("ICPL?"))
        try:
            return _COUPLING_MAP[value]
        except KeyError as e:
            raise RuntimeError(f"Unexpected coupling mode: {value}") from e

    def get_slope(self) -> int:
        value = int(self.ask_float("SLOP?"))
        try:
            return _SLOPE_MAP[value]
        except KeyError as e:
            raise RuntimeError(f"Unexpected slope mode: {value}") from e

    def get_overload_status(self) -> dict:
        value = int(self.ask_float("OVCR?"))
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
        return multiplier * self.get_time_constant()

    def auto_phase(self):
        self.inst.write("APHS")

    def auto_sensitivity(self):
        self.inst.write("ASEN")

    def auto_measure(self):
        self.inst.write("ASEN")

    def set_sensitivity(self, value: float):
        index = self._resolve_index_from_table(float(value), _SENSITIVITY_TABLE, "sensitivity")
        self.inst.write(f"VSEN {index}")

    def set_time_constant(self, value: float):
        index = self._resolve_index_from_table(float(value), _TIME_CONSTANT_TABLE, "time constant")
        self.inst.write(f"TCON {index}")

    def set_ac_gain(self, value: float):
        raise NotImplementedError("LI5640 does not expose SR7265-style AC gain.")

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
        self.inst.write(f"SLOP {index}")

    def release_remote(self):
        if hasattr(self.inst, "control_ren"):
            self.inst.control_ren(constants.RENLineOperation.address_gtl)

    def close(self):
        try:
            self.release_remote()
        finally:
            self.inst.close()

    def is_connected(self) -> bool:
        return getattr(self.inst, "session", None) is not None
