from __future__ import annotations

import pytest

from kohdalab.api import Experiment, set_lockin_settings
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


def config_with_lockin() -> dict:
    return {
        "instruments": {
            "lockin": {
                "main": {},
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


def test_device_session_lockin_settings_auto_connect_by_default(monkeypatch):
    lockin = FakeLockin()
    monkeypatch.setattr(session_module, "connect_lockin", lambda config: lockin)

    session = DeviceSession(config_with_lockin())

    applied = session.set_lockin_settings(sensitivity=2e-6)

    assert applied == {"Sensitivity": 2e-6}
    assert session.lockins["main"] is lockin


def test_device_session_lockin_settings_can_require_explicit_connect(monkeypatch):
    monkeypatch.setattr(session_module, "connect_lockin", lambda config: pytest.fail("unexpected auto-connect"))
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
