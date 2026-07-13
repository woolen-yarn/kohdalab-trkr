from __future__ import annotations

import hashlib
import json
from datetime import datetime

import pytest

from kohdalab.api.measurements import write_measurement_rows
from kohdalab.api.run_metadata import RunMetadata, metadata_path, utc_now_iso


def test_utc_now_iso_is_timezone_aware_utc():
    timestamp = utc_now_iso()

    assert timestamp.endswith("Z")
    assert (
        datetime.fromisoformat(timestamp.replace("Z", "+00:00"))
        .utcoffset()
        .total_seconds()
        == 0
    )


def test_run_metadata_redacts_secrets_and_hashes_finished_csv(tmp_path):
    output = tmp_path / "scan.csv"
    output.write_text("value\n1\n", encoding="utf-8")
    metadata = RunMetadata(
        output_path=output,
        measurement="trkr",
        config={"profile": "test", "api_token": "do-not-persist"},
        expected_points=1,
    )

    metadata.write()
    metadata.finish(status="completed", rows_written=1)

    saved = json.loads(metadata_path(output).read_text(encoding="utf-8"))
    expected_digest = hashlib.sha256(output.read_bytes()).hexdigest()
    assert saved["schema_version"] == 1
    assert saved["status"] == "completed"
    assert saved["measurement"] == "trkr"
    assert saved["expected_points"] == saved["rows_written"] == 1
    assert saved["config"]["api_token"] == "<redacted>"
    assert "do-not-persist" not in metadata_path(output).read_text(encoding="utf-8")
    assert saved["output_sha256"] == f"sha256:{expected_digest}"
    assert saved["started_at"].endswith("Z")
    assert saved["finished_at"].endswith("Z")
    assert not list(tmp_path.glob("*.tmp"))


def test_run_metadata_redacts_secrets_nested_in_lists_and_tuples(tmp_path):
    metadata = RunMetadata(
        output_path=tmp_path / "scan.csv",
        measurement="trkr",
        config={
            "devices": [
                {"password": "list-secret"},
                ({"api_key": "tuple-secret"}, "preserved"),
            ]
        },
        expected_points=0,
    )

    assert metadata.data["config"]["devices"] == [
        {"password": "<redacted>"},
        [{"api_key": "<redacted>"}, "preserved"],
    ]


def test_failed_run_without_output_records_error_notes_without_digest(tmp_path):
    metadata = RunMetadata(
        output_path=tmp_path / "missing.csv",
        measurement="trkr",
        config={},
        expected_points=1,
    )
    error = RuntimeError("acquisition failed")
    error.add_note("controller stopped responding")

    metadata.finish(status="failed", rows_written=0, error=error)

    saved = json.loads(metadata.path.read_text(encoding="utf-8"))
    assert saved["output_sha256"] is None
    assert saved["error"] == {
        "type": "RuntimeError",
        "message": "acquisition failed",
        "notes": ["controller stopped responding"],
    }

    unnoted = RunMetadata(
        output_path=tmp_path / "unnoted.csv",
        measurement="trkr",
        config={},
        expected_points=1,
    )
    unnoted.finish(status="failed", rows_written=0, error=ValueError("invalid"))
    assert "notes" not in unnoted.data["error"]


def test_write_measurement_rows_keeps_csv_and_metadata_hash_in_sync(tmp_path):
    output = tmp_path / "manual.csv"

    write_measurement_rows(
        [
            {"measurement": "custom", "value": 1},
            {"measurement": "custom", "extra": "x"},
        ],
        output=output,
        config={"profile": "manual"},
        measurement_name="custom",
    )

    saved = json.loads(metadata_path(output).read_text(encoding="utf-8"))
    expected_digest = hashlib.sha256(output.read_bytes()).hexdigest()
    assert saved["status"] == "completed"
    assert saved["rows_written"] == 2
    assert saved["output_sha256"] == f"sha256:{expected_digest}"
    assert (
        output.read_text(encoding="utf-8").splitlines()[0] == "measurement,value,extra"
    )


def test_manual_export_refuses_implicit_overwrite_and_preserves_existing_pair(tmp_path):
    output = tmp_path / "manual.csv"
    sidecar = metadata_path(output)
    output.write_text("original csv\n", encoding="utf-8")
    sidecar.write_text('{"original": true}\n', encoding="utf-8")

    with pytest.raises(FileExistsError, match="output already exists"):
        write_measurement_rows(
            [{"measurement": "custom", "value": 2}],
            output=output,
            config={"profile": "manual"},
            measurement_name="custom",
        )

    assert output.read_text(encoding="utf-8") == "original csv\n"
    assert sidecar.read_text(encoding="utf-8") == '{"original": true}\n'


def test_explicit_manual_overwrite_atomically_replaces_csv_and_metadata(tmp_path):
    output = tmp_path / "manual.csv"
    sidecar = metadata_path(output)
    output.write_text("original csv\n", encoding="utf-8")
    sidecar.write_text('{"original": true}\n', encoding="utf-8")

    write_measurement_rows(
        [{"measurement": "custom", "value": 2}],
        output=output,
        config={"profile": "manual"},
        measurement_name="custom",
        overwrite=True,
    )

    saved = json.loads(sidecar.read_text(encoding="utf-8"))
    assert output.read_text(encoding="utf-8").splitlines() == [
        "measurement,value",
        "custom,2",
    ]
    assert saved["status"] == "completed"
    assert saved["rows_written"] == 1
    assert (
        saved["output_sha256"]
        == f"sha256:{hashlib.sha256(output.read_bytes()).hexdigest()}"
    )
    assert not list(tmp_path.glob("*.tmp"))


def test_manual_export_removes_new_csv_when_metadata_finalization_fails(
    monkeypatch, tmp_path
):
    output = tmp_path / "manual.csv"

    def fail_finish(self, **_kwargs):
        raise OSError("metadata unavailable")

    monkeypatch.setattr(RunMetadata, "finish", fail_finish)

    with pytest.raises(OSError, match="metadata unavailable"):
        write_measurement_rows(
            [{"measurement": "custom", "value": 1}],
            output=output,
            config={"profile": "manual"},
            measurement_name="custom",
        )

    assert not output.exists()
    assert not metadata_path(output).exists()
    assert not list(tmp_path.glob(".*.tmp"))


def test_manual_overwrite_restores_original_pair_when_metadata_finalization_fails(
    monkeypatch, tmp_path
):
    output = tmp_path / "manual.csv"
    sidecar = metadata_path(output)
    output.write_text("original csv\n", encoding="utf-8")
    sidecar.write_text('{"original": true}\n', encoding="utf-8")

    def fail_finish(self, **_kwargs):
        raise OSError("metadata unavailable")

    monkeypatch.setattr(RunMetadata, "finish", fail_finish)

    with pytest.raises(OSError, match="metadata unavailable"):
        write_measurement_rows(
            [{"measurement": "custom", "value": 2}],
            output=output,
            config={"profile": "manual"},
            measurement_name="custom",
            overwrite=True,
        )

    assert output.read_text(encoding="utf-8") == "original csv\n"
    assert sidecar.read_text(encoding="utf-8") == '{"original": true}\n'
    assert not list(tmp_path.glob(".*.bak"))


@pytest.mark.parametrize("expected_points", [-1, 1.5, True])
def test_run_metadata_rejects_invalid_expected_point_counts(tmp_path, expected_points):
    with pytest.raises(ValueError, match="expected_points"):
        RunMetadata(
            output_path=tmp_path / "scan.csv",
            measurement="trkr",
            config={},
            expected_points=expected_points,
        )


def test_run_metadata_enforces_terminal_state_invariants(tmp_path):
    metadata = RunMetadata(
        output_path=tmp_path / "scan.csv",
        measurement="trkr",
        config={},
        expected_points=2,
    )

    with pytest.raises(ValueError, match="Unsupported run status"):
        metadata.finish(status="unknown", rows_written=0)
    with pytest.raises(ValueError, match="between zero and expected_points"):
        metadata.finish(status="failed", rows_written=3)
    with pytest.raises(ValueError, match="exactly expected_points"):
        metadata.finish(status="completed", rows_written=1)


def test_initial_metadata_write_refuses_concurrent_overwrite(tmp_path):
    output = tmp_path / "scan.csv"
    sidecar = metadata_path(output)
    sidecar.write_text('{"owner": "other-run"}\n', encoding="utf-8")
    metadata = RunMetadata(
        output_path=output,
        measurement="trkr",
        config={},
        expected_points=1,
    )

    with pytest.raises(FileExistsError):
        metadata.write()

    assert sidecar.read_text(encoding="utf-8") == '{"owner": "other-run"}\n'
