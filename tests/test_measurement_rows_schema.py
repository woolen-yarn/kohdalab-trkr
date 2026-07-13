from __future__ import annotations

import math

import pytest

from kohdalab.api.measurement_rows import (
    MEASUREMENT_FIELDS,
    axis_target_key,
    fields_for_row,
    fields_for_rows,
    output_row,
    output_rows,
    scan2d_row,
    signal_row,
    signal_monitor_row,
    srkr_row,
    trkr_row,
)
from kohdalab.api.models import Position


SIGNAL = {"X": 1.0, "Y": 2.0, "R": 3.0, "Theta": 4.0}


def test_signal_monitor_row_order():
    row = signal_monitor_row(
        timestamp="t0", target_elapsed_s=1.0, elapsed_s=1.0, signal=SIGNAL
    )

    assert list(row) == MEASUREMENT_FIELDS
    assert fields_for_row(row) == MEASUREMENT_FIELDS
    assert row["measurement"] == "signal_monitor"
    assert row["fast_axis"] == "elapsed_s"
    assert row["target_elapsed_s"] == 1.0


def test_trkr_row_order():
    row = trkr_row(
        timestamp="t0",
        target_t_cor_ps=-10.0,
        t_cor_ps=-10.0,
        t_ps=112.0,
        signal=SIGNAL,
        coordinate="measurement",
        delay_stage_mm=1.2,
        delay_stage_pulse=123,
    )

    assert list(row) == MEASUREMENT_FIELDS
    assert fields_for_row(row) == MEASUREMENT_FIELDS
    assert row["measurement"] == "trkr"
    assert row["fast_axis"] == "t"
    assert row["target_t_cor_ps"] == -10.0


def test_srkr_row_uses_fast_axis_target():
    row = srkr_row(
        timestamp="t0",
        fast_axis="x",
        target_cor_um=5.0,
        cor_um=5.0,
        position_um=66.0,
        signal=SIGNAL,
        coordinate="measurement",
        scanner_unit="mm",
        scanner_value=0.1,
    )

    assert list(row) == MEASUREMENT_FIELDS
    assert fields_for_row(row) == MEASUREMENT_FIELDS
    assert row["measurement"] == "srkr"
    assert row["fast_axis"] == "x"
    assert row["target_x_cor_um"] == 5.0
    assert row["target_y_cor_um"] is None


def test_scan2d_row_carries_targets_for_both_axes():
    row = scan2d_row(
        timestamp="t0",
        measurement="strkr",
        fast_axis="t",
        slow_axis="x",
        targets={"t": -10.0, "x": 5.0},
        position=Position(
            t_ps=90.0, x_um=15.0, y_um=20.0, scanner_x_value=0.1, scanner_x_unit="mm"
        ),
        zero={"t_ps": 100.0, "x_um": 10.0, "y_um": 20.0},
        signal=SIGNAL,
    )

    assert list(row) == MEASUREMENT_FIELDS
    assert row["measurement"] == "strkr"
    assert row["fast_axis"] == "t"
    assert row["slow_axis"] == "x"
    assert row["target_t_cor_ps"] == -10.0
    assert row["target_x_cor_um"] == 5.0
    assert row["t_cor_ps"] == -10.0
    assert row["x_cor_um"] == 5.0


def test_fields_for_rows_uses_unified_columns():
    signal = signal_monitor_row(
        timestamp="t0", target_elapsed_s=0.0, elapsed_s=0.0, signal=SIGNAL
    )
    srkr = srkr_row(
        timestamp="t1",
        fast_axis="y",
        target_cor_um=3.0,
        cor_um=3.0,
        position_um=4.0,
        signal=SIGNAL,
        coordinate="measurement",
        scanner_unit="deg",
        scanner_value=0.02,
    )

    assert fields_for_rows([signal, srkr]) == MEASUREMENT_FIELDS


def test_output_row_formats_only_signal_voltages_as_scientific_notation():
    row = signal_monitor_row(
        timestamp="t0", target_elapsed_s=0.0, elapsed_s=0.0, signal=SIGNAL
    )

    output = output_row(row)

    assert output["X_V"] == "1.000000e+00"
    assert output["Y_V"] == "2.000000e+00"
    assert output["R_V"] == "3.000000e+00"
    assert output["Theta_deg"] == 4.0


def test_empty_and_custom_rows_preserve_first_seen_column_order():
    custom_rows = [
        {"sample": 1, "shared": "first"},
        {"shared": "second", "quality": "ok"},
    ]

    assert fields_for_rows([]) == []
    assert fields_for_row(custom_rows[0]) == ["sample", "shared"]
    assert fields_for_rows(custom_rows) == ["sample", "shared", "quality"]


def test_known_schema_keeps_extra_columns_after_canonical_columns():
    row = signal_monitor_row(
        timestamp="t0", target_elapsed_s=0.0, elapsed_s=0.0, signal=SIGNAL
    )
    row["quality"] = "ok"

    fields = fields_for_rows([row])

    assert fields[: len(MEASUREMENT_FIELDS)] == MEASUREMENT_FIELDS
    assert fields[-1] == "quality"


def test_output_rows_preserves_non_numeric_voltage_types_and_empty_input():
    rows = [{"X_V": True, "Y_V": "2.0", "R_V": 3, "measurement": "custom"}]

    assert output_rows([]) == []
    assert output_rows(rows) == [
        {
            "X_V": True,
            "Y_V": "2.0",
            "R_V": "3.000000e+00",
            "measurement": "custom",
        }
    ]


@pytest.mark.parametrize(
    ("signal", "error", "message"),
    [
        ({"X": 1.0, "Y": 2.0, "Theta": 4.0}, KeyError, "R"),
        ({"X": True, "Y": 2.0, "R": 3.0, "Theta": 4.0}, ValueError, "boolean"),
        (
            {"X": math.nan, "Y": 2.0, "R": 3.0, "Theta": 4.0},
            ValueError,
            "finite",
        ),
    ],
)
def test_signal_row_rejects_schema_and_value_type_mismatches(signal, error, message):
    with pytest.raises(error, match=message):
        signal_row(signal)


def test_axis_target_key_normalizes_elapsed_axis_and_rejects_unknown_axis():
    assert axis_target_key(" ELAPSED_S ") == "target_elapsed_s"

    with pytest.raises(ValueError, match="Unsupported axis: z"):
        axis_target_key(" Z ")
