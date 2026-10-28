"""Focused protocol tests for the optional external GUI client."""
from __future__ import annotations

import importlib.util
import gc
import json
import socket
import threading
import time
import weakref
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


def descriptor(path: Path, port: int, *, host: str = "127.0.0.1", pid: int = 4321) -> Path:
    path.write_text(json.dumps({
        "revision": 1, "host": host, "port": port, "token": "secret",
        "instance_id": "gui-1", "pid": pid,
    }), encoding="utf-8")
    return path


def test_gui_client_sends_authenticated_one_socket_rpcs(api_module, server, tmp_path):
    records = [{"id": "run-1", "status": "completed", "rows": 3, "output": "run.csv", "error": None, "kind": "trkr"}]

    def handler(request):
        action = request["action"]
        if action == "status":
            result = {"snapshot": {"state": "ready"}, "active_run": None}
        elif action == "plan":
            result = {"plan": {"interval_s": 1}, "workflow": request.get("workflow", "trkr"), "source": "gui"}
        elif action in {"acquire", "heartbeat", "release"}:
            result = {"owned": action != "release", "client_id": request["client_id"] if action != "release" else None,
                      "status": "python" if action != "release" else "idle", "active_run": None}
        elif action in {"start", "move", "initialize", "connect", "disconnect", "read", "set_lockin", "auto_lockin", "stop"}:
            result = records[0]
        elif action == "result":
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
    assert [item["action"] for item in rpc.requests] == ["status", "plan", "acquire", "start", "result", "stop"]
    assert all(item["token"] == "secret" for item in rpc.requests)
    assert len({item["client_id"] for item in rpc.requests}) == 1
    start = rpc.requests[3]
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
            result = {"id": request["run_id"], "status": state["status"], "rows": 0, "output": None, "error": "broken" if state["status"] == "error" else None, "kind": "trkr"}
            return {"ok": True, "result": result}
        if request["action"] == "start":
            return {"ok": True, "result": {"id": "run-1", "status": "running", "rows": 0, "output": None, "error": None, "kind": "trkr"}}
        if request["action"] in {"acquire", "heartbeat", "release"}:
            active = None
            return {"ok": True, "result": {"owned": request["action"] != "release", "client_id": request["client_id"] if request["action"] != "release" else None, "status": "python" if request["action"] != "release" else "idle", "active_run": active}}
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


def test_control_lease_and_full_gui_surface(api_module, server, tmp_path):
    def receipt(request):
        return {"id": request.get("request_id", "job"), "status": "completed", "rows": 0,
                "output": None, "error": None, "kind": request["action"]}

    def handler(request):
        action = request["action"]
        if action in {"acquire", "heartbeat"}:
            return {"ok": True, "result": {"owned": True, "client_id": request["client_id"], "status": "python", "active_run": None}}
        if action == "release":
            return {"ok": True, "result": {"owned": False, "client_id": None, "status": "idle", "active_run": None}}
        if action == "config":
            return {"ok": True, "result": {"profile": {"instruments": {}}, "workflow": "trkr", "source": "demo", "config_path": None}}
        if action == "output":
            return {"ok": True, "result": {"dir": "/tmp/data", "filename": "run", "auto_timestamp_suffix": True}}
        if action in {"load_config", "configure", "set_plan", "set_output"}:
            return {"ok": True, "result": {"profile": {}, "workflow": "trkr", "source": "hardware", "config_path": "/tmp/kim.json"}}
        return {"ok": True, "result": receipt(request)}

    rpc = server(handler)
    client = api_module.GuiClient(descriptor(tmp_path / "session.json", rpc.port))
    assert client.config()["source"] == "demo"
    assert client.settings()["kind"] == "lockin_settings"
    assert client.output()["filename"] == "run"
    assert client.load_config("kim.json")["config_path"] == "/tmp/kim.json"
    assert client.configure({"instruments": {}}, source="hardware")["source"] == "hardware"
    assert client.set_plan({"workflow": "trkr"})["workflow"] == "trkr"
    assert client.set_output(directory="data", filename="field.csv", auto_timestamp_suffix=False)["source"] == "hardware"
    assert client.move([1.0, None, 3.0])["kind"] == "move"
    assert client.initialize(1, home=True)["kind"] == "initialize"
    assert client.connect(["lockin"])["kind"] == "connect"
    assert client.disconnect()["kind"] == "disconnect"
    assert client.read()["kind"] == "read"
    assert client.set_lockin("sensitivity", "1 mV")["kind"] == "set_lockin"
    assert client.auto_lockin("auto_sensitivity")["kind"] == "auto_lockin"
    assert all("client_id" in request for request in rpc.requests)
    assert all("request_id" in request for request in rpc.requests if request["action"] in {
        "move", "initialize", "connect", "disconnect", "read", "lockin_settings", "set_lockin", "auto_lockin"
    })
    client.close()
    assert rpc.requests[-1]["action"] == "release"


def test_revoked_lease_refuses_future_mutation(api_module, server, tmp_path):
    state = {"beats": 0}

    def handler(request):
        if request["action"] == "acquire":
            result = {"owned": True, "client_id": request["client_id"], "status": "python", "active_run": None}
        elif request["action"] == "heartbeat":
            state["beats"] += 1
            result = {"owned": False, "client_id": None, "status": "detached", "active_run": None}
        else:
            result = {"id": request.get("request_id", "x"), "status": "completed", "rows": 0, "output": None, "error": None, "kind": request["action"]}
        return {"ok": True, "result": result}

    rpc = server(handler)
    client = api_module.GuiClient(descriptor(tmp_path / "session.json", rpc.port))
    client.acquire()
    client.heartbeat()
    with pytest.raises(RuntimeError, match="revoked"):
        client.move([None, 0.0, None])
    client.close()


def test_dropped_client_stops_heartbeat_without_releasing_or_stopping(api_module):
    requests = []
    client = api_module.GuiClient("unused-session.json")

    def request(operation, **_data):
        requests.append(operation)
        return {"owned": True, "client_id": client_id, "status": "python", "active_run": None}

    client_id = client._client_id
    client._request = request
    client.acquire()
    reference = weakref.ref(client)
    del client
    gc.collect()
    assert reference() is None
    time.sleep(1.05)
    assert requests == ["acquire"]


def test_launch_attaches_only_to_its_own_descriptor(api_module, server, tmp_path, monkeypatch):
    home = tmp_path / "home"
    run_dir = home / ".kohdalab/run"
    run_dir.mkdir(parents=True)
    binary = tmp_path / "kohdalab-trkr"
    binary.write_text("binary", encoding="utf-8")
    ready = server(lambda request: {"ok": True, "result": {"snapshot": {}, "active_run": None}})
    other = descriptor(run_dir / "trkr-gui-other.json", ready.port, pid=9999)
    launched = descriptor(run_dir / "trkr-gui-own.json", ready.port)
    monkeypatch.setattr(api_module.Path, "home", classmethod(lambda cls: home))
    calls = []

    class Process:
        pid = 4321

        @staticmethod
        def poll():
            return None

    def popen(command, **options):
        calls.append((command, options))
        return Process()

    monkeypatch.setattr(api_module.subprocess, "Popen", popen)
    # Always enumerate the unrelated process first: filesystem order differs
    # between Windows/macOS and must never select the GUI to control.
    original_glob = api_module.Path.glob
    monkeypatch.setattr(api_module.Path, "glob", lambda directory, pattern:
                        iter([other, launched]) if directory == run_dir
                        else original_glob(directory, pattern))
    client = api_module.GuiClient.launch(profile="kim.json", binary=binary)
    assert client._session_path == launched
    assert calls[0][0] == [str(binary), "--gui-api", "--config", str(Path("kim.json").resolve())]
    assert calls[0][1]["stdin"] is api_module.subprocess.DEVNULL


def test_launch_reports_early_process_exit(api_module, tmp_path, monkeypatch):
    home = tmp_path / "home"
    (home / ".kohdalab/run").mkdir(parents=True)
    binary = tmp_path / "kohdalab-trkr"
    binary.write_text("binary", encoding="utf-8")
    monkeypatch.setattr(api_module.Path, "home", classmethod(lambda cls: home))

    class Process:
        pid = 4321

        @staticmethod
        def poll():
            return 7

    monkeypatch.setattr(api_module.subprocess, "Popen", lambda *_args, **_kwargs: Process())
    with pytest.raises(RuntimeError, match="exited.*7"):
        api_module.GuiClient.launch(binary=binary)


def test_invalid_descriptor_rejects_non_loopback_and_bad_port(api_module, tmp_path):
    with pytest.raises(ValueError, match="host"):
        api_module.GuiClient(descriptor(tmp_path / "remote.json", 5000, host="192.168.1.9")).status()
    path = tmp_path / "bad-port.json"
    path.write_text(json.dumps({"revision": 1, "host": "127.0.0.1", "port": 0, "token": "x", "instance_id": "x", "pid": 1}))
    with pytest.raises(ValueError, match="port"):
        api_module.GuiClient(path).status()


def policy_server(server, tmp_path):
    state = {"status": "running", "error_code": None}
    def handler(request):
        action = request["action"]
        if action in {"acquire", "heartbeat", "release"}:
            result = {"owned": action != "release", "client_id": request["client_id"],
                      "status": "idle" if action == "release" else "python", "active_run": None,
                      "stop_policy": {k: request.get(k, False) for k in
                                      ("stop_on_disconnect", "stop_on_error", "stop_on_timeout")}}
        else:
            result = {"id": request.get("run_id", "job"), "status": state["status"],
                      "rows": 2, "output": "test.csv", "error": "failed" if state["status"] == "error" else None,
                      "error_code": state["error_code"], "kind": "start"}
        return {"ok": True, "result": result}
    rpc = server(handler)
    return rpc, descriptor(tmp_path / "policy.json", rpc.port), state


def test_stop_conditions_are_independent_and_transmitted(api_module, server, tmp_path):
    rpc, path, _ = policy_server(server, tmp_path)
    with api_module.GuiClient(path, stop_on_error=True) as client:
        client.start()
    acquire = next(r for r in rpc.requests if r["action"] == "acquire")
    assert {key: acquire[key] for key in ("stop_on_disconnect", "stop_on_error", "stop_on_timeout")} == {
        "stop_on_disconnect": False, "stop_on_error": True, "stop_on_timeout": False}
    assert rpc.requests[-1]["reason"] == "disconnect"
    with pytest.raises(ValueError, match="experiment failure"):
        with api_module.GuiClient(path, stop_on_error=True):
            raise ValueError("experiment failure")
    assert rpc.requests[-1]["reason"] == "python_error"


def test_wait_timeout_opt_in_reports_last_receipt_and_requests_stop_once(api_module, server, tmp_path):
    rpc, path, state = policy_server(server, tmp_path)
    with api_module.GuiClient(path, stop_on_timeout=True) as client:
        with pytest.raises(api_module.GuiTimeoutError) as caught:
            client.wait("job", timeout=0)
        assert isinstance(caught.value, TimeoutError)
        assert caught.value.receipt["status"] == "running"
        assert caught.value.code == "timeout"
        assert caught.value.stop_error is None
    assert len([r for r in rpc.requests if r["action"] == "stop"]) == 1


def test_stop_and_wait_handles_stop_completion_race_and_timeout(api_module, server, tmp_path):
    rpc, path, state = policy_server(server, tmp_path)
    with api_module.GuiClient(path, stop_on_timeout=True) as client:
        with pytest.raises(api_module.GuiTimeoutError):
            client.stop_and_wait("job", timeout=0)
        # A bounded confirmation wait does not issue a second timeout Stop.
        assert len([r for r in rpc.requests if r["action"] == "stop"]) == 1
        state["status"] = "stopped"
        assert client.stop_and_wait("job", timeout=0)["status"] == "stopped"
        state["status"] = "completed"
        assert client.stop_and_wait("job", timeout=0)["status"] == "completed"
        assert len([r for r in rpc.requests if r["action"] == "stop"]) == 1
        state.update(status="error", error_code="save_failed")
        with pytest.raises(api_module.GuiSaveError) as caught:
            client.stop_and_wait("job", timeout=0)
        assert caught.value.receipt["rows"] == 2


@pytest.mark.parametrize("code,name", [("transport", "GuiTransportError"),
    ("ownership_lost", "GuiOwnershipError"), ("device_error", "GuiDeviceError"),
    ("protection", "GuiProtectionError"), ("save_failed", "GuiSaveError"),
    ("initialization_failed", "GuiInitializationError"), ("job_failed", "GuiError")])
def test_machine_codes_determine_exception_types(api_module, server, tmp_path, code, name):
    rpc, path, state = policy_server(server, tmp_path)
    state.update(status="error", error_code=code)
    with pytest.raises(getattr(api_module, name)) as caught:
        api_module.GuiClient(path).wait("job", timeout=0)
    assert caught.value.code == code and caught.value.receipt["status"] == "error"


def test_stop_wait_validates_before_any_mutation(api_module, server, tmp_path):
    rpc, path, _ = policy_server(server, tmp_path)
    client = api_module.GuiClient(path)
    with pytest.raises(ValueError):
        client.stop_and_wait("job", timeout=float("nan"))
    with pytest.raises(ValueError):
        client.run(timeout=-1)
    assert not rpc.requests
    with pytest.raises(TypeError):
        api_module.GuiClient(path, stop_on_disconnect="yes")


def test_old_gui_cannot_silently_ignore_opted_in_policy(api_module, server, tmp_path):
    def handler(request):
        return {"ok": True, "result": {"owned": request["action"] != "release",
            "client_id": request["client_id"], "status": "python" if request["action"] != "release" else "idle",
            "active_run": None}}
    rpc = server(handler)
    client = api_module.GuiClient(descriptor(tmp_path / "old.json", rpc.port), stop_on_disconnect=True)
    with pytest.raises(api_module.GuiError) as caught:
        client.start()
    assert caught.value.code == "unsupported_policy"
    assert [r["action"] for r in rpc.requests] == ["acquire", "release"]
    client.close()


def test_failed_timeout_stop_is_exposed_without_masking_timeout(api_module, server, tmp_path, monkeypatch):
    rpc, path, _ = policy_server(server, tmp_path)
    client = api_module.GuiClient(path, stop_on_timeout=True)
    def failed_stop(_):
        raise api_module.GuiTransportError("socket lost", "transport")
    monkeypatch.setattr(client, "stop", failed_stop)
    with pytest.raises(api_module.GuiTimeoutError) as caught:
        client.wait("job", timeout=0)
    assert isinstance(caught.value.stop_error, api_module.GuiTransportError)
    assert caught.value.receipt["status"] == "running"
    assert not [r for r in rpc.requests if r["action"] == "start"]
