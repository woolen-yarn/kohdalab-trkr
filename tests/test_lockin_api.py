from __future__ import annotations

import pytest

from kohdalab.api import Experiment, set_lockin_settings
import kohdalab.api.devices.lockin as lockin_module
from kohdalab.api.session import DeviceSession
import kohdalab.api.session as session_module


class FakeLockin:
    def __init__(self):
        self.calls: list[tuple[str, object]] = []
        self.sensitivity = 1e-3
        self.time_constant = 0.3
        self.ac_gain = 10.0
        self.coupling = "AC"
        self.slope = 12

    def set_sensitivity(self, value: float):
        self.calls.append(("sensitivity", value))
        self.sensitivity = value

    def get_sensitivity(self) -> float:
        return self.sensitivity

    def set_time_constant(self, value: float):
        self.calls.append(("time_constant", value))
        self.time_constant = value

    def get_time_constant(self) -> float:
        return self.time_constant

    def set_ac_gain(self, value: float):
        self.calls.append(("ac_gain", value))
        self.ac_gain = value

    def get_ac_gain(self) -> float:
        return self.ac_gain

    def set_coupling(self, value: str):
        self.calls.append(("coupling", value))
        self.coupling = value

    def get_coupling(self) -> str:
        return self.coupling

    def set_slope(self, value: int):
        self.calls.append(("slope", value))
        self.slope = value

    def get_slope(self) -> int:
        return self.slope

    def get_live_data_raw(self) -> dict[str, float]:
        self.calls.append(("live_data", None))
        return {"X": 1.0, "Y": 2.0}

    def get_ref_freq(self) -> float:
        self.calls.append(("ref_freq", None))
        return 137.0

    def get_overload_status(self) -> dict[str, bool]:
        self.calls.append(("overload", None))
        return {"overload": False}

    def get_wait_time(self, *, multiplier: float = 4.0) -> float:
        self.calls.append(("wait_time", multiplier))
        return self.time_constant * multiplier


def config_with_lockin() -> dict:
    return {
        "instruments": {
            "lockin": {
                "main": {"resource": "LOCKIN"},
            },
        },
    }


def test_set_lockin_settings_applies_only_requested_values():
    lockin = FakeLockin()

    applied = set_lockin_settings(
        lockin=lockin,
        sensitivity=100e-6,
        time_constant=1.0,
        coupling="DC",
        slope=24,
    )

    assert applied == {
        "Sensitivity": 100e-6,
        "Time Constant": 1.0,
        "Coupling": "DC",
        "Slope": 24,
    }
    assert lockin.calls == [
        ("sensitivity", 100e-6),
        ("time_constant", 1.0),
        ("coupling", "DC"),
        ("slope", 24),
    ]


def test_set_lockin_settings_supports_model_specific_ac_gain():
    lockin = FakeLockin()

    applied = set_lockin_settings(lockin=lockin, ac_gain=100.0)

    assert applied == {"AC Gain": 100.0}
    assert lockin.calls == [("ac_gain", 100.0)]


def test_set_lockin_settings_noops_when_no_values_are_supplied():
    lockin = FakeLockin()

    assert set_lockin_settings(lockin=lockin) == {}
    assert lockin.calls == []


@pytest.mark.parametrize(
    ("keyword", "value"),
    [
        ("sensitivity", float("nan")),
        ("time_constant", float("inf")),
        ("ac_gain", True),
        ("coupling", 1),
        ("slope", 12.5),
        ("slope", True),
    ],
)
def test_set_lockin_settings_validates_all_inputs_before_writing(keyword, value):
    lockin = FakeLockin()

    with pytest.raises(ValueError):
        set_lockin_settings(lockin=lockin, **{keyword: value})

    assert lockin.calls == []


def test_set_lockin_settings_rejects_non_numeric_object_before_writing():
    lockin = FakeLockin()

    with pytest.raises(ValueError, match="sensitivity must be a finite number"):
        set_lockin_settings(lockin=lockin, sensitivity=object())

    assert lockin.calls == []


@pytest.mark.parametrize(
    ("keyword", "readback_attribute", "readback"),
    [
        ("sensitivity", "sensitivity", 2e-3),
        ("time_constant", "time_constant", 2.0),
        ("ac_gain", "ac_gain", 20.0),
        ("coupling", "coupling", "DC"),
        ("slope", "slope", 24),
    ],
)
def test_set_lockin_settings_rejects_readback_mismatch(
    monkeypatch, keyword, readback_attribute, readback
):
    lockin = FakeLockin()
    setter = getattr(lockin, f"set_{keyword}")

    def ignore_write(value):
        lockin.calls.append((keyword, value))

    monkeypatch.setattr(lockin, f"set_{keyword}", ignore_write)
    setattr(lockin, readback_attribute, readback)

    with pytest.raises(RuntimeError, match="read-back mismatch"):
        set_lockin_settings(
            lockin=lockin,
            **{
                keyword: 1e-3
                if keyword != "coupling" and keyword != "slope"
                else ("AC" if keyword == "coupling" else 12)
            },
        )

    assert setter is not None


def test_set_lockin_settings_rejects_invalid_coupling_readback_type(monkeypatch):
    lockin = FakeLockin()
    monkeypatch.setattr(lockin, "get_coupling", lambda: 1)

    with pytest.raises(RuntimeError, match="invalid type: int"):
        set_lockin_settings(lockin=lockin, coupling="DC")

    assert lockin.calls == [("coupling", "DC")]


def test_device_session_lockin_settings_auto_connect_by_default(monkeypatch):
    lockin = FakeLockin()
    monkeypatch.setattr(session_module, "connect_lockin", lambda config: lockin)

    session = DeviceSession(config_with_lockin())

    applied = session.set_lockin_settings(sensitivity=2e-6)

    assert applied == {"Sensitivity": 2e-6}
    assert session.lockins["main"] is lockin


def test_device_session_lockin_settings_can_require_explicit_connect(monkeypatch):
    monkeypatch.setattr(
        session_module,
        "connect_lockin",
        lambda config: pytest.fail("unexpected auto-connect"),
    )
    session = DeviceSession(config_with_lockin(), auto_connect=False)

    with pytest.raises(RuntimeError, match="Device not connected: lockin.main"):
        session.set_lockin_settings(sensitivity=2e-6)

    lockin = FakeLockin()
    session.lockins["main"] = lockin

    assert session.set_lockin_settings(time_constant=3.0) == {"Time Constant": 3.0}


def test_experiment_exposes_lockin_settings_facade():
    experiment = Experiment(config_with_lockin(), auto_connect=False)
    lockin = FakeLockin()
    experiment.session.lockins["main"] = lockin

    applied = experiment.set_lockin_settings("lockin.main", coupling="AC", slope=6)

    assert applied == {"Coupling": "AC", "Slope": 6}
    assert lockin.calls == [("coupling", "AC"), ("slope", 6)]


def test_lockin_read_helpers_use_explicit_handle_and_delegate_multiplier(monkeypatch):
    lockin = FakeLockin()
    monkeypatch.setattr(
        lockin_module,
        "_connect_lockin",
        lambda _config: pytest.fail("explicit handle must bypass connection"),
    )

    assert lockin_module.read_lockin_signal(lockin=lockin) == {"X": 1.0, "Y": 2.0}
    assert lockin_module.read_lockin_settings(lockin=lockin) == {
        "Sensitivity": 1e-3,
        "Time Constant": 0.3,
        "Ref. Freq": 137.0,
    }
    assert lockin_module.read_lockin_overload(lockin=lockin) == {"overload": False}
    assert lockin_module.get_lockin_wait_time(lockin=lockin, multiplier=2.5) == 0.75
    assert lockin.calls == [
        ("live_data", None),
        ("ref_freq", None),
        ("overload", None),
        ("wait_time", 2.5),
    ]


def test_lockin_helpers_connect_from_config_and_preserve_reference(monkeypatch):
    lockin = FakeLockin()
    config = {"resource": "GPIB0::8::INSTR"}
    connected: list[dict] = []
    monkeypatch.setattr(
        lockin_module,
        "_connect_lockin",
        lambda received: connected.append(received) or lockin,
    )

    assert lockin_module.connect_lockin(config) is lockin
    assert lockin_module.read_lockin_signal(config) == {"X": 1.0, "Y": 2.0}

    assert connected == [config, config]


def test_lockin_connect_and_disconnect_failures_propagate(monkeypatch):
    config = {"resource": "missing"}

    def fail_connect(received):
        assert received is config
        raise OSError("VISA unavailable")

    def fail_disconnect(received):
        assert received is config
        raise OSError("VISA close failed")

    monkeypatch.setattr(lockin_module, "_connect_lockin", fail_connect)
    monkeypatch.setattr(lockin_module, "_disconnect_lockin", fail_disconnect)

    with pytest.raises(OSError, match="VISA unavailable"):
        lockin_module.connect_lockin(config)
    with pytest.raises(OSError, match="VISA close failed"):
        lockin_module.disconnect_lockin(config)
