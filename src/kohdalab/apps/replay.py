from __future__ import annotations

import csv
import math
from dataclasses import dataclass
from pathlib import Path
from typing import Any


REPLAY_MEASUREMENTS = ("trkr", "srkr", "strkr", "srkr_2d")
SIGNAL_KEYS = ("X_V", "Y_V", "R_V", "Theta_deg")
TEXT_KEYS = {
    "timestamp",
    "measurement",
    "fast_axis",
    "slow_axis",
    "coordinate",
}


def _number(value: object, *, row_number: int, key: str) -> float:
    if isinstance(value, bool):
        raise ValueError(f"Row {row_number}: {key} must be a finite number.")
    try:
        result = float(value)  # type: ignore[arg-type]
    except (TypeError, ValueError) as exc:
        raise ValueError(f"Row {row_number}: {key} must be a finite number.") from exc
    if not math.isfinite(result):
        raise ValueError(f"Row {row_number}: {key} must be finite.")
    return result


def _row_axis_value(
    row: dict[str, Any],
    axis: str,
    *,
    row_number: int,
) -> float:
    axis = axis.strip().lower()
    keys = (
        ("t_cor_ps", "target_t_cor_ps", "t_ps")
        if axis == "t"
        else (f"{axis}_cor_um", f"target_{axis}_cor_um", f"{axis}_um")
    )
    for key in keys:
        if row.get(key) is not None:
            return _number(row[key], row_number=row_number, key=key)
    raise ValueError(
        f"Row {row_number}: missing replay position for axis {axis!r} "
        f"({', '.join(keys)})."
    )


def replay_axis_value(row: dict[str, Any], axis: str) -> float:
    """Return a corrected target/position suitable for replay plotting."""
    return _row_axis_value(row, axis, row_number=1)


def _target_key(axis: str) -> str:
    return "target_t_cor_ps" if axis == "t" else f"target_{axis}_cor_um"


def _normalize_target(row: dict[str, Any], axis: str, row_number: int) -> None:
    key = _target_key(axis)
    if row.get(key) is None:
        row[key] = _row_axis_value(row, axis, row_number=row_number)


def _infer_srkr_axis(row: dict[str, Any], row_number: int) -> str:
    explicit = str(row.get("fast_axis") or "").strip().lower()
    if explicit in {"x", "y"}:
        return explicit
    populated = [
        axis
        for axis in ("x", "y")
        if any(
            row.get(key) is not None
            for key in (
                f"{axis}_cor_um",
                f"target_{axis}_cor_um",
                f"{axis}_um",
            )
        )
    ]
    if len(populated) == 1:
        return populated[0]
    raise ValueError(f"Row {row_number}: cannot determine the SRKR scan axis.")


def _validate_axes(
    measurement: str,
    row: dict[str, Any],
    row_number: int,
) -> tuple[str, str | None]:
    if measurement == "trkr":
        _row_axis_value(row, "t", row_number=row_number)
        return "t", None
    if measurement == "srkr":
        axis = _infer_srkr_axis(row, row_number)
        row["fast_axis"] = axis
        _row_axis_value(row, axis, row_number=row_number)
        return axis, None

    fast_axis = str(row.get("fast_axis") or "").strip().lower()
    slow_axis = str(row.get("slow_axis") or "").strip().lower()
    allowed = (
        {("t", "x"), ("t", "y"), ("x", "t"), ("y", "t")}
        if measurement == "strkr"
        else {("x", "y"), ("y", "x")}
    )
    if (fast_axis, slow_axis) not in allowed:
        raise ValueError(
            f"Row {row_number}: invalid {measurement} axes {fast_axis!r}/{slow_axis!r}."
        )
    _row_axis_value(row, fast_axis, row_number=row_number)
    _row_axis_value(row, slow_axis, row_number=row_number)
    return fast_axis, slow_axis


def _parse_row(raw: dict[str, str | None], row_number: int) -> dict[str, Any]:
    row: dict[str, Any] = {}
    for key, raw_value in raw.items():
        if key is None:
            continue
        value = "" if raw_value is None else raw_value.strip()
        if not value:
            row[key] = None
        elif key in TEXT_KEYS:
            row[key] = value
        else:
            try:
                row[key] = _number(value, row_number=row_number, key=key)
            except ValueError:
                # Preserve non-schema annotation columns from real measurements.
                row[key] = value
    return row


@dataclass(frozen=True)
class ReplayDataset:
    path: Path
    measurement: str
    rows: tuple[dict[str, Any], ...]
    fast_axis: str
    slow_axis: str | None

    @property
    def point_count(self) -> int:
        return len(self.rows)


def load_replay_csv(
    path: str | Path,
    *,
    expected_measurement: str | None = None,
) -> ReplayDataset:
    csv_path = Path(path).expanduser().resolve()
    with csv_path.open(newline="", encoding="utf-8-sig") as stream:
        reader = csv.DictReader(stream)
        if reader.fieldnames is None:
            raise ValueError(f"CSV has no header: {csv_path}")
        rows = [
            _parse_row(raw, row_number)
            for row_number, raw in enumerate(reader, start=2)
        ]
    if not rows:
        raise ValueError(f"CSV has no measurement rows: {csv_path}")

    measurements = {str(row.get("measurement") or "").strip().lower() for row in rows}
    measurements.discard("")
    if len(measurements) != 1:
        raise ValueError(f"CSV must contain exactly one measurement type: {csv_path}")
    measurement = next(iter(measurements))
    if measurement not in REPLAY_MEASUREMENTS:
        raise ValueError(f"Unsupported replay measurement {measurement!r}: {csv_path}")
    if (
        expected_measurement is not None
        and measurement != expected_measurement.strip().lower()
    ):
        raise ValueError(
            f"Expected {expected_measurement!r}, found {measurement!r}: {csv_path}"
        )

    for row_number, row in enumerate(rows, start=2):
        if str(row.get("measurement") or "").strip().lower() != measurement:
            raise ValueError(
                f"Row {row_number}: inconsistent measurement type in {csv_path}."
            )
        row["measurement"] = measurement
        for key in SIGNAL_KEYS:
            if row.get(key) is None:
                raise ValueError(f"Row {row_number}: missing required signal {key}.")
            row[key] = _number(row[key], row_number=row_number, key=key)
        if float(row["R_V"]) < 0:
            raise ValueError(f"Row {row_number}: R_V must be non-negative.")

    first_axes = _validate_axes(measurement, rows[0], 2)
    _normalize_target(rows[0], first_axes[0], 2)
    if first_axes[1] is not None:
        _normalize_target(rows[0], first_axes[1], 2)
    for row_number, row in enumerate(rows[1:], start=3):
        axes = _validate_axes(measurement, row, row_number)
        if axes != first_axes:
            raise ValueError(
                f"Row {row_number}: scan axes changed from "
                f"{first_axes[0]}/{first_axes[1]} to {axes[0]}/{axes[1]}."
            )
        _normalize_target(row, axes[0], row_number)
        if axes[1] is not None:
            _normalize_target(row, axes[1], row_number)
    return ReplayDataset(
        path=csv_path,
        measurement=measurement,
        rows=tuple(rows),
        fast_axis=first_axes[0],
        slow_axis=first_axes[1],
    )


def discover_replay_datasets(
    data_dir: str | Path,
) -> dict[str, ReplayDataset]:
    directory = Path(data_dir).expanduser().resolve()
    if not directory.is_dir():
        raise FileNotFoundError(f"Replay data directory not found: {directory}")

    datasets: dict[str, ReplayDataset] = {}
    for path in sorted(directory.glob("*.csv")):
        dataset = load_replay_csv(path)
        if dataset.measurement in datasets:
            previous = datasets[dataset.measurement].path
            raise ValueError(
                f"Multiple {dataset.measurement} CSV files found: "
                f"{previous.name}, {path.name}"
            )
        datasets[dataset.measurement] = dataset

    if not datasets:
        raise ValueError(f"No measurement CSV files found: {directory}")
    return datasets
