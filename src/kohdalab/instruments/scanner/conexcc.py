from __future__ import annotations

import math
import time
from collections.abc import Callable
from string import hexdigits
from typing import Any

import serial


class ConexCC:
    """Raw controller API for Newport CONEX-CC."""

    AXIS = 1
    STATE_MOVING = "28"
    STATE_HOMING = "1E"
    MOTION_STATES = {STATE_MOVING, STATE_HOMING}
    READY_STATES = {"32", "33", "34", "36", "37", "38"}
    DISABLE_STATES = {"3C", "3D", "3E", "3F"}
    NOT_REFERENCED_STATES = {"0A", "0B", "0C", "0D", "0E", "0F", "10"}
    KNOWN_STATES = {
        *MOTION_STATES,
        *READY_STATES,
        *DISABLE_STATES,
        *NOT_REFERENCED_STATES,
    }

    def __init__(
        self,
        *,
        port: str,
        baudrate: int = 921600,
        timeout: float = 1.0,
        ser: serial.Serial | None = None,
        axis: int | str = 1,
        controller_address: int = 1,
        pos_unit: str = "mm",
        ensure_closed_loop_on_move: bool = True,
    ):
        self.port = port
        self.axis = 1
        self.controller_address = int(controller_address)
        self.pos_unit = pos_unit
        self.ensure_closed_loop_on_move = bool(ensure_closed_loop_on_move)
        self._closed_loop_prepared = False
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

    def configure(
        self,
        *,
        axis: int | str | None = None,
        controller_address: int | None = None,
        pos_unit: str | None = None,
    ) -> None:
        if controller_address is not None:
            controller_address = int(controller_address)
            if controller_address != self.controller_address:
                self._closed_loop_prepared = False
            self.controller_address = controller_address
        if pos_unit is not None:
            self.pos_unit = pos_unit
        return None

    def close(self) -> None:
        if self._owns_serial and self.ser is not None:
            self.ser.close()

    def is_connected(self) -> bool:
        return self.ser is not None and self.ser.is_open

    def _cmd(self, body: str, *, expect_response: bool = False) -> str:
        if not self.is_connected():
            raise ConnectionError(f"{self.port} CONEX-CC serial connection is closed")
        cmd = f"{self.controller_address}{body}\r\n".encode("ascii")
        try:
            self.ser.reset_input_buffer()
            self.ser.write(cmd)
            self.ser.flush()
            raw = self.ser.readline()
        except (serial.SerialException, OSError) as e:
            raise ConnectionError(
                f"{self.port} CONEX-CC serial I/O failed for {body}: {e}"
            ) from e
        try:
            response = raw.decode("ascii").strip()
        except UnicodeDecodeError as e:
            raise RuntimeError(f"{self.port} CONEX-CC returned non-ASCII data") from e
        if expect_response and not response:
            raise TimeoutError(
                f"{self.port} CONEX-CC timed out waiting for {body} response"
            )
        return response

    def write(self, body: str) -> None:
        self._cmd(body)

    def query(self, body: str) -> str:
        return self._cmd(body, expect_response=True)

    def debug_query(self, body: str) -> str:
        resp = self.query(body)
        print(f"{body!r} -> {resp!r}")
        return resp

    def get_state(self) -> str:
        return self.query("TS")

    def _parse_ts(self, response: str | None = None) -> tuple[str, str]:
        state = response if response is not None else self.get_state()
        prefix = f"{self.controller_address}TS"
        if not state.startswith(prefix):
            raise RuntimeError(f"Unexpected CONEX-CC TS response: {state!r}")
        payload = state[len(prefix) :]
        if len(payload) != 6 or any(
            character not in hexdigits for character in payload
        ):
            raise RuntimeError(f"Unexpected CONEX-CC TS response: {state!r}")
        return payload[:4].upper(), payload[4:6].upper()

    def get_state_code(self) -> str:
        return self._read_status()[1]

    def get_error_code(self) -> str:
        return self._parse_ts()[0]

    def _check_error(self, response: str | None = None) -> None:
        error, _state = self._parse_ts(response)
        if error != "0000":
            try:
                detail = self.query("TB")
            except Exception:
                detail = ""
            suffix = f" ({detail})" if detail else ""
            raise RuntimeError(f"{self.port} CONEX-CC error {error}{suffix}")

    def _read_status(self, response: str | None = None) -> tuple[str, str]:
        state_response = response if response is not None else self.get_state()
        self._check_error(state_response)
        error, state = self._parse_ts(state_response)
        if state not in self.KNOWN_STATES:
            raise RuntimeError(f"{self.port} CONEX-CC returned unknown state {state}")
        return error, state

    def is_moving(self) -> bool:
        return self.get_state_code() in self.MOTION_STATES

    def _set_closed_loop(self) -> None:
        response = self.query("SC?")
        prefix = f"{self.controller_address}SC"
        if not response.startswith(prefix):
            raise RuntimeError(f"Unexpected SC response: {response}")
        state = response[len(prefix) :].strip()
        if state != "1":
            self.write("SC1")

    def _wait_for_state(
        self,
        expected: set[str],
        *,
        timeout: float = 5.0,
        poll_interval: float = 0.05,
        description: str,
    ) -> None:
        t0 = time.monotonic()
        while True:
            response = self.get_state()
            _error, code = self._read_status(response)
            if code in expected:
                return
            if code in self.NOT_REFERENCED_STATES:
                raise RuntimeError(
                    f"{self.port} CONEX-CC is not referenced; run HOME/OR before moving"
                )
            if time.monotonic() - t0 > timeout:
                raise TimeoutError(f"{self.port} failed to reach {description} state")
            time.sleep(poll_interval)

    def _wait_for_ready(
        self, timeout: float = 5.0, poll_interval: float = 0.05
    ) -> None:
        self._wait_for_state(
            self.READY_STATES,
            timeout=timeout,
            poll_interval=poll_interval,
            description="READY",
        )

    def _wait_for_disable(
        self, timeout: float = 5.0, poll_interval: float = 0.05
    ) -> None:
        self._wait_for_state(
            self.DISABLE_STATES,
            timeout=timeout,
            poll_interval=poll_interval,
            description="DISABLE",
        )

    def _prepare_closed_loop_from_disable(self, timeout: float = 5.0) -> None:
        self._set_closed_loop()
        self.write("MM1")
        self._wait_for_ready(timeout=timeout)
        self._closed_loop_prepared = True

    def _ensure_ready_closed_loop(self, timeout: float = 5.0) -> None:
        response = self.get_state()
        _error, code = self._read_status(response)
        if code in self.READY_STATES:
            if self.ensure_closed_loop_on_move and not self._closed_loop_prepared:
                self.write("MM0")
                self._wait_for_disable(timeout=timeout)
                self._prepare_closed_loop_from_disable(timeout=timeout)
            return
        if code in self.DISABLE_STATES:
            self._prepare_closed_loop_from_disable(timeout=timeout)
            return
        if code in self.NOT_REFERENCED_STATES:
            raise RuntimeError(
                f"{self.port} CONEX-CC is not referenced; run HOME/OR before moving"
            )
        if code == self.STATE_HOMING:
            self.wait_until_stopped(timeout=timeout)
            self._wait_for_ready(timeout=timeout)
            return
        raise RuntimeError(
            f"{self.port} CONEX-CC is not ready for motion (state {code})"
        )

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
                    f"{self.port} CONEX-CC motion stopped in unsafe state {code}"
                )
            if on_position is not None:
                on_position(self.get_pos_raw())
            if time.monotonic() - t0 > timeout:
                raise TimeoutError(f"{self.port} motion timeout")
            time.sleep(poll_interval)

    def get_pos_unit(self) -> str:
        return self.pos_unit

    def get_pos_raw(self) -> float:
        ans = self.query("TP")
        prefix = f"{self.controller_address}TP"
        if not ans.startswith(prefix):
            raise RuntimeError(f"Unexpected TP response: {ans}")
        value = float(ans[len(prefix) :])
        if not math.isfinite(value):
            raise RuntimeError(f"{self.port} CONEX-CC returned non-finite position")
        return value

    def initialize(self, home: bool = False, timeout: float = 30.0) -> dict[str, Any]:
        if home:
            self.home()
            self.wait_until_stopped(timeout=timeout)
        else:
            response = self.get_state()
            _error, code = self._read_status(response)
            if code in self.DISABLE_STATES:
                self._set_closed_loop()
                self.write("MM1")
                self._wait_for_ready(timeout=timeout)
        return {
            "axis": self.AXIS,
            "state": self.get_state(),
            "moving": self.is_moving(),
            "error_code": self.get_error_code(),
            "state_code": self.get_state_code(),
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
            raise ValueError("CONEX-CC absolute target must be finite.")
        self._ensure_ready_closed_loop()
        self.write(f"PA{float(pos_raw):.4f}")
        self.wait_until_stopped(timeout=timeout, on_position=on_position)
        self._check_error()
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
            raise ValueError("CONEX-CC relative target must be finite.")
        self._ensure_ready_closed_loop()
        self.write(f"PR{float(delta_raw):.4f}")
        self.wait_until_stopped(timeout=timeout, on_position=on_position)
        self._check_error()
        pos = self.get_pos_raw()
        if on_position is not None:
            on_position(pos)
        return pos

    def stop(self) -> None:
        self.write("ST")

    def home(self) -> None:
        self.write("OR")
