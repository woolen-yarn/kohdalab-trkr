from __future__ import annotations

import pytest

from kohdalab.api.models import (
    LiveStatus,
    LockinSettings,
    LockinSignal,
    MeasurementPoint,
    Position,
)


def test_position_normalizes_valid_numeric_inputs():
    position = Position(
        t_ps="1.5",  # type: ignore[arg-type]
        delay_stage_pulse=2.0,  # type: ignore[arg-type]
        x_um=3,
        scanner_x_value="0.25",  # type: ignore[arg-type]
        scanner_x_unit=" MM ",
    )

    assert position.t_ps == 1.5
    assert position.delay_stage_pulse == 2
    assert position.x_um == 3.0
    assert position.scanner_x_value == 0.25
    assert position.scanner_x_unit == "mm"


@pytest.mark.parametrize("value", [1.5, float("nan"), float("inf"), True])
def test_position_rejects_invalid_pulse_values(value):
    with pytest.raises(ValueError):
        Position(delay_stage_pulse=value)


def test_position_requires_scanner_value_and_unit_together():
    with pytest.raises(ValueError, match="supplied together"):
        Position(scanner_x_value=1.0)
    with pytest.raises(ValueError, match="supplied together"):
        Position(scanner_y_unit="deg")
    with pytest.raises(ValueError, match="must be 'mm', 'deg'"):
        Position(scanner_x_value=1.0, scanner_x_unit="inch")


def test_position_from_rows_merges_aliases_only_when_values_agree():
    position = Position.from_rows(
        {"t_ps": 1.0, "stage_mm": 2.0, "stage_pulse": 3},
        {
            "delay_stage_mm": 2.0,
            "delay_stage_pulse": 3,
            "x_um": 4.0,
            "x_mm": 0.5,
            "x_scanner_mm": 0.5,
        },
    )

    assert position == Position(
        t_ps=1.0,
        delay_stage_mm=2.0,
        delay_stage_pulse=3,
        x_um=4.0,
        scanner_x_value=0.5,
        scanner_x_unit="mm",
    )

    with pytest.raises(ValueError, match="Conflicting values"):
        Position.from_rows({"stage_mm": 1.0, "delay_stage_mm": 2.0})
    with pytest.raises(ValueError, match="Conflicting scanner units"):
        Position.from_rows({"x_mm": 1.0, "x_deg": 2.0})


def test_position_from_rows_rejects_non_mapping_and_fractional_pulse():
    with pytest.raises(TypeError, match="mapping"):
        Position.from_rows([])  # type: ignore[arg-type]
    with pytest.raises(ValueError, match="integer"):
        Position.from_rows({"stage_pulse": 1.25})


@pytest.mark.parametrize(
    "kwargs",
    [
        {"x_v": float("nan"), "y_v": 0.0, "r_v": 0.0, "theta_deg": 0.0},
        {"x_v": 0.0, "y_v": 0.0, "r_v": -1.0, "theta_deg": 0.0},
        {"x_v": 0.0, "y_v": 0.0, "r_v": 0.0, "theta_deg": float("inf")},
    ],
)
def test_lockin_signal_rejects_nonfinite_or_negative_magnitude(kwargs):
    with pytest.raises(ValueError):
        LockinSignal(**kwargs)


def test_lockin_signal_mapping_is_normalized():
    signal = LockinSignal.from_mapping({"X": "1", "Y": 2, "R": 3, "Theta": "4"})

    assert signal == LockinSignal(x_v=1.0, y_v=2.0, r_v=3.0, theta_deg=4.0)


@pytest.mark.parametrize(
    "kwargs",
    [
        {"sensitivity_v": 0.0, "time_constant_s": 1.0},
        {"sensitivity_v": 1.0, "time_constant_s": 0.0},
        {"sensitivity_v": 1.0, "time_constant_s": 1.0, "ref_freq_hz": -1.0},
        {"sensitivity_v": float("nan"), "time_constant_s": 1.0},
    ],
)
def test_lockin_settings_enforces_physical_ranges(kwargs):
    with pytest.raises(ValueError):
        LockinSettings(**kwargs)


def test_measurement_point_validates_progress_values_and_copies_row():
    row = {"measurement": "signal_monitor", "X_V": 1.0}
    point = MeasurementPoint(index=1, total_points=1, row=row)
    row["X_V"] = 2.0

    assert point.row["X_V"] == 1.0
    with pytest.raises(ValueError, match="progress"):
        MeasurementPoint(index=0, total_points=1, row={})
    with pytest.raises(ValueError, match="finite"):
        MeasurementPoint(index=1, total_points=1, row={"X_V": float("nan")})


def test_live_status_validates_nested_payloads_and_copies_mappings():
    connected = {"lockin.main": True}
    status = LiveStatus(
        connected=connected,
        position=Position(t_ps=1.0),
        signal={"X": 1.0, "Y": 2.0, "R": 3.0, "Theta": 4.0},
        lockin_settings={
            "Sensitivity": 1e-3,
            "Time Constant": 1.0,
            "Ref. Freq": 1000.0,
        },
        lockin_overload={"overload": False},
    )
    connected["lockin.main"] = False

    assert status.connected == {"lockin.main": True}
    with pytest.raises(ValueError, match="boolean"):
        LiveStatus(connected={"lockin.main": 1})  # type: ignore[dict-item]
    with pytest.raises(KeyError):
        LiveStatus(signal={"X": 1.0})
    with pytest.raises(ValueError, match="overload must be boolean"):
        LiveStatus(lockin_overload={"overload": 1})


@pytest.mark.parametrize("value", [object(), "not-a-number"])
def test_position_rejects_values_that_cannot_be_converted_to_float(value):
    with pytest.raises(ValueError, match="Position.t_ps must be a finite number"):
        Position(t_ps=value)  # type: ignore[arg-type]


@pytest.mark.parametrize("row", [{"": 1}, {1: "value"}])
def test_position_from_rows_rejects_invalid_mapping_keys(row):
    with pytest.raises(ValueError, match="keys must be non-empty strings"):
        Position.from_rows(row)  # type: ignore[arg-type]


def test_position_from_rows_ignores_none_and_empty_rows():
    assert Position.from_rows(None, {}, None) == Position()


def test_lockin_signal_to_row_uses_output_schema_and_normalized_floats():
    signal = LockinSignal(x_v="1", y_v=2, r_v="3", theta_deg=4)  # type: ignore[arg-type]

    assert signal.to_row() == {
        "X_V": 1.0,
        "Y_V": 2.0,
        "R_V": 3.0,
        "Theta_deg": 4.0,
    }


def test_live_status_rejects_wrong_position_type():
    with pytest.raises(TypeError, match="position must be a Position"):
        LiveStatus(position={})  # type: ignore[arg-type]


@pytest.mark.parametrize("value", [True, -1, 1.5, "1"])
def test_live_status_rejects_invalid_overload_byte(value):
    with pytest.raises(
        ValueError, match="overload_byte must be a non-negative integer"
    ):
        LiveStatus(lockin_overload={"overload": False, "overload_byte": value})


def test_live_status_rejects_non_boolean_overload_detail():
    with pytest.raises(ValueError, match="raw_input_overload must be boolean"):
        LiveStatus(lockin_overload={"overload": False, "raw_input_overload": "false"})


def test_live_status_accepts_non_negative_overload_byte_and_boolean_details():
    status = LiveStatus(
        lockin_overload={
            "overload": True,
            "overload_byte": 3,
            "raw_input_overload": False,
        }
    )

    assert status.lockin_overload == {
        "overload": True,
        "overload_byte": 3,
        "raw_input_overload": False,
    }


def test_live_status_settings_without_optional_frequency_omits_it():
    status = LiveStatus(lockin_settings={"Sensitivity": "0.001", "Time Constant": "1"})

    assert status.lockin_settings == {
        "Sensitivity": 0.001,
        "Time Constant": 1.0,
    }


def test_measurement_point_rejects_empty_row_and_non_mapping():
    with pytest.raises(ValueError, match="must not be empty"):
        MeasurementPoint(index=1, total_points=1, row={})
    with pytest.raises(TypeError, match="row must be a mapping"):
        MeasurementPoint(index=1, total_points=1, row=[])  # type: ignore[arg-type]
