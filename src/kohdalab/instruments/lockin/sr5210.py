from __future__ import annotations

import math
import re
import time


_SENSITIVITY_TABLE = {
    0: 100e-9,
    1: 300e-9,
    2: 1e-6,
    3: 3e-6,
    4: 10e-6,
    5: 30e-6,
    6: 100e-6,
    7: 300e-6,
    8: 1e-3,
    9: 3e-3,
    10: 10e-3,
    11: 30e-3,
    12: 100e-3,
    13: 300e-3,
    14: 1.0,
    15: 3.0,
}

_TIME_CONSTANT_TABLE = {
    0: 1e-3,
    1: 3e-3,
    2: 10e-3,
    3: 30e-3,
    4: 100e-3,
    5: 300e-3,
    6: 1.0,
    7: 3.0,
    8: 10.0,
    9: 30.0,
    10: 100.0,
    11: 300.0,
    12: 1e3,
    13: 3e3,
}

_SLOPE_MAP = {
    0: 6,
    1: 12,
}

_SLOPE_MAP_INV = {value: key for key, value in _SLOPE_MAP.items()}

_FLOAT_RE = re.compile(r"[-+]?(?:\d+(?:\.\d*)?|\.\d+)(?:[Ee][-+]?\d+)?")


class SR5210:
    def __init__(self, inst):
        self.inst = inst
        self.inst.write_termination = "\r"
        self.inst.read_termination = "\n"
        self.default_delay = 0.02
        self.retry_delays = (0.02, 0.05)

    def configure(self):
        return None

    def ask(self, cmd: str, delay: float | None = None) -> str:
        delay = self.default_delay if delay is None else delay
        self.inst.write(cmd)
        time.sleep(delay)
        response = self.inst.read().strip()
        for retry_delay in self.retry_delays:
            if response:
                break
            time.sleep(retry_delay)
            try:
                response = self.inst.read().strip()
            except Exception:
                response = ""
        return response

    def ask_responses(self, cmd: str, response_count: int, delay: float | None = None) -> list[str]:
        delay = self.default_delay if delay is None else delay
        self.inst.write(cmd)
        time.sleep(delay)
        responses = []
        for _ in range(response_count):
            response = self.inst.read().strip()
            for retry_delay in self.retry_delays:
                if response:
                    break
                time.sleep(retry_delay)
                try:
                    response = self.inst.read().strip()
                except Exception:
                    response = ""
            responses.append(response)
        return responses

    def ask_float(self, cmd: str, delay: float | None = None) -> float:
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

    def ask_floats(self, cmd: str, expected_count: int, delay: float | None = None) -> list[float]:
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

    @staticmethod
    def _scaled_signal_value(raw: float, sensitivity: float) -> float:
        return float(raw) * float(sensitivity) / 10000.0

    def get_live_data_raw(self) -> dict:
        sensitivity = self.get_sensitivity()
        x_raw = y_raw = r_raw = theta_mdeg = None
        try:
            xy_response, mp_response = self.ask_responses("XY;MP", response_count=2)
            x_raw, y_raw = self.parse_float_response(xy_response, expected_count=2, cmd="XY")
            r_raw, theta_mdeg = self.parse_float_response(mp_response, expected_count=2, cmd="MP")
        except Exception:
            pass

        if x_raw is None or y_raw is None:
            try:
                x_raw, y_raw = self.ask_floats("XY", expected_count=2)
            except Exception:
                x_raw = y_raw = None

        if r_raw is None or theta_mdeg is None:
            try:
                r_raw, theta_mdeg = self.ask_floats("MP", expected_count=2)
            except Exception:
                r_raw = theta_mdeg = None

        if x_raw is None or y_raw is None or r_raw is None or theta_mdeg is None:
            try:
                x_response, y_response, r_response, theta_response = self.ask_responses(
                    "X;Y;MAG;PHA",
                    response_count=4,
                )
                if x_raw is None:
                    x_raw = self.parse_float_response(x_response, expected_count=1, cmd="X")[0]
                if y_raw is None:
                    y_raw = self.parse_float_response(y_response, expected_count=1, cmd="Y")[0]
                if r_raw is None:
                    r_raw = self.parse_float_response(r_response, expected_count=1, cmd="MAG")[0]
                if theta_mdeg is None:
                    theta_mdeg = self.parse_float_response(theta_response, expected_count=1, cmd="PHA")[0]
            except Exception:
                pass

        if x_raw is None or y_raw is None:
            x_raw = self.ask_float("X")
            y_raw = self.ask_float("Y")

        if r_raw is None or theta_mdeg is None:
            try:
                r_raw = self.ask_float("MAG")
                theta_mdeg = self.ask_float("PHA")
            except Exception:
                r_raw = math.hypot(float(x_raw), float(y_raw))
                theta_mdeg = math.degrees(math.atan2(float(y_raw), float(x_raw))) * 1000.0

        return {
            "X": self._scaled_signal_value(x_raw, sensitivity),
            "Y": self._scaled_signal_value(y_raw, sensitivity),
            "R": self._scaled_signal_value(r_raw, sensitivity),
            "Theta": float(theta_mdeg) / 1000.0,
        }

    def get_time_constant(self) -> float:
        index = int(self.ask_float("TC"))
        return self._table_value(index, _TIME_CONSTANT_TABLE, "time constant")

    def get_ac_gain(self) -> None:
        return None

    def get_sensitivity(self) -> float:
        index = int(self.ask_float("SEN"))
        return self._table_value(index, _SENSITIVITY_TABLE, "sensitivity")

    def get_ref_freq(self) -> float:
        return self.ask_float("FRQ") / 1000.0

    def get_available_couplings(self) -> list[str]:
        return ["AC"]

    def get_available_slopes(self) -> list[int]:
        return list(_SLOPE_MAP.values())

    def get_available_time_constants(self) -> list[float]:
        return list(_TIME_CONSTANT_TABLE.values())

    def get_available_sensitivities(self) -> list[float]:
        return list(_SENSITIVITY_TABLE.values())

    def get_available_ac_gains(self) -> list[float]:
        return []

    def get_coupling(self) -> str:
        return "AC"

    def get_slope(self) -> int:
        value = int(self.ask_float("XDB"))
        try:
            return _SLOPE_MAP[value]
        except KeyError as e:
            raise RuntimeError(f"Unexpected slope mode: {value}") from e

    def get_overload_status(self) -> dict:
        value = int(self.ask_float("N"))
        raw_input_overload = bool(value & (1 << 6))
        y_output_overload = bool(value & (1 << 3))
        x_output_overload = bool(value & (1 << 4))
        psd_overload = bool(value & (1 << 5))
        any_overload = any((y_output_overload, x_output_overload, psd_overload, raw_input_overload))
        status = {
            "current_mode_1e8": bool(value & (1 << 1)),
            "current_mode_1e6": bool(value & (1 << 2)),
            "y_output_overload": y_output_overload,
            "x_output_overload": x_output_overload,
            "psd_overload": psd_overload,
            "raw_input_overload": raw_input_overload,
            "input_overload": any_overload,
            "reference_unlock": bool(value & (1 << 7)),
            "overload_byte": value,
            "overload": any_overload,
        }
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
        index = self._resolve_index_from_table(float(value), _SENSITIVITY_TABLE, "sensitivity")
        self.inst.write(f"SEN {index}")

    def set_time_constant(self, value: float):
        index = self._resolve_index_from_table(float(value), _TIME_CONSTANT_TABLE, "time constant")
        self.inst.write(f"TC {index}")

    def set_ac_gain(self, value: float):
        raise NotImplementedError("SR5210 does not expose SR7265-style AC gain.")

    def set_coupling(self, value: str):
        if value.strip().upper() != "AC":
            raise ValueError("SR5210 exposes only AC coupling through this API.")

    def set_slope(self, value: int):
        try:
            index = _SLOPE_MAP_INV[int(value)]
        except KeyError as e:
            raise ValueError(f"Unsupported slope: {value}. Use one of 6, 12 dB/oct.") from e
        self.inst.write(f"XDB {index}")

    def close(self):
        self.inst.close()

    def is_connected(self) -> bool:
        return getattr(self.inst, "session", None) is not None
