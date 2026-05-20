from __future__ import annotations

import json

from kohdalab.api import cli
from kohdalab.api.models import MeasurementPoint, Position
from kohdalab.api.status import STATUS_RUNNING, STATUS_STOPPED


class FakeExperiment:
    calls: list[tuple[str, object]] = []

    def __init__(self, config):
        self.config = config

    def run_signal_monitor(self, **kwargs):
        self.calls.append(("signal_monitor", kwargs))
        kwargs["on_status"](STATUS_RUNNING)
        kwargs["on_point"](
            MeasurementPoint(
                index=1,
                total_points=1,
                row={"elapsed_s": 0.0, "X_V": 1.0, "Y_V": 2.0, "R_V": 3.0, "Theta_deg": 4.0},
            )
        )
        kwargs["on_status"](STATUS_STOPPED)
        return [kwargs["on_point"]]

    def run_trkr(self, **kwargs):
        self.calls.append(("trkr", kwargs))
        kwargs["on_status"](STATUS_RUNNING)
        kwargs["on_point"](
            MeasurementPoint(
                index=1,
                total_points=1,
                row={"t_cor_ps": -50.0, "X_V": 1.0, "Y_V": 2.0, "R_V": 3.0, "Theta_deg": 4.0},
            )
        )
        kwargs["on_status"](STATUS_STOPPED)
        return [kwargs["on_point"]]

    def run_srkr(self, **kwargs):
        self.calls.append(("srkr", kwargs))
        kwargs["on_status"](STATUS_RUNNING)
        kwargs["on_point"](
            MeasurementPoint(
                index=1,
                total_points=1,
                row={"y_cor_um": -1.0, "X_V": 1.0, "Y_V": 2.0, "R_V": 3.0, "Theta_deg": 4.0},
            )
        )
        kwargs["on_status"](STATUS_STOPPED)
        return [kwargs["on_point"]]

    def run_strkr(self, **kwargs):
        self.calls.append(("strkr", kwargs))
        kwargs["on_status"](STATUS_RUNNING)
        kwargs["on_point"](
            MeasurementPoint(
                index=1,
                total_points=1,
                row={"t_cor_ps": 0.0, "X_V": 1.0, "Y_V": 2.0, "R_V": 3.0, "Theta_deg": 4.0},
            )
        )
        kwargs["on_status"](STATUS_STOPPED)
        return [kwargs["on_point"]]

    def run_srkr_2d(self, **kwargs):
        self.calls.append(("srkr_2d", kwargs))
        kwargs["on_status"](STATUS_RUNNING)
        kwargs["on_point"](
            MeasurementPoint(
                index=1,
                total_points=1,
                row={"x_cor_um": 0.0, "X_V": 1.0, "Y_V": 2.0, "R_V": 3.0, "Theta_deg": 4.0},
            )
        )
        kwargs["on_status"](STATUS_STOPPED)
        return [kwargs["on_point"]]

    def move_delay_stage(self, value, *, coordinate="measurement"):
        self.calls.append(("move_delay_stage", {"value": value, "coordinate": coordinate}))
        return Position(t_ps=value, delay_stage_mm=value / 10.0, delay_stage_pulse=int(value * 100))

    def move_scanner(self, axis, value, *, coordinate="measurement"):
        self.calls.append(("move_scanner", {"axis": axis, "value": value, "coordinate": coordinate}))
        if axis == "x":
            return Position(x_um=value, scanner_x_value=value / 100.0, scanner_x_unit="mm")
        return Position(y_um=value, scanner_y_value=value / 100.0, scanner_y_unit="mm")


def config_file(tmp_path):
    path = tmp_path / "config.json"
    path.write_text(
        json.dumps(
            {
                "measurements": {
                    "move_abs": {"zero": {"t_ps": -122.0, "x_um": 61.5, "y_um": 477.0}},
                    "signal_monitor": {"interval_s": 0.1, "n_points": 2},
                    "trkr": {
                        "scan": {"min": -50.0, "max": 0.0, "step": 50.0},
                        "output": {"dir": str(tmp_path), "filename": "trkr", "auto_timestamp_suffix": False},
                    },
                    "srkr": {
                        "scan": {"axis": "x", "min": -1.0, "max": 1.0, "step": 1.0},
                        "output": {"dir": str(tmp_path), "filename": "srkr", "auto_timestamp_suffix": False},
                    },
                    "strkr": {
                        "scan": {
                            "fast_axis": "t",
                            "slow_axis": "x",
                            "ranges": {
                                "t": {"min": 0.0, "max": 1.0, "step": 1.0},
                                "x": {"min": 0.0, "max": 1.0, "step": 1.0},
                                "y": {"min": 0.0, "max": 1.0, "step": 1.0},
                            },
                        },
                        "output": {"dir": str(tmp_path), "filename": "strkr", "auto_timestamp_suffix": False},
                    },
                    "srkr_2d": {
                        "scan": {
                            "fast_axis": "x",
                            "slow_axis": "y",
                            "ranges": {
                                "x": {"min": 0.0, "max": 1.0, "step": 1.0},
                                "y": {"min": 0.0, "max": 1.0, "step": 1.0},
                            },
                        },
                        "output": {"dir": str(tmp_path), "filename": "srkr_2d", "auto_timestamp_suffix": False},
                    },
                }
            }
        ),
        encoding="utf-8",
    )
    return path


def test_cli_trkr_uses_config_plan_and_prints_progress(monkeypatch, tmp_path, capsys):
    FakeExperiment.calls = []
    monkeypatch.setattr(cli, "Experiment", FakeExperiment)

    assert cli.main(["--config", str(config_file(tmp_path)), "trkr"]) == 0

    name, kwargs = FakeExperiment.calls[0]
    plan = kwargs["plan"]
    assert name == "trkr"
    assert plan.target_points == [-50.0, 0.0]
    assert plan.scan_points == [-172.0, -122.0]
    assert kwargs["output"] == tmp_path / "trkr.csv"
    output = capsys.readouterr().out
    assert "Starting TRKR: 2 points (measurement)" in output
    assert f"status: {STATUS_RUNNING}" in output
    assert "[1/1] t_cor_ps=-50" in output
    assert f"Saved -> {tmp_path / 'trkr.csv'}" in output


def test_cli_srkr_axis_override_uses_config_plan_and_prints_progress(monkeypatch, tmp_path, capsys):
    FakeExperiment.calls = []
    monkeypatch.setattr(cli, "Experiment", FakeExperiment)

    assert cli.main(["--config", str(config_file(tmp_path)), "srkr", "--axis", "y"]) == 0

    name, kwargs = FakeExperiment.calls[0]
    plan = kwargs["plan"]
    assert name == "srkr"
    assert plan.axis == "y"
    assert plan.target_points == [-1.0, 0.0, 1.0]
    assert plan.scan_points == [476.0, 477.0, 478.0]
    assert kwargs["output"] == tmp_path / "srkr.csv"
    output = capsys.readouterr().out
    assert "Starting SRKR: Y measurement, 3 points" in output
    assert "[1/1] y_cor_um=-1" in output


def test_cli_signal_monitor_prints_progress(monkeypatch, tmp_path, capsys):
    FakeExperiment.calls = []
    monkeypatch.setattr(cli, "Experiment", FakeExperiment)

    assert cli.main(["--config", str(config_file(tmp_path)), "signal-monitor"]) == 0

    name, kwargs = FakeExperiment.calls[0]
    assert name == "signal_monitor"
    assert kwargs["output"].name.startswith("signal_monitor_run_")
    output = capsys.readouterr().out
    assert "Starting Signal Monitor" in output
    assert f"status: {STATUS_STOPPED}" in output
    assert "[1/1] elapsed_s=0" in output


def test_cli_strkr_prints_progress(monkeypatch, tmp_path, capsys):
    FakeExperiment.calls = []
    monkeypatch.setattr(cli, "Experiment", FakeExperiment)

    assert cli.main(["--config", str(config_file(tmp_path)), "strkr", "--fast-axis", "t", "--slow-axis", "x"]) == 0

    name, kwargs = FakeExperiment.calls[0]
    assert name == "strkr"
    assert kwargs["plan"].fast_axis == "t"
    assert kwargs["plan"].slow_axis == "x"
    assert kwargs["output"] == tmp_path / "strkr.csv"
    output = capsys.readouterr().out
    assert "Starting STRKR T fast / X slow" in output
    assert "[1/1] t_cor_ps=0" in output


def test_cli_srkr_2d_prints_progress(monkeypatch, tmp_path, capsys):
    FakeExperiment.calls = []
    monkeypatch.setattr(cli, "Experiment", FakeExperiment)

    assert cli.main(["--config", str(config_file(tmp_path)), "srkr-2d", "--fast-axis", "x", "--slow-axis", "y"]) == 0

    name, kwargs = FakeExperiment.calls[0]
    assert name == "srkr_2d"
    assert kwargs["plan"].fast_axis == "x"
    assert kwargs["plan"].slow_axis == "y"
    assert kwargs["output"] == tmp_path / "srkr_2d.csv"
    output = capsys.readouterr().out
    assert "Starting SRKR 2D X fast / Y slow" in output
    assert "[1/1] x_cor_um=0" in output


def test_cli_move_abs_formats_position(monkeypatch, tmp_path, capsys):
    FakeExperiment.calls = []
    monkeypatch.setattr(cli, "Experiment", FakeExperiment)

    assert cli.main(["--config", str(config_file(tmp_path)), "move-abs", "--axis", "x", "--value", "62.5"]) == 0

    assert FakeExperiment.calls == [("move_scanner", {"axis": "x", "value": 62.5, "coordinate": "measurement"})]
    output = capsys.readouterr().out
    assert "[1/1] x_um=62.5 x_cor_um=1" in output
    assert "target=62.5 coordinate=measurement" in output
