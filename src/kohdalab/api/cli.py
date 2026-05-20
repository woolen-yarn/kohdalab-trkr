from __future__ import annotations

import argparse
import sys
from pathlib import Path

from kohdalab.api.config import DEFAULT_CONFIG_PATH, load_config, measurement_output_settings, move_abs_zero, output_path
from kohdalab.api.experiment import Experiment
from kohdalab.api.notebook import format_move_abs_row, format_point, move_abs_row_from_position
from kohdalab.api.scan_plan import srkr_2d_plan_from_config, srkr_plan_from_config, strkr_plan_from_config, trkr_plan_from_config


def _status_printer(status: str) -> None:
    print(f"status: {status}", flush=True)


def _point_printer(axis_key: str):
    def print_point(point) -> None:
        print(format_point(point, axis_key=axis_key), flush=True)

    return print_point


def _measurement_output_path(config: dict, measurement_name: str, default_name: str) -> Path:
    return output_path(measurement_output_settings(config, measurement_name), default_name)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Run KohdaLab measurements from the command line.")
    parser.add_argument(
        "--config",
        type=Path,
        default=DEFAULT_CONFIG_PATH,
        help=f"Config JSON path. Default: {DEFAULT_CONFIG_PATH}",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("signal-monitor", help="Run signal monitor.")
    subparsers.add_parser("trkr", help="Run TRKR scan.")
    srkr_parser = subparsers.add_parser("srkr", help="Run SRKR scan.")
    srkr_parser.add_argument("--axis", choices=("x", "y"), default=None, help="Scan axis.")
    strkr_parser = subparsers.add_parser("strkr", help="Run STRKR 2D scan.")
    strkr_parser.add_argument("--fast-axis", choices=("t", "x", "y"), default=None)
    strkr_parser.add_argument("--slow-axis", choices=("t", "x", "y"), default=None)
    srkr_2d_parser = subparsers.add_parser("srkr-2d", help="Run SRKR 2D scan.")
    srkr_2d_parser.add_argument("--fast-axis", choices=("x", "y"), default=None)
    srkr_2d_parser.add_argument("--slow-axis", choices=("x", "y"), default=None)
    move_parser = subparsers.add_parser("move-abs", help="Move one axis to an absolute position.")
    move_parser.add_argument("--axis", choices=("t", "x", "y"), required=True)
    move_parser.add_argument(
        "--coordinate",
        default="measurement",
        help="measurement/interface/instrument, or t_ps/pos_mm/pulse/um/mm/deg.",
    )
    move_parser.add_argument("--value", type=float, required=True)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        config = load_config(args.config)
        experiment = Experiment(config)
        if args.command == "signal-monitor":
            out = _measurement_output_path(config, "signal_monitor", "signal_monitor_run.csv")
            print(f"Starting Signal Monitor -> {out}", flush=True)
            experiment.run_signal_monitor(
                output=out,
                on_status=_status_printer,
                on_point=_point_printer("elapsed_s"),
            )
            print(f"Saved -> {out}", flush=True)
            return 0
        if args.command == "trkr":
            out = _measurement_output_path(config, "trkr", "trkr_run.csv")
            plan = trkr_plan_from_config(config)
            print(f"Starting TRKR: {plan.summary} -> {out}", flush=True)
            experiment.run_trkr(
                plan=plan,
                output=out,
                on_status=_status_printer,
                on_point=_point_printer("t_cor_ps"),
            )
            print(f"Saved -> {out}", flush=True)
            return 0
        if args.command == "srkr":
            out = _measurement_output_path(config, "srkr", "srkr_run.csv")
            plan = srkr_plan_from_config(config, axis=args.axis)
            print(f"Starting SRKR: {plan.summary} -> {out}", flush=True)
            experiment.run_srkr(
                plan=plan,
                output=out,
                on_status=_status_printer,
                on_point=_point_printer(f"{plan.axis}_cor_um"),
            )
            print(f"Saved -> {out}", flush=True)
            return 0
        if args.command == "strkr":
            out = _measurement_output_path(config, "strkr", "strkr_run.csv")
            plan = strkr_plan_from_config(config, fast_axis=args.fast_axis, slow_axis=args.slow_axis)
            print(f"Starting {plan.summary} -> {out}", flush=True)
            experiment.run_strkr(
                plan=plan,
                output=out,
                on_status=_status_printer,
                on_point=_point_printer(f"{plan.fast_axis}_cor_ps" if plan.fast_axis == "t" else f"{plan.fast_axis}_cor_um"),
            )
            print(f"Saved -> {out}", flush=True)
            return 0
        if args.command == "srkr-2d":
            out = _measurement_output_path(config, "srkr_2d", "srkr_2d_run.csv")
            plan = srkr_2d_plan_from_config(config, fast_axis=args.fast_axis, slow_axis=args.slow_axis)
            print(f"Starting {plan.summary} -> {out}", flush=True)
            experiment.run_srkr_2d(
                plan=plan,
                output=out,
                on_status=_status_printer,
                on_point=_point_printer(f"{plan.fast_axis}_cor_um"),
            )
            print(f"Saved -> {out}", flush=True)
            return 0
        if args.command == "move-abs":
            if args.axis == "t":
                position = experiment.move_delay_stage(args.value, coordinate=args.coordinate)
            else:
                position = experiment.move_scanner(args.axis, args.value, coordinate=args.coordinate)
            row = move_abs_row_from_position(
                args.axis,
                position,
                target=args.value,
                coordinate=args.coordinate,
                zero=move_abs_zero(config),
            )
            print(format_move_abs_row(row), flush=True)
            return 0
    except KeyboardInterrupt:
        print("Interrupted.", file=sys.stderr, flush=True)
        return 130
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr, flush=True)
        return 1
    raise ValueError(f"Unsupported command: {args.command}")


if __name__ == "__main__":
    raise SystemExit(main())
