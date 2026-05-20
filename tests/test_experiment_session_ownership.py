from __future__ import annotations

import pytest

from kohdalab.api import Experiment, measurements, signal_monitor_plan


def minimal_config(tmp_path):
    return {
        "measurements": {
            "move_abs": {"zero": {"t_ps": 0.0, "x_um": 0.0, "y_um": 0.0}},
            "signal_monitor": {
                "output": {"dir": str(tmp_path), "filename": "signal", "auto_timestamp_suffix": False}
            },
            "trkr": {
                "scan": {"min": 0.0, "max": 0.0, "step": 1.0},
                "output": {"dir": str(tmp_path), "filename": "trkr", "auto_timestamp_suffix": False},
            },
            "srkr": {
                "scan": {"axis": "x", "min": 0.0, "max": 0.0, "step": 1.0},
                "output": {"dir": str(tmp_path), "filename": "srkr", "auto_timestamp_suffix": False},
            },
            "strkr": {
                "scan": {
                    "fast_axis": "t",
                    "slow_axis": "x",
                    "ranges": {
                        "t": {"min": 0.0, "max": 0.0, "step": 1.0},
                        "x": {"min": 0.0, "max": 0.0, "step": 1.0},
                        "y": {"min": 0.0, "max": 0.0, "step": 1.0},
                    },
                },
                "output": {"dir": str(tmp_path), "filename": "strkr", "auto_timestamp_suffix": False},
            },
            "srkr_2d": {
                "scan": {
                    "fast_axis": "x",
                    "slow_axis": "y",
                    "ranges": {
                        "x": {"min": 0.0, "max": 0.0, "step": 1.0},
                        "y": {"min": 0.0, "max": 0.0, "step": 1.0},
                    },
                },
                "output": {"dir": str(tmp_path), "filename": "srkr_2d", "auto_timestamp_suffix": False},
            },
        }
    }


class RaisingSession:
    def __init__(self, config):
        self.config = config
        self.disconnected = False

    def read_lockin_signal(self):
        raise RuntimeError("boom")

    def disconnect_all(self):
        self.disconnected = True


def test_experiment_run_signal_monitor_passes_existing_session(monkeypatch, tmp_path):
    experiment = Experiment(minimal_config(tmp_path), auto_connect=False)
    captured = {}

    def run_signal_monitor(config, **kwargs):
        captured.update(kwargs)
        return [{"ok": True}]

    monkeypatch.setattr(measurements, "run_signal_monitor", run_signal_monitor)

    plan = signal_monitor_plan(interval_s=0.0, n_points=1)

    assert experiment.run_signal_monitor(plan=plan) == [{"ok": True}]
    assert captured["plan"] is plan
    assert captured["session"] is experiment.session


def test_experiment_run_trkr_passes_existing_session(monkeypatch, tmp_path):
    experiment = Experiment(minimal_config(tmp_path), auto_connect=False)
    captured = {}

    def run_trkr(config, **kwargs):
        captured.update(kwargs)
        return [{"ok": True}]

    monkeypatch.setattr(measurements, "run_trkr", run_trkr)

    assert experiment.run_trkr(scan_points=[0.0], wait_s=0.0) == [{"ok": True}]
    assert captured["session"] is experiment.session


def test_experiment_run_srkr_passes_existing_session(monkeypatch, tmp_path):
    experiment = Experiment(minimal_config(tmp_path), auto_connect=False)
    captured = {}

    def run_srkr(config, **kwargs):
        captured.update(kwargs)
        return [{"ok": True}]

    monkeypatch.setattr(measurements, "run_srkr", run_srkr)

    assert experiment.run_srkr(axis="x", scan_points=[0.0], wait_s=0.0) == [{"ok": True}]
    assert captured["session"] is experiment.session


def test_experiment_run_strkr_passes_existing_session(monkeypatch, tmp_path):
    experiment = Experiment(minimal_config(tmp_path), auto_connect=False)
    captured = {}

    def run_strkr(config, **kwargs):
        captured.update(kwargs)
        return [{"ok": True}]

    monkeypatch.setattr(measurements, "run_strkr", run_strkr)

    assert experiment.run_strkr(wait_s=0.0) == [{"ok": True}]
    assert captured["session"] is experiment.session


def test_experiment_run_srkr_2d_passes_existing_session(monkeypatch, tmp_path):
    experiment = Experiment(minimal_config(tmp_path), auto_connect=False)
    captured = {}

    def run_srkr_2d(config, **kwargs):
        captured.update(kwargs)
        return [{"ok": True}]

    monkeypatch.setattr(measurements, "run_srkr_2d", run_srkr_2d)

    assert experiment.run_srkr_2d(wait_s=0.0) == [{"ok": True}]
    assert captured["session"] is experiment.session


def test_standalone_measurement_disconnects_temporary_session_on_error(monkeypatch, tmp_path):
    sessions: list[RaisingSession] = []

    def make_session(config):
        session = RaisingSession(config)
        sessions.append(session)
        return session

    monkeypatch.setattr(measurements, "DeviceSession", make_session)

    with pytest.raises(RuntimeError, match="boom"):
        measurements.run_signal_monitor(minimal_config(tmp_path), interval_s=0.0, n_points=1)

    assert sessions[0].disconnected


def test_provided_session_is_not_disconnected_on_error(tmp_path):
    session = RaisingSession(minimal_config(tmp_path))

    with pytest.raises(RuntimeError, match="boom"):
        measurements.run_signal_monitor(
            minimal_config(tmp_path),
            interval_s=0.0,
            n_points=1,
            session=session,
        )

    assert not session.disconnected
