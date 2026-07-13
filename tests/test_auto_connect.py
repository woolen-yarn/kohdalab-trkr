from __future__ import annotations

import pytest

from kohdalab.api import Experiment
from kohdalab.api.session import DeviceSession
import kohdalab.api.session as session_module


def config_with_devices() -> dict:
    return {
        "instruments": {
            "lockin": {"main": {"resource": "LOCKIN"}},
            "delay_stage": {"t": {"port": "STAGE"}},
            "scanner": {
                "x": {"port": "SCANNER", "axis": 1},
                "y": {"port": "SCANNER", "axis": 2},
            },
        }
    }


def test_device_session_auto_connects_by_default(monkeypatch):
    connected: list[str] = []

    def connect_lockin(config):
        connected.append("lockin")
        return object()

    monkeypatch.setattr(session_module, "connect_lockin", connect_lockin)
    monkeypatch.setattr(
        session_module,
        "read_lockin_signal",
        lambda config, *, lockin: {"X": 1, "Y": 2, "R": 3, "Theta": 4},
    )

    session = DeviceSession(config_with_devices())

    assert session.read_lockin_signal() == {"X": 1, "Y": 2, "R": 3, "Theta": 4}
    assert connected == ["lockin"]
    assert session.connected_devices()["lockin.main"]


def test_device_session_can_require_explicit_lockin_connect(monkeypatch):
    monkeypatch.setattr(
        session_module,
        "connect_lockin",
        lambda config: pytest.fail("unexpected auto-connect"),
    )

    session = DeviceSession(config_with_devices(), auto_connect=False)

    with pytest.raises(RuntimeError, match="Device not connected: lockin.main"):
        session.read_lockin_signal()


def test_explicit_connect_still_works_when_auto_connect_is_disabled(monkeypatch):
    monkeypatch.setattr(
        session_module, "connect_lockin", lambda config: "lockin-handle"
    )
    monkeypatch.setattr(
        session_module,
        "read_lockin_signal",
        lambda config, *, lockin: {"handle": lockin},
    )

    session = DeviceSession(config_with_devices(), auto_connect=False)

    assert session.connect_device("lockin.main") == "lockin-handle"
    assert session.read_lockin_signal() == {"handle": "lockin-handle"}


def test_motion_requires_explicit_connect_when_auto_connect_is_disabled(monkeypatch):
    monkeypatch.setattr(
        session_module,
        "connect_delay_stage",
        lambda config: pytest.fail("unexpected delay auto-connect"),
    )
    monkeypatch.setattr(
        session_module,
        "connect_scanner",
        lambda config: pytest.fail("unexpected scanner auto-connect"),
    )

    session = DeviceSession(config_with_devices(), auto_connect=False)

    with pytest.raises(RuntimeError, match="Device not connected: delay_stage.t"):
        session.move_delay_stage(1.0)
    with pytest.raises(RuntimeError, match="Device not connected: scanner.x"):
        session.move_scanner("x", 1.0)


def test_experiment_passes_auto_connect_policy_to_session():
    experiment = Experiment(config_with_devices(), auto_connect=False)

    assert experiment.auto_connect is False
    assert experiment.session.auto_connect is False
