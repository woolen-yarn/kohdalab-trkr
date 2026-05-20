from __future__ import annotations

from threading import Event, Thread

from kohdalab.api.session import DeviceSession
import kohdalab.api.session as session_module


def config_with_devices() -> dict:
    return {
        "instruments": {
            "lockin": {"main": {}},
            "delay_stage": {"t": {}},
            "scanner": {"x": {}, "y": {}},
        }
    }


def _run_threaded(target, errors: list[Exception]) -> Thread:
    def runner():
        try:
            target()
        except Exception as e:  # pragma: no cover - assertion below reports it
            errors.append(e)

    thread = Thread(target=runner)
    thread.start()
    return thread


def test_device_session_serializes_lockin_io(monkeypatch):
    session = DeviceSession(config_with_devices(), auto_connect=False)
    session.lockins["main"] = object()
    entered_signal = Event()
    release_signal = Event()
    entered_wait_time = Event()
    errors: list[Exception] = []

    def read_signal(config, *, lockin):
        entered_signal.set()
        assert release_signal.wait(2)
        return {"X": 1.0}

    def wait_time(config, *, lockin, multiplier):
        entered_wait_time.set()
        return multiplier

    monkeypatch.setattr(session_module, "read_lockin_signal", read_signal)
    monkeypatch.setattr(session_module, "get_lockin_wait_time", wait_time)

    signal_thread = _run_threaded(session.read_lockin_signal, errors)
    assert entered_signal.wait(1)

    wait_thread = _run_threaded(session.lockin_wait_time, errors)
    assert not entered_wait_time.wait(0.1)

    release_signal.set()
    signal_thread.join(2)
    wait_thread.join(2)

    assert entered_wait_time.is_set()
    assert errors == []


def test_device_session_allows_lockin_io_during_delay_stage_move(monkeypatch):
    session = DeviceSession(config_with_devices(), auto_connect=False)
    session.lockins["main"] = object()
    session.delay_stages["t"] = object()
    entered_move = Event()
    release_move = Event()
    entered_wait_time = Event()
    errors: list[Exception] = []

    def move_delay_stage(**kwargs):
        entered_move.set()
        assert release_move.wait(2)
        return {"t_ps": kwargs["value"], "stage_mm": 0.0, "stage_pulse": 0}

    def wait_time(config, *, lockin, multiplier):
        entered_wait_time.set()
        return multiplier

    monkeypatch.setattr(session_module, "move_delay_stage_abs", move_delay_stage)
    monkeypatch.setattr(session_module, "get_lockin_wait_time", wait_time)

    move_thread = _run_threaded(lambda: session.move_delay_stage(1.0), errors)
    assert entered_move.wait(1)

    wait_thread = _run_threaded(session.lockin_wait_time, errors)
    assert entered_wait_time.wait(1)

    release_move.set()
    move_thread.join(2)
    wait_thread.join(2)

    assert errors == []
