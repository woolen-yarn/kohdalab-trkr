from __future__ import annotations

import time
from collections.abc import Callable

import serial


class ConexCC:
    """Raw controller API for Newport CONEX-CC."""

    AXIS = 1

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
    ):
        self.port = port
        self.axis = 1
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

    def configure(
        self,
        *,
        axis: int | str | None = None,
        controller_address: int | None = None,
        pos_unit: str | None = None,
    ):
        if controller_address is not None:
            self.controller_address = int(controller_address)
        if pos_unit is not None:
            self.pos_unit = pos_unit
        return None

    def close(self):
        if self._owns_serial and self.ser is not None:
            self.ser.close()

    def is_connected(self) -> bool:
        return self.ser is not None and self.ser.is_open

    def _cmd(self, body: str) -> str:
        cmd = f"{self.AXIS}{body}\r\n".encode("ascii")
        self.ser.reset_input_buffer()
        self.ser.write(cmd)
        self.ser.flush()
        return self.ser.readline().decode("ascii", errors="ignore").strip()

    def write(self, body: str):
        self._cmd(body)

    def query(self, body: str) -> str:
        return self._cmd(body)

    def debug_query(self, body: str) -> str:
        resp = self.query(body)
        print(f"{body!r} -> {resp!r}")
        return resp

    def get_state(self) -> str:
        return self.query("TS")

    def is_moving(self) -> bool:
        state = self.get_state()
        return "28" in state or "MOV" in state.upper()

    def wait_until_stopped(
        self,
        timeout: float = 30.0,
        poll_interval: float = 0.05,
        on_position: Callable[[float], None] | None = None,
    ):
        t0 = time.time()
        while True:
            if not self.is_moving():
                return
            if on_position is not None:
                on_position(self.get_pos_raw())
            if time.time() - t0 > timeout:
                raise TimeoutError(f"{self.port} motion timeout")
            time.sleep(poll_interval)

    def get_pos_unit(self) -> str:
        return self.pos_unit

    def get_pos_raw(self) -> float:
        ans = self.query("TP")
        prefix = f"{self.AXIS}TP"
        if not ans.startswith(prefix):
            raise RuntimeError(f"Unexpected TP response: {ans}")
        return float(ans[len(prefix):])

    def initialize(self, home: bool = False, timeout: float = 30.0) -> dict:
        if home:
            self.home()
            self.wait_until_stopped(timeout=timeout)
        return {
            "axis": self.AXIS,
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
        self.write(f"PA{float(pos_raw):.4f}")
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
        self.write(f"PR{float(delta_raw):.4f}")
        self.wait_until_stopped(timeout=timeout, on_position=on_position)
        pos = self.get_pos_raw()
        if on_position is not None:
            on_position(pos)
        return pos

    def stop(self):
        self.write("ST")

    def home(self):
        self.write("OR")
