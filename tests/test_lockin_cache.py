from __future__ import annotations

import pytest

import kohdalab.interfaces.lockin as lockin_module


class FakeVisa:
    instances: list["FakeVisa"] = []

    def __init__(self, resource: str):
        self.resource = resource
        self.timeout = None
        FakeVisa.instances.append(self)


class FakeLockinController:
    instances: list["FakeLockinController"] = []

    def __init__(self, inst: FakeVisa):
        self.inst = inst
        self.connected = True
        self.configure_calls = 0
        self.close_count = 0
        FakeLockinController.instances.append(self)

    def configure(self):
        self.configure_calls += 1

    def close(self):
        self.close_count += 1
        self.connected = False

    def is_connected(self) -> bool:
        return self.connected

    def get_live_data_raw(self) -> dict[str, float]:
        return {"X": 1.0, "Y": 2.0, "R": 3.0, "Theta": 4.0}


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
    OtherFakeLockinController.instances.clear()
    monkeypatch.setattr(lockin_module, "open_visa", FakeVisa)
    monkeypatch.setitem(lockin_module.LOCKIN_CONTROLLERS, "FAKE", FakeLockinController)
    monkeypatch.setitem(lockin_module.LOCKIN_CONTROLLERS, "OTHER_FAKE", OtherFakeLockinController)
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


def test_connect_lockin_replaces_stale_cached_handle():
    first = lockin_module.connect_lockin(lockin_config())
    first.controller.connected = False

    second = lockin_module.connect_lockin(lockin_config())

    assert second is not first
    assert len(FakeVisa.instances) == 2
    assert len(FakeLockinController.instances) == 2
    assert lockin_module._LOCKIN_CONNECTIONS[("FAKE", "GPIB0::1::INSTR")] is second


def test_lockin_cache_key_includes_model_and_resource():
    first = lockin_module.connect_lockin(lockin_config(resource="GPIB0::1::INSTR", model="FAKE"))
    second = lockin_module.connect_lockin(lockin_config(resource="GPIB0::2::INSTR", model="FAKE"))
    third = lockin_module.connect_lockin(lockin_config(resource="GPIB0::1::INSTR", model="OTHER_FAKE"))

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
