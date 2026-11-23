"""Execute the GUI simulation notebook against a fake common-protocol TRKR GUI.

No GUI, controller or instrument is started; the endpoint is a local socket that
speaks KohdaLab Control Protocol 1 and records every request.
"""

from __future__ import annotations

import json
import os
import socket
import sys
import threading
import uuid
from pathlib import Path

import pytest

ROOT = Path(__file__).parents[1]
NOTEBOOK = ROOT / "notebook" / "gui_simulation_notebook.ipynb"


class AppError(ValueError):
    def __init__(self, application_code, message, code="validation"):
        super().__init__(message)
        self.application_code = application_code
        self.code = code


class FakeGui:
    def __init__(self, *, source="demo", finishes=True, lose_start=None,
                 failing=()):
        self.source = source
        self.finishes = finishes
        # None, "reply" (job created, reply lost) or "request" (never arrives).
        self.lose_start = lose_start
        self.failing = set(failing)
        self.requests: list[dict] = []
        self.jobs: dict[str, dict] = {}
        self.lease = None
        self.listener = socket.socket()
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen()
        self.listener.settimeout(0.1)
        self.stopping = threading.Event()
        self.thread = threading.Thread(target=self.serve, daemon=True)
        self.thread.start()

    def descriptor(self, directory: Path) -> Path:
        path = directory / "trkr-gui-1-test.json"
        path.write_text(json.dumps({
            "control_protocol": 1, "discovery_version": 1, "transport": "tcp-jsonl",
            "application": "kohdalab-trkr", "host": "127.0.0.1",
            "port": self.listener.getsockname()[1], "pid": 1,
            "token": "token-" + uuid.uuid4().hex, "instance_id": str(uuid.uuid4()),
        }), encoding="utf-8")
        os.chmod(path, 0o600)
        return path

    def methods(self) -> list[str]:
        return [r["method"].removeprefix("control.") for r in self.requests
                if r["method"] != "control.heartbeat"]

    def close(self):
        self.stopping.set()
        self.thread.join(2)
        self.listener.close()

    def serve(self):
        while not self.stopping.is_set():
            try:
                peer, _ = self.listener.accept()
            except (TimeoutError, socket.timeout):
                continue
            with peer:
                body = json.loads(peer.makefile("rb").readline())
                self.requests.append(body)
                lost = body["method"] == "control.start" and self.lose_start
                if lost == "request":
                    continue  # closed without effect or reply
                try:
                    result, error = self.handle(body), None
                except ValueError as failure:
                    result, error = None, {
                        "code": getattr(failure, "code", "validation"),
                        "application_code": getattr(failure, "application_code",
                                                    "invalid_request"),
                        "message": str(failure), "retryable": False}
                if lost == "reply":
                    continue  # applied, but the acknowledgement is lost
                peer.sendall((json.dumps({
                    "control_protocol": 1, "ok": error is None,
                    "request_id": body.get("request_id"), "result": result,
                    "error": error}) + "\n").encode())

    def receipt(self, job_id):
        job = self.jobs[job_id]
        return {"job_id": job_id, "kind": "start", "status": job["status"],
                "terminal": job["status"] in ("completed", "stopped", "error"),
                "cleanup": "verified" if job["status"] in ("completed", "stopped")
                else "pending", "linked_job_id": None, "native": job}

    def handle(self, body):
        method = body["method"].removeprefix("control.")
        if method in self.failing:
            raise AppError("busy", f"{method} failed in the fake GUI")
        mutation = method not in ("describe", "status", "config", "plan", "result")
        if mutation and method != "acquire" and body.get("lease") != self.lease:
            raise ValueError("lease mismatch")
        if method == "describe":
            return {"control_protocol": 1, "application": "kohdalab-trkr",
                    "application_version": "test", "methods": [
                        "control." + m for m in ("describe", "status", "config", "plan",
                        "acquire", "heartbeat", "release", "result", "configure",
                        "start", "stop")],
                    "safety": {"on_release": "continue", "stop_requires_lease": True}}
        if method == "status":
            return {"phase": "ready", "busy": False, "cleanup": "not_required",
                    "ownership": {"owned": self.lease is not None}}
        if method == "config":
            return {"source": self.source}
        if method == "plan":
            return {"workflow": body.get("workflow"),
                    "plan": {"workflow": "signal_monitor", "n_points": 100, "interval_s": 1.0}}
        if method == "acquire":
            self.lease = "lease-" + uuid.uuid4().hex
            return {"client_id": body["client_id"], "lease": self.lease,
                    "heartbeat_seconds": 0.5, "timeout_seconds": 6}
        if method == "heartbeat":
            return {"alive": True}
        if method == "configure":
            self.source = body["source"]
            return {"source": self.source}
        if method == "start":
            assert self.source == "demo"
            job_id = body["request_id"]
            if job_id in self.jobs:
                raise AppError("duplicate", "start was resent")
            self.jobs[job_id] = {"id": job_id, "status": "running", "polls": 0,
                                 "rows": 0, "output": body["output"], "plan": body["plan"]}
            return self.receipt(job_id)
        if method == "result":
            if body["job_id"] not in self.jobs:
                raise AppError("unknown_run", "Unknown or expired run ID", "not_found")
            job = self.jobs[body["job_id"]]
            job["polls"] += 1
            if self.finishes and job["status"] == "running" and job["polls"] >= 2:
                job.update(status="completed", rows=job["plan"]["n_points"])
            return self.receipt(body["job_id"])
        if method == "stop":
            if not body.get("job_id"):
                raise AppError("untargeted_stop", "stop without job_id")
            job = self.jobs[body["job_id"]]
            requested = job["status"] == "running"
            job["status"] = "stopped"
            return {"requested": requested, "job_id": body["job_id"],
                    "cleanup": "pending" if requested else "verified", "native": job}
        if method == "release":
            self.lease = None
            return {"released": True, "cleanup": "verified", "continuing": False}
        raise ValueError("unknown method " + method)


@pytest.fixture
def notebook_env(tmp_path, monkeypatch):
    monkeypatch.chdir(ROOT / "notebook")
    monkeypatch.setenv("HOME", str(tmp_path))
    monkeypatch.setenv("USERPROFILE", str(tmp_path))
    monkeypatch.setattr(sys, "path", list(sys.path))
    monkeypatch.delenv("TRKR_CONTROL_ENDPOINT", raising=False)
    sys.modules.pop("kohdalab_control", None)
    gui = FakeGui()
    yield gui, tmp_path
    gui.close()


def cells():
    notebook = json.loads(NOTEBOOK.read_text(encoding="utf-8"))
    return [c for c in notebook["cells"] if c["cell_type"] == "code"]


def execute(overrides=None):
    namespace = {"__name__": "__main__"}
    for cell in cells():
        exec(compile("".join(cell["source"]), f"{NOTEBOOK.name}:{cell['id']}", "exec"),
             namespace)
        if cell["id"] == "setup":
            namespace.update(overrides or {})
    return namespace


def test_gui_notebook_is_clean_and_has_no_headless_controller():
    notebook = json.loads(NOTEBOOK.read_text(encoding="utf-8"))
    assert notebook["metadata"]["kernelspec"]["name"] == "python3"
    source = "".join("".join(c["source"]) for c in notebook["cells"])
    for forbidden in ("kohdalab_trkr", "find_binary", "subprocess", "Popen",
                      "SWITCH_TO_DEMO", '"configure"', "client.stop("):
        assert forbidden not in source
    for cell in cells():
        assert cell["execution_count"] is None
        assert cell["outputs"] == []


def test_gui_notebook_acquires_inside_the_protected_scope():
    run = next(c for c in cells() if c["id"] == "run")
    source = "".join(run["source"])
    assert source.index("try:\n    client.acquire(") < source.index("finally:")
    # Every mutation after acquisition is in the same cell; none elsewhere.
    for cell in cells():
        if cell["id"] != "run":
            text = "".join(cell["source"])
            assert "acquire(" not in text and "invoke(" not in text


def start_requests(gui):
    return [r for r in gui.requests if r["method"] == "control.start"]


def test_gui_notebook_requires_an_explicit_endpoint(notebook_env):
    gui, _ = notebook_env
    with pytest.raises(RuntimeError, match="Set ENDPOINT"):
        execute()
    assert gui.requests == []


def test_gui_notebook_runs_one_demo_job_by_receipt_and_releases(notebook_env, monkeypatch):
    gui, tmp_path = notebook_env
    monkeypatch.setenv("TRKR_CONTROL_ENDPOINT", str(gui.descriptor(tmp_path)))
    namespace = execute({"POLL_INTERVAL_S": 0.01})
    assert namespace["HARDWARE"] is False
    methods = gui.methods()
    assert methods[:4] == ["describe", "status", "acquire", "config"]
    # Opt-in stop policy for this lease only; app defaults are untouched.
    [acquire] = [r for r in gui.requests if r["method"] == "control.acquire"]
    assert acquire["stop_on_disconnect"] is True
    assert acquire["stop_on_error"] is True
    assert "configure" not in methods and gui.source == "demo"
    assert "stop" not in methods
    assert methods[-1] == "release"
    [start] = start_requests(gui)
    job_id = namespace["receipt"]["job_id"]
    assert start["request_id"] == job_id == namespace["START_REQUEST_ID"]
    assert namespace["cleanup_errors"] == []
    assert start["output"] == str(namespace["OUTPUT_CSV"])  # explicit, not GUI default
    assert {r["job_id"] for r in gui.requests if r["method"] == "control.result"} == {job_id}
    assert namespace["finished"]["status"] == "completed"
    assert namespace["finished"]["native"]["rows"] == 5
    assert namespace["released"]["cleanup"] == "verified"
    assert gui.lease is None


def test_gui_notebook_stops_its_job_before_release_on_timeout(notebook_env, monkeypatch):
    gui, tmp_path = notebook_env
    gui.finishes = False
    monkeypatch.setenv("TRKR_CONTROL_ENDPOINT", str(gui.descriptor(tmp_path)))
    with pytest.raises(Exception) as caught:
        execute({"JOB_TIMEOUT_S": 0.2, "POLL_INTERVAL_S": 0.01})
    assert getattr(caught.value, "code", None) == "timeout"
    methods = gui.methods()
    stop = next(r for r in gui.requests if r["method"] == "control.stop")
    job_id = next(r for r in gui.requests if r["method"] == "control.start")["request_id"]
    assert stop["job_id"] == job_id
    assert methods.index("stop") < methods.index("release")
    assert methods[-1] == "release"
    assert gui.jobs[job_id]["status"] == "stopped"
    assert gui.lease is None


def test_gui_notebook_requires_demo_already_selected_and_never_switches(
        notebook_env, monkeypatch):
    gui, tmp_path = notebook_env
    gui.source = "hardware"
    monkeypatch.setenv("TRKR_CONTROL_ENDPOINT", str(gui.descriptor(tmp_path)))
    with pytest.raises(RuntimeError, match="Select Demo in the GUI first"):
        execute()
    methods = gui.methods()
    assert methods == ["describe", "status", "acquire", "config", "release"]
    assert gui.source == "hardware"
    assert gui.lease is None


def test_gui_notebook_reconciles_a_lost_start_reply_without_resending(
        notebook_env, monkeypatch):
    gui, tmp_path = notebook_env
    gui.lose_start = "reply"
    monkeypatch.setenv("TRKR_CONTROL_ENDPOINT", str(gui.descriptor(tmp_path)))
    namespace = execute({"POLL_INTERVAL_S": 0.01})
    job_id = namespace["START_REQUEST_ID"]
    [start] = start_requests(gui)
    assert start["request_id"] == job_id
    assert namespace["receipt"]["job_id"] == job_id
    assert {r["job_id"] for r in gui.requests if r["method"] == "control.result"} == {job_id}
    assert "stop" not in gui.methods()
    assert namespace["finished"]["status"] == "completed"
    assert gui.methods()[-1] == "release" and gui.lease is None
    assert namespace["cleanup_errors"] == []


def test_gui_notebook_lost_start_that_never_ran_raises_original_without_stop(
        notebook_env, monkeypatch):
    gui, tmp_path = notebook_env
    gui.lose_start = "request"
    monkeypatch.setenv("TRKR_CONTROL_ENDPOINT", str(gui.descriptor(tmp_path)))
    with pytest.raises(Exception) as caught:
        execute({"POLL_INTERVAL_S": 0.01, "RECONCILE_TIMEOUT_S": 0.1})
    error = caught.value  # the original lost-reply error, not a cleanup error
    assert error.code == "transport" and error.outcome_unknown is True
    [start] = start_requests(gui)  # never resent
    assert error.request_id == start["request_id"]
    assert gui.jobs == {}
    assert "stop" not in gui.methods()
    assert {r["job_id"] for r in gui.requests if r["method"] == "control.result"} == {
        start["request_id"]}
    assert gui.methods()[-1] == "release" and gui.lease is None
    if hasattr(error, "__notes__"):
        assert any("was not resent" in note for note in error.__notes__)


def test_gui_notebook_keeps_original_error_and_reports_cleanup_errors(
        notebook_env, monkeypatch, capsys):
    gui, tmp_path = notebook_env
    gui.finishes = False
    gui.failing = {"stop"}
    monkeypatch.setenv("TRKR_CONTROL_ENDPOINT", str(gui.descriptor(tmp_path)))
    with pytest.raises(Exception) as caught:
        execute({"JOB_TIMEOUT_S": 0.2, "POLL_INTERVAL_S": 0.01})
    error = caught.value
    assert error.code == "timeout"  # the wait timeout, not the stop failure
    assert "Cleanup error during stop" in capsys.readouterr().err
    if hasattr(error, "__notes__"):
        assert any("Cleanup error during stop" in note for note in error.__notes__)
    # Release still runs after the failed stop.
    assert gui.methods()[-1] == "release" and gui.lease is None


def test_gui_notebook_raises_cleanup_error_when_the_run_succeeded(
        notebook_env, monkeypatch, capsys):
    gui, tmp_path = notebook_env
    gui.failing = {"release"}
    monkeypatch.setenv("TRKR_CONTROL_ENDPOINT", str(gui.descriptor(tmp_path)))
    with pytest.raises(RuntimeError, match="Cleanup failed during release") as caught:
        execute({"POLL_INTERVAL_S": 0.01})
    assert getattr(caught.value.__cause__, "application_code", None) == "busy"
    assert "Cleanup error during release" in capsys.readouterr().err
    assert "stop" not in gui.methods()
