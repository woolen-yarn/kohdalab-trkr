from __future__ import annotations

import pytest

import kohdalab.interfaces.scanner as scanner_module


class FakeSerial:
    instances: list["FakeSerial"] = []

    def __init__(self, *, port: str, **_kwargs):
        self.port = port
        self.is_open = True
        self.close_count = 0
        FakeSerial.instances.append(self)

    def close(self):
        self.close_count += 1
        self.is_open = False


class FakeScannerController:
    instances: list["FakeScannerController"] = []

    def __init__(
        self,
        *,
        port: str,
        ser: FakeSerial,
        axis: int | str = 1,
        pos_unit: str = "mm",
        controller_address: int = 1,
        **_kwargs,
    ):
        self.port = port
        self.ser = ser
        self.axis = axis
        self.pos_unit = pos_unit
        self.controller_address = controller_address
        self.configure_calls: list[dict[str, object]] = []
        self.close_count = 0
        FakeScannerController.instances.append(self)

    def configure(
        self,
        *,
        axis: int | str | None = None,
        controller_address: int | None = None,
        pos_unit: str | None = None,
    ):
        self.configure_calls.append(
            {
                "axis": axis,
                "controller_address": controller_address,
                "pos_unit": pos_unit,
            }
        )
        if axis is not None:
            self.axis = axis
        if controller_address is not None:
            self.controller_address = controller_address
        if pos_unit is not None:
            self.pos_unit = pos_unit

    def close(self):
        self.close_count += 1

    def is_connected(self) -> bool:
        return self.ser.is_open

    def get_pos_raw(self) -> float:
        return 0.0

    def get_state(self) -> str:
        return "ready"

    def is_moving(self) -> bool:
        return False

    def wait_until_stopped(self, timeout: float = 30.0, on_position=None):
        if on_position is not None:
            on_position(self.get_pos_raw())
        return None

    def initialize(self, home: bool = False, timeout: float = 30.0) -> dict:
        return {"axis": self.axis, "pos_raw": 0.0, "pos_unit": self.pos_unit}

    def move_abs_raw(self, pos_raw: float, timeout: float = 30.0, on_position=None) -> float:
        if on_position is not None:
            on_position(float(pos_raw))
        return float(pos_raw)

    def move_rel_raw(self, delta_raw: float, timeout: float = 30.0, on_position=None) -> float:
        if on_position is not None:
            on_position(float(delta_raw))
        return float(delta_raw)

    def stop(self):
        return None

    def home(self):
        return None


@pytest.fixture(autouse=True)
def fake_scanner_backend(monkeypatch):
    scanner_module._SCANNER_CONNECTIONS.clear()
    scanner_module._SCANNER_SERIALS.clear()
    FakeSerial.instances.clear()
    FakeScannerController.instances.clear()
    monkeypatch.setattr(scanner_module.serial, "Serial", FakeSerial)
    monkeypatch.setitem(scanner_module.SCANNER_CONTROLLERS, "CONEXAGAP", FakeScannerController)
    monkeypatch.setitem(scanner_module.SCANNER_CONTROLLERS, "CONEXCC", FakeScannerController)
    yield
    scanner_module._SCANNER_CONNECTIONS.clear()
    scanner_module._SCANNER_SERIALS.clear()


def agap_config(axis: str) -> dict:
    return {
        "controller": "CONEXAGAP",
        "actuator": "AG-M100D",
        "port": "COM11",
        "axis": axis,
        "sample_um_per_unit": 582.0,
    }


def conexcc_config(*, port: str = "COM5", axis: int = 1) -> dict:
    return {
        "controller": "CONEXCC",
        "actuator": "TRA12CC",
        "port": port,
        "axis": axis,
        "sample_um_per_unit": 582.0,
    }


def test_connect_scanner_reuses_cached_handle_for_same_config():
    first = scanner_module.connect_scanner(conexcc_config())
    second = scanner_module.connect_scanner(conexcc_config())

    assert second is first
    assert len(FakeSerial.instances) == 1
    assert len(FakeScannerController.instances) == 1
    assert first.controller.configure_calls == [
        {"axis": 1, "controller_address": 1, "pos_unit": "mm"},
    ]


def test_scanner_move_position_callback_reports_control_position():
    scanner = scanner_module.connect_scanner(conexcc_config())
    positions: list[float] = []

    scanner.move_pos_mm(1.25, on_position=positions.append)

    assert positions == [1.25]


def test_conexagap_axes_share_one_serial_on_same_port():
    x_scanner = scanner_module.connect_scanner(agap_config("U"))
    y_scanner = scanner_module.connect_scanner(agap_config("V"))

    assert x_scanner is not y_scanner
    assert x_scanner.ser is y_scanner.ser
    assert len(FakeSerial.instances) == 1
    assert scanner_module._SCANNER_SERIALS["COM11"] is x_scanner.ser


def test_conexagap_disconnect_keeps_shared_serial_until_last_axis():
    x_scanner = scanner_module.connect_scanner(agap_config("U"))
    y_scanner = scanner_module.connect_scanner(agap_config("V"))
    shared = x_scanner.ser

    scanner_module.disconnect_scanner(agap_config("U"))

    assert x_scanner.controller.close_count == 1
    assert y_scanner.controller.close_count == 0
    assert shared.is_open
    assert shared.close_count == 0
    assert scanner_module._SCANNER_SERIALS["COM11"] is shared

    scanner_module.disconnect_scanner(agap_config("V"))

    assert y_scanner.controller.close_count == 1
    assert not shared.is_open
    assert shared.close_count == 1
    assert "COM11" not in scanner_module._SCANNER_SERIALS


def test_disconnect_all_closes_each_serial_once():
    agap_x = scanner_module.connect_scanner(agap_config("U"))
    agap_y = scanner_module.connect_scanner(agap_config("V"))
    cc = scanner_module.connect_scanner(conexcc_config(port="COM5"))

    scanner_module.disconnect_scanner()

    assert agap_x.ser is agap_y.ser
    assert agap_x.ser.close_count == 1
    assert cc.ser.close_count == 1
    assert scanner_module._SCANNER_CONNECTIONS == {}
    assert scanner_module._SCANNER_SERIALS == {}


def test_conexcc_different_ports_use_different_handles_and_serials():
    x_scanner = scanner_module.connect_scanner(conexcc_config(port="COM5"))
    y_scanner = scanner_module.connect_scanner(conexcc_config(port="COM4"))

    assert x_scanner is not y_scanner
    assert x_scanner.ser is not y_scanner.ser
    assert len(FakeSerial.instances) == 2
