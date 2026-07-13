from __future__ import annotations

import re
import time
from collections.abc import Callable
from numbers import Integral
from typing import Any, Optional

import serial


class Shot302GS:
    """Raw controller API for Sigma Koki SHOT-302GS.

    This class talks directly to the SHOT-302GS controller and exposes
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
        inst: serial.Serial | None = None,
    ) -> None:
        self.port = port
        self.axis_count, self.default_axis = self._validated_axes(
            axis_count,
            default_axis,
        )
        self.pos_unit = pos_unit
        self.write_termination = write_termination
        self.read_termination = read_termination
        self.inst = inst or serial.Serial(
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
    ) -> None:
        candidate_axis_count = self.axis_count if axis_count is None else axis_count
        candidate_default_axis = (
            self.default_axis if default_axis is None else default_axis
        )
        validated_axis_count, validated_default_axis = self._validated_axes(
            candidate_axis_count,
            candidate_default_axis,
        )
        if pos_unit is not None:
            self.pos_unit = pos_unit
        self.axis_count = validated_axis_count
        self.default_axis = validated_default_axis

    @staticmethod
    def _validated_axes(axis_count: int, default_axis: int) -> tuple[int, int]:
        if isinstance(axis_count, bool) or not isinstance(axis_count, Integral):
            raise ValueError("axis_count must be a positive integer.")
        if isinstance(default_axis, bool) or not isinstance(default_axis, Integral):
            raise ValueError("default_axis must be an integer.")
        normalized_count = int(axis_count)
        normalized_axis = int(default_axis)
        if normalized_count < 1:
            raise ValueError("axis_count must be a positive integer.")
        if normalized_axis < 1 or normalized_axis > normalized_count:
            raise ValueError(
                f"default_axis {normalized_axis} is outside 1..{normalized_count}"
            )
        return normalized_count, normalized_axis

    def close(self) -> None:
        if self.inst is not None:
            self.inst.close()

    def is_connected(self) -> bool:
        return self.inst is not None and self.inst.is_open

    def ask(self, cmd: str) -> str:
        """Send one command and return the controller response as text."""
        raw = self._exchange(cmd, expect_response=True)
        try:
            response = raw.decode("ascii").strip()
        except UnicodeDecodeError as e:
            raise RuntimeError(f"{self.port} SHOT-302GS returned non-ASCII data") from e
        if not response:
            raise TimeoutError(
                f"{self.port} SHOT-302GS timed out waiting for {cmd} response"
            )
        return response

    def _exchange(self, cmd: str, *, expect_response: bool) -> bytes:
        if not self.is_connected():
            raise ConnectionError(f"{self.port} SHOT-302GS serial connection is closed")
        termination = self.read_termination.encode("ascii")
        try:
            self.inst.reset_input_buffer()
            self.inst.write((cmd + self.write_termination).encode("ascii"))
            if not expect_response:
                return b""
            raw = self.inst.read_until(termination)
        except (serial.SerialException, OSError) as e:
            raise ConnectionError(
                f"{self.port} SHOT-302GS serial I/O failed for {cmd}: {e}"
            ) from e
        if not raw or not raw.endswith(termination):
            raise TimeoutError(
                f"{self.port} SHOT-302GS timed out waiting for {cmd} response"
            )
        return raw

    def debug_query(self, cmd: str) -> str:
        resp = self.ask(cmd)
        print(f"{cmd!r} -> {resp!r}")
        return resp

    def debug_query_raw(
        self, cmd: str, *, write_termination: Optional[str] = None, wait: float = 0.2
    ) -> bytes:
        term = (
            self.write_termination if write_termination is None else write_termination
        )
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

    def send(self, cmd: str) -> None:
        self._exchange(cmd, expect_response=False)

    def get_status(self) -> str:
        return self.ask("Q:")

    def is_ready(self) -> bool:
        response = self.ask("!:").upper()
        if response not in {"R", "B"}:
            raise RuntimeError(f"Unexpected SHOT-302GS ! response: {response!r}")
        return response == "R"

    def get_positions(self) -> list[int]:
        """Parse raw axis positions from the `Q:` status response."""
        response = self.get_status()
        fields = [field.strip() for field in response.split(",")]
        if len(fields) < self.axis_count:
            raise RuntimeError(f"Unexpected SHOT-302GS Q response: {response!r}")
        positions: list[int] = []
        for field in fields[: self.axis_count]:
            if re.fullmatch(r"[+-]?\s*\d+", field) is None:
                raise RuntimeError(f"Unexpected SHOT-302GS Q response: {response!r}")
            positions.append(int(field.replace(" ", "")))
        return positions

    def _axis(self, axis: Optional[int]) -> int:
        selected = self.default_axis if axis is None else axis
        if isinstance(selected, bool) or not isinstance(selected, Integral):
            raise ValueError(f"Axis must be an integer: {selected!r}")
        normalized = int(selected)
        if normalized < 1 or normalized > self.axis_count:
            raise ValueError(f"Axis {normalized} is outside 1..{self.axis_count}")
        return normalized

    @staticmethod
    def _pulse(value: int, name: str) -> int:
        if isinstance(value, bool) or not isinstance(value, Integral):
            raise ValueError(f"{name} must be an integer pulse value.")
        return int(value)

    def get_pos_raw(self, axis: Optional[int] = None) -> int:
        axis = self._axis(axis)
        positions = self.get_positions()
        return positions[axis - 1]

    def get_pos_unit(self) -> str:
        return self.pos_unit

    def wait_ready(self, poll_interval: float = 0.1, timeout: float = 240.0) -> None:
        t0 = time.monotonic()
        while time.monotonic() - t0 < timeout:
            if self.is_ready():
                return
            time.sleep(poll_interval)
        raise TimeoutError("SHOT-302GS stayed busy.")

    def wait_position(
        self,
        target_pos_raw: int,
        axis: Optional[int] = None,
        poll_interval: float = 0.2,
        timeout: float = 240.0,
        on_position: Callable[[int], None] | None = None,
    ) -> None:
        target_pos_raw = self._pulse(target_pos_raw, "target_pos_raw")
        axis = self._axis(axis)
        t0 = time.monotonic()
        while time.monotonic() - t0 < timeout:
            current = self.get_pos_raw(axis=axis)
            if on_position is not None:
                on_position(current)
            if current == target_pos_raw and self.is_ready():
                return
            time.sleep(poll_interval)
        current = self.get_pos_raw(axis=axis)
        raise TimeoutError(
            f"SHOT-302GS did not reach target: target={target_pos_raw}, current={current}"
        )

    def initialize(self, home: bool = False) -> dict[str, Any]:
        axis = self._axis(None)
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

    def execute_drive(self) -> None:
        self._check_response(self.ask("G:"), "G")

    def set_logical_zero(self) -> None:
        self._check_response(self.ask("R:"), "R")

    def set_excitation(self, enabled: bool, axis: Optional[int] = None) -> None:
        if not isinstance(enabled, bool):
            raise ValueError("enabled must be boolean.")
        axis = self._axis(axis)
        mode = 1 if enabled else 0
        self._check_response(self.ask(f"C:{axis}{mode}"), "C")

    def jog(self, positive: bool = True, axis: Optional[int] = None) -> None:
        if not isinstance(positive, bool):
            raise ValueError("positive must be boolean.")
        axis = self._axis(axis)
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
        pos_raw = self._pulse(pos_raw, "pos_raw")
        axis = self._axis(axis)
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
        delta_raw = self._pulse(delta_raw, "delta_raw")
        axis = self._axis(axis)
        start = self.get_pos_raw(axis=axis)
        target = start + delta_raw
        sign = "+" if delta_raw >= 0 else "-"
        self._check_response(self.ask(f"M:{axis}{sign}P{abs(delta_raw)}"), "M")
        self.execute_drive()
        self.wait_position(target, axis=axis, on_position=on_position)
        return self.get_pos_raw(axis=axis)

    def home(self, axis: Optional[int] = None) -> None:
        axis = self._axis(axis)
        self._check_response(self.ask(f"H:{axis}"), "H")
        self.wait_ready()

    def stop(self) -> None:
        self._check_response(self.ask("L:E"), "L")

    def set_speed(self, axis: int, min_pps: int, max_pps: int, accel_ms: int) -> None:
        axis = self._axis(axis)
        min_pps = self._pulse(min_pps, "min_pps")
        max_pps = self._pulse(max_pps, "max_pps")
        accel_ms = self._pulse(accel_ms, "accel_ms")
        if min_pps < 0 or max_pps < 0 or accel_ms < 0:
            raise ValueError("speed and acceleration values must be non-negative.")
        if min_pps > max_pps:
            raise ValueError("min_pps must not exceed max_pps.")
        self._check_response(self.ask(f"D:{axis}S{min_pps}F{max_pps}R{accel_ms}"), "D")

    def query_internal(self, code: str) -> str:
        """Query an internal SHOT-302GS parameter via `?:...`."""
        return self.ask(f"?:{code}")

    def get_microstep_division(self, axis: Optional[int] = None) -> int:
        # SHOT-302GS returns the microstep division itself via `?:S{axis}`.
        axis = self._axis(axis)
        responses = []
        for _attempt in range(3):
            resp = self.query_internal(f"S{axis}")
            responses.append(resp)
            values = re.findall(r"\d+", resp)
            if values:
                return int(values[-1])
            time.sleep(0.05)
        raise RuntimeError(f"Unexpected SHOT-302GS ?S response: {responses[-1]}")

    def query_sensor_status(self) -> str:
        return self.query_internal("L")

    def query_microstep(self) -> str:
        return self.query_internal("MT")

    def write_internal(self, code: str, value: str | int) -> str:
        return self.ask(f"Z:{code}{value}")

    @staticmethod
    def _check_response(resp: str, cmd_name: str) -> None:
        if not resp:
            raise RuntimeError(f"{cmd_name} command returned an empty response")
        if "NG" in resp.upper() or "ERR" in resp.upper():
            raise RuntimeError(f"{cmd_name} command failed: {resp}")
