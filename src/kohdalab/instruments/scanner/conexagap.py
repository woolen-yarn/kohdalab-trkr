from __future__ import annotations

import math
import time
from collections.abc import Callable
from string import hexdigits
from typing import Any

import serial


class ConexAgap:
    """Raw controller API for Newport CONEX-AGAP."""

    STATE_CONFIGURATION = "14"
    STATE_MOVING_CLOSED_LOOP = "28"
    STATE_STEPPING_OPEN_LOOP = "29"
    READY_STATES = {"32", "33", "34", "35", "36"}
    DISABLE_STATES = {"3C", "3D"}
    STATE_JOGGING_OPEN_LOOP = "46"
    MOTION_STATES = {
        STATE_MOVING_CLOSED_LOOP,
        STATE_STEPPING_OPEN_LOOP,
        STATE_JOGGING_OPEN_LOOP,
    }
    KNOWN_STATES = {
        STATE_CONFIGURATION,
        *READY_STATES,
        *DISABLE_STATES,
        *MOTION_STATES,
    }
    COMMAND_ERROR_CODES = {"@", "A", "B", "C", "D", "G", "I", "J", "M", "S", "U", "V"}

    def __init__(
        self,
        *,
        port: str,
        baudrate: int = 921600,
        timeout: float = 1.0,
        ser: serial.Serial | None = None,
        axis: int | str = 1,
        controller_address: int = 1,
        pos_unit: str = "deg",
    ) -> None:
        self.port = port
        self.axis = self._normalize_axis(axis)
        self.controller_address = int(controller_address)
        self.pos_unit = pos_unit
        self._owns_serial = ser is None
        self.ser = ser or serial.Serial(
            port=port,
            baudrate=baudrate,
            bytesize=serial.EIGHTBITS,
            parity=serial.PARITY_NONE,
            stopbits=serial.STOPBITS_ONE,
            timeout=timeout,
            xonxoff=False,
            rtscts=False,
            dsrdtr=False,
        )

    @staticmethod
    def _normalize_axis(axis: int | str) -> str:
        if isinstance(axis, str):
            normalized = axis.strip().upper()
            if normalized in {"U", "V"}:
                return normalized
            if normalized.isdigit():
                axis = int(normalized)
            else:
                raise ValueError(f"Unsupported CONEX-AGAP axis: {axis}")
        if int(axis) == 1:
            return "U"
        if int(axis) == 2:
            return "V"
        raise ValueError(f"Unsupported CONEX-AGAP axis: {axis}")

    def configure(
        self,
        *,
        axis: int | str | None = None,
        controller_address: int | None = None,
        pos_unit: str | None = None,
    ) -> None:
        if axis is not None:
            self.axis = self._normalize_axis(axis)
        if controller_address is not None:
            self.controller_address = int(controller_address)
        if pos_unit is not None:
            self.pos_unit = pos_unit

    def close(self) -> None:
        if self._owns_serial and self.ser is not None:
            self.ser.close()

    def is_connected(self) -> bool:
        return self.ser is not None and self.ser.is_open

    def _cmd(self, body: str, *, expect_response: bool = False) -> str:
        if not self.is_connected():
            raise ConnectionError(f"{self.port} CONEX-AGAP serial connection is closed")
        cmd = f"{self.controller_address}{body}\r\n".encode("ascii")
        try:
            self.ser.reset_input_buffer()
            self.ser.write(cmd)
            self.ser.flush()
            raw = self.ser.readline()
        except (serial.SerialException, OSError) as e:
            raise ConnectionError(
                f"{self.port} CONEX-AGAP serial I/O failed for {body}: {e}"
            ) from e
        try:
            response = raw.decode("ascii").strip()
        except UnicodeDecodeError as e:
            raise RuntimeError(f"{self.port} CONEX-AGAP returned non-ASCII data") from e
        if expect_response and not response:
            raise TimeoutError(
                f"{self.port} CONEX-AGAP timed out waiting for {body} response"
            )
        return response

    def write(self, body: str) -> None:
        self._cmd(body)
        self._check_command_error()

    def query(self, body: str) -> str:
        return self._cmd(body, expect_response=True)

    def debug_query(self, body: str) -> str:
        resp = self.query(body)
        print(f"{body!r} -> {resp!r}")
        return resp

    def get_state(self) -> str:
        return self.query("TS")

    def _check_command_error(self) -> None:
        response = self.query("TE")
        prefix = f"{self.controller_address}TE"
        if not response.startswith(prefix):
            raise RuntimeError(f"Unexpected CONEX-AGAP TE response: {response!r}")
        error = response[len(prefix) :].upper()
        if len(error) != 1 or error not in self.COMMAND_ERROR_CODES:
            raise RuntimeError(f"Unexpected CONEX-AGAP TE response: {response!r}")
        if error != "@":
            raise RuntimeError(f"{self.port} CONEX-AGAP command error {error}")

    def _parse_ts(self, response: str | None = None) -> tuple[str, str]:
        state = response if response is not None else self.get_state()
        prefix = f"{self.controller_address}TS"
        if not state.startswith(prefix):
            raise RuntimeError(f"Unexpected CONEX-AGAP TS response: {state!r}")
        payload = state[len(prefix) :]
        if len(payload) != 6 or any(
            character not in hexdigits for character in payload
        ):
            raise RuntimeError(f"Unexpected CONEX-AGAP TS response: {state!r}")
        return payload[:4].upper(), payload[4:].upper()

    def _read_status(self) -> tuple[str, str]:
        error, state = self._parse_ts()
        if error != "0000":
            raise RuntimeError(f"{self.port} CONEX-AGAP positioner error {error}")
        if state not in self.KNOWN_STATES:
            raise RuntimeError(f"{self.port} CONEX-AGAP returned unknown state {state}")
        return error, state

    def get_state_code(self) -> str:
        return self._read_status()[1]

    def get_error_code(self) -> str:
        return self._read_status()[0]

    def _ensure_ready(self, timeout: float = 5.0, poll_interval: float = 0.05) -> None:
        _error, code = self._read_status()
        if code in self.READY_STATES:
            return
        if code in self.DISABLE_STATES:
            self.write("MM1")
            t0 = time.monotonic()
            while True:
                _error, code = self._read_status()
                if code in self.READY_STATES:
                    return
                if code not in self.DISABLE_STATES:
                    raise RuntimeError(
                        f"{self.port} CONEX-AGAP failed to enter READY state (state {code})"
                    )
                if time.monotonic() - t0 > timeout:
                    raise TimeoutError(f"{self.port} failed to leave DISABLE state")
                time.sleep(poll_interval)
        raise RuntimeError(
            f"{self.port} CONEX-AGAP is not ready for motion (state {code})"
        )

    def is_moving(self) -> bool:
        return self.get_state_code() in self.MOTION_STATES

    def wait_until_stopped(
        self,
        timeout: float = 30.0,
        poll_interval: float = 0.05,
        on_position: Callable[[float], None] | None = None,
    ) -> None:
        t0 = time.monotonic()
        while True:
            _error, code = self._read_status()
            if code in self.READY_STATES:
                return
            if code not in self.MOTION_STATES:
                raise RuntimeError(
                    f"{self.port} CONEX-AGAP motion stopped in unsafe state {code}"
                )
            if on_position is not None:
                on_position(self.get_pos_raw())
            if time.monotonic() - t0 > timeout:
                raise TimeoutError(f"{self.port} motion timeout")
            time.sleep(poll_interval)

    def get_pos_unit(self) -> str:
        return self.pos_unit

    def get_pos_raw(self) -> float:
        ans = self.query(f"TP{self.axis}")
        prefix = f"{self.controller_address}TP{self.axis}"
        if not ans.startswith(prefix):
            raise RuntimeError(f"Unexpected TP response: {ans}")
        value = float(ans[len(prefix) :])
        if not math.isfinite(value):
            raise RuntimeError(f"{self.port} CONEX-AGAP returned non-finite position")
        return value

    def initialize(self, home: bool = False, timeout: float = 30.0) -> dict[str, Any]:
        self._ensure_ready()
        if home:
            self.home()
            self.wait_until_stopped(timeout=timeout)
        return {
            "axis": self.axis,
            "state": self.get_state(),
            "moving": self.is_moving(),
            "pos_raw": self.get_pos_raw(),
            "pos_unit": self.get_pos_unit(),
        }

    def move_abs_raw(
        self,
        pos_raw: float,
        timeout: float = 30.0,
        *,
        on_position: Callable[[float], None] | None = None,
    ) -> float:
        if isinstance(pos_raw, bool) or not math.isfinite(float(pos_raw)):
            raise ValueError("CONEX-AGAP absolute target must be finite.")
        self._ensure_ready()
        self.write(f"PA{self.axis}{float(pos_raw):.4f}")
        self.wait_until_stopped(timeout=timeout, on_position=on_position)
        pos = self.get_pos_raw()
        if on_position is not None:
            on_position(pos)
        return pos

    def move_rel_raw(
        self,
        delta_raw: float,
        timeout: float = 30.0,
        *,
        on_position: Callable[[float], None] | None = None,
    ) -> float:
        if isinstance(delta_raw, bool) or not math.isfinite(float(delta_raw)):
            raise ValueError("CONEX-AGAP relative target must be finite.")
        self._ensure_ready()
        self.write(f"PR{self.axis}{float(delta_raw):.4f}")
        self.wait_until_stopped(timeout=timeout, on_position=on_position)
        pos = self.get_pos_raw()
        if on_position is not None:
            on_position(pos)
        return pos

    def stop(self) -> None:
        self.write(f"ST{self.axis}")

    def home(self) -> None:
        self._ensure_ready()
