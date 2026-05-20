from __future__ import annotations

import re
import time
from collections.abc import Callable
from typing import Optional

import serial


class GSC01:
    """Raw controller API for Sigma Koki GSC01.

    This class talks directly to the GSC01 controller and exposes
    low-level commands in raw controller units. Physical-unit conversion
    such as `raw <-> um` is handled by `kohdalab.interfaces.delay_stage.DelayStage`.
    """

    def __init__(
        self,
        *,
        port: str,
        baudrate: int = 9600,
        timeout: float = 1.0,
        write_termination: str = "\r\n",
        read_termination: str = "\r\n",
        axis_count: int = 1,
        default_axis: int = 1,
        pos_unit: str = "pulse",
    ):
        self.port = port
        self.axis_count = axis_count
        self.default_axis = default_axis
        self.pos_unit = pos_unit
        self.write_termination = write_termination
        self.read_termination = read_termination
        self.inst = serial.Serial(
            port=port,
            baudrate=baudrate,
            bytesize=serial.EIGHTBITS,
            parity=serial.PARITY_NONE,
            stopbits=serial.STOPBITS_ONE,
            timeout=timeout,
        )

    def configure(
        self,
        *,
        axis_count: Optional[int] = None,
        default_axis: Optional[int] = None,
        pos_unit: Optional[str] = None,
    ):
        if axis_count is not None:
            self.axis_count = axis_count
        if default_axis is not None:
            self.default_axis = default_axis
        if pos_unit is not None:
            self.pos_unit = pos_unit

    def close(self):
        if self.inst is not None:
            self.inst.close()

    def is_connected(self) -> bool:
        return self.inst is not None and self.inst.is_open

    def ask(self, cmd: str) -> str:
        """Send one command and return the controller response as text."""
        self.inst.reset_input_buffer()
        self.inst.write((cmd + self.write_termination).encode("ascii"))
        return self.inst.read_until(self.read_termination.encode("ascii")).decode("ascii", errors="ignore").strip()

    def debug_query(self, cmd: str) -> str:
        resp = self.ask(cmd)
        print(f"{cmd!r} -> {resp!r}")
        return resp

    def debug_query_raw(self, cmd: str, *, write_termination: Optional[str] = None, wait: float = 0.2) -> bytes:
        term = self.write_termination if write_termination is None else write_termination
        self.inst.reset_input_buffer()
        self.inst.write((cmd + term).encode("ascii"))
        time.sleep(wait)
        data = bytearray()
        while self.inst.in_waiting:
            data.extend(self.inst.read(self.inst.in_waiting))
            time.sleep(0.02)
        raw = bytes(data)
        print(f"{cmd!r} + {term!r} -> {raw!r}")
        return raw

    def send(self, cmd: str):
        self.inst.reset_input_buffer()
        self.inst.write((cmd + self.write_termination).encode("ascii"))

    def get_status(self) -> str:
        return self.ask("Q:")

    def is_ready(self) -> bool:
        return self.ask("!:").upper() == "R"

    def get_positions(self) -> list[int]:
        """Parse raw axis positions from the `Q:` status response."""
        values = re.findall(r"[+-]?\s*\d+", self.get_status())
        positions = [int(value.replace(" ", "")) for value in values]
        return positions[: self.axis_count]

    def get_pos_raw(self, axis: Optional[int] = None) -> int:
        axis = axis or self.default_axis
        positions = self.get_positions()
        if not positions:
            raise RuntimeError(f"Unexpected GSC01 Q response: {self.get_status()}")
        if axis < 1 or axis > len(positions):
            raise ValueError(f"Axis {axis} is not in position response: {positions}")
        return positions[axis - 1]

    def get_pos_unit(self) -> str:
        return self.pos_unit

    def wait_ready(self, poll_interval: float = 0.1, timeout: float = 240.0):
        t0 = time.time()
        while time.time() - t0 < timeout:
            if self.is_ready():
                return
            time.sleep(poll_interval)
        raise TimeoutError("GSC01 stayed busy.")

    def wait_position(
        self,
        target_pos_raw: int,
        axis: Optional[int] = None,
        poll_interval: float = 0.2,
        timeout: float = 240.0,
        on_position: Callable[[int], None] | None = None,
    ):
        axis = axis or self.default_axis
        t0 = time.time()
        while time.time() - t0 < timeout:
            current = self.get_pos_raw(axis=axis)
            if on_position is not None:
                on_position(current)
            if current == target_pos_raw and self.is_ready():
                return
            time.sleep(poll_interval)
        current = self.get_pos_raw(axis=axis)
        raise TimeoutError(f"GSC01 did not reach target: target={target_pos_raw}, current={current}")

    def initialize(self, home: bool = False) -> dict:
        axis = self.default_axis
        if home:
            self.home(axis=axis)
        return {
            "ready": self.is_ready(),
            "status": self.get_status(),
            "axis": axis,
            "axis_count": self.axis_count,
            "pos_raw": self.get_pos_raw(axis=axis),
            "pos_unit": self.get_pos_unit(),
        }

    def execute_drive(self):
        self._check_response(self.ask("G:"), "G")

    def set_logical_zero(self):
        self._check_response(self.ask("R:"), "R")

    def set_excitation(self, enabled: bool, axis: Optional[int] = None):
        axis = axis or self.default_axis
        mode = 1 if enabled else 0
        self._check_response(self.ask(f"C:{axis}{mode}"), "C")

    def jog(self, positive: bool = True, axis: Optional[int] = None):
        axis = axis or self.default_axis
        sign = "+" if positive else "-"
        self._check_response(self.ask(f"J:{axis}{sign}"), "J")
        self.execute_drive()

    def move_abs_raw(
        self,
        pos_raw: int,
        axis: Optional[int] = None,
        *,
        on_position: Callable[[int], None] | None = None,
    ) -> int:
        axis = axis or self.default_axis
        sign = "+" if pos_raw >= 0 else "-"
        self._check_response(self.ask(f"A:{axis}{sign}P{abs(pos_raw)}"), "A")
        self.execute_drive()
        self.wait_position(pos_raw, axis=axis, on_position=on_position)
        return self.get_pos_raw(axis=axis)

    def move_rel_raw(
        self,
        delta_raw: int,
        axis: Optional[int] = None,
        *,
        on_position: Callable[[int], None] | None = None,
    ) -> int:
        axis = axis or self.default_axis
        start = self.get_pos_raw(axis=axis)
        target = start + delta_raw
        sign = "+" if delta_raw >= 0 else "-"
        self._check_response(self.ask(f"M:{axis}{sign}P{abs(delta_raw)}"), "M")
        self.execute_drive()
        self.wait_position(target, axis=axis, on_position=on_position)
        return self.get_pos_raw(axis=axis)

    def home(self, axis: Optional[int] = None):
        axis = axis or self.default_axis
        self._check_response(self.ask(f"H:{axis}"), "H")
        self.wait_ready()

    def stop(self):
        self._check_response(self.ask("L:E"), "L")

    def set_speed(self, axis: int, min_pps: int, max_pps: int, accel_ms: int):
        self._check_response(self.ask(f"D:{axis}S{min_pps}F{max_pps}R{accel_ms}"), "D")

    def query_internal(self, code: str) -> str:
        """Query an internal GSC01 parameter via `?:...`."""
        return self.ask(f"?:{code}")

    def get_microstep_division(self, axis: Optional[int] = None) -> int:
        # On GSC01, the effective step-division setting tracks `?:MS`.
        responses = []
        for _attempt in range(3):
            resp = self.query_internal("MS")
            responses.append(resp)
            values = re.findall(r"\d+", resp)
            if values:
                break
            time.sleep(0.05)
        else:
            raise RuntimeError(f"Unexpected GSC01 ?MS response: {responses[-1]}")

        mode = int(values[0])
        if mode == 0:
            return 1
        if mode == 1:
            return 2
        raise RuntimeError(f"Unsupported GSC01 microstep mode from ?MS: {resp}")

    def query_sensor_status(self) -> str:
        return self.query_internal("L")

    def write_internal(self, code: str, value: str | int) -> str:
        return self.ask(f"Z:{code}{value}")

    @staticmethod
    def _check_response(resp: str, cmd_name: str):
        if resp and ("NG" in resp.upper() or "ERR" in resp.upper()):
            raise RuntimeError(f"{cmd_name} command failed: {resp}")
