from __future__ import annotations

import argparse
import json
import runpy
import sys

import pytest

from kohdalab.api import cli
from kohdalab.api.models import MeasurementPoint, Position
from kohdalab.api.status import STATUS_RUNNING, STATUS_STOPPED


class FakeExperiment:
    calls: list[tuple[str, object]] = []
    close_count = 0

    def __init__(self, config):
        self.config = config

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, traceback):
        del exc_type, exc, traceback
        type(self).close_count += 1
        return False

    def run_signal_monitor(self, **kwargs):
        self.calls.append(("signal_monitor", kwargs))
        kwargs["on_status"](STATUS_RUNNING)
        kwargs["on_point"](
            MeasurementPoint(
                index=1,
                total_points=1,
                row={
                    "elapsed_s": 0.0,
                    "X_V": 1.0,
                    "Y_V": 2.0,
                    "R_V": 3.0,
                    "Theta_deg": 4.0,
                },
            )
        )
        kwargs["on_status"](STATUS_STOPPED)
        return [{}] * kwargs["plan"].n_points

    def run_trkr(self, **kwargs):
        self.calls.append(("trkr", kwargs))
        kwargs["on_status"](STATUS_RUNNING)
        kwargs["on_point"](
            MeasurementPoint(
                index=1,
                total_points=1,
                row={
                    "t_cor_ps": -50.0,
                    "X_V": 1.0,
                    "Y_V": 2.0,
                    "R_V": 3.0,
                    "Theta_deg": 4.0,
                },
            )
        )
        kwargs["on_status"](STATUS_STOPPED)
        return [{}] * len(kwargs["plan"].scan_points)

    def run_srkr(self, **kwargs):
        self.calls.append(("srkr", kwargs))
        kwargs["on_status"](STATUS_RUNNING)
        kwargs["on_point"](
            MeasurementPoint(
                index=1,
                total_points=1,
                row={
                    "y_cor_um": -1.0,
                    "X_V": 1.0,
                    "Y_V": 2.0,
                    "R_V": 3.0,
                    "Theta_deg": 4.0,
                },
            )
        )
        kwargs["on_status"](STATUS_STOPPED)
        return [{}] * len(kwargs["plan"].scan_points)

    def run_strkr(self, **kwargs):
        self.calls.append(("strkr", kwargs))
        kwargs["on_status"](STATUS_RUNNING)
        kwargs["on_point"](
            MeasurementPoint(
                index=1,
                total_points=1,
                row={
                    "t_cor_ps": 0.0,
                    "X_V": 1.0,
                    "Y_V": 2.0,
                    "R_V": 3.0,
                    "Theta_deg": 4.0,
                },
            )
        )
        kwargs["on_status"](STATUS_STOPPED)
        return [{}] * kwargs["plan"].total_points

    def run_srkr_2d(self, **kwargs):
        self.calls.append(("srkr_2d", kwargs))
        kwargs["on_status"](STATUS_RUNNING)
        kwargs["on_point"](
            MeasurementPoint(
                index=1,
                total_points=1,
                row={
                    "x_cor_um": 0.0,
                    "X_V": 1.0,
                    "Y_V": 2.0,
                    "R_V": 3.0,
                    "Theta_deg": 4.0,
                },
            )
        )
        kwargs["on_status"](STATUS_STOPPED)
        return [{}] * kwargs["plan"].total_points

    def move_delay_stage(self, value, *, coordinate="measurement"):
        self.calls.append(
            ("move_delay_stage", {"value": value, "coordinate": coordinate})
        )
        return Position(
            t_ps=value, delay_stage_mm=value / 10.0, delay_stage_pulse=int(value * 100)
        )

    def move_scanner(self, axis, value, *, coordinate="measurement"):
        self.calls.append(
            ("move_scanner", {"axis": axis, "value": value, "coordinate": coordinate})
        )
        if axis == "x":
            return Position(
                x_um=value, scanner_x_value=value / 100.0, scanner_x_unit="mm"
            )
        return Position(y_um=value, scanner_y_value=value / 100.0, scanner_y_unit="mm")


@pytest.fixture(autouse=True)
def reset_fake_experiment():
    FakeExperiment.calls = []
    FakeExperiment.close_count = 0


def config_file(tmp_path):
    path = tmp_path / "config.json"
    path.write_text(
        json.dumps(
            {
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
                            "sample_um_per_unit": 582.0,
                        },
                        "y": {
                            "controller": "CONEXAGAP",
                            "actuator": "AG-M100D",
                            "port": "fake",
                            "axis": "V",
                            "sample_um_per_unit": 412.0,
                        },
                    },
                },
                "measurements": {
                    "move_abs": {"zero": {"t_ps": -122.0, "x_um": 61.5, "y_um": 477.0}},
                    "signal_monitor": {"interval_s": 0.1, "n_points": 2},
                    "trkr": {
                        "scan": {"min": -50.0, "max": 0.0, "step": 50.0},
                        "output": {
                            "dir": str(tmp_path),
                            "filename": "trkr",
                            "auto_timestamp_suffix": False,
                        },
                    },
                    "srkr": {
                        "scan": {"axis": "x", "min": -1.0, "max": 1.0, "step": 1.0},
                        "output": {
                            "dir": str(tmp_path),
                            "filename": "srkr",
                            "auto_timestamp_suffix": False,
                        },
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
                        "output": {
                            "dir": str(tmp_path),
                            "filename": "strkr",
                            "auto_timestamp_suffix": False,
                        },
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
                        "output": {
                            "dir": str(tmp_path),
                            "filename": "srkr_2d",
                            "auto_timestamp_suffix": False,
                        },
                    },
                },
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


def test_cli_srkr_axis_override_uses_config_plan_and_prints_progress(
    monkeypatch, tmp_path, capsys
):
    FakeExperiment.calls = []
    monkeypatch.setattr(cli, "Experiment", FakeExperiment)

    assert (
        cli.main(["--config", str(config_file(tmp_path)), "srkr", "--axis", "y"]) == 0
    )

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

    assert (
        cli.main(
            [
                "--config",
                str(config_file(tmp_path)),
                "strkr",
                "--fast-axis",
                "t",
                "--slow-axis",
                "x",
            ]
        )
        == 0
    )

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

    assert (
        cli.main(
            [
                "--config",
                str(config_file(tmp_path)),
                "srkr-2d",
                "--fast-axis",
                "x",
                "--slow-axis",
                "y",
            ]
        )
        == 0
    )

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

    assert (
        cli.main(
            [
                "--config",
                str(config_file(tmp_path)),
                "move-abs",
                "--axis",
                "x",
                "--value",
                "62.5",
            ]
        )
        == 0
    )

    assert FakeExperiment.calls == [
        ("move_scanner", {"axis": "x", "value": 62.5, "coordinate": "measurement"})
    ]
    output = capsys.readouterr().out
    assert "[1/1] x_um=62.5 x_cor_um=1" in output
    assert "target=62.5 coordinate=measurement" in output


def test_cli_move_abs_delay_stage_uses_validated_coordinate_alias(
    monkeypatch, tmp_path, capsys
):
    monkeypatch.setattr(cli, "Experiment", FakeExperiment)

    assert (
        cli.main(
            [
                "--config",
                str(config_file(tmp_path)),
                "move-abs",
                "--axis",
                "t",
                "--coordinate",
                "ps",
                "--value",
                "2",
            ]
        )
        == cli.EXIT_SUCCESS
    )

    assert FakeExperiment.calls == [
        ("move_delay_stage", {"value": 2.0, "coordinate": "ps"})
    ]
    assert "coordinate=ps" in capsys.readouterr().out


def test_cli_closes_experiment_before_printing_success(monkeypatch, tmp_path, capsys):
    class CloseFailingExperiment(FakeExperiment):
        def __exit__(self, exc_type, exc, traceback):
            del exc_type, exc, traceback
            raise OSError("disconnect failed")

    monkeypatch.setattr(cli, "Experiment", CloseFailingExperiment)

    assert (
        cli.main(
            [
                "--config",
                str(config_file(tmp_path)),
                "move-abs",
                "--axis",
                "x",
                "--value",
                "1",
            ]
        )
        == cli.EXIT_FAILURE
    )

    captured = capsys.readouterr()
    assert "Saved ->" not in captured.out
    assert "[1/1]" not in captured.out
    assert "Error: disconnect failed" in captured.err


def test_cli_reports_incomplete_measurement_as_failure(monkeypatch, tmp_path, capsys):
    class PartialExperiment(FakeExperiment):
        def run_trkr(self, **kwargs):
            self.calls.append(("trkr", kwargs))
            return [{}]

    monkeypatch.setattr(cli, "Experiment", PartialExperiment)

    assert (
        cli.main(["--config", str(config_file(tmp_path)), "trkr"]) == cli.EXIT_FAILURE
    )

    captured = capsys.readouterr()
    assert "Saved ->" not in captured.out
    assert "expected 2 rows, received 1" in captured.err
    assert PartialExperiment.close_count == 1


def test_cli_interrupt_returns_130_and_reports_cleanup_note(
    monkeypatch, tmp_path, capsys
):
    class InterruptedExperiment(FakeExperiment):
        def run_signal_monitor(self, **_kwargs):
            raise KeyboardInterrupt

        def __exit__(self, exc_type, exc, traceback):
            del exc_type, traceback
            assert exc is not None
            exc.add_note("Experiment cleanup also failed: disconnect failed")
            return False

    monkeypatch.setattr(cli, "Experiment", InterruptedExperiment)

    assert (
        cli.main(["--config", str(config_file(tmp_path)), "signal-monitor"])
        == cli.EXIT_INTERRUPTED
    )

    captured = capsys.readouterr()
    assert "Saved ->" not in captured.out
    assert "Interrupted." in captured.err
    assert "disconnect failed" in captured.err


@pytest.mark.parametrize("value", ["nan", "inf", "-inf"])
def test_cli_parser_rejects_nonfinite_move_value_before_loading_config(
    monkeypatch, value
):
    monkeypatch.setattr(
        cli, "load_config", lambda _path: pytest.fail("config was loaded")
    )

    with pytest.raises(SystemExit) as caught:
        cli.main(["move-abs", "--axis", "x", "--value", value])

    assert caught.value.code == cli.EXIT_USAGE


def test_cli_parser_rejects_non_numeric_move_value_before_loading_config(monkeypatch):
    monkeypatch.setattr(
        cli, "load_config", lambda _path: pytest.fail("config was loaded")
    )

    with pytest.raises(SystemExit) as caught:
        cli.main(["move-abs", "--axis", "x", "--value", "not-a-number"])

    assert caught.value.code == cli.EXIT_USAGE


def test_cli_execute_rejects_unsupported_programmatic_command():
    with pytest.raises(ValueError, match="Unsupported command"):
        cli._execute({}, argparse.Namespace(command="unknown"))


def test_cli_usage_value_preserves_invalid_configuration_as_usage_error():
    def invalid_builder():
        raise ValueError("invalid scan configuration")

    with pytest.raises(cli.CLIUsageError, match="invalid scan configuration"):
        cli._usage_value(invalid_builder)


def test_cli_move_abs_fails_closed_if_coordinate_validation_returns_no_value(
    monkeypatch,
):
    monkeypatch.setattr(cli, "_validate_move_coordinate", lambda _axis, _value: None)

    with pytest.raises(cli.CLIUsageError, match="requires a coordinate"):
        cli._execute(
            {},
            argparse.Namespace(
                command="move-abs",
                axis="x",
                coordinate="measurement",
                value=1.0,
            ),
        )


def test_cli_rejects_invalid_axis_coordinate_before_experiment_creation(
    monkeypatch, tmp_path, capsys
):
    monkeypatch.setattr(
        cli,
        "Experiment",
        lambda _config: pytest.fail("Experiment was created for invalid input"),
    )

    assert (
        cli.main(
            [
                "--config",
                str(config_file(tmp_path)),
                "move-abs",
                "--axis",
                "t",
                "--coordinate",
                "deg",
                "--value",
                "1",
            ]
        )
        == cli.EXIT_USAGE
    )

    assert "Unsupported coordinate for axis t" in capsys.readouterr().err


def test_cli_missing_config_returns_usage_error(tmp_path, capsys):
    missing = tmp_path / "missing.json"

    assert cli.main(["--config", str(missing), "trkr"]) == cli.EXIT_USAGE

    captured = capsys.readouterr()
    assert captured.out == ""
    assert "Error:" in captured.err


def test_cli_runtime_value_error_is_not_misclassified_as_usage_error(
    monkeypatch, tmp_path, capsys
):
    class InvalidDeviceExperiment(FakeExperiment):
        def move_scanner(self, axis, value, *, coordinate="measurement"):
            del axis, value, coordinate
            raise ValueError("invalid device response")

    monkeypatch.setattr(cli, "Experiment", InvalidDeviceExperiment)

    assert (
        cli.main(
            [
                "--config",
                str(config_file(tmp_path)),
                "move-abs",
                "--axis",
                "x",
                "--value",
                "1",
            ]
        )
        == cli.EXIT_FAILURE
    )

    assert "invalid device response" in capsys.readouterr().err


def test_cli_module_entrypoint_propagates_main_exit_code(monkeypatch, capsys):
    monkeypatch.setattr(sys, "argv", ["python -m kohdalab.api.cli", "--help"])

    with pytest.warns(RuntimeWarning, match="found in sys.modules"):
        with pytest.raises(SystemExit) as caught:
            runpy.run_module("kohdalab.api.cli", run_name="__main__")

    assert caught.value.code == cli.EXIT_SUCCESS
    assert "Run KohdaLab measurements" in capsys.readouterr().out
