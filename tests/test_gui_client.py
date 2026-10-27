"""Focused protocol tests for the optional external GUI client."""
from __future__ import annotations

import importlib.util
import json
import socket
import threading
from pathlib import Path

import pytest


ROOT = Path(__file__).parents[1]


@pytest.fixture
def api_module():
    spec = importlib.util.spec_from_file_location(
        "trkr_gui_client_test", ROOT / "bindings/python/kohdalab_trkr.py"
    )
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class RpcServer:
    def __init__(self, handler):
        self.handler = handler
        self.requests = []
        self._closed = threading.Event()
        self.socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.socket.bind(("127.0.0.1", 0))
        self.socket.listen()
        self.socket.settimeout(0.05)
        self.port = self.socket.getsockname()[1]
        self.thread = threading.Thread(target=self._serve, daemon=True)
        self.thread.start()

    def _serve(self):
        while not self._closed.is_set():
            try:
                connection, _ = self.socket.accept()
            except TimeoutError:
                continue
            with connection:
                request = bytearray()
                while b"\n" not in request:
                    chunk = connection.recv(4096)
                    if not chunk:
                        break
                    request.extend(chunk)
                if not request:
                    continue
                payload = json.loads(request.split(b"\n", 1)[0])
                self.requests.append(payload)
                response = self.handler(payload)
                connection.sendall((json.dumps(response) + "\n").encode())

    def close(self):
        self._closed.set()
        self.thread.join(timeout=1)
        self.socket.close()


@pytest.fixture
def server():
    servers = []

    def start(handler):
        item = RpcServer(handler)
        servers.append(item)
        return item

    yield start
    for item in servers:
        item.close()


def descriptor(path: Path, port: int, *, host: str = "127.0.0.1") -> Path:
    path.write_text(json.dumps({
        "revision": 1, "host": host, "port": port, "token": "secret",
        "instance_id": "gui-1", "pid": 4321,
    }), encoding="utf-8")
    return path


def test_gui_client_sends_authenticated_one_socket_rpcs(api_module, server, tmp_path):
    records = [{"id": "run-1", "status": "completed", "rows": 3, "output": "run.csv", "error": None}]

    def handler(request):
        action = request["action"]
        if action == "status":
            result = {"snapshot": {"state": "ready"}, "active_run": None}
        elif action == "plan":
            result = {"plan": {"interval_s": 1}, "workflow": request.get("workflow", "trkr"), "source": "gui"}
        elif action == "start":
            result = records[0]
        elif action == "result":
            result = records[0]
        elif action == "stop":
            result = records[0]
        else:
            return {"ok": False, "error": "unknown action"}
        return {"ok": True, "result": result}

    rpc = server(handler)
    client = api_module.GuiClient(descriptor(tmp_path / "session.json", rpc.port))
    assert client.status()["snapshot"]["state"] == "ready"
    assert client.plan("trkr")["source"] == "gui"
    request_id = "12345678-1234-4678-9234-123456789abc"
    assert client.start("trkr", plan={"interval_s": 2}, output=Path("run.csv"), request_id=request_id)["id"] == "run-1"
    assert client.result("run-1")["rows"] == 3
    assert client.stop("run-1")["status"] == "completed"
    assert [item["action"] for item in rpc.requests] == ["status", "plan", "start", "result", "stop"]
    assert all(item["token"] == "secret" for item in rpc.requests)
    start = rpc.requests[2]
    assert start["workflow"] == "trkr" and start["plan"] == {"interval_s": 2}
    assert start["output"] == str(Path("run.csv").resolve()) and start["request_id"] == request_id


def test_discovery_ignores_stale_session_and_rejects_multiple_live_ones(api_module, server, tmp_path, monkeypatch):
    home = tmp_path / "home"
    run_dir = home / ".kohdalab/run"
    run_dir.mkdir(parents=True)
    monkeypatch.setattr(api_module.Path, "home", classmethod(lambda cls: home))

    def ready(_request):
        return {"ok": True, "result": {"snapshot": {}, "active_run": None}}

    live = server(ready)
    descriptor(run_dir / "trkr-gui-stale.json", 1)
    descriptor(run_dir / "trkr-gui-live.json", live.port)
    assert api_module.GuiClient().status()["active_run"] is None
    other = server(ready)
    descriptor(run_dir / "trkr-gui-other.json", other.port)
    with pytest.raises(RuntimeError, match="Multiple running"):
        api_module.GuiClient().status()


def test_wait_terminal_statuses_timeout_and_close_never_stops(api_module, server, tmp_path):
    state = {"status": "running"}

    def handler(request):
        if request["action"] == "result":
            result = {"id": request["run_id"], "status": state["status"], "rows": 0, "output": None, "error": "broken" if state["status"] == "error" else None}
            return {"ok": True, "result": result}
        if request["action"] == "start":
            return {"ok": True, "result": {"id": "run-1", "status": "running", "rows": 0, "output": None, "error": None}}
        return {"ok": True, "result": {"snapshot": {}, "active_run": None}}

    rpc = server(handler)
    client = api_module.GuiClient(descriptor(tmp_path / "session.json", rpc.port))
    with pytest.raises(TimeoutError):
        client.wait("run-1", timeout=0, poll_interval=0.01)
    assert "stop" not in [item["action"] for item in rpc.requests]
    state["status"] = "stopped"
    with pytest.raises(api_module.GuiRunStopped):
        client.wait("run-1", timeout=1)
    state["status"] = "error"
    with pytest.raises(RuntimeError, match="broken"):
        client.wait("run-1", timeout=1)
    before_close = len(rpc.requests)
    client.close()
    assert len(rpc.requests) == before_close
    assert "stop" not in [item["action"] for item in rpc.requests]


def test_invalid_descriptor_rejects_non_loopback_and_bad_port(api_module, tmp_path):
    with pytest.raises(ValueError, match="host"):
        api_module.GuiClient(descriptor(tmp_path / "remote.json", 5000, host="192.168.1.9")).status()
    path = tmp_path / "bad-port.json"
    path.write_text(json.dumps({"revision": 1, "host": "127.0.0.1", "port": 0, "token": "x", "instance_id": "x", "pid": 1}))
    with pytest.raises(ValueError, match="port"):
        api_module.GuiClient(path).status()
