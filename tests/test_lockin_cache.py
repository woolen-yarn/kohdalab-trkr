from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
from threading import Barrier, Event

import pytest

import kohdalab.interfaces.lockin as lockin_module


REAL_OPEN_VISA = lockin_module.open_visa


class FakeVisa:
    instances: list["FakeVisa"] = []

    def __init__(self, resource: str):
        self.resource = resource
        self.timeout = None
        self.close_count = 0
        FakeVisa.instances.append(self)

    def close(self):
        self.close_count += 1


class FakeLockinController:
    instances: list["FakeLockinController"] = []
    fail_reads = False
    construction_started: Event | None = None
    construction_release: Event | None = None

    def __init__(self, inst: FakeVisa):
        if self.construction_started is not None:
            self.construction_started.set()
        if self.construction_release is not None and not self.construction_release.wait(
            2
        ):
            raise RuntimeError("timed out waiting to release controller construction")
        self.inst = inst
        self.connected = True
        self.configure_calls = 0
        self.close_count = 0
        self.close_error: Exception | None = None
        FakeLockinController.instances.append(self)

    def configure(self):
        self.configure_calls += 1

    def close(self):
        self.close_count += 1
        if self.close_error is not None:
            raise self.close_error
        self.connected = False

    def is_connected(self) -> bool:
        return self.connected

    def get_live_data_raw(self) -> dict[str, float]:
        if self.fail_reads:
            raise OSError("initial read failed")
        return {"X": 1.0, "Y": 2.0, "R": 3.0, "Theta": 4.0}


class DelegatingController:
    def __init__(self):
        self.calls: list[tuple[str, object]] = []

    def close(self):
        self.calls.append(("close", None))

    def configure(self):
        self.calls.append(("configure", None))

    def is_connected(self):
        return True

    def ask(self, cmd, *, delay):
        self.calls.append(("ask", (cmd, delay)))
        return "reply"

    def ask_float(self, cmd, *, delay):
        self.calls.append(("ask_float", (cmd, delay)))
        return 1.25

    def get_live_data_raw(self):
        return {"X": 1.0}

    def get_time_constant(self):
        return 0.3

    def get_ac_gain(self):
        return None

    def get_sensitivity(self):
        return 2e-6

    def get_ref_freq(self):
        return 137.0

    def get_available_couplings(self):
        return ["AC", "DC"]

    def get_available_slopes(self):
        return [6, 12]

    def get_available_time_constants(self):
        return [0.1, 0.3]

    def get_available_sensitivities(self):
        return [1e-6, 2e-6]

    def get_available_ac_gains(self):
        return [0.0, 10.0]

    def get_coupling(self):
        return "AC"

    def get_slope(self):
        return 12

    def get_overload_status(self):
        return {"overload": False}

    def get_wait_time(self, *, multiplier):
        self.calls.append(("get_wait_time", multiplier))
        return 0.3 * multiplier

    def auto_phase(self):
        self.calls.append(("auto_phase", None))

    def auto_sensitivity(self):
        self.calls.append(("auto_sensitivity", None))

    def auto_measure(self):
        self.calls.append(("auto_measure", None))

    def set_sensitivity(self, value):
        if value <= 0:
            raise ValueError("sensitivity must be positive")
        self.calls.append(("set_sensitivity", value))

    def set_time_constant(self, value):
        self.calls.append(("set_time_constant", value))

    def set_ac_gain(self, value):
        self.calls.append(("set_ac_gain", value))

    def set_coupling(self, value):
        self.calls.append(("set_coupling", value))

    def set_slope(self, value):
        self.calls.append(("set_slope", value))


class OtherFakeLockinController(FakeLockinController):
    instances: list["OtherFakeLockinController"] = []

    def __init__(self, inst: FakeVisa):
        super().__init__(inst)
        OtherFakeLockinController.instances.append(self)


@pytest.fixture(autouse=True)
def fake_lockin_backend(monkeypatch):
    lockin_module._LOCKIN_CONNECTIONS.clear()
    FakeVisa.instances.clear()
    FakeLockinController.instances.clear()
    FakeLockinController.fail_reads = False
    FakeLockinController.construction_started = None
    FakeLockinController.construction_release = None
    OtherFakeLockinController.instances.clear()
    monkeypatch.setattr(lockin_module, "open_visa", FakeVisa)
    monkeypatch.setitem(lockin_module.LOCKIN_CONTROLLERS, "FAKE", FakeLockinController)
    monkeypatch.setitem(
        lockin_module.LOCKIN_CONTROLLERS, "OTHER_FAKE", OtherFakeLockinController
    )
    yield
    lockin_module._LOCKIN_CONNECTIONS.clear()


def lockin_config(
    *,
    resource: str = "GPIB0::1::INSTR",
    model: str = "FAKE",
    **extra: str,
) -> dict[str, str]:
    return {
        "model": model,
        "resource": resource,
        **extra,
    }


def test_connect_lockin_reuses_cached_handle_for_same_config():
    first = lockin_module.connect_lockin(lockin_config())
    second = lockin_module.connect_lockin(lockin_config(lockin_model="FAKE"))

    assert second is first
    assert len(FakeVisa.instances) == 1
    assert len(FakeLockinController.instances) == 1
    assert first.controller.configure_calls == 1


def test_cached_lockin_probe_failure_restores_previous_config():
    first = lockin_module.connect_lockin(lockin_config())
    previous_config = first.config
    FakeLockinController.fail_reads = True

    with pytest.raises(RuntimeError, match="initial read failed"):
        lockin_module.connect_lockin(lockin_config(profile="changed"))

    assert first.config is previous_config
    assert first.controller.configure_calls == 2
    assert lockin_module._LOCKIN_CONNECTIONS[("FAKE", "GPIB0::1::INSTR")] is first


def test_concurrent_connect_lockin_creates_one_cached_handle():
    callers_ready = Barrier(4)
    construction_started = Event()
    construction_release = Event()
    FakeLockinController.construction_started = construction_started
    FakeLockinController.construction_release = construction_release

    def connect_after_all_callers_are_ready():
        callers_ready.wait()
        return lockin_module.connect_lockin(lockin_config())

    with ThreadPoolExecutor(max_workers=4) as executor:
        futures = [
            executor.submit(connect_after_all_callers_are_ready) for _ in range(4)
        ]
        assert construction_started.wait(2)
        construction_release.set()
        handles = [future.result(timeout=2) for future in futures]

    assert all(handle is handles[0] for handle in handles)
    assert len(FakeVisa.instances) == 1
    assert len(FakeLockinController.instances) == 1


def test_connect_lockin_replaces_stale_cached_handle():
    first = lockin_module.connect_lockin(lockin_config())
    first.controller.connected = False

    second = lockin_module.connect_lockin(lockin_config())

    assert second is not first
    assert first.controller.close_count == 1
    assert len(FakeVisa.instances) == 2
    assert len(FakeLockinController.instances) == 2
    assert lockin_module._LOCKIN_CONNECTIONS[("FAKE", "GPIB0::1::INSTR")] is second


def test_connect_lockin_rolls_back_failed_initial_read():
    FakeLockinController.fail_reads = True

    with pytest.raises(RuntimeError, match="initial read failed"):
        lockin_module.connect_lockin(lockin_config())

    controller = FakeLockinController.instances[0]
    assert controller.close_count == 1
    assert not controller.connected
    assert lockin_module._LOCKIN_CONNECTIONS == {}


def test_lockin_controller_construction_failure_closes_visa(monkeypatch):
    class FailingController:
        def __init__(self, _inst):
            raise OSError("constructor failed")

    monkeypatch.setitem(lockin_module.LOCKIN_CONTROLLERS, "FAKE", FailingController)

    with pytest.raises(RuntimeError, match="constructor failed"):
        lockin_module.connect_lockin(lockin_config())

    assert FakeVisa.instances[0].close_count == 1
    assert lockin_module._LOCKIN_CONNECTIONS == {}


def test_lockin_cache_key_includes_model_and_resource():
    first = lockin_module.connect_lockin(
        lockin_config(resource="GPIB0::1::INSTR", model="FAKE")
    )
    second = lockin_module.connect_lockin(
        lockin_config(resource="GPIB0::2::INSTR", model="FAKE")
    )
    third = lockin_module.connect_lockin(
        lockin_config(resource="GPIB0::1::INSTR", model="OTHER_FAKE")
    )

    assert first is not second
    assert first is not third
    assert len(lockin_module._LOCKIN_CONNECTIONS) == 3
    assert len(FakeVisa.instances) == 3


def test_disconnect_lockin_closes_specific_cached_handle():
    first = lockin_module.connect_lockin(lockin_config(resource="GPIB0::1::INSTR"))
    second = lockin_module.connect_lockin(lockin_config(resource="GPIB0::2::INSTR"))

    lockin_module.disconnect_lockin(lockin_config(resource="GPIB0::1::INSTR"))

    assert first.controller.close_count == 1
    assert second.controller.close_count == 0
    assert ("FAKE", "GPIB0::1::INSTR") not in lockin_module._LOCKIN_CONNECTIONS
    assert lockin_module._LOCKIN_CONNECTIONS[("FAKE", "GPIB0::2::INSTR")] is second


def test_disconnect_all_lockins_closes_each_cached_handle_once():
    first = lockin_module.connect_lockin(lockin_config(resource="GPIB0::1::INSTR"))
    second = lockin_module.connect_lockin(lockin_config(resource="GPIB0::2::INSTR"))

    lockin_module.disconnect_lockin()

    assert first.controller.close_count == 1
    assert second.controller.close_count == 1
    assert lockin_module._LOCKIN_CONNECTIONS == {}


def test_disconnect_all_lockins_reports_failures_and_keeps_failed_handle():
    failed = lockin_module.connect_lockin(lockin_config(resource="GPIB0::1::INSTR"))
    closed = lockin_module.connect_lockin(lockin_config(resource="GPIB0::2::INSTR"))
    failed.controller.close_error = OSError("VISA close failed")

    with pytest.raises(
        RuntimeError, match=r"FAKE @ GPIB0::1::INSTR: VISA close failed"
    ):
        lockin_module.disconnect_lockin()

    assert failed.controller.close_count == 1
    assert closed.controller.close_count == 1
    assert lockin_module._LOCKIN_CONNECTIONS == {("FAKE", "GPIB0::1::INSTR"): failed}

    failed.controller.close_error = None
    lockin_module.disconnect_lockin()
    assert failed.controller.close_count == 2
    assert lockin_module._LOCKIN_CONNECTIONS == {}


def test_lockin_common_interface_delegates_values_and_rejections():
    controller = DelegatingController()
    lockin = lockin_module.Lockin(controller=controller, config={"profile": "stable"})

    assert lockin.is_connected() is True
    assert lockin.ask("ID?", delay=0.25) == "reply"
    assert lockin.ask_float("X?", delay=0.5) == 1.25
    assert lockin.get_live_data_raw() == {"X": 1.0}
    assert lockin.get_time_constant() == 0.3
    assert lockin.get_ac_gain() is None
    assert lockin.get_sensitivity() == 2e-6
    assert lockin.get_ref_freq() == 137.0
    assert lockin.get_available_couplings() == ["AC", "DC"]
    assert lockin.get_available_slopes() == [6, 12]
    assert lockin.get_available_time_constants() == [0.1, 0.3]
    assert lockin.get_available_sensitivities() == [1e-6, 2e-6]
    assert lockin.get_available_ac_gains() == [0.0, 10.0]
    assert lockin.get_coupling() == "AC"
    assert lockin.get_slope() == 12
    assert lockin.get_overload_status() == {"overload": False}
    assert lockin.get_wait_time(multiplier=5.0) == 1.5

    replacement_config = {"profile": "replacement"}
    lockin.configure(replacement_config)

    lockin.auto_phase()
    lockin.auto_sensitivity()
    lockin.auto_measure()
    lockin.set_sensitivity(1e-6)
    lockin.set_time_constant(0.1)
    lockin.set_ac_gain(10.0)
    lockin.set_coupling("DC")
    lockin.set_slope(6)
    with pytest.raises(ValueError, match="sensitivity must be positive"):
        lockin.set_sensitivity(0.0)
    lockin.close()

    assert lockin.config is replacement_config
    assert controller.calls == [
        ("ask", ("ID?", 0.25)),
        ("ask_float", ("X?", 0.5)),
        ("get_wait_time", 5.0),
        ("configure", None),
        ("auto_phase", None),
        ("auto_sensitivity", None),
        ("auto_measure", None),
        ("set_sensitivity", 1e-6),
        ("set_time_constant", 0.1),
        ("set_ac_gain", 10.0),
        ("set_coupling", "DC"),
        ("set_slope", 6),
        ("close", None),
    ]


def test_lockin_read_helpers_use_supplied_handle_without_connecting(monkeypatch):
    controller = DelegatingController()
    lockin = lockin_module.Lockin(controller=controller, config={})
    monkeypatch.setattr(
        lockin_module,
        "connect_lockin",
        lambda _config: pytest.fail("explicit handle must bypass connection"),
    )

    assert lockin_module.read_lockin_signal(lockin=lockin) == {"X": 1.0}
    assert lockin_module.get_lockin_wait_time(lockin=lockin, multiplier=2.0) == 0.6
    assert controller.calls == [("get_wait_time", 2.0)]


def test_unknown_lockin_model_is_rejected_without_opening_visa():
    with pytest.raises(RuntimeError, match="Unsupported lockin model: UNKNOWN"):
        lockin_module.connect_lockin(lockin_config(model="UNKNOWN"))

    assert FakeVisa.instances == []
    assert lockin_module._LOCKIN_CONNECTIONS == {}


def test_disconnect_unknown_lockin_is_idempotent():
    lockin_module.disconnect_lockin(lockin_config(resource="GPIB0::404::INSTR"))

    assert lockin_module._LOCKIN_CONNECTIONS == {}


def test_lockin_controller_construction_reports_visa_cleanup_failure(monkeypatch):
    class FailingVisa:
        def close(self):
            raise OSError("VISA cleanup failed")

    class FailingController:
        def __init__(self, _inst):
            raise OSError("constructor failed")

    monkeypatch.setattr(lockin_module, "open_visa", lambda _resource: FailingVisa())
    monkeypatch.setitem(lockin_module.LOCKIN_CONTROLLERS, "FAKE", FailingController)

    with pytest.raises(
        RuntimeError,
        match=(
            r"Controller construction failed: constructor failed; "
            r"VISA cleanup failed: VISA cleanup failed"
        ),
    ):
        lockin_module.connect_lockin(lockin_config())

    assert lockin_module._LOCKIN_CONNECTIONS == {}


def test_connect_lockin_reports_probe_and_handle_cleanup_failures(monkeypatch):
    class CleanupFailingController(FakeLockinController):
        def __init__(self, inst):
            super().__init__(inst)
            self.close_error = OSError("controller close failed")

    FakeLockinController.fail_reads = True
    monkeypatch.setitem(
        lockin_module.LOCKIN_CONTROLLERS, "FAKE", CleanupFailingController
    )

    with pytest.raises(
        RuntimeError,
        match=r"initial read failed; cleanup failed: controller close failed",
    ):
        lockin_module.connect_lockin(lockin_config())

    assert lockin_module._LOCKIN_CONNECTIONS == {}


def test_cached_lockin_reports_probe_and_configuration_rollback_failures(monkeypatch):
    lockin = lockin_module.connect_lockin(lockin_config())
    configure_count = 0

    def configure():
        nonlocal configure_count
        configure_count += 1
        if configure_count == 2:
            raise OSError("configuration rollback failed")

    monkeypatch.setattr(lockin.controller, "configure", configure)
    FakeLockinController.fail_reads = True

    with pytest.raises(
        RuntimeError,
        match=(
            r"Cached lock-in probe failed: initial read failed; "
            r"configuration rollback failed: configuration rollback failed"
        ),
    ) as exc_info:
        lockin_module.connect_lockin(lockin_config(profile="replacement"))

    connection_cause = exc_info.value.__cause__
    assert isinstance(connection_cause, RuntimeError)
    assert isinstance(connection_cause.__cause__, OSError)
    assert str(connection_cause.__cause__) == "initial read failed"
    assert lockin_module._LOCKIN_CONNECTIONS[("FAKE", "GPIB0::1::INSTR")] is lockin


def test_lockin_read_helpers_connect_with_the_original_config(monkeypatch):
    controller = DelegatingController()
    lockin = lockin_module.Lockin(controller=controller, config={})
    config = {"resource": "GPIB0::9::INSTR"}
    connected: list[dict] = []
    monkeypatch.setattr(
        lockin_module,
        "connect_lockin",
        lambda received: connected.append(received) or lockin,
    )

    assert lockin_module.read_lockin_signal(config) == {"X": 1.0}
    assert lockin_module.get_lockin_wait_time(config, multiplier=3.0) == pytest.approx(
        0.9
    )
    assert connected == [config, config]
    assert controller.calls == [("get_wait_time", 3.0)]


def test_lockin_configure_without_controller_hook_still_updates_config():
    class MinimalController:
        pass

    replacement = {"profile": "replacement"}
    lockin = lockin_module.Lockin(controller=MinimalController(), config={})

    lockin.configure(replacement)

    assert lockin.config is replacement


def test_visa_resource_helpers_set_timeout_and_close_listing_manager(monkeypatch):
    class Instrument:
        timeout = 0

    class ResourceManager:
        def __init__(self):
            self.instrument = Instrument()
            self.opened: list[str] = []
            self.close_count = 0

        def open_resource(self, resource):
            self.opened.append(resource)
            return self.instrument

        def list_resources(self):
            return ("GPIB0::1::INSTR", "GPIB0::2::INSTR")

        def close(self):
            self.close_count += 1

    managers: list[ResourceManager] = []

    def resource_manager():
        manager = ResourceManager()
        managers.append(manager)
        return manager

    monkeypatch.setattr(lockin_module.pyvisa, "ResourceManager", resource_manager)

    instrument = REAL_OPEN_VISA("GPIB0::8::INSTR", timeout=1234)
    resources = lockin_module.list_visa_resources()

    assert instrument.timeout == 1234
    assert managers[0].opened == ["GPIB0::8::INSTR"]
    assert resources == ("GPIB0::1::INSTR", "GPIB0::2::INSTR")
    assert managers[1].close_count == 1


def test_list_visa_resources_closes_manager_when_listing_fails(monkeypatch):
    class FailingResourceManager:
        close_count = 0

        def list_resources(self):
            raise OSError("resource listing failed")

        def close(self):
            self.close_count += 1

    manager = FailingResourceManager()
    monkeypatch.setattr(lockin_module.pyvisa, "ResourceManager", lambda: manager)

    with pytest.raises(OSError, match="resource listing failed"):
        lockin_module.list_visa_resources()

    assert manager.close_count == 1
