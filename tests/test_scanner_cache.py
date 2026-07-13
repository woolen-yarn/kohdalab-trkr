from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
from threading import Barrier, Event, Lock

import pytest

import kohdalab.interfaces.scanner as scanner_module


class FakeSerial:
    instances: list["FakeSerial"] = []

    def __init__(self, *, port: str, **_kwargs):
        self.port = port
        self.is_open = True
        self.close_count = 0
        self.close_error: Exception | None = None
        FakeSerial.instances.append(self)

    def close(self):
        self.close_count += 1
        if self.close_error is not None:
            raise self.close_error
        self.is_open = False


class FakeScannerController:
    instances: list["FakeScannerController"] = []
    fail_positions = False
    block_operations = False
    operation_entered = Event()
    operation_release = Event()
    active_operations = 0
    max_active_operations = 0
    operation_lock = Lock()

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
        self.configure_errors: list[Exception] = []
        self.close_count = 0
        self.close_error: Exception | None = None
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
        if self.configure_errors:
            raise self.configure_errors.pop(0)

    def close(self):
        self.close_count += 1
        if self.close_error is not None:
            raise self.close_error

    def is_connected(self) -> bool:
        return self.ser.is_open

    def get_pos_raw(self) -> float:
        with self.operation_lock:
            type(self).active_operations += 1
            type(self).max_active_operations = max(
                type(self).max_active_operations,
                type(self).active_operations,
            )
        try:
            type(self).operation_entered.set()
            if type(self).block_operations:
                if not type(self).operation_release.wait(timeout=5.0):
                    raise TimeoutError("test operation release was not signaled")
            if self.fail_positions:
                raise OSError("initial position read failed")
            return 0.0
        finally:
            with self.operation_lock:
                type(self).active_operations -= 1

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

    def move_abs_raw(
        self, pos_raw: float, timeout: float = 30.0, on_position=None
    ) -> float:
        if on_position is not None:
            on_position(float(pos_raw))
        return float(pos_raw)

    def move_rel_raw(
        self, delta_raw: float, timeout: float = 30.0, on_position=None
    ) -> float:
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
    scanner_module._SCANNER_SERIAL_LOCKS.clear()
    FakeSerial.instances.clear()
    FakeScannerController.instances.clear()
    FakeScannerController.fail_positions = False
    FakeScannerController.block_operations = False
    FakeScannerController.operation_entered = Event()
    FakeScannerController.operation_release = Event()
    FakeScannerController.active_operations = 0
    FakeScannerController.max_active_operations = 0
    monkeypatch.setattr(scanner_module.serial, "Serial", FakeSerial)
    monkeypatch.setitem(
        scanner_module.SCANNER_CONTROLLERS, "CONEXAGAP", FakeScannerController
    )
    monkeypatch.setitem(
        scanner_module.SCANNER_CONTROLLERS, "CONEXCC", FakeScannerController
    )
    yield
    scanner_module._SCANNER_CONNECTIONS.clear()
    scanner_module._SCANNER_SERIALS.clear()
    scanner_module._SCANNER_SERIAL_LOCKS.clear()


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


def test_shared_serial_axes_share_one_io_lock():
    x_scanner = scanner_module.connect_scanner(agap_config("U"))
    y_scanner = scanner_module.connect_scanner(agap_config("V"))
    FakeScannerController.max_active_operations = 0
    FakeScannerController.block_operations = True
    FakeScannerController.operation_entered = Event()
    FakeScannerController.operation_release = Event()
    first_started = Event()
    second_started = Event()

    def read(scanner, started: Event):
        started.set()
        return scanner.get_pos_raw()

    with ThreadPoolExecutor(max_workers=2) as pool:
        first = pool.submit(read, x_scanner, first_started)
        assert first_started.wait(timeout=5.0)
        assert FakeScannerController.operation_entered.wait(timeout=5.0)
        second = pool.submit(read, y_scanner, second_started)
        assert second_started.wait(timeout=5.0)
        FakeScannerController.operation_release.set()
        results = [first.result(timeout=5.0), second.result(timeout=5.0)]

    assert results == [0.0, 0.0]
    assert x_scanner._io_lock is y_scanner._io_lock
    assert FakeScannerController.max_active_operations == 1


def test_scanner_configure_failure_rolls_back_controller_and_config():
    scanner = scanner_module.connect_scanner(conexcc_config())
    previous_config = scanner.config
    scanner.controller.configure_errors = [OSError("configure failed")]
    updated = conexcc_config()
    updated["controller_address"] = 2

    with pytest.raises(OSError, match="configure failed"):
        scanner.configure(updated)

    assert scanner.config is previous_config
    assert scanner.controller.controller_address == 1
    assert scanner.controller.configure_calls == [
        {"axis": 1, "controller_address": 2, "pos_unit": "mm"},
        {"axis": 1, "controller_address": 1, "pos_unit": "mm"},
    ]


def test_cached_scanner_probe_failure_restores_previous_config():
    scanner = scanner_module.connect_scanner(conexcc_config())
    previous_config = scanner.config
    FakeScannerController.fail_positions = True
    updated = conexcc_config()
    updated["controller_address"] = 2

    with pytest.raises(RuntimeError, match="initial position read failed"):
        scanner_module.connect_scanner(updated)

    assert scanner.config is previous_config
    assert scanner.controller.controller_address == 1
    assert scanner.controller.configure_calls == [
        {"axis": 1, "controller_address": 2, "pos_unit": "mm"},
        {"axis": 1, "controller_address": 1, "pos_unit": "mm"},
    ]


def test_concurrent_connect_scanner_creates_one_handle_and_serial():
    start = Barrier(4)

    def connect(_index):
        start.wait(timeout=5.0)
        return scanner_module.connect_scanner(conexcc_config())

    with ThreadPoolExecutor(max_workers=4) as executor:
        handles = list(executor.map(connect, range(4)))

    assert all(handle is handles[0] for handle in handles)
    assert len(FakeScannerController.instances) == 1
    assert len(FakeSerial.instances) == 1


def test_concurrent_scanner_axes_share_one_serial():
    start = Barrier(2)

    def connect(config):
        start.wait(timeout=5.0)
        return scanner_module.connect_scanner(config)

    with ThreadPoolExecutor(max_workers=2) as executor:
        handles = list(executor.map(connect, [agap_config("U"), agap_config("V")]))

    assert handles[0] is not handles[1]
    assert handles[0].ser is handles[1].ser
    assert len(FakeSerial.instances) == 1


def test_connect_scanner_rolls_back_failed_initial_read_and_new_serial():
    FakeScannerController.fail_positions = True

    with pytest.raises(RuntimeError, match="initial position read failed"):
        scanner_module.connect_scanner(conexcc_config())

    controller = FakeScannerController.instances[0]
    ser = FakeSerial.instances[0]
    assert controller.close_count == 1
    assert ser.close_count == 1
    assert not ser.is_open
    assert scanner_module._SCANNER_CONNECTIONS == {}
    assert scanner_module._SCANNER_SERIALS == {}


def test_connect_scanner_failure_does_not_close_shared_serial():
    first = scanner_module.connect_scanner(agap_config("U"))
    shared = first.ser
    FakeScannerController.fail_positions = True

    with pytest.raises(RuntimeError, match="initial position read failed"):
        scanner_module.connect_scanner(agap_config("V"))

    failed_controller = FakeScannerController.instances[1]
    assert failed_controller.close_count == 1
    assert shared.close_count == 0
    assert shared.is_open
    assert scanner_module._SCANNER_CONNECTIONS == {("CONEXAGAP", "COM11", "U"): first}
    assert scanner_module._SCANNER_SERIALS == {"COM11": shared}


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


def test_disconnect_scanner_keeps_failed_axis_and_shared_serial_for_retry():
    failed = scanner_module.connect_scanner(agap_config("U"))
    closed = scanner_module.connect_scanner(agap_config("V"))
    shared = failed.ser
    failed.controller.close_error = OSError("controller close failed")

    with pytest.raises(
        RuntimeError, match=r"CONEXAGAP @ COM11 axis=U: controller close failed"
    ):
        scanner_module.disconnect_scanner()

    assert failed.controller.close_count == 1
    assert closed.controller.close_count == 1
    assert scanner_module._SCANNER_CONNECTIONS == {("CONEXAGAP", "COM11", "U"): failed}
    assert scanner_module._SCANNER_SERIALS == {"COM11": shared}
    assert shared.is_open
    assert shared.close_count == 0

    failed.controller.close_error = None
    scanner_module.disconnect_scanner()
    assert failed.controller.close_count == 2
    assert not shared.is_open
    assert scanner_module._SCANNER_CONNECTIONS == {}
    assert scanner_module._SCANNER_SERIALS == {}


def test_disconnect_scanner_keeps_serial_cache_when_serial_close_fails():
    scanner = scanner_module.connect_scanner(conexcc_config())
    ser = scanner.ser
    ser.close_error = OSError("port close failed")

    with pytest.raises(RuntimeError, match=r"serial @ COM5: port close failed"):
        scanner_module.disconnect_scanner()

    assert scanner.controller.close_count == 1
    assert scanner_module._SCANNER_CONNECTIONS == {}
    assert scanner_module._SCANNER_SERIALS == {"COM5": ser}
    assert ser.is_open

    ser.close_error = None
    scanner_module.disconnect_scanner()
    assert ser.close_count == 2
    assert scanner_module._SCANNER_SERIALS == {}


def test_conexcc_different_ports_use_different_handles_and_serials():
    x_scanner = scanner_module.connect_scanner(conexcc_config(port="COM5"))
    y_scanner = scanner_module.connect_scanner(conexcc_config(port="COM4"))

    assert x_scanner is not y_scanner
    assert x_scanner.ser is not y_scanner.ser
    assert len(FakeSerial.instances) == 2


@pytest.mark.parametrize(
    ("axis", "expected"),
    [(1, "U"), (2, "V"), (" u ", "U"), ("v", "V")],
)
def test_conexagap_axis_key_normalizes_supported_axes(axis, expected):
    assert (
        scanner_module._axis_key({"controller": "CONEXAGAP", "axis": axis}) == expected
    )


def test_conexagap_axis_key_rejects_unsupported_axis():
    with pytest.raises(ValueError, match="Unsupported CONEXAGAP axis: 3"):
        scanner_module._axis_key({"controller": "CONEXAGAP", "axis": 3})


def test_scanner_config_rejects_incompatible_actuator_controller():
    config = conexcc_config()
    config["controller"] = "CONEXAGAP"

    with pytest.raises(
        ValueError,
        match=r"Actuator 'TRA12CC' is only supported by \['CONEXCC'\], not 'CONEXAGAP'",
    ):
        scanner_module.connect_scanner(config)

    assert FakeSerial.instances == []
    assert FakeScannerController.instances == []


def test_scanner_configure_reports_primary_and_rollback_failures():
    scanner = scanner_module.connect_scanner(conexcc_config())
    previous_config = scanner.config
    scanner.controller.configure_errors = [
        OSError("configure failed"),
        OSError("rollback failed"),
    ]

    with pytest.raises(
        RuntimeError,
        match="Scanner configuration failed: configure failed; rollback failed: rollback failed",
    ):
        scanner.configure({**conexcc_config(), "controller_address": 2})

    assert scanner.config is previous_config
    assert scanner.controller.configure_calls[-2:] == [
        {"axis": 1, "controller_address": 2, "pos_unit": "mm"},
        {"axis": 1, "controller_address": 1, "pos_unit": "mm"},
    ]


def test_scanner_move_rejects_absolute_and_relative_targets_outside_limits():
    scanner = scanner_module.connect_scanner(conexcc_config())

    with pytest.raises(ValueError, match=r"pos=-0\.0002 is below limit 0\.0"):
        scanner.move_pos_mm(-0.0002)
    with pytest.raises(ValueError, match=r"pos=12\.0002 is above limit 12\.0"):
        scanner.move_pos_raw(12.0002)
    with pytest.raises(ValueError, match=r"pos=12\.0002 is above limit 12\.0"):
        scanner.move_relative_pos_mm(12.0002)


def test_scanner_unit_specific_methods_reject_wrong_unit():
    scanner = scanner_module.connect_scanner(conexcc_config())

    with pytest.raises(ValueError, match="Scanner actuator unit is 'mm', not 'deg'"):
        scanner.get_pos_deg()
    with pytest.raises(ValueError, match="Scanner actuator unit is 'mm', not 'deg'"):
        scanner.move_pos_deg(1.0)
    with pytest.raises(ValueError, match="Scanner actuator unit is 'mm', not 'deg'"):
        scanner.move_relative_pos_deg(1.0)


def test_scanner_initialize_reports_normalized_actuator_metadata():
    scanner = scanner_module.connect_scanner(conexcc_config(axis=2))

    assert scanner.initialize(home=True, timeout=4.0) == {
        "axis": 2,
        "state": "ready",
        "moving": False,
        "actuator": "TRA12CC",
        "pos_mm": 0.0,
        "pos_limits": (0.0, 12.0),
        "origin_pos": 6.0,
        "pos_unit": "mm",
        "pos_digits": 4,
    }
    assert scanner.port == "COM5"
    assert scanner.get_pos_unit() == "mm"
    assert scanner.normalize_pos(1.23456) == 1.2346


def test_scanner_wait_and_moves_forward_position_callbacks():
    scanner = scanner_module.connect_scanner(conexcc_config())
    positions: list[float] = []

    scanner.wait_until_stopped(timeout=2.0, on_position=positions.append)
    assert scanner.move_pos_raw(1.5, timeout=2.0) == 1.5
    assert scanner.move_relative_pos_raw(0.5, timeout=2.0) == 0.5
    assert (
        scanner.move_relative_pos_raw(0.25, timeout=2.0, on_position=positions.append)
        == 0.25
    )

    assert positions == [0.0, 0.25]


def test_scanner_helpers_handle_missing_actuator_and_unknown_controller_before_io():
    assert scanner_module._normalize_actuator(None) is None
    assert scanner_module._actuator_settings(None) == {}

    with pytest.raises(
        RuntimeError, match="Connection failed: UNKNOWN.*Unsupported scanner controller"
    ):
        scanner_module.connect_scanner(
            {"controller": "UNKNOWN", "port": "COM9", "axis": 1}
        )

    assert len(FakeSerial.instances) == 1
    assert FakeSerial.instances[0].close_count == 1
    assert scanner_module._SCANNER_SERIALS == {}


def test_scanner_accepts_exact_range_boundaries_and_forwards_callback():
    scanner = scanner_module.connect_scanner(conexcc_config())
    positions: list[float] = []

    assert scanner.move_pos_mm(0.0, on_position=positions.append) == 0.0
    assert scanner.move_pos_mm(12.0, on_position=positions.append) == 12.0

    assert positions == [0.0, 12.0]


def test_scanner_degree_unit_methods_use_same_normalized_transport_contract():
    config = {
        "controller": "CONEXCC",
        "port": "COM8",
        "axis": 1,
        "pos_unit": "deg",
        "min_pos": -5.0,
        "max_pos": 5.0,
        "origin_pos": 0.0,
        "pos_digits": 2,
    }
    scanner = scanner_module.connect_scanner(config)
    positions: list[float] = []

    assert scanner.get_pos_deg() == 0.0
    assert scanner.move_pos_deg(1.234, on_position=positions.append) == 1.23
    assert scanner.move_relative_pos_deg(-0.5) == -0.5
    assert positions == [1.23]


def test_scanner_wait_without_callback_uses_controller_legacy_signature():
    scanner = scanner_module.connect_scanner(conexcc_config())

    scanner.wait_until_stopped(timeout=1.5)


def test_scanner_configure_without_controller_hook_updates_config():
    class MinimalController:
        pass

    replacement = {"axis": "V", "pos_unit": "deg"}
    scanner = scanner_module.Scanner(controller=MinimalController(), config={})

    scanner.configure(replacement)

    assert scanner.config is replacement
    assert scanner.axis == 2
    assert scanner.get_pos_unit() == "deg"


def test_scanner_position_metadata_uses_resolution_rounding_and_midpoint():
    controller = FakeScannerController(
        port="COM8", ser=FakeSerial(port="COM8"), axis="U", pos_unit="deg"
    )
    scanner = scanner_module.Scanner(
        controller=controller,
        config={
            "axis": "U",
            "pos_unit": "deg",
            "min_pos": "-1.2346",
            "max_pos": "2.3457",
            "resolution": "0.001",
        },
    )

    assert scanner.axis == 1
    assert scanner.pos_digits == 3
    assert scanner.get_pos_limits() == (-1.235, 2.346)
    assert scanner.origin_pos == 0.555
    assert scanner.normalize_pos(1.23456) == 1.235


def test_scanner_wait_selects_legacy_or_callback_controller_signature(monkeypatch):
    scanner = scanner_module.connect_scanner(conexcc_config())
    calls: list[tuple[float, object]] = []

    def wait_until_stopped(timeout, **kwargs):
        calls.append((timeout, kwargs.get("on_position")))

    monkeypatch.setattr(scanner.controller, "wait_until_stopped", wait_until_stopped)

    def callback(_position):
        return None

    scanner.wait_until_stopped(timeout=1.0)
    scanner.wait_until_stopped(timeout=2.0, on_position=callback)

    assert calls == [(1.0, None), (2.0, callback)]


def test_cached_scanner_reports_probe_and_configuration_rollback_failures(
    monkeypatch,
):
    scanner = scanner_module.connect_scanner(conexcc_config())
    original_configure = scanner.controller.configure
    configure_count = 0

    def configure(**settings):
        nonlocal configure_count
        configure_count += 1
        if configure_count == 2:
            raise OSError("configuration rollback failed")
        return original_configure(**settings)

    monkeypatch.setattr(scanner.controller, "configure", configure)
    FakeScannerController.fail_positions = True

    with pytest.raises(
        RuntimeError,
        match=(
            r"Cached scanner probe failed: initial position read failed; "
            r"configuration rollback failed: configuration rollback failed"
        ),
    ) as exc_info:
        scanner_module.connect_scanner({**conexcc_config(), "controller_address": 2})

    connection_cause = exc_info.value.__cause__
    assert isinstance(connection_cause, RuntimeError)
    assert isinstance(connection_cause.__cause__, OSError)
    assert scanner_module._SCANNER_CONNECTIONS[("CONEXCC", "COM5", "1")] is scanner


def test_connect_scanner_reports_controller_and_serial_cleanup_failures(monkeypatch):
    class CleanupFailingSerial(FakeSerial):
        def __init__(self, *, port: str, **kwargs):
            super().__init__(port=port, **kwargs)
            self.close_error = OSError("serial cleanup failed")

    class CleanupFailingController(FakeScannerController):
        def __init__(self, **kwargs):
            super().__init__(**kwargs)
            self.close_error = OSError("controller cleanup failed")

    monkeypatch.setattr(scanner_module.serial, "Serial", CleanupFailingSerial)
    monkeypatch.setitem(
        scanner_module.SCANNER_CONTROLLERS, "CONEXCC", CleanupFailingController
    )
    FakeScannerController.fail_positions = True

    with pytest.raises(
        RuntimeError,
        match=(
            r"initial position read failed; controller cleanup failed: "
            r"controller cleanup failed; serial cleanup failed: serial cleanup failed"
        ),
    ):
        scanner_module.connect_scanner(conexcc_config())

    assert scanner_module._SCANNER_CONNECTIONS == {}
    assert scanner_module._SCANNER_SERIALS["COM5"].is_open


def test_scanner_axis_and_actuator_optional_metadata_fallbacks(monkeypatch):
    assert scanner_module._axis_key({"controller": "CONEXAGAP", "axis": " 1 "}) == "U"
    monkeypatch.setitem(scanner_module.ACTUATORS, "GENERIC", {"pos_unit": "mm"})

    config = scanner_module._build_scanner_config(
        {"controller": "CONEXCC", "actuator": "GENERIC", "port": "COM20"}
    )

    assert config["actuator"] == "GENERIC"
    assert config["pos_unit"] == "mm"


def test_conexcc_controller_receives_optional_closed_loop_policy(monkeypatch):
    captured: list[bool] = []

    class CapturingController(FakeScannerController):
        def __init__(self, *, ensure_closed_loop_on_move, **kwargs):
            captured.append(ensure_closed_loop_on_move)
            super().__init__(**kwargs)

    monkeypatch.setitem(
        scanner_module.SCANNER_CONTROLLERS, "CONEXCC", CapturingController
    )
    config = conexcc_config()
    config["ensure_closed_loop_on_move"] = False

    scanner_module.connect_scanner(config)

    assert captured == [False]


def test_scanner_default_digits_configured_origin_get_mm_and_stop(monkeypatch):
    scanner = scanner_module.connect_scanner(conexcc_config())
    scanner.config.pop("resolution", None)
    scanner.config.pop("min_step", None)
    scanner.config["origin_pos"] = 1.23456
    stops: list[bool] = []
    monkeypatch.setattr(scanner.controller, "stop", lambda: stops.append(True))

    assert scanner.pos_digits == 4
    assert scanner.origin_pos == 1.2346
    assert scanner.get_pos_mm() == 0.0
    scanner.stop()

    assert stops == [True]


def test_connect_scanner_replaces_stale_cached_handle_and_serial():
    first = scanner_module.connect_scanner(conexcc_config())
    first.ser.is_open = False

    second = scanner_module.connect_scanner(conexcc_config())

    assert second is not first
    assert first.controller.close_count == 1
    assert len(FakeSerial.instances) == 2
    assert scanner_module._SCANNER_CONNECTIONS[("CONEXCC", "COM5", "1")] is second


def test_disconnect_unknown_scanner_and_missing_serial_are_idempotent():
    scanner_module.disconnect_scanner(conexcc_config(port="COM404"))

    assert scanner_module._SCANNER_CONNECTIONS == {}
    assert scanner_module._SCANNER_SERIALS == {}


def test_disconnect_removes_already_closed_uncached_serial():
    ser = FakeSerial(port="COM30")
    ser.is_open = False
    scanner_module._SCANNER_SERIALS["COM30"] = ser
    scanner_module._SCANNER_SERIAL_LOCKS["COM30"] = Lock()

    scanner_module.disconnect_scanner(
        {"controller": "CONEXCC", "port": "COM30", "axis": 1}
    )

    assert ser.close_count == 0
    assert "COM30" not in scanner_module._SCANNER_SERIALS
    assert "COM30" not in scanner_module._SCANNER_SERIAL_LOCKS


def test_scanner_initialize_without_home_skips_home_wait(monkeypatch):
    scanner = scanner_module.connect_scanner(conexcc_config())
    monkeypatch.setattr(
        scanner.controller,
        "home",
        lambda: pytest.fail("home called when home=False"),
    )
    monkeypatch.setattr(
        scanner.controller,
        "wait_until_stopped",
        lambda **_kwargs: pytest.fail("wait called when home=False"),
    )

    info = scanner.initialize(home=False)

    assert info["state"] == "ready"
    assert info["pos_mm"] == 0.0


def test_connect_cleanup_does_not_reclose_serial_closed_by_constructor(monkeypatch):
    class ClosingFailingController:
        def __init__(self, *, ser, **_kwargs):
            ser.is_open = False
            raise OSError("controller construction failed after serial close")

    monkeypatch.setitem(
        scanner_module.SCANNER_CONTROLLERS, "CONEXCC", ClosingFailingController
    )

    with pytest.raises(
        RuntimeError, match="controller construction failed after serial close"
    ):
        scanner_module.connect_scanner(conexcc_config())

    ser = FakeSerial.instances[0]
    assert not ser.is_open
    assert ser.close_count == 0
    assert scanner_module._SCANNER_SERIALS == {}
