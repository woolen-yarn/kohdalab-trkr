"""Exercise the packaged executable's five recording workflows without hardware."""
from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
import os
from pathlib import Path
import subprocess
import tempfile

WORKFLOWS = ("signal_monitor", "trkr", "srkr", "strkr", "srkr_2d")
MAIN_FIELDS = (
    "run_id point_index timestamp_utc elapsed_s quality_flags measurement fast_axis slow_axis "
    "line_index target_elapsed_s target_t_cor_ps target_x_cor_um target_y_cor_um "
    "target_u_cor_um target_v_cor_um t_ps x_um y_um t_cor_ps x_cor_um y_cor_um "
    "u_cor_um v_cor_um spatial_theta_deg X_V Y_V R_V Theta_deg signal_unit sample_power_W"
).split()
RAW_FIELDS = (
    "run_id event_index point_index reading_index event_type device_role quantity command "
    "raw_response value unit host_started_at_utc host_completed_at_utc started_elapsed_s "
    "completed_elapsed_s result error details_json"
).split()


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify_recording(path: Path, workflow: str) -> dict:
    metadata = json.loads(path.with_suffix(".run.json").read_text(encoding="utf-8"))
    raw_path = path.with_suffix(".raw.csv")
    with path.open(newline="", encoding="utf-8") as stream:
        reader = csv.DictReader(stream)
        if reader.fieldnames != MAIN_FIELDS:
            raise ValueError("Analysis CSV schema mismatch")
        rows = list(reader)
    if not rows or metadata["rows"] != len(rows):
        raise ValueError("Missing or mismatched recorded rows")
    if metadata["simulation"] is not True or metadata["status"] != "complete":
        raise ValueError("Recording is not a completed simulation")
    for index, row in enumerate(rows, 1):
        if row["run_id"] != metadata["run_id"] or int(row["point_index"]) != index:
            raise ValueError("Run/point identity mismatch")
        if row["measurement"] != workflow or row["signal_unit"] != "V":
            raise ValueError("Workflow or simulated signal unit mismatch")
        if any(not math.isfinite(float(row[key])) for key in ("X_V", "Y_V", "R_V", "Theta_deg")):
            raise ValueError("Non-finite simulated signal")
    log_path = path.with_suffix(".log")
    log = log_path.read_text(encoding="utf-8")
    if "\tfinished\t" not in log or metadata["run_id"] not in log:
        raise ValueError("Missing or unrelated measurement log")
    hashes = {"csv_sha256": sha(path), "raw_csv_sha256": sha(raw_path), "log_sha256": sha(log_path)}
    if metadata["file_hashes"] != hashes:
        raise ValueError("Recorded file hashes do not match actual CSV bytes")
    with raw_path.open(newline="", encoding="utf-8") as stream:
        reader = csv.DictReader(stream)
        if reader.fieldnames != RAW_FIELDS:
            raise ValueError("Raw CSV schema mismatch")
        raw_rows = list(reader)
    if not raw_rows or any(row["run_id"] != metadata["run_id"] for row in raw_rows):
        raise ValueError("Missing or unrelated raw evidence")
    if not any(row["event_type"] in ("finished", "run_finished") for row in raw_rows):
        raise ValueError("Missing terminal raw evidence")
    return {"workflow": workflow, "rows": len(rows), **hashes}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    binary = args.executable.resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    results = []
    with tempfile.TemporaryDirectory(prefix="trkr-package-demo-") as home:
        environment = dict(os.environ, HOME=home, USERPROFILE=home)
        for workflow in WORKFLOWS:
            path = output / f"{workflow}.csv"
            command = [str(binary), "--demo-csv", str(path), "--workflow", workflow]
            subprocess.run(command, env=environment, check=True, timeout=120, capture_output=True)
            result = verify_recording(path, workflow)
            files = [path, path.with_suffix(".raw.csv"), path.with_suffix(".run.json"), path.with_suffix(".log")]
            before = {file: sha(file) for file in files}
            retry = subprocess.run(command, env=environment, timeout=120, capture_output=True)
            if retry.returncode == 0 or before != {file: sha(file) for file in files}:
                raise ValueError("Existing recording was accepted or modified")
            results.append({**result, "overwrite_refused": True})
    (output / "validation.json").write_text(json.dumps({
        "executable_sha256": sha(binary), "hardware_opened": False,
        "scope": "packaged executable demo recording; not physical-device qualification",
        "results": results,
    }, indent=2) + "\n", encoding="utf-8")
    print(f"PASS: {len(results)} packaged workflows, main/raw provenance and no overwrite")


if __name__ == "__main__":
    main()
