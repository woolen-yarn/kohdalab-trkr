from __future__ import annotations

import json
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from types import SimpleNamespace

import pytest

from kohdalab.api import measurements
from kohdalab.api.models import MeasurementPoint, Position
from kohdalab.api.scan_plan import (
    signal_monitor_plan,
    srkr_2d_plan,
    srkr_plan,
    strkr_plan,
    trkr_plan,
)
from kohdalab.api.status import (
    STATUS_MOVING_DELAY_STAGE,
    STATUS_READING_LOCKIN,
    STATUS_RUNNING,
    STATUS_STOPPED,
    STATUS_WAITING,
    moving_scanner_status,
)


SIGNAL = {"X": 1.0, "Y": 2.0, "R": 3.0, "Theta": 4.0}


@dataclass
class FakeSession:
    config: dict

    def __post_init__(self):
        self.moves: list[tuple[str, float, str]] = []
        self.scanner_hysteresis_flags: list[bool] = []
        self.disconnected = False
        self.position = Position(
            t_ps=0.0,
            delay_stage_mm=1.0,
            delay_stage_pulse=100,
            x_um=0.0,
            y_um=0.0,
            scanner_x_value=6.0,
            scanner_x_unit="mm",
            scanner_y_value=7.0,
            scanner_y_unit="mm",
        )

    def read_lockin_signal(self):
        return SIGNAL

    def move_delay_stage(
        self, value: float, *, coordinate: str = "measurement", on_status=None
    ):
        if on_status is not None:
            on_status(STATUS_MOVING_DELAY_STAGE)
        self.moves.append(("t", value, coordinate))
        self.position.t_ps = value
        self.position.delay_stage_mm = value / 10.0
        self.position.delay_stage_pulse = int(value * 100)
        return self.position

    def move_scanner(
        self,
        axis: str,
        value: float,
        *,
        coordinate: str = "measurement",
        apply_software_hysteresis: bool = True,
        on_status=None,
    ):
        if on_status is not None:
            on_status(moving_scanner_status(axis))
        self.moves.append((axis, value, coordinate))
        self.scanner_hysteresis_flags.append(bool(apply_software_hysteresis))
        if axis == "x":
            self.position.x_um = value
            self.position.scanner_x_value = value / 100.0
        else:
            self.position.y_um = value
            self.position.scanner_y_value = value / 100.0
        return self.position

    def read_position(self):
        return self.position

    def disconnect_all(self):
        self.disconnected = True


def base_config(tmp_path):
    return {
        "instruments": {
            "lockin": {"main": {"model": "SR7265", "resource": "fake"}},
            "delay_stage": {
                "t": {
                    "controller": "SHOT302GS",
                    "stage": "SGSP46-500",
                    "port": "fake",
                    "direction": 1,
                }
            },
            "scanner": {
                "x": {
                    "controller": "CONEXAGAP",
                    "actuator": "AG-M100D",
                    "port": "fake",
                    "axis": "U",
                    "sample_um_per_unit": 100.0,
                },
                "y": {
                    "controller": "CONEXAGAP",
                    "actuator": "AG-M100D",
                    "port": "fake",
                    "axis": "V",
                    "sample_um_per_unit": 100.0,
                },
            },
        },
        "measurements": {
            "move_abs": {
                "zero": {
                    "t_ps": 10.0,
                    "x_um": 1.0,
                    "y_um": 2.0,
                }
            },
            "signal_monitor": {
                "output": {
                    "dir": str(tmp_path),
                    "filename": "signal",
                    "auto_timestamp_suffix": False,
                }
            },
            "trkr": {
                "coordinate": "measurement",
                "wait_s": 0.0,
                "return_to_zero": True,
                "output": {
                    "dir": str(tmp_path),
                    "filename": "trkr",
                    "auto_timestamp_suffix": False,
                },
            },
            "srkr": {
                "coordinate": "measurement",
                "wait_s": 0.0,
                "return_to_zero": True,
                "output": {
                    "dir": str(tmp_path),
                    "filename": "srkr",
                    "auto_timestamp_suffix": False,
                },
                "scan": {"axis": "x", "min": 0.0, "max": 1.0, "step": 1.0},
            },
            "strkr": {
                "wait_s": 0.0,
                "return_to_zero": {"fast_axis": True, "slow_axis": True},
                "output": {
                    "dir": str(tmp_path),
                    "filename": "strkr",
                    "auto_timestamp_suffix": False,
                },
            },
            "srkr_2d": {
                "wait_s": 0.0,
                "return_to_zero": {"fast_axis": True, "slow_axis": True},
                "output": {
                    "dir": str(tmp_path),
                    "filename": "srkr_2d",
                    "auto_timestamp_suffix": False,
                },
            },
        },
    }


def test_signal_monitor_row_shape(monkeypatch, tmp_path):
    monkeypatch.setattr(measurements, "DeviceSession", FakeSession)

    rows = measurements.run_signal_monitor(
        base_config(tmp_path), interval_s=0.0, n_points=1
    )

    assert len(rows) == 1
    assert {
        "timestamp",
        "measurement",
        "fast_axis",
        "target_elapsed_s",
        "elapsed_s",
        "X_V",
        "Y_V",
        "R_V",
        "Theta_deg",
    } <= rows[0].keys()
    assert "actual_elapsed_s" not in rows[0]
    assert rows[0]["measurement"] == "signal_monitor"
    assert rows[0]["fast_axis"] == "elapsed_s"
    assert rows[0]["target_elapsed_s"] == rows[0]["elapsed_s"]
    assert rows[0]["X_V"] == 1.0
    assert rows[0]["timestamp"].endswith("Z")
    assert (
        datetime.fromisoformat(rows[0]["timestamp"].replace("Z", "+00:00"))
        .utcoffset()
        .total_seconds()
        == 0
    )

    metadata = json.loads(
        (tmp_path / "signal.csv.meta.json").read_text(encoding="utf-8")
    )
    assert metadata["measurement"] == "signal_monitor"
    assert metadata["status"] == "completed"
    assert metadata["expected_points"] == metadata["rows_written"] == 1
    assert metadata["output_sha256"].startswith("sha256:")


def test_signal_monitor_uses_provided_session_without_disconnect(tmp_path):
    session = FakeSession(base_config(tmp_path))

    rows = measurements.run_signal_monitor(
        base_config(tmp_path),
        interval_s=0.0,
        n_points=1,
        session=session,
    )

    assert len(rows) == 1
    assert not session.disconnected


def test_stopped_measurement_records_incomplete_metadata(tmp_path):
    session = FakeSession(base_config(tmp_path))

    rows = measurements.run_signal_monitor(
        base_config(tmp_path),
        interval_s=0.0,
        n_points=2,
        should_continue=lambda: False,
        session=session,
    )

    metadata = json.loads(
        (tmp_path / "signal.csv.meta.json").read_text(encoding="utf-8")
    )
    assert rows == []
    assert metadata["status"] == "stopped"
    assert metadata["expected_points"] == 2
    assert metadata["rows_written"] == 0


def test_failed_row_serialization_is_not_counted_as_written(monkeypatch, tmp_path):
    session = FakeSession(base_config(tmp_path))

    def fail_output_row(_row):
        raise OSError("CSV conversion failed")

    monkeypatch.setattr(measurements, "output_row", fail_output_row)

    with pytest.raises(OSError, match="CSV conversion failed"):
        measurements.run_signal_monitor(
            base_config(tmp_path),
            interval_s=0.0,
            n_points=1,
            session=session,
        )

    metadata = json.loads(
        (tmp_path / "signal.csv.meta.json").read_text(encoding="utf-8")
    )
    assert metadata["status"] == "failed"
    assert metadata["rows_written"] == 0
    assert metadata["error"]["type"] == "OSError"
    assert (tmp_path / "signal.csv").read_text(encoding="utf-8").count("\n") == 1


def test_callback_failure_closes_owned_iterator_and_preserves_primary_error(
    monkeypatch, tmp_path
):
    class FailingDisconnectSession(FakeSession):
        def disconnect_all(self):
            self.disconnected = True
            raise OSError("disconnect failed")

    session = FailingDisconnectSession(base_config(tmp_path))
    monkeypatch.setattr(measurements, "DeviceSession", lambda _config: session)

    def fail_callback(_point):
        raise ValueError("callback failed")

    with pytest.raises(ValueError, match="callback failed") as caught:
        measurements.run_signal_monitor(
            base_config(tmp_path),
            interval_s=0.0,
            n_points=2,
            on_point=fail_callback,
        )

    assert session.disconnected
    assert any("disconnect failed" in note for note in caught.value.__notes__)
    metadata = json.loads(
        (tmp_path / "signal.csv.meta.json").read_text(encoding="utf-8")
    )
    assert metadata["status"] == "failed"
    assert metadata["rows_written"] == 1
    assert metadata["error"]["type"] == "ValueError"
    assert any("disconnect failed" in note for note in metadata["error"]["notes"])


def test_initial_metadata_failure_notes_owned_session_cleanup_failure(
    monkeypatch, tmp_path
):
    class FailingDisconnectSession(FakeSession):
        def disconnect_all(self):
            raise OSError("disconnect unavailable")

    session = FailingDisconnectSession(base_config(tmp_path))
    monkeypatch.setattr(measurements, "DeviceSession", lambda _config: session)
    monkeypatch.setattr(
        measurements.RunMetadata,
        "write",
        lambda _self: (_ for _ in ()).throw(ValueError("metadata invalid")),
    )

    with pytest.raises(ValueError, match="metadata invalid") as caught:
        measurements.run_signal_monitor(
            base_config(tmp_path),
            interval_s=0.0,
            n_points=1,
            output=tmp_path / "double-failure.csv",
        )

    assert any("disconnect unavailable" in note for note in caught.value.__notes__)


def test_keyboard_interrupt_from_callback_marks_run_interrupted_and_cleans_up(
    monkeypatch, tmp_path
):
    session = FakeSession(base_config(tmp_path))
    monkeypatch.setattr(measurements, "DeviceSession", lambda _config: session)

    def interrupt(_point):
        raise KeyboardInterrupt("operator stopped acquisition")

    with pytest.raises(KeyboardInterrupt, match="operator stopped acquisition"):
        measurements.run_signal_monitor(
            base_config(tmp_path),
            interval_s=0.0,
            n_points=2,
            on_point=interrupt,
        )

    assert session.disconnected
    metadata = json.loads(
        (tmp_path / "signal.csv.meta.json").read_text(encoding="utf-8")
    )
    assert metadata["status"] == "interrupted"
    assert metadata["rows_written"] == 1
    assert metadata["error"]["type"] == "KeyboardInterrupt"


def test_callback_failure_does_not_close_borrowed_session(tmp_path):
    session = FakeSession(base_config(tmp_path))

    def fail_callback(_point):
        raise RuntimeError("consumer unavailable")

    with pytest.raises(RuntimeError, match="consumer unavailable"):
        measurements.run_signal_monitor(
            base_config(tmp_path),
            interval_s=0.0,
            n_points=1,
            on_point=fail_callback,
            session=session,
        )

    assert not session.disconnected
    metadata = json.loads(
        (tmp_path / "signal.csv.meta.json").read_text(encoding="utf-8")
    )
    assert metadata["status"] == "failed"
    assert metadata["rows_written"] == 1


def test_measurement_refuses_to_overwrite_existing_csv_before_device_io(tmp_path):
    output = tmp_path / "existing.csv"
    output.write_text("important existing data\n", encoding="utf-8")
    session = FakeSession(base_config(tmp_path))
    session.read_lockin_signal = lambda: pytest.fail(
        "device I/O occurred before output collision check"
    )

    with pytest.raises(FileExistsError, match="existing.csv"):
        measurements.run_signal_monitor(
            base_config(tmp_path),
            interval_s=0.0,
            n_points=1,
            output=output,
            session=session,
        )

    assert output.read_text(encoding="utf-8") == "important existing data\n"
    assert not (tmp_path / "existing.csv.meta.json").exists()


def test_measurement_refuses_orphan_metadata_before_device_io(tmp_path):
    output = tmp_path / "new.csv"
    sidecar = tmp_path / "new.csv.meta.json"
    sidecar.write_text('{"status": "running"}\n', encoding="utf-8")
    session = FakeSession(base_config(tmp_path))
    session.read_lockin_signal = lambda: pytest.fail(
        "device I/O occurred before metadata collision check"
    )

    with pytest.raises(FileExistsError, match="metadata already exists"):
        measurements.run_signal_monitor(
            base_config(tmp_path),
            interval_s=0.0,
            n_points=1,
            output=output,
            session=session,
        )

    assert not output.exists()
    assert sidecar.read_text(encoding="utf-8") == '{"status": "running"}\n'


def test_signal_monitor_accepts_scan_plan(tmp_path):
    session = FakeSession(base_config(tmp_path))
    plan = signal_monitor_plan(interval_s=0.0, n_points=2)

    rows = measurements.run_signal_monitor(
        base_config(tmp_path), plan=plan, session=session
    )

    assert len(rows) == 2
    assert [row["target_elapsed_s"] for row in rows] == [0.0, 0.0]
    assert not session.disconnected


def test_signal_monitor_explicit_arguments_override_plan_defaults(tmp_path):
    session = FakeSession(base_config(tmp_path))
    plan = signal_monitor_plan(interval_s=100.0, n_points=3)

    rows = measurements.run_signal_monitor(
        base_config(tmp_path),
        plan=plan,
        interval_s=0.0,
        n_points=1,
        session=session,
    )

    assert len(rows) == 1
    assert rows[0]["target_elapsed_s"] == 0.0


def test_initial_metadata_write_failure_removes_new_csv(monkeypatch, tmp_path):
    output = tmp_path / "metadata-failure.csv"
    session = FakeSession(base_config(tmp_path))

    def fail_write(_self):
        raise OSError("metadata directory unavailable")

    monkeypatch.setattr(measurements.RunMetadata, "write", fail_write)

    with pytest.raises(OSError, match="metadata directory unavailable"):
        measurements.run_signal_monitor(
            base_config(tmp_path),
            interval_s=0.0,
            n_points=1,
            output=output,
            session=session,
        )

    assert not output.exists()
    assert not measurements.metadata_path(output).exists()


def test_initial_metadata_write_failure_disconnects_owned_session(
    monkeypatch, tmp_path
):
    output = tmp_path / "owned-metadata-failure.csv"
    session = FakeSession(base_config(tmp_path))
    monkeypatch.setattr(measurements, "DeviceSession", lambda _config: session)
    monkeypatch.setattr(
        measurements.RunMetadata,
        "write",
        lambda _self: (_ for _ in ()).throw(OSError("metadata unavailable")),
    )

    with pytest.raises(OSError, match="metadata unavailable"):
        measurements.run_signal_monitor(
            base_config(tmp_path), interval_s=0.0, n_points=1, output=output
        )

    assert session.disconnected


def test_iterator_cleanup_failure_is_terminal_when_acquisition_succeeds():
    class CloseFailingIterator:
        def __iter__(self):
            return self

        def __next__(self):
            raise StopIteration

        def close(self):
            raise OSError("iterator close failed")

    with pytest.raises(OSError, match="iterator close failed"):
        measurements._write_rows(
            [],
            output=None,
            point_iter=CloseFailingIterator(),
            on_point=None,
            config={},
            measurement_name="test",
            expected_points=0,
        )


def test_callback_error_keeps_primary_exception_and_notes_iterator_cleanup():
    class CloseFailingIterator:
        yielded = False

        def __iter__(self):
            return self

        def __next__(self):
            if self.yielded:
                raise StopIteration
            self.yielded = True
            return MeasurementPoint(index=1, total_points=1, row={"value": 1})

        def close(self):
            raise OSError("iterator close failed")

    def fail_callback(_point):
        raise ValueError("callback failed")

    with pytest.raises(ValueError, match="callback failed") as caught:
        measurements._write_rows(
            [],
            output=None,
            point_iter=CloseFailingIterator(),
            on_point=fail_callback,
            config={},
            measurement_name="test",
            expected_points=1,
        )

    assert caught.value.__notes__ == [
        "Measurement iterator cleanup also failed: iterator close failed"
    ]


def test_scan_preflight_rejects_unsafe_target_before_session_creation(
    monkeypatch, tmp_path
):
    def fail_if_session_is_created(_config):
        pytest.fail("DeviceSession was created before scan preflight completed")

    monkeypatch.setattr(measurements, "DeviceSession", fail_if_session_is_created)

    with pytest.raises(ValueError, match="outside"):
        measurements.run_trkr(
            base_config(tmp_path),
            scan_points=[1_000_000.0],
            wait_s=0.0,
            return_to_zero=False,
        )


def test_trkr_row_shape_and_return_to_zero(monkeypatch, tmp_path):
    sessions: list[FakeSession] = []

    def make_session(config):
        session = FakeSession(config)
        sessions.append(session)
        return session

    monkeypatch.setattr(measurements, "DeviceSession", make_session)

    rows = measurements.run_trkr(
        base_config(tmp_path), scan_points=[10.0, 20.0], wait_s=0.0
    )

    assert [row["target_t_cor_ps"] for row in rows] == [10.0, 20.0]
    assert rows[1]["t_cor_ps"] == 10.0
    assert {
        "fast_axis",
        "delay_stage_mm",
        "delay_stage_pulse",
        "X_V",
        "Theta_deg",
    } <= rows[0].keys()
    assert rows[0]["measurement"] == "trkr"
    assert rows[0]["fast_axis"] == "t"
    assert rows[0]["elapsed_s"] is None
    assert sessions[0].moves == [
        ("t", 10.0, "measurement"),
        ("t", 20.0, "measurement"),
        ("t", 10.0, "measurement"),
    ]
    assert sessions[0].disconnected


def test_trkr_target_points_are_corrected_display_values(tmp_path):
    session = FakeSession(base_config(tmp_path))

    rows = measurements.run_trkr(
        base_config(tmp_path),
        scan_points=[-40.0, -30.0],
        target_points=[-50.0, -40.0],
        wait_s=0.0,
        session=session,
    )

    assert [row["target_t_cor_ps"] for row in rows] == [-50.0, -40.0]
    assert [move[1] for move in session.moves[:2]] == [-40.0, -30.0]


def test_trkr_accepts_scan_plan(tmp_path):
    session = FakeSession(base_config(tmp_path))
    plan = trkr_plan(
        minimum_ps=-50.0,
        maximum_ps=-40.0,
        step_ps=10.0,
        t_zero_ps=10.0,
        coordinate="measurement",
    )

    rows = measurements.run_trkr(
        base_config(tmp_path), plan=plan, wait_s=0.0, session=session
    )

    assert [row["target_t_cor_ps"] for row in rows] == [-50.0, -40.0]
    assert [move[1] for move in session.moves[:2]] == [-40.0, -30.0]


def test_trkr_uses_provided_session_without_disconnect(tmp_path):
    session = FakeSession(base_config(tmp_path))

    rows = measurements.run_trkr(
        base_config(tmp_path), scan_points=[10.0], wait_s=0.0, session=session
    )

    assert rows[0]["target_t_cor_ps"] == 10.0
    assert session.moves[-1] == ("t", 10.0, "measurement")
    assert not session.disconnected


def test_trkr_normalizes_coordinate_aliases(tmp_path):
    session = FakeSession(base_config(tmp_path))

    rows = measurements.run_trkr(
        base_config(tmp_path),
        scan_points=[1.0],
        coordinate="control",
        wait_s=0.0,
        return_to_zero=False,
        session=session,
    )

    assert rows[0]["coordinate"] == "interface"
    assert session.moves == [("t", 1.0, "interface")]


def test_srkr_row_shape_and_return_to_zero(monkeypatch, tmp_path):
    sessions: list[FakeSession] = []

    def make_session(config):
        session = FakeSession(config)
        sessions.append(session)
        return session

    monkeypatch.setattr(measurements, "DeviceSession", make_session)

    rows = measurements.run_srkr(
        base_config(tmp_path), axis="x", scan_points=[1.0, 2.0], wait_s=0.0
    )

    assert [row["target_x_cor_um"] for row in rows] == [1.0, 2.0]
    assert rows[0]["x_cor_um"] == 0.0
    assert {
        "fast_axis",
        "x_um",
        "x_cor_um",
        "x_scanner_mm",
        "X_V",
        "Theta_deg",
    } <= rows[0].keys()
    assert rows[0]["fast_axis"] == "x"
    assert rows[0]["y_um"] is None
    assert rows[0]["y_cor_um"] is None
    assert rows[0]["y_scanner_mm"] is None
    assert rows[0]["elapsed_s"] is None
    assert sessions[0].moves == [
        ("x", 1.0, "measurement"),
        ("x", 2.0, "measurement"),
        ("x", 1.0, "measurement"),
    ]
    assert sessions[0].scanner_hysteresis_flags == [True, False, True]
    assert sessions[0].disconnected


def test_srkr_target_points_are_corrected_display_values(tmp_path):
    session = FakeSession(base_config(tmp_path))

    rows = measurements.run_srkr(
        base_config(tmp_path),
        axis="x",
        scan_points=[51.0],
        target_points=[50.0],
        wait_s=0.0,
        session=session,
    )

    assert rows[0]["target_x_cor_um"] == 50.0
    assert session.moves[0] == ("x", 51.0, "measurement")


def test_srkr_accepts_scan_plan(tmp_path):
    session = FakeSession(base_config(tmp_path))
    plan = srkr_plan(
        axis="x",
        minimum_um=50.0,
        maximum_um=60.0,
        step_um=10.0,
        zero_by_axis={"x": 1.0, "y": 2.0},
        coordinate="measurement",
    )

    rows = measurements.run_srkr(
        base_config(tmp_path), plan=plan, wait_s=0.0, session=session
    )

    assert [row["target_x_cor_um"] for row in rows] == [50.0, 60.0]
    assert [move[1] for move in session.moves[:2]] == [51.0, 61.0]
    assert session.scanner_hysteresis_flags[:2] == [True, False]


def test_srkr_uses_provided_session_without_disconnect(tmp_path):
    session = FakeSession(base_config(tmp_path))

    rows = measurements.run_srkr(
        base_config(tmp_path), axis="x", scan_points=[1.0], wait_s=0.0, session=session
    )

    assert rows[0]["target_x_cor_um"] == 1.0
    assert session.moves[-1] == ("x", 1.0, "measurement")
    assert not session.disconnected


def test_srkr_normalizes_coordinate_aliases(tmp_path):
    session = FakeSession(base_config(tmp_path))

    rows = measurements.run_srkr(
        base_config(tmp_path),
        axis="x",
        scan_points=[0.5],
        coordinate="device",
        wait_s=0.0,
        return_to_zero=False,
        session=session,
    )

    assert rows[0]["coordinate"] == "interface"
    assert session.moves == [("x", 0.5, "interface")]


def test_strkr_runs_fast_slow_scan_and_returns_moved_axes(tmp_path):
    session = FakeSession(base_config(tmp_path))
    plan = strkr_plan(
        fast_axis="t",
        slow_axis="x",
        ranges={
            "t": {"min": 0.0, "max": 10.0, "step": 10.0},
            "x": {"min": 0.0, "max": 1.0, "step": 1.0},
            "y": {"min": 0.0, "max": 1.0, "step": 1.0},
        },
        zero_by_axis={"t_ps": 10.0, "x_um": 1.0, "y_um": 2.0},
        return_to_zero={"fast_axis": True, "slow_axis": True},
    )

    rows = measurements.run_strkr(
        base_config(tmp_path), plan=plan, wait_s=0.0, session=session
    )

    assert len(rows) == 4
    assert rows[0]["measurement"] == "strkr"
    assert rows[0]["fast_axis"] == "t"
    assert rows[0]["slow_axis"] == "x"
    assert [row["target_t_cor_ps"] for row in rows] == [0.0, 10.0, 0.0, 10.0]
    assert [row["target_x_cor_um"] for row in rows] == [0.0, 0.0, 1.0, 1.0]
    assert session.moves == [
        ("x", 1.0, "measurement"),
        ("t", 10.0, "measurement"),
        ("t", 20.0, "measurement"),
        ("t", 10.0, "measurement"),
        ("x", 2.0, "measurement"),
        ("t", 10.0, "measurement"),
        ("t", 20.0, "measurement"),
        ("t", 10.0, "measurement"),
        ("x", 1.0, "measurement"),
    ]
    assert session.scanner_hysteresis_flags == [True, False, True]


def test_srkr_2d_runs_xy_scan_without_touching_t(tmp_path):
    session = FakeSession(base_config(tmp_path))
    statuses: list[str] = []
    points = []
    plan = srkr_2d_plan(
        fast_axis="x",
        slow_axis="y",
        ranges={
            "x": {"min": 0.0, "max": 1.0, "step": 1.0},
            "y": {"min": 0.0, "max": 1.0, "step": 1.0},
        },
        zero_by_axis={"t_ps": 10.0, "x_um": 1.0, "y_um": 2.0},
        return_to_zero={"fast_axis": False, "slow_axis": True},
    )

    rows = measurements.run_srkr_2d(
        base_config(tmp_path),
        plan=plan,
        wait_s=0.0,
        on_status=statuses.append,
        on_point=points.append,
        session=session,
    )

    assert len(rows) == 4
    assert rows[0]["measurement"] == "srkr_2d"
    assert rows[0]["fast_axis"] == "x"
    assert rows[0]["slow_axis"] == "y"
    assert [move[0] for move in session.moves] == [
        "y",
        "x",
        "x",
        "x",
        "y",
        "x",
        "x",
        "y",
    ]
    assert session.scanner_hysteresis_flags == [
        True,
        True,
        False,
        False,
        False,
        False,
        False,
        True,
    ]
    assert ("t", 10.0, "measurement") not in session.moves
    assert statuses[0] == STATUS_RUNNING
    assert statuses[-1] == STATUS_STOPPED
    assert statuses.count(STATUS_READING_LOCKIN) == 4
    assert [point.index for point in points] == [1, 2, 3, 4]
    assert [point.row for point in points] == rows


def test_srkr_2d_read_failure_records_partial_metadata_and_skips_zero_return(
    monkeypatch, tmp_path
):
    class FailingSecondReadSession(FakeSession):
        def __post_init__(self):
            super().__post_init__()
            self.read_count = 0

        def read_lockin_signal(self):
            self.read_count += 1
            if self.read_count == 2:
                raise OSError("lock-in read failed")
            return SIGNAL

    session = FailingSecondReadSession(base_config(tmp_path))
    monkeypatch.setattr(measurements, "DeviceSession", lambda _config: session)
    plan = srkr_2d_plan(
        fast_axis="x",
        slow_axis="y",
        ranges={
            "x": {"min": 0.0, "max": 1.0, "step": 1.0},
            "y": {"min": 0.0, "max": 0.0, "step": 1.0},
        },
        zero_by_axis={"t_ps": 10.0, "x_um": 1.0, "y_um": 2.0},
        return_to_zero={"fast_axis": True, "slow_axis": True},
    )
    statuses: list[str] = []
    points = []

    with pytest.raises(OSError, match="lock-in read failed"):
        measurements.run_srkr_2d(
            base_config(tmp_path),
            plan=plan,
            wait_s=0.0,
            on_status=statuses.append,
            on_point=points.append,
        )

    assert session.moves == [
        ("y", 2.0, "measurement"),
        ("x", 1.0, "measurement"),
        ("x", 2.0, "measurement"),
    ]
    assert [point.index for point in points] == [1]
    assert statuses[0] == STATUS_RUNNING
    assert statuses.count(STATUS_READING_LOCKIN) == 2
    assert STATUS_STOPPED not in statuses
    assert session.disconnected
    metadata = json.loads(
        (tmp_path / "srkr_2d.csv.meta.json").read_text(encoding="utf-8")
    )
    assert metadata["status"] == "failed"
    assert metadata["expected_points"] == 2
    assert metadata["rows_written"] == 1
    assert metadata["error"]["type"] == "OSError"


@pytest.mark.parametrize("measurement_name", ["signal", "trkr", "srkr", "strkr"])
def test_measurement_read_failure_skips_return_and_records_owned_cleanup(
    monkeypatch, tmp_path, measurement_name: str
):
    class FailingReadSession(FakeSession):
        def read_lockin_signal(self):
            raise OSError(f"{measurement_name} lock-in read failed")

    config = base_config(tmp_path)
    session = FailingReadSession(config)
    monkeypatch.setattr(measurements, "DeviceSession", lambda _config: session)
    statuses: list[str] = []

    if measurement_name == "signal":

        def run():
            return measurements.run_signal_monitor(
                config, interval_s=0.0, n_points=1, on_status=statuses.append
            )

        expected_moves = []
        metadata_name = "signal"
    elif measurement_name == "trkr":

        def run():
            return measurements.run_trkr(
                config,
                scan_points=[20.0],
                wait_s=0.0,
                return_to_zero=True,
                on_status=statuses.append,
            )

        expected_moves = [("t", 20.0, "measurement")]
        metadata_name = "trkr"
    elif measurement_name == "srkr":

        def run():
            return measurements.run_srkr(
                config,
                axis="x",
                scan_points=[5.0],
                wait_s=0.0,
                return_to_zero=True,
                on_status=statuses.append,
            )

        expected_moves = [("x", 5.0, "measurement")]
        metadata_name = "srkr"
    else:
        plan = strkr_plan(
            fast_axis="t",
            slow_axis="x",
            ranges={
                "t": {"min": 0.0, "max": 0.0, "step": 1.0},
                "x": {"min": 0.0, "max": 0.0, "step": 1.0},
            },
            zero_by_axis={"t_ps": 10.0, "x_um": 1.0, "y_um": 2.0},
            return_to_zero={"fast_axis": True, "slow_axis": True},
        )

        def run():
            return measurements.run_strkr(
                config, plan=plan, wait_s=0.0, on_status=statuses.append
            )

        expected_moves = [
            ("x", 1.0, "measurement"),
            ("t", 10.0, "measurement"),
        ]
        metadata_name = "strkr"

    with pytest.raises(OSError, match=rf"{measurement_name} lock-in read failed"):
        run()

    assert session.moves == expected_moves
    assert session.disconnected
    assert statuses[0] == STATUS_RUNNING
    assert STATUS_STOPPED not in statuses
    metadata = json.loads(
        (tmp_path / f"{metadata_name}.csv.meta.json").read_text(encoding="utf-8")
    )
    assert metadata["status"] == "failed"
    assert metadata["expected_points"] == 1
    assert metadata["rows_written"] == 0
    assert metadata["error"]["type"] == "OSError"


@pytest.mark.parametrize(
    ("n_points", "message"),
    [(True, "must be an integer"), (0, "must be between")],
)
def test_signal_monitor_rejects_invalid_point_count_before_session_creation(
    monkeypatch, tmp_path, n_points, message: str
):
    monkeypatch.setattr(
        measurements,
        "DeviceSession",
        lambda _config: pytest.fail("invalid point count reached session creation"),
    )

    with pytest.raises(ValueError, match=message):
        measurements.run_signal_monitor(
            base_config(tmp_path), interval_s=0.0, n_points=n_points
        )


def test_srkr_rejects_invalid_axis_before_session_creation(monkeypatch, tmp_path):
    monkeypatch.setattr(
        measurements,
        "DeviceSession",
        lambda _config: pytest.fail("invalid axis reached session creation"),
    )

    with pytest.raises(ValueError, match="axis must be 'x' or 'y'"):
        measurements.run_srkr(
            base_config(tmp_path), axis="z", scan_points=[0.0], wait_s=0.0
        )


def test_metadata_creation_failure_removes_new_csv_before_borrowed_session_io(
    monkeypatch, tmp_path
):
    config = base_config(tmp_path)
    session = FakeSession(config)
    output = tmp_path / "metadata-write-failure.csv"
    session.read_lockin_signal = lambda: pytest.fail(
        "device read after metadata failure"
    )
    monkeypatch.setattr(
        measurements.RunMetadata,
        "write",
        lambda _self: (_ for _ in ()).throw(OSError("metadata write failed")),
    )

    with pytest.raises(OSError, match="metadata write failed"):
        measurements.run_signal_monitor(
            config, interval_s=0.0, n_points=1, output=output, session=session
        )

    assert not output.exists()
    assert not (tmp_path / "metadata-write-failure.csv.meta.json").exists()
    assert not session.disconnected


def test_metadata_finalization_failure_is_raised_after_csv_close(monkeypatch, tmp_path):
    config = base_config(tmp_path)
    session = FakeSession(config)
    output = tmp_path / "metadata-finish-failure.csv"
    original_finish = measurements.RunMetadata.finish

    def fail_finish(self, **kwargs):
        if kwargs.get("status") == "completed":
            raise OSError("metadata finish failed")
        return original_finish(self, **kwargs)

    monkeypatch.setattr(measurements.RunMetadata, "finish", fail_finish)

    with pytest.raises(OSError, match="metadata finish failed"):
        measurements.run_signal_monitor(
            config,
            interval_s=0.0,
            n_points=1,
            output=output,
            session=session,
        )

    assert output.exists()
    assert output.read_text(encoding="utf-8").count("\n") == 2
    assert not session.disconnected


def test_atomic_measurement_export_publishes_csv_metadata_and_cleans_temporary_files(
    tmp_path,
):
    output = tmp_path / "exports" / "signal-export.csv"
    row = measurements.signal_monitor_row(
        timestamp="2026-01-01T00:00:00Z",
        target_elapsed_s=0.0,
        elapsed_s=0.0,
        signal=SIGNAL,
    )

    published = measurements.write_measurement_rows(
        [row],
        output=output,
        config=base_config(tmp_path),
        measurement_name="signal_monitor",
    )

    assert published == output
    assert output.read_text(encoding="utf-8").count("\n") == 2
    metadata = json.loads(
        (tmp_path / "exports" / "signal-export.csv.meta.json").read_text(
            encoding="utf-8"
        )
    )
    assert metadata["status"] == "completed"
    assert metadata["rows_written"] == 1
    assert not list(output.parent.glob(".*.tmp"))
    assert not list(output.parent.glob(".*.bak"))


def test_trkr_return_failure_keeps_row_and_records_failed_metadata(tmp_path):
    class FailingReturnSession(FakeSession):
        def move_delay_stage(
            self, value: float, *, coordinate: str = "measurement", on_status=None
        ):
            if value == 10.0:
                raise OSError("return-to-zero failed")
            return super().move_delay_stage(
                value, coordinate=coordinate, on_status=on_status
            )

    config = base_config(tmp_path)
    session = FailingReturnSession(config)

    with pytest.raises(OSError, match="return-to-zero failed"):
        measurements.run_trkr(
            config,
            scan_points=[20.0],
            wait_s=0.0,
            return_to_zero=True,
            session=session,
        )

    assert session.moves == [("t", 20.0, "measurement")]
    assert not session.disconnected
    metadata = json.loads((tmp_path / "trkr.csv.meta.json").read_text(encoding="utf-8"))
    assert metadata["status"] == "failed"
    assert metadata["rows_written"] == 1
    assert metadata["error"]["message"] == "return-to-zero failed"


def test_measurement_failure_remains_primary_when_owned_session_cleanup_fails(
    monkeypatch, tmp_path
):
    class FailingReturnAndDisconnectSession(FakeSession):
        def move_delay_stage(
            self, value: float, *, coordinate: str = "measurement", on_status=None
        ):
            if value == 10.0:
                raise ValueError("return-to-zero failed")
            return super().move_delay_stage(
                value, coordinate=coordinate, on_status=on_status
            )

        def disconnect_all(self):
            self.disconnected = True
            raise OSError("disconnect failed")

    config = base_config(tmp_path)
    session = FailingReturnAndDisconnectSession(config)
    monkeypatch.setattr(measurements, "DeviceSession", lambda _config: session)

    with pytest.raises(ValueError, match="return-to-zero failed") as caught:
        measurements.run_trkr(
            config, scan_points=[20.0], wait_s=0.0, return_to_zero=True
        )

    assert session.disconnected
    assert any("disconnect failed" in note for note in caught.value.__notes__)
    metadata = json.loads((tmp_path / "trkr.csv.meta.json").read_text(encoding="utf-8"))
    assert metadata["error"]["type"] == "ValueError"
    assert any("disconnect failed" in note for note in metadata["error"]["notes"])


def test_scan2d_wait_interruption_skips_read_then_runs_zero_returns(
    monkeypatch, tmp_path
):
    config = base_config(tmp_path)
    session = FakeSession(config)
    plan = srkr_2d_plan(
        fast_axis="x",
        slow_axis="y",
        ranges={
            "x": {"min": 0.0, "max": 0.0, "step": 1.0},
            "y": {"min": 0.0, "max": 0.0, "step": 1.0},
        },
        zero_by_axis={"t_ps": 10.0, "x_um": 1.0, "y_um": 2.0},
        return_to_zero={"fast_axis": True, "slow_axis": True},
    )
    monkeypatch.setattr(measurements, "_sleep_interruptible", lambda *_args: False)
    session.read_lockin_signal = lambda: pytest.fail("read after interrupted wait")

    rows = measurements.run_srkr_2d(config, plan=plan, wait_s=1.0, session=session)

    assert rows == []
    assert session.moves == [
        ("y", 2.0, "measurement"),
        ("x", 1.0, "measurement"),
        ("x", 1.0, "measurement"),
        ("y", 2.0, "measurement"),
    ]
    metadata = json.loads(
        (tmp_path / "srkr_2d.csv.meta.json").read_text(encoding="utf-8")
    )
    assert metadata["status"] == "stopped"
    assert metadata["rows_written"] == 0


def test_signal_monitor_emits_lifecycle_status_and_ordered_point_callbacks(tmp_path):
    session = FakeSession(base_config(tmp_path))
    statuses = []
    points = []

    rows = measurements.run_signal_monitor(
        base_config(tmp_path),
        interval_s=0.0,
        n_points=2,
        on_status=statuses.append,
        on_point=points.append,
        session=session,
    )

    assert statuses == [STATUS_RUNNING, STATUS_STOPPED]
    assert [point.index for point in points] == [1, 2]
    assert all(point.total_points == 2 for point in points)
    assert [point.row for point in points] == rows


def test_trkr_callback_stop_skips_remaining_points_and_return_to_zero(tmp_path):
    session = FakeSession(base_config(tmp_path))
    keep_running = True
    points = []

    def stop_after_first(point):
        nonlocal keep_running
        points.append(point)
        keep_running = False

    rows = measurements.run_trkr(
        base_config(tmp_path),
        scan_points=[20.0, 30.0],
        wait_s=0.0,
        on_point=stop_after_first,
        should_continue=lambda: keep_running,
        session=session,
    )

    assert len(rows) == len(points) == 1
    assert session.moves == [("t", 20.0, "measurement")]
    metadata = json.loads((tmp_path / "trkr.csv.meta.json").read_text(encoding="utf-8"))
    assert metadata["status"] == "stopped"
    assert metadata["rows_written"] == 1


def test_scan2d_callback_stop_skips_line_reset_and_both_zero_returns(tmp_path):
    session = FakeSession(base_config(tmp_path))
    keep_running = True

    def stop_after_first(_point):
        nonlocal keep_running
        keep_running = False

    plan = strkr_plan(
        fast_axis="t",
        slow_axis="x",
        ranges={
            "t": {"min": 0.0, "max": 10.0, "step": 10.0},
            "x": {"min": 0.0, "max": 1.0, "step": 1.0},
        },
        zero_by_axis={"t_ps": 10.0, "x_um": 1.0, "y_um": 2.0},
        return_to_zero={"fast_axis": True, "slow_axis": True},
    )

    rows = measurements.run_strkr(
        base_config(tmp_path),
        plan=plan,
        wait_s=0.0,
        on_point=stop_after_first,
        should_continue=lambda: keep_running,
        session=session,
    )

    assert len(rows) == 1
    assert session.moves == [
        ("x", 1.0, "measurement"),
        ("t", 10.0, "measurement"),
    ]


def test_trkr_wait_interruption_skips_read_callback_and_zero_return(
    monkeypatch, tmp_path
):
    session = FakeSession(base_config(tmp_path))
    keep_running = True
    statuses = []

    def interrupt_wait(_duration, _should_continue):
        nonlocal keep_running
        keep_running = False
        return False

    monkeypatch.setattr(measurements, "_sleep_interruptible", interrupt_wait)
    session.read_lockin_signal = lambda: pytest.fail("read occurred after interruption")

    rows = measurements.run_trkr(
        base_config(tmp_path),
        scan_points=[20.0],
        wait_s=1.0,
        on_status=statuses.append,
        should_continue=lambda: keep_running,
        session=session,
    )

    assert rows == []
    assert session.moves == [("t", 20.0, "measurement")]
    assert statuses == [
        STATUS_RUNNING,
        STATUS_MOVING_DELAY_STAGE,
        STATUS_WAITING,
        STATUS_STOPPED,
    ]
    assert STATUS_READING_LOCKIN not in statuses


@pytest.mark.parametrize("measurement", ["trkr", "srkr"])
def test_scan_rejects_target_schema_length_mismatch_before_device_io(
    measurement, tmp_path
):
    session = FakeSession(base_config(tmp_path))
    runner = measurements.run_trkr if measurement == "trkr" else measurements.run_srkr

    with pytest.raises(ValueError, match="target_points length must match"):
        runner(
            base_config(tmp_path),
            scan_points=[1.0, 2.0],
            target_points=[1.0],
            wait_s=0.0,
            session=session,
        )

    assert session.moves == []


def test_atomic_export_refuses_existing_csv_and_orphan_metadata(tmp_path):
    row = {"measurement": "custom", "value": 1}
    output = tmp_path / "existing.csv"
    output.write_text("original\n", encoding="utf-8")

    with pytest.raises(FileExistsError, match="output already exists"):
        measurements.write_measurement_rows(
            [row], output=output, config={}, measurement_name="custom"
        )

    output.unlink()
    sidecar = measurements.metadata_path(output)
    sidecar.write_text("original metadata\n", encoding="utf-8")
    with pytest.raises(FileExistsError, match="metadata already exists"):
        measurements.write_measurement_rows(
            [row], output=output, config={}, measurement_name="custom"
        )

    assert not output.exists()
    assert sidecar.read_text(encoding="utf-8") == "original metadata\n"


def test_atomic_export_overwrite_replaces_both_csv_and_metadata(tmp_path):
    output = tmp_path / "replace.csv"
    sidecar = measurements.metadata_path(output)
    output.write_text("old csv\n", encoding="utf-8")
    sidecar.write_text('{"old": true}\n', encoding="utf-8")

    measurements.write_measurement_rows(
        [{"measurement": "custom", "value": 2}],
        output=output,
        config={},
        measurement_name="custom",
        overwrite=True,
    )

    assert "old csv" not in output.read_text(encoding="utf-8")
    metadata = json.loads(sidecar.read_text(encoding="utf-8"))
    assert metadata["status"] == "completed"
    assert metadata["rows_written"] == 1


def test_scan2d_callback_failure_closes_measurement_generator(tmp_path):
    plan = strkr_plan(
        fast_axis="t",
        slow_axis="x",
        ranges={
            "t": {"min": 0.0, "max": 0.0, "step": 1.0},
            "x": {"min": 0.0, "max": 0.0, "step": 1.0},
        },
        zero_by_axis={"t_ps": 10.0, "x_um": 1.0, "y_um": 2.0},
        return_to_zero={"fast_axis": False, "slow_axis": False},
    )

    with pytest.raises(ValueError, match="callback failed"):
        measurements.run_strkr(
            base_config(tmp_path),
            plan=plan,
            wait_s=0.0,
            on_point=lambda _point: (_ for _ in ()).throw(
                ValueError("callback failed")
            ),
            session=FakeSession(base_config(tmp_path)),
        )


def test_trkr_callback_failure_closes_measurement_generator(tmp_path):
    with pytest.raises(ValueError, match="callback failed"):
        measurements.run_trkr(
            base_config(tmp_path),
            scan_points=[20.0],
            wait_s=0.0,
            on_point=lambda _point: (_ for _ in ()).throw(
                ValueError("callback failed")
            ),
            session=FakeSession(base_config(tmp_path)),
        )


def test_srkr_callback_failure_closes_measurement_generator(tmp_path):
    with pytest.raises(ValueError, match="callback failed"):
        measurements.run_srkr(
            base_config(tmp_path),
            scan_points=[1.0],
            wait_s=0.0,
            on_point=lambda _point: (_ for _ in ()).throw(
                ValueError("callback failed")
            ),
            session=FakeSession(base_config(tmp_path)),
        )
    assert not list(tmp_path.glob(".*.bak"))
    assert not list(tmp_path.glob(".*.tmp"))


def test_atomic_export_restores_existing_pair_when_metadata_finish_fails(
    monkeypatch, tmp_path
):
    output = tmp_path / "rollback.csv"
    sidecar = measurements.metadata_path(output)
    output.write_text("old csv\n", encoding="utf-8")
    sidecar.write_text('{"old": true}\n', encoding="utf-8")
    monkeypatch.setattr(
        measurements.RunMetadata,
        "finish",
        lambda _self, **_kwargs: (_ for _ in ()).throw(
            OSError("metadata publish failed")
        ),
    )

    with pytest.raises(OSError, match="metadata publish failed"):
        measurements.write_measurement_rows(
            [{"measurement": "custom", "value": 3}],
            output=output,
            config={},
            measurement_name="custom",
            overwrite=True,
        )

    assert output.read_text(encoding="utf-8") == "old csv\n"
    assert sidecar.read_text(encoding="utf-8") == '{"old": true}\n'
    assert not list(tmp_path.glob(".*.bak"))
    assert not list(tmp_path.glob(".*.tmp"))


def test_atomic_export_removes_new_pair_when_metadata_finish_fails(
    monkeypatch, tmp_path
):
    output = tmp_path / "new-rollback.csv"
    original_write = measurements.RunMetadata.write

    def write_then_fail(self, **_kwargs):
        original_write(self)
        raise OSError("metadata finalization failed")

    monkeypatch.setattr(measurements.RunMetadata, "finish", write_then_fail)

    with pytest.raises(OSError, match="metadata finalization failed"):
        measurements.write_measurement_rows(
            [{"measurement": "custom", "value": 4}],
            output=output,
            config={},
            measurement_name="custom",
        )

    assert not output.exists()
    assert not measurements.metadata_path(output).exists()
    assert not list(tmp_path.glob(".*.tmp"))


def test_atomic_export_retains_recovery_backups_when_rollback_fails(
    monkeypatch, tmp_path
):
    output = tmp_path / "rollback-failure.csv"
    sidecar = measurements.metadata_path(output)
    output.write_text("old csv\n", encoding="utf-8")
    sidecar.write_text('{"old": true}\n', encoding="utf-8")
    original_replace = Path.replace

    monkeypatch.setattr(
        measurements.RunMetadata,
        "finish",
        lambda _self, **_kwargs: (_ for _ in ()).throw(OSError("publish failed")),
    )

    def fail_backup_restore(self, target):
        if self.name.endswith(".bak"):
            raise PermissionError("restore denied")
        return original_replace(self, target)

    monkeypatch.setattr(Path, "replace", fail_backup_restore)

    with pytest.raises(OSError, match="publish failed") as caught:
        measurements.write_measurement_rows(
            [{"measurement": "custom", "value": 5}],
            output=output,
            config={},
            measurement_name="custom",
            overwrite=True,
        )

    backups = list(tmp_path.glob(".*.bak"))
    assert len(backups) == 2
    assert {path.read_text(encoding="utf-8") for path in backups} == {
        "old csv\n",
        '{"old": true}\n',
    }
    assert any("rollback also failed" in note for note in caught.value.__notes__)


def test_atomic_export_cleanup_failure_is_terminal_after_success(monkeypatch, tmp_path):
    output = tmp_path / "cleanup-failure.csv"
    output.write_text("old csv\n", encoding="utf-8")
    measurements.metadata_path(output).write_text('{"old": true}\n', encoding="utf-8")
    original_unlink = Path.unlink

    def fail_temporary_cleanup(self, *args, **kwargs):
        if self.name.endswith(".tmp") and ".meta.json." not in self.name:
            raise OSError("temporary cleanup failed")
        return original_unlink(self, *args, **kwargs)

    monkeypatch.setattr(Path, "unlink", fail_temporary_cleanup)

    with pytest.raises(OSError, match="temporary cleanup failed"):
        measurements.write_measurement_rows(
            [{"measurement": "custom", "value": 6}],
            output=output,
            config={},
            measurement_name="custom",
            overwrite=True,
        )


def test_atomic_export_cleanup_failure_is_noted_on_primary_error(monkeypatch, tmp_path):
    output = tmp_path / "cleanup-secondary.csv"
    output.write_text("old csv\n", encoding="utf-8")
    measurements.metadata_path(output).write_text('{"old": true}\n', encoding="utf-8")
    original_unlink = Path.unlink
    monkeypatch.setattr(
        measurements.RunMetadata,
        "finish",
        lambda _self, **_kwargs: (_ for _ in ()).throw(OSError("publish failed")),
    )

    def fail_temporary_cleanup(self, *args, **kwargs):
        if self.name.endswith(".tmp") and ".meta.json." not in self.name:
            raise PermissionError("temporary cleanup denied")
        return original_unlink(self, *args, **kwargs)

    monkeypatch.setattr(Path, "unlink", fail_temporary_cleanup)

    with pytest.raises(OSError, match="publish failed") as caught:
        measurements.write_measurement_rows(
            [{"measurement": "custom", "value": 7}],
            output=output,
            config={},
            measurement_name="custom",
            overwrite=True,
        )

    assert any("temporary cleanup denied" in note for note in caught.value.__notes__)


def test_callback_and_metadata_finish_failures_preserve_primary_and_note_secondary(
    monkeypatch, tmp_path
):
    session = FakeSession(base_config(tmp_path))
    monkeypatch.setattr(
        measurements.RunMetadata,
        "finish",
        lambda _self, **_kwargs: (_ for _ in ()).throw(
            OSError("metadata finish also failed")
        ),
    )

    with pytest.raises(ValueError, match="callback failed") as caught:
        measurements.run_signal_monitor(
            base_config(tmp_path),
            interval_s=0.0,
            n_points=1,
            output=tmp_path / "combined-failure.csv",
            on_point=lambda _point: (_ for _ in ()).throw(
                ValueError("callback failed")
            ),
            session=session,
        )

    assert caught.value.__notes__ == [
        "Run metadata finalization also failed: metadata finish also failed"
    ]


def test_scan2d_spatial_fast_temporal_slow_without_zero_returns(tmp_path):
    session = FakeSession(base_config(tmp_path))
    plan = strkr_plan(
        fast_axis="x",
        slow_axis="t",
        ranges={
            "t": {"min": 0.0, "max": 0.0, "step": 1.0},
            "x": {"min": 0.0, "max": 0.0, "step": 1.0},
        },
        zero_by_axis={"t_ps": 10.0, "x_um": 1.0, "y_um": 2.0},
        return_to_zero={"fast_axis": False, "slow_axis": False},
    )

    rows = measurements.run_strkr(
        base_config(tmp_path), plan=plan, wait_s=0.0, session=session
    )

    assert len(rows) == 1
    assert session.moves == [
        ("t", 10.0, "measurement"),
        ("x", 1.0, "measurement"),
    ]
    assert session.scanner_hysteresis_flags == [True]


@pytest.mark.parametrize("value", [-0.1, float("nan"), float("inf")])
def test_measurement_wait_validation_rejects_unsafe_values(value):
    with pytest.raises(ValueError, match="finite and non-negative"):
        measurements._validated_non_negative(value, "wait")


def test_scan_points_helper_and_interruptible_sleep_boundaries(tmp_path):
    config = base_config(tmp_path)
    config["measurements"]["trkr"]["scan"] = {
        "min": -1.0,
        "max": 1.0,
        "step": 1.0,
    }

    assert measurements.scan_points_from_config(config, "trkr") == [-1.0, 0.0, 1.0]
    assert measurements._sleep_interruptible(1.0, lambda: False) is False


def test_interruptible_sleep_performs_bounded_sleep_then_rechecks(monkeypatch):
    times = iter([0.0, 0.0, 0.0, 1.0])
    sleeps = []
    monkeypatch.setattr(measurements.time, "monotonic", lambda: next(times))
    monkeypatch.setattr(measurements.time, "sleep", sleeps.append)

    assert measurements._sleep_interruptible(0.5, lambda: True) is True
    assert sleeps == [0.05]


def test_write_rows_accepts_iterator_without_close_method():
    rows = measurements._write_rows(
        [],
        output=None,
        point_iter=iter([]),
        on_point=None,
        config={},
        measurement_name="custom",
        expected_points=0,
    )

    assert rows == []


def test_write_rows_surfaces_csv_close_failure_after_success(monkeypatch, tmp_path):
    output = tmp_path / "close-failure.csv"
    original_open = type(output).open

    class CloseFailingStream:
        def __init__(self, stream):
            self.stream = stream

        def __getattr__(self, name):
            return getattr(self.stream, name)

        def close(self):
            self.stream.close()
            raise OSError("CSV close failed")

    def open_with_close_failure(path, *args, **kwargs):
        stream = original_open(path, *args, **kwargs)
        return (
            CloseFailingStream(stream)
            if path == output and args and args[0] == "x"
            else stream
        )

    monkeypatch.setattr(type(output), "open", open_with_close_failure)

    with pytest.raises(OSError, match="CSV close failed"):
        measurements._write_rows(
            [],
            output=output,
            point_iter=iter([]),
            on_point=None,
            config={},
            measurement_name="custom",
            expected_points=0,
        )


def test_write_rows_notes_csv_close_failure_on_primary_callback_error(
    monkeypatch, tmp_path
):
    output = tmp_path / "callback-close-failure.csv"
    original_open = type(output).open

    class CloseFailingStream:
        def __init__(self, stream):
            self.stream = stream

        def __getattr__(self, name):
            return getattr(self.stream, name)

        def close(self):
            self.stream.close()
            raise OSError("CSV close also failed")

    def open_with_close_failure(path, *args, **kwargs):
        stream = original_open(path, *args, **kwargs)
        return (
            CloseFailingStream(stream)
            if path == output and args and args[0] == "x"
            else stream
        )

    monkeypatch.setattr(type(output), "open", open_with_close_failure)
    point = MeasurementPoint(index=1, total_points=1, row={"value": 1})

    with pytest.raises(ValueError, match="callback failed") as caught:
        measurements._write_rows(
            [],
            output=output,
            point_iter=iter([point]),
            on_point=lambda _point: (_ for _ in ()).throw(
                ValueError("callback failed")
            ),
            config={},
            measurement_name="custom",
            expected_points=1,
        )

    assert "CSV close also failed" in caught.value.__notes__[0]


def test_atomic_export_notes_rollback_secondary_failure(monkeypatch, tmp_path):
    output = tmp_path / "rollback-secondary.csv"
    sidecar = measurements.metadata_path(output)
    output.write_text("old csv\n", encoding="utf-8")
    sidecar.write_text('{"old": true}\n', encoding="utf-8")
    original_replace = type(output).replace

    def fail_backup_restore(path, target):
        if path.name.endswith(".bak"):
            raise OSError("backup restore failed")
        return original_replace(path, target)

    monkeypatch.setattr(type(output), "replace", fail_backup_restore)
    monkeypatch.setattr(
        measurements.RunMetadata,
        "finish",
        lambda _self, **_kwargs: (_ for _ in ()).throw(
            OSError("metadata publish failed")
        ),
    )

    with pytest.raises(OSError, match="metadata publish failed") as caught:
        measurements.write_measurement_rows(
            [{"measurement": "custom", "value": 5}],
            output=output,
            config={},
            measurement_name="custom",
            overwrite=True,
        )

    assert caught.value.__notes__ == [
        "Measurement export rollback also failed: backup restore failed"
    ]


def test_atomic_export_serialization_failure_never_publishes(monkeypatch, tmp_path):
    output = tmp_path / "serialization-failure.csv"
    monkeypatch.setattr(
        measurements,
        "output_rows",
        lambda _rows: (_ for _ in ()).throw(ValueError("row conversion failed")),
    )

    with pytest.raises(ValueError, match="row conversion failed"):
        measurements.write_measurement_rows(
            [{"measurement": "custom", "value": object()}],
            output=output,
            config={},
            measurement_name="custom",
        )

    assert not output.exists()
    assert not measurements.metadata_path(output).exists()
    assert not list(tmp_path.glob(".*.tmp"))


@pytest.mark.parametrize("existing", ["output", "metadata"])
def test_atomic_export_overwrite_handles_only_one_existing_artifact(existing, tmp_path):
    output = tmp_path / f"partial-{existing}.csv"
    sidecar = measurements.metadata_path(output)
    if existing == "output":
        output.write_text("old csv\n", encoding="utf-8")
    else:
        sidecar.write_text('{"old": true}\n', encoding="utf-8")

    measurements.write_measurement_rows(
        [{"measurement": "custom", "value": 6}],
        output=output,
        config={},
        measurement_name="custom",
        overwrite=True,
    )

    assert output.exists()
    assert json.loads(sidecar.read_text(encoding="utf-8"))["status"] == "completed"


def test_atomic_export_immediate_finish_failure_removes_only_published_csv(
    monkeypatch, tmp_path
):
    output = tmp_path / "immediate-finish-failure.csv"
    monkeypatch.setattr(
        measurements.RunMetadata,
        "finish",
        lambda _self, **_kwargs: (_ for _ in ()).throw(
            OSError("finish failed before metadata write")
        ),
    )

    with pytest.raises(OSError, match="finish failed before metadata write"):
        measurements.write_measurement_rows(
            [{"measurement": "custom", "value": 7}],
            output=output,
            config={},
            measurement_name="custom",
        )

    assert not output.exists()
    assert not measurements.metadata_path(output).exists()


def test_scan2d_rejects_empty_plan_and_unknown_axis_before_session_creation(
    monkeypatch, tmp_path
):
    monkeypatch.setattr(
        measurements,
        "DeviceSession",
        lambda _config: pytest.fail("session created for invalid plan"),
    )
    empty_plan = SimpleNamespace(total_points=0)

    with pytest.raises(ValueError, match="scan point count must be between"):
        measurements._run_scan2d(
            base_config(tmp_path),
            measurement_name="strkr",
            plan=empty_plan,
            wait_s=0.0,
            output=None,
            on_status=None,
            on_point=None,
            should_continue=None,
            session=None,
        )

    with pytest.raises(ValueError, match="Unsupported scan axis: z"):
        measurements._move_axis(
            FakeSession(base_config(tmp_path)),
            "z",
            0.0,
            zero={},
        )


def test_srkr_stop_before_first_point_skips_io_and_return(tmp_path):
    session = FakeSession(base_config(tmp_path))

    rows = measurements.run_srkr(
        base_config(tmp_path),
        axis="x",
        scan_points=[1.0],
        wait_s=0.0,
        should_continue=lambda: False,
        session=session,
    )

    assert rows == []
    assert session.moves == []


@pytest.mark.parametrize("measurement", ["signal_monitor", "srkr"])
def test_measurement_wait_interruption_skips_read_and_return(
    measurement, monkeypatch, tmp_path
):
    session = FakeSession(base_config(tmp_path))
    keep_running = True
    session.read_lockin_signal = lambda: pytest.fail("read after interrupted wait")

    def interrupt_wait(_wait, _continue):
        nonlocal keep_running
        keep_running = False
        return False

    monkeypatch.setattr(measurements, "_sleep_interruptible", interrupt_wait)

    if measurement == "signal_monitor":
        rows = measurements.run_signal_monitor(
            base_config(tmp_path),
            interval_s=1.0,
            n_points=1,
            output=tmp_path / "interrupted-signal.csv",
            should_continue=lambda: keep_running,
            session=session,
        )
    else:
        rows = measurements.run_srkr(
            base_config(tmp_path),
            axis="x",
            scan_points=[1.0],
            wait_s=1.0,
            output=tmp_path / "interrupted-srkr.csv",
            should_continue=lambda: keep_running,
            session=session,
        )

    assert rows == []
    if measurement == "srkr":
        assert session.moves == [("x", 1.0, "measurement")]
