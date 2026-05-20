from __future__ import annotations

import json
from pathlib import Path

from kohdalab.api.models import LiveStatus, MeasurementPoint, Position
from kohdalab.api.status import STATUS_RUNNING, STATUS_STOPPED
from kohdalab.apps.web_ui import WebExperimentController


class FakeExperiment:
    def __init__(self, config, *, auto_connect=True):
        self.config = config
        self.auto_connect = auto_connect
        self.connected = {}
        for kind, devices in config.get("instruments", {}).items():
            for key in devices:
                self.connected[f"{kind}.{key}"] = False

    def connected_devices(self):
        return dict(self.connected)

    def connect_all(self):
        for ref in self.connected:
            self.connected[ref] = True

    def disconnect_all(self):
        for ref in self.connected:
            self.connected[ref] = False

    def connect_device(self, ref):
        self.connected[ref] = True

    def disconnect_device(self, ref):
        self.connected[ref] = False

    def initialize_delay_stage(self, ref="delay_stage", *, on_status=None):
        self.connected[ref] = True
        return {"ref": ref}

    def initialize_scanner(self, axis, ref=None, *, on_status=None):
        self.connected[ref or f"scanner.{axis}"] = True
        return {"axis": axis}

    def read_live_status(self):
        return LiveStatus(
            connected=dict(self.connected),
            position=Position(t_ps=1.0, x_um=2.0, y_um=3.0),
            signal={"X": 1.0, "Y": 2.0, "R": 3.0, "Theta": 4.0},
            lockin_settings={"Sensitivity": 1.0, "Time Constant": 0.1},
        )

    def move_delay_stage(self, value, *, coordinate="measurement", on_status=None):
        return Position(t_ps=float(value))

    def move_scanner(self, axis, value, *, coordinate="measurement", on_status=None):
        if axis == "x":
            return Position(x_um=float(value))
        return Position(y_um=float(value))

    def missing_devices(self, measurement_name, *, axis=None, fast_axis=None, slow_axis=None):
        return []

    def run_signal_monitor(self, **kwargs):
        kwargs["on_status"](STATUS_RUNNING)
        kwargs["on_point"](
            MeasurementPoint(
                index=1,
                total_points=1,
                row={
                    "timestamp": "2026-05-20T00:00:00",
                    "measurement": "signal_monitor",
                    "elapsed_s": 0.0,
                    "X_V": 1.0,
                    "Y_V": 2.0,
                    "R_V": 3.0,
                    "Theta_deg": 4.0,
                },
            )
        )
        kwargs["on_status"](STATUS_STOPPED)
        return []


def config_file(tmp_path: Path) -> Path:
    path = tmp_path / "config.json"
    path.write_text(
        json.dumps(
            {
                "instruments": {
                    "lockin": {"main": {"model": "SR7265", "resource": "GPIB0::12::INSTR"}},
                    "delay_stage": {"t": {"controller": "SHOT302GS", "stage": "SGSP46-500", "port": "COM6", "direction": 1}},
                    "scanner": {
                        "x": {"controller": "CONEXCC", "actuator": "TRA12CC", "port": "COM5", "axis": 1, "sample_um_per_unit": 1.0},
                        "y": {"controller": "CONEXCC", "actuator": "TRA12CC", "port": "COM4", "axis": 1, "sample_um_per_unit": 1.0},
                    },
                },
                "measurements": {
                    "move_abs": {"zero": {"t_ps": 0.0, "x_um": 0.0, "y_um": 0.0}},
                    "signal_monitor": {
                        "interval_s": 0.1,
                        "n_points": 1,
                        "output": {"dir": str(tmp_path), "filename": "signal", "auto_timestamp_suffix": False},
                    },
                },
            }
        ),
        encoding="utf-8",
    )
    return path


def test_web_controller_owns_non_autoconnect_experiment(tmp_path):
    controller = WebExperimentController(
        config_file(tmp_path),
        experiment_factory=FakeExperiment,
        last_config_state_path=tmp_path / "last_config.json",
    )

    assert controller.experiment.auto_connect is False
    assert controller.state()["connected"]["lockin.main"] is False

    controller.connect_device("lockin.main")

    assert controller.state()["connected"]["lockin.main"] is True


def test_web_controller_runs_and_saves_signal_rows(tmp_path):
    controller = WebExperimentController(
        config_file(tmp_path),
        experiment_factory=FakeExperiment,
        last_config_state_path=tmp_path / "last_config.json",
    )

    controller.start_measurement(
        {
            "measurement": "signal_monitor",
            "settings": {"interval_s": 0.1, "n_points": 1},
            "output": {"output_dir": str(tmp_path), "filename": "signal", "auto_timestamp_suffix": False},
        }
    )

    assert controller.wait_for_idle()
    state = controller.state()
    assert state["row_counts"]["signal_monitor"] == 1
    assert state["job"]["status"] == "completed"

    controller.save_rows(
        "signal_monitor",
        {"output_dir": str(tmp_path), "filename": "manual", "auto_timestamp_suffix": False},
    )

    saved = tmp_path / "manual.csv"
    assert saved.exists()
    assert "X_V" in saved.read_text(encoding="utf-8")


def test_web_controller_uses_last_config_when_omitted(tmp_path):
    config = config_file(tmp_path)
    state_path = tmp_path / "last_config.json"
    WebExperimentController(config, experiment_factory=FakeExperiment, last_config_state_path=state_path)

    controller = WebExperimentController(None, experiment_factory=FakeExperiment, last_config_state_path=state_path)
    state = controller.state()

    assert state["config_path"] == str(config)
    assert state["config_source"] == "last"


def test_web_controller_can_start_without_any_config(tmp_path):
    controller = WebExperimentController(
        None,
        experiment_factory=FakeExperiment,
        last_config_state_path=tmp_path / "missing_last.json",
        lab_default_path=tmp_path / "missing_default.json",
    )
    state = controller.state()

    assert state["has_config"] is False
    assert state["config_path"] is None
    assert state["connected"] == {}
