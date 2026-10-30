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
from kohdalab_control import ControlClient


def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("executable", type=Path)
    p.add_argument("--output", type=Path, required=True)
    a = p.parse_args()
    a.output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="kohdalab-trkr-control-") as home:
        env = dict(os.environ, HOME=home, USERPROFILE=home)
        log = (a.output / "gui.log").open("w")
        process = subprocess.Popen(
            [str(a.executable.resolve()), "--gui-api"],
            env=env,
            stdout=log,
            stderr=subprocess.STDOUT,
        )
        try:
            directory = Path(home) / ".kohdalab" / "trkr" / "run"
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline and not list(directory.glob("*.json")):
                time.sleep(0.05)
            client = ControlClient("kohdalab-trkr", directory=directory)
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
        finally:
            process.terminate()
            process.wait(timeout=10)
            log.close()


if __name__ == "__main__":
    main()
