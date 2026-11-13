"""Run the real TRKR GUI through the shared ControlClient in Demo mode."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "bindings" / "control"))
from kohdalab_control import ControlClient, discover
from verify_packaged_measurements import verify_recording


def wait_for_client(directory: Path, process, *, timeout: float = 10.0):
    """Wait for a responding GUI, not merely its early-published descriptor."""
    deadline = time.monotonic() + timeout
    while (remaining := deadline - time.monotonic()) > 0:
        if (code := process.poll()) is not None:
            raise RuntimeError(
                f"GUI exited with code {code} before its API became ready; see gui.log"
            )
        endpoints = discover(
            "kohdalab-trkr", directory=directory, probe_timeout=min(0.25, remaining)
        )
        if len(endpoints) > 1:
            raise RuntimeError("Multiple GUIs in an isolated validation profile")
        if endpoints:
            return ControlClient("kohdalab-trkr", endpoint=endpoints[0])
        time.sleep(min(0.05, max(0.0, deadline - time.monotonic())))
    raise TimeoutError("GUI API did not respond within startup deadline; see gui.log")


def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("executable", type=Path)
    p.add_argument("--output", type=Path, required=True)
    a = p.parse_args()
    a.output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="kohdalab-trkr-control-") as home:
        env = dict(os.environ, HOME=home, USERPROFILE=home, RUST_BACKTRACE="1")
        # Never let a headless run escape Xvfb onto a developer's Wayland session.
        env.pop("WAYLAND_DISPLAY", None)
        # Start explicitly from a private copy of the bundled synthetic profile:
        # no per-user settings are inspected and the payload is never written.
        bundled = a.executable.resolve().parent / "config" / "default_demo.json"
        if not bundled.is_file():
            bundled = ROOT / "crates/trkr-desktop/assets/default_demo.json"
        profile_path = Path(home) / "default_demo.json"
        profile_path.write_bytes(bundled.read_bytes())
        log_path = a.output / "gui.log"
        log = log_path.open("w")
        process = subprocess.Popen(
            [str(a.executable.resolve()), "--gui-api", "--config", str(profile_path)],
            env=env,
            stdout=log,
            stderr=subprocess.STDOUT,
        )
        try:
            directory = Path(home) / ".kohdalab" / "trkr" / "run"
            client = wait_for_client(directory, process)
            profile = json.loads(
                (ROOT / "crates/trkr-desktop/assets/default.json").read_text()
            )
            profile["measurements"]["signal_monitor"]["interval_s"] = 0.0
            client.describe()
            client.acquire()
            client.invoke("configure", profile=profile, source="demo")
            plan = client.invoke("plan")["plan"]
            plan["n_points"] = 3
            plan["interval_s"] = 0.0
            client.invoke("set_plan", plan=plan, workflow="signal_monitor")
            receipt = client.invoke(
                "start",
                workflow="signal_monitor",
                output=str((a.output / "run.csv").resolve()),
            )
            duplicate = client.invoke(
                "start",
                workflow="signal_monitor",
                output=str((a.output / "run.csv").resolve()),
                request_id=receipt["job_id"],
            )
            assert duplicate["job_id"] == receipt["job_id"]
            result = client.wait(receipt["job_id"], timeout=20)
            recording = verify_recording(a.output / "run.csv", "signal_monitor")
            if recording["rows"] != 3 or result["native"].get("rows") != 3:
                raise ValueError("GUI API did not record the requested three points")
            terminal_stop = client.stop(job_id=receipt["job_id"])
            assert terminal_stop["requested"] is False
            assert terminal_stop["job_id"] == receipt["job_id"]
            released = client.release()
            (a.output / "evidence.json").write_text(
                json.dumps(
                    {
                        "describe": True,
                        "status": True,
                        "configure": "demo",
                        "start": receipt,
                        "result": result,
                        "release": released,
                        "recording": recording,
                    },
                    indent=2,
                )
                + "\n"
            )
            print(
                json.dumps(
                    {
                        "ok": True,
                        "job_id": receipt["job_id"],
                        "rows": result["native"].get("rows"),
                    }
                )
            )
        except BaseException:
            log.flush()
            print(f"--- {log_path} (exit {process.poll()}) ---", file=sys.stderr)
            print(log_path.read_text(errors="replace")[-20000:], file=sys.stderr)
            raise
        finally:
            process.terminate()
            process.wait(timeout=10)
            log.close()


if __name__ == "__main__":
    main()
