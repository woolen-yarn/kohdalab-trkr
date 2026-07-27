from __future__ import annotations

import csv
from pathlib import Path

import pytest

from kohdalab.apps.replay import (
    _number,
    _parse_row,
    discover_replay_datasets,
    load_replay_csv,
    replay_axis_value,
)


SIGNALS = {
    "X_V": "1.0",
    "Y_V": "2.0",
    "R_V": "3.0",
    "Theta_deg": "4.0",
}


def write_csv(path: Path, rows: list[dict[str, object]]) -> None:
    fields: list[str] = []
    for row in rows:
        for key in row:
            if key not in fields:
                fields.append(key)
    with path.open("w", newline="", encoding="utf-8") as stream:
        writer = csv.DictWriter(stream, fieldnames=fields)
        writer.writeheader()
        writer.writerows(rows)


def test_loads_trkr_and_ignores_original_timestamp(tmp_path):
    path = tmp_path / "recorded.csv"
    write_csv(
        path,
        [
            {
                "timestamp": "2026-01-01T00:00:00Z",
                "measurement": "trkr",
                "t_cor_ps": "-10",
                **SIGNALS,
            },
            {
                "timestamp": "2026-01-01T00:00:01Z",
                "measurement": "trkr",
                "t_cor_ps": "0",
                **SIGNALS,
            },
        ],
    )

    dataset = load_replay_csv(path)

    assert dataset.measurement == "trkr"
    assert dataset.fast_axis == "t"
    assert dataset.slow_axis is None
    assert dataset.point_count == 2
    assert dataset.rows[0]["timestamp"] == "2026-01-01T00:00:00Z"
    assert replay_axis_value(dataset.rows[0], "t") == -10.0


@pytest.mark.parametrize(
    ("measurement", "axes", "position_values"),
    [
        ("srkr", {"fast_axis": "x"}, {"x_cor_um": 1.0}),
        (
            "strkr",
            {"fast_axis": "t", "slow_axis": "x"},
            {"t_cor_ps": 1.0, "x_cor_um": 2.0},
        ),
        (
            "srkr_2d",
            {"fast_axis": "x", "slow_axis": "y"},
            {"x_cor_um": 1.0, "y_cor_um": 2.0},
        ),
    ],
)
def test_loads_supported_scan_modes(tmp_path, measurement, axes, position_values):
    path = tmp_path / f"{measurement}.csv"
    write_csv(
        path,
        [{"measurement": measurement, **axes, **position_values, **SIGNALS}],
    )

    dataset = load_replay_csv(path)

    assert dataset.measurement == measurement
    assert dataset.fast_axis == axes["fast_axis"]
    assert dataset.slow_axis == axes.get("slow_axis")


def test_srkr_axis_can_be_inferred_from_position_column(tmp_path):
    path = tmp_path / "srkr.csv"
    write_csv(
        path,
        [{"measurement": "srkr", "y_cor_um": 5.0, **SIGNALS}],
    )

    dataset = load_replay_csv(path)

    assert dataset.fast_axis == "y"
    assert dataset.rows[0]["fast_axis"] == "y"


@pytest.mark.parametrize(
    ("row", "message"),
    [
        (
            {"measurement": "trkr", "t_cor_ps": 1.0, "X_V": 1.0},
            "missing required signal Y_V",
        ),
        (
            {
                "measurement": "srkr_2d",
                "fast_axis": "x",
                "slow_axis": "x",
                "x_cor_um": 1.0,
                **SIGNALS,
            },
            "invalid srkr_2d axes",
        ),
        (
            {
                "measurement": "trkr",
                "t_cor_ps": 1.0,
                **SIGNALS,
                "R_V": "-1",
            },
            "R_V must be non-negative",
        ),
    ],
)
def test_rejects_invalid_replay_rows(tmp_path, row, message):
    path = tmp_path / "invalid.csv"
    write_csv(path, [row])

    with pytest.raises(ValueError, match=message):
        load_replay_csv(path)


def test_discovers_one_csv_per_measurement(tmp_path):
    write_csv(
        tmp_path / "a.csv",
        [{"measurement": "trkr", "t_cor_ps": 1.0, **SIGNALS}],
    )
    write_csv(
        tmp_path / "b.csv",
        [
            {
                "measurement": "srkr_2d",
                "fast_axis": "x",
                "slow_axis": "y",
                "x_cor_um": 1.0,
                "y_cor_um": 2.0,
                **SIGNALS,
            }
        ],
    )

    datasets = discover_replay_datasets(tmp_path)

    assert set(datasets) == {"trkr", "srkr_2d"}


def test_rejects_duplicate_measurement_files(tmp_path):
    row = {"measurement": "trkr", "t_cor_ps": 1.0, **SIGNALS}
    write_csv(tmp_path / "one.csv", [row])
    write_csv(tmp_path / "two.csv", [row])

    with pytest.raises(ValueError, match="Multiple trkr CSV files"):
        discover_replay_datasets(tmp_path)


@pytest.mark.parametrize("value", [True, "not-a-number", float("nan")])
def test_number_rejects_invalid_values(value):
    with pytest.raises(ValueError, match="must be"):
        _number(value, row_number=2, key="value")


def test_axis_value_uses_target_and_raw_fallbacks():
    assert replay_axis_value({"target_t_cor_ps": 2.0}, "t") == 2.0
    assert replay_axis_value({"t_ps": 3.0}, "t") == 3.0
    assert replay_axis_value({"target_x_cor_um": 4.0}, "x") == 4.0
    assert replay_axis_value({"x_um": 5.0}, "x") == 5.0

    with pytest.raises(ValueError, match="missing replay position"):
        replay_axis_value({}, "y")


def test_existing_target_value_is_preserved(tmp_path):
    path = tmp_path / "target.csv"
    write_csv(
        path,
        [
            {
                "measurement": "trkr",
                "target_t_cor_ps": 7.0,
                "t_cor_ps": 8.0,
                **SIGNALS,
            }
        ],
    )

    loaded = load_replay_csv(path)

    assert loaded.rows[0]["target_t_cor_ps"] == 7.0


def test_parse_row_handles_extra_blank_text_and_annotation_fields():
    row = _parse_row(
        {
            None: "extra",  # type: ignore[dict-item]
            "measurement": " TRKR ",
            "coordinate": None,
            "quality": "good",
            "value": "1.5",
        },
        2,
    )

    assert row == {
        "measurement": "TRKR",
        "coordinate": None,
        "quality": "good",
        "value": 1.5,
    }


def test_rejects_srkr_with_ambiguous_axis(tmp_path):
    path = tmp_path / "ambiguous.csv"
    write_csv(
        path,
        [
            {
                "measurement": "srkr",
                "x_cor_um": 1.0,
                "y_cor_um": 2.0,
                **SIGNALS,
            }
        ],
    )

    with pytest.raises(ValueError, match="cannot determine"):
        load_replay_csv(path)


def test_rejects_headerless_empty_and_unsupported_csvs(tmp_path):
    empty = tmp_path / "empty.csv"
    empty.write_text("", encoding="utf-8")
    with pytest.raises(ValueError, match="no header"):
        load_replay_csv(empty)

    header = tmp_path / "header.csv"
    header.write_text("measurement,X_V\n", encoding="utf-8")
    with pytest.raises(ValueError, match="no measurement rows"):
        load_replay_csv(header)

    unsupported = tmp_path / "unsupported.csv"
    write_csv(
        unsupported,
        [{"measurement": "signal_monitor", "elapsed_s": 1.0, **SIGNALS}],
    )
    with pytest.raises(ValueError, match="Unsupported replay measurement"):
        load_replay_csv(unsupported)


def test_rejects_missing_mixed_and_unexpected_measurement_types(tmp_path):
    missing = tmp_path / "missing.csv"
    write_csv(missing, [{"t_cor_ps": 1.0, **SIGNALS}])
    with pytest.raises(ValueError, match="exactly one measurement"):
        load_replay_csv(missing)

    mixed = tmp_path / "mixed.csv"
    write_csv(
        mixed,
        [
            {"measurement": "trkr", "t_cor_ps": 1.0, **SIGNALS},
            {"measurement": "srkr", "x_cor_um": 1.0, **SIGNALS},
        ],
    )
    with pytest.raises(ValueError, match="exactly one measurement"):
        load_replay_csv(mixed)

    expected = tmp_path / "expected.csv"
    write_csv(expected, [{"measurement": "trkr", "t_cor_ps": 1.0, **SIGNALS}])
    with pytest.raises(ValueError, match="Expected"):
        load_replay_csv(expected, expected_measurement="srkr")


def test_rejects_missing_measurement_on_later_row(tmp_path):
    path = tmp_path / "missing-later.csv"
    write_csv(
        path,
        [
            {"measurement": "trkr", "t_cor_ps": 1.0, **SIGNALS},
            {"measurement": "", "t_cor_ps": 2.0, **SIGNALS},
        ],
    )

    with pytest.raises(ValueError, match="inconsistent measurement"):
        load_replay_csv(path)


def test_rejects_scan_axis_changes_between_rows(tmp_path):
    path = tmp_path / "changed-axis.csv"
    write_csv(
        path,
        [
            {
                "measurement": "srkr",
                "fast_axis": "x",
                "x_cor_um": 1.0,
                "y_cor_um": "",
                **SIGNALS,
            },
            {
                "measurement": "srkr",
                "fast_axis": "y",
                "x_cor_um": "",
                "y_cor_um": 2.0,
                **SIGNALS,
            },
        ],
    )

    with pytest.raises(ValueError, match="scan axes changed"):
        load_replay_csv(path)


def test_normalizes_both_axes_on_later_2d_rows(tmp_path):
    path = tmp_path / "two-dimensional.csv"
    write_csv(
        path,
        [
            {
                "measurement": "srkr_2d",
                "fast_axis": "x",
                "slow_axis": "y",
                "x_cor_um": 1.0,
                "y_cor_um": 2.0,
                **SIGNALS,
            },
            {
                "measurement": "srkr_2d",
                "fast_axis": "x",
                "slow_axis": "y",
                "x_cor_um": 3.0,
                "y_cor_um": 4.0,
                **SIGNALS,
            },
        ],
    )

    loaded = load_replay_csv(path)

    assert loaded.rows[1]["target_x_cor_um"] == 3.0
    assert loaded.rows[1]["target_y_cor_um"] == 4.0


def test_rejects_missing_or_empty_data_directory(tmp_path):
    with pytest.raises(FileNotFoundError, match="not found"):
        discover_replay_datasets(tmp_path / "missing")

    with pytest.raises(ValueError, match="No measurement CSV"):
        discover_replay_datasets(tmp_path)
