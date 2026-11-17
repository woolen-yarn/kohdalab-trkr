"""Optional end-to-end checks against a locally built native controller."""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).parents[1]


def test_standalone_move_compensation_default_and_explicit_override() -> None:
    import importlib.util

    spec = importlib.util.spec_from_file_location(
        "trkr_move_client_test", ROOT / "bindings/python/kohdalab_trkr.py"
    )
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    client = object.__new__(module.Client)
    commands = []
    client.send = commands.append
    for targets in ([None, 20.0, None], [None, None, 30.0], [None, 20.0, 30.0], [10.0, None, None]):
        client.move({}, targets)
        assert commands[-1]["targets"] == targets
        assert commands[-1]["approach"] == [False, True, True]
    client.move({}, [None, 20.0, None], approach=[False, False, False])
    assert commands[-1]["approach"] == [False, False, False]


def _fake_native_client(events: list[dict | None]):
    import importlib.util
    import queue
    import threading

    spec = importlib.util.spec_from_file_location(
        "trkr_move_wait_test", ROOT / "bindings/python/kohdalab_trkr.py"
    )
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    client = object.__new__(module.Client)
    client._job_lock = threading.Lock()
    client._last_job = None
    client._unsynced = None
    client._events = queue.Queue()
    for event in events:
        client._events.put(event)
    sent: list[dict] = []

    def send(command: dict) -> None:
        sent.append(command)
        if command["command"] in {"move", "initialize", "run", "replay"}:
            client._last_job = command["command"]

    client.send = send
    return module, client, sent


def _state(running: bool, phase: str) -> dict:
    return {"event": "state", "data": {"running": running, "phase": phase}}


def test_move_and_wait_completes_on_its_own_terminal_state() -> None:
    # An idle refresh (running false) and an idle error precede the job and
    # must not be mistaken for the move's completion or failure.
    module, client, sent = _fake_native_client([
        _state(False, "Complete"),
        {"event": "error", "data": "idle position read failed"},
        _state(True, "Preparing"),
        _state(True, "Moving X"),
        _state(False, "Complete"),
    ])
    events = client.move_and_wait({}, [None, 20.0, None], timeout=1.0)
    assert sent[-1]["command"] == "move" and sent[-1]["approach"] == [False, True, True]
    assert events[-1] == _state(False, "Complete")
    assert client._events.empty()


def test_send_style_move_then_wait_for_finished_does_not_hang() -> None:
    module, client, _ = _fake_native_client([_state(True, "Preparing"), _state(False, "Complete")])
    client.move({}, [10.0, None, None])
    assert client.wait_for_finished(timeout=1.0)[-1]["data"]["phase"] == "Complete"


def test_move_and_wait_reports_failure_including_stop_failure() -> None:
    module, client, _ = _fake_native_client([
        _state(True, "Preparing"),
        {"event": "error", "data": "limit; STOP failed: timeout"},
        _state(False, "Error"),
    ])
    with pytest.raises(RuntimeError, match="STOP failed") as error:
        client.move_and_wait({}, [None, 20.0, None], timeout=1.0)
    assert not isinstance(error.value, module.NativeStopped)


def test_move_and_wait_reports_rejection_and_cancellation() -> None:
    module, client, _ = _fake_native_client([{"event": "rejected", "data": "busy"}])
    with pytest.raises(RuntimeError, match="busy"):
        client.move_and_wait({}, [None, 20.0, None], timeout=1.0)
    module, client, _ = _fake_native_client([_state(True, "Preparing"), _state(False, "Stopped")])
    with pytest.raises(module.NativeStopped):
        client.move_and_wait({}, [None, 20.0, None], timeout=1.0)


def test_move_and_wait_timeout_requests_stop_and_drains_terminal_state() -> None:
    module, client, sent = _fake_native_client([_state(True, "Preparing")])
    original_send = client.send

    def send(command: dict) -> None:
        original_send(command)
        if command["command"] == "stop":
            client._events.put(_state(False, "Stopped"))

    client.send = send
    with pytest.raises(module.NativeTimeoutError) as error:
        client.move_and_wait({}, [None, 20.0, None], timeout=0.05)
    assert [command["command"] for command in sent] == ["move", "stop"]
    assert error.value.events[-1] == _state(False, "Stopped")
    assert client._unsynced is None  # stream drained; client stays usable


def test_move_and_wait_timeout_without_terminal_state_is_bounded(monkeypatch) -> None:
    module, client, sent = _fake_native_client([_state(True, "Preparing")])
    monkeypatch.setattr(module, "_STOP_DRAIN_S", 0.05)
    with pytest.raises(module.NativeTimeoutError):
        client.move_and_wait({}, [None, 20.0, None], timeout=0.05)
    assert sent[-1]["command"] == "stop"
    with pytest.raises(RuntimeError, match="reopen"):
        client.move_and_wait({}, [None, 20.0, None], timeout=0.05)
    with pytest.raises(ValueError):
        client.move_and_wait({}, [None, 20.0, None], timeout=0.0)


def test_run_warns_when_native_sample_events_were_dropped() -> None:
    finished = {"event": "finished", "data": {"Ok": [6, None]}}
    module, client, _ = _fake_native_client([
        {"event": "state", "data": {"running": False, "phase": "Complete", "dropped_samples": 5}},
        finished,
    ])
    with pytest.warns(RuntimeWarning, match="5 native sample events were dropped"):
        events = client.run({}, {}, {"type": "demo"})
    assert events[-1] == finished
    module, client, _ = _fake_native_client([
        {"event": "state", "data": {"running": False, "phase": "Complete"}},
        finished,
    ])
    import warnings as _warnings
    with _warnings.catch_warnings():
        _warnings.simplefilter("error")
        client.run({}, {}, {"type": "demo"})


def test_notebook_runs_native_workflows_recovers_errors_and_stops(
    tmp_path: Path,
) -> None:
    binary = Path(
        os.environ.get(
            "TRKR_NATIVE_BINARY",
            ROOT

            / "target"
            / "debug"
            / (
                "kohdalab-trkr.exe"
                if sys.platform == "win32"
                else "kohdalab-trkr"
            ),
        )
    )
    if not binary.is_file():
        pytest.skip("Build native TRKR or set TRKR_NATIVE_BINARY")
    code = r"""
import csv, json, subprocess, sys, threading
from pathlib import Path
sys.path.insert(0, str(Path(sys.argv[1]) / 'bindings/python'))
from kohdalab_trkr import Client
root, binary, output = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])
prepared = json.loads(subprocess.check_output([binary, '--profile-json', root/'crates/trkr-desktop/assets/default.json'], text=True))
with Client(binary) as client:
    for key, plan in prepared['plans'].items():
        plan.update(interval_s=0., n_points=7)
        path=output/f'{key}.csv'
        events=client.run(prepared['hardware'], plan, {'type':'demo'}, path)
        count=len(list(csv.DictReader(path.open())))
        meta=json.loads(path.with_suffix('.run.json').read_text())
        assert meta['status']=='complete' and meta['rows']==count and meta['simulation']
        assert next(e for e in events if e['event']=='finished')['data']['Ok'][0]==count
    plan=prepared['plans']['signal_monitor']
    collision=output/'signal_monitor.csv'
    original=collision.read_bytes()
    try:
        client.run(prepared['hardware'], plan, {'type':'demo'}, collision)
    except RuntimeError:
        pass
    else:
        raise AssertionError('existing CSV was overwritten')
    assert collision.read_bytes()==original
    events=client.run(prepared['hardware'], plan, {'type':'demo'}, output/'recovered.csv')
    assert next(e for e in events if e['event']=='finished')['data']['Ok'][0]==7
    try:
        client.run(prepared['hardware'], {}, {'type':'demo'})
    except RuntimeError:
        pass
    else:
        raise AssertionError('invalid command was accepted')
    plan.update(interval_s=1., n_points=100)
    timer=threading.Timer(.1, client.stop)
    timer.start()
    client.run(prepared['hardware'], plan, {'type':'demo'}, output/'stopped.csv')
    timer.join()
    meta=json.loads((output/'stopped.run.json').read_text())
    assert meta['status']=='cancelled' and meta['rows']<100
"""
    result = subprocess.run(  # noqa: S603 - locally built controller via fixed validation script
        [sys.executable, "-c", code, str(ROOT), str(binary.resolve()), str(tmp_path)],
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr
