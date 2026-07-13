from __future__ import annotations

import argparse
import math
import sys
from pathlib import Path
from typing import Any, Callable, TypeVar

from kohdalab.api.config import (
    DEFAULT_CONFIG_PATH,
    load_config,
    measurement_output_settings,
    move_abs_zero,
    output_path,
)
from kohdalab.api.experiment import Experiment
from kohdalab.api.notebook import (
    format_move_abs_row,
    format_point,
    move_abs_row_from_position,
)
from kohdalab.api.scan_plan import (
    signal_monitor_plan_from_config,
    srkr_2d_plan_from_config,
    srkr_plan_from_config,
    strkr_plan_from_config,
    trkr_plan_from_config,
)


EXIT_SUCCESS = 0
EXIT_FAILURE = 1
EXIT_USAGE = 2
EXIT_INTERRUPTED = 130
T = TypeVar("T")


class CLIUsageError(ValueError):
    """Invalid configuration or command combination supplied to the CLI."""


def _usage_value(builder: Callable[[], T]) -> T:
    try:
        return builder()
    except ValueError as error:
        raise CLIUsageError(str(error)) from error


def _status_printer(status: str) -> None:
    print(f"status: {status}", flush=True)


def _point_printer(axis_key: str) -> Callable[[Any], None]:
    def print_point(point: Any) -> None:
        print(format_point(point, axis_key=axis_key), flush=True)

    return print_point


def _measurement_output_path(
    config: dict[str, Any], measurement_name: str, default_name: str
) -> Path:
    return output_path(
        measurement_output_settings(config, measurement_name), default_name
    )


def _finite_float_arg(value: str) -> float:
    try:
        result = float(value)
    except ValueError as exc:
        raise argparse.ArgumentTypeError("must be a number") from exc
    if not math.isfinite(result):
        raise argparse.ArgumentTypeError("must be finite")
    return result


def _validate_move_coordinate(axis: str, coordinate: str) -> str:
    normalized = coordinate.strip().lower()
    allowed = (
        {
            "measurement",
            "t_ps",
            "ps",
            "interface",
            "pos_mm",
            "mm",
            "instrument",
            "pulse",
            "device",
        }
        if axis == "t"
        else {
            "measurement",
            "um",
            "sample_um",
            "interface",
            "control",
            "instrument",
            "device",
            "mm",
            "deg",
            "pos_mm",
            "pos_deg",
        }
    )
    if normalized not in allowed:
        available = ", ".join(sorted(allowed))
        raise CLIUsageError(
            f"Unsupported coordinate for axis {axis}: {coordinate!r}. Use one of: {available}."
        )
    return normalized


def _require_complete(rows: list[dict[str, Any]], expected: int, name: str) -> None:
    if len(rows) != expected:
        raise RuntimeError(
            f"{name} stopped before completion: expected {expected} rows, received {len(rows)}."
        )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Run KohdaLab measurements from the command line."
    )
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
    srkr_parser.add_argument(
        "--axis", choices=("x", "y"), default=None, help="Scan axis."
    )
    strkr_parser = subparsers.add_parser("strkr", help="Run STRKR 2D scan.")
    strkr_parser.add_argument("--fast-axis", choices=("t", "x", "y"), default=None)
    strkr_parser.add_argument("--slow-axis", choices=("t", "x", "y"), default=None)
    srkr_2d_parser = subparsers.add_parser("srkr-2d", help="Run SRKR 2D scan.")
    srkr_2d_parser.add_argument("--fast-axis", choices=("x", "y"), default=None)
    srkr_2d_parser.add_argument("--slow-axis", choices=("x", "y"), default=None)
    move_parser = subparsers.add_parser(
        "move-abs", help="Move one axis to an absolute position."
    )
    move_parser.add_argument("--axis", choices=("t", "x", "y"), required=True)
    move_parser.add_argument(
        "--coordinate",
        default="measurement",
        help="measurement/interface/instrument, or t_ps/pos_mm/pulse/um/mm/deg.",
    )
    move_parser.add_argument("--value", type=_finite_float_arg, required=True)
    return parser


def _execute(config: dict[str, Any], args: argparse.Namespace) -> str:
    move_coordinate = (
        _validate_move_coordinate(args.axis, args.coordinate)
        if args.command == "move-abs"
        else None
    )
    with Experiment(config) as experiment:
        if args.command == "signal-monitor":
            out = _measurement_output_path(
                config, "signal_monitor", "signal_monitor_run.csv"
            )
            signal_plan = _usage_value(lambda: signal_monitor_plan_from_config(config))
            print(f"Starting Signal Monitor -> {out}", flush=True)
            rows = experiment.run_signal_monitor(
                plan=signal_plan,
                output=out,
                on_status=_status_printer,
                on_point=_point_printer("elapsed_s"),
            )
            _require_complete(rows, signal_plan.n_points, "Signal Monitor")
            return f"Saved -> {out}"
        if args.command == "trkr":
            out = _measurement_output_path(config, "trkr", "trkr_run.csv")
            trkr_plan = _usage_value(lambda: trkr_plan_from_config(config))
            print(f"Starting TRKR: {trkr_plan.summary} -> {out}", flush=True)
            rows = experiment.run_trkr(
                plan=trkr_plan,
                output=out,
                on_status=_status_printer,
                on_point=_point_printer("t_cor_ps"),
            )
            _require_complete(rows, len(trkr_plan.scan_points), "TRKR")
            return f"Saved -> {out}"
        if args.command == "srkr":
            out = _measurement_output_path(config, "srkr", "srkr_run.csv")
            srkr_plan = _usage_value(
                lambda: srkr_plan_from_config(config, axis=args.axis)
            )
            print(f"Starting SRKR: {srkr_plan.summary} -> {out}", flush=True)
            rows = experiment.run_srkr(
                plan=srkr_plan,
                output=out,
                on_status=_status_printer,
                on_point=_point_printer(f"{srkr_plan.axis}_cor_um"),
            )
            _require_complete(rows, len(srkr_plan.scan_points), "SRKR")
            return f"Saved -> {out}"
        if args.command == "strkr":
            out = _measurement_output_path(config, "strkr", "strkr_run.csv")
            strkr_plan = _usage_value(
                lambda: strkr_plan_from_config(
                    config, fast_axis=args.fast_axis, slow_axis=args.slow_axis
                )
            )
            print(f"Starting {strkr_plan.summary} -> {out}", flush=True)
            rows = experiment.run_strkr(
                plan=strkr_plan,
                output=out,
                on_status=_status_printer,
                on_point=_point_printer(
                    f"{strkr_plan.fast_axis}_cor_ps"
                    if strkr_plan.fast_axis == "t"
                    else f"{strkr_plan.fast_axis}_cor_um"
                ),
            )
            _require_complete(rows, strkr_plan.total_points, "STRKR")
            return f"Saved -> {out}"
        if args.command == "srkr-2d":
            out = _measurement_output_path(config, "srkr_2d", "srkr_2d_run.csv")
            srkr_2d_plan = _usage_value(
                lambda: srkr_2d_plan_from_config(
                    config, fast_axis=args.fast_axis, slow_axis=args.slow_axis
                )
            )
            print(f"Starting {srkr_2d_plan.summary} -> {out}", flush=True)
            rows = experiment.run_srkr_2d(
                plan=srkr_2d_plan,
                output=out,
                on_status=_status_printer,
                on_point=_point_printer(f"{srkr_2d_plan.fast_axis}_cor_um"),
            )
            _require_complete(rows, srkr_2d_plan.total_points, "SRKR 2D")
            return f"Saved -> {out}"
        if args.command == "move-abs":
            if move_coordinate is None:
                raise CLIUsageError("move-abs requires a coordinate system.")
            if args.axis == "t":
                position = experiment.move_delay_stage(
                    args.value, coordinate=move_coordinate
                )
            else:
                position = experiment.move_scanner(
                    args.axis, args.value, coordinate=move_coordinate
                )
            row = move_abs_row_from_position(
                args.axis,
                position,
                target=args.value,
                coordinate=move_coordinate,
                zero=move_abs_zero(config),
            )
            return format_move_abs_row(row)
        raise ValueError(f"Unsupported command: {args.command}")


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        config = _usage_value(lambda: load_config(args.config))
        final_message = _execute(config, args)
        print(final_message, flush=True)
        return EXIT_SUCCESS
    except KeyboardInterrupt as error:
        print("Interrupted.", file=sys.stderr, flush=True)
        for note in getattr(error, "__notes__", ()):
            print(f"Cleanup error: {note}", file=sys.stderr, flush=True)
        return EXIT_INTERRUPTED
    except (FileNotFoundError, CLIUsageError) as error:
        print(f"Error: {error}", file=sys.stderr, flush=True)
        return EXIT_USAGE
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr, flush=True)
        return EXIT_FAILURE


if __name__ == "__main__":
    raise SystemExit(main())
