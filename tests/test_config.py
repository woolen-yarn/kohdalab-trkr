from __future__ import annotations

import pytest

from kohdalab.api.config import build_range_points, normalize_config, output_path


def test_normalize_config_adds_profile_measurement_defaults_and_legacy_scanner_scale():
    config = {
        "instruments": {
            "scanner": {
                "x": {
                    "controller": "CONEXCC",
                    "actuator": "TRA12CC",
                    "port": "COM5",
                    "axis": 1,
                    "pos_unit": "mm",
                    "sample_um_per_actuator_mm": 582.0,
                }
            }
        },
        "measurement": {
            "trkr": {
                "scan": {"min": 0.0, "max": 10.0, "step": 5.0},
            }
        },
    }

    normalized = normalize_config(config, source="custom_profile.json")

    assert normalized["profile"]["name"] == "custom_profile"
    assert "measurements" in normalized
    assert normalized["measurements"]["trkr"]["scan"] == {"min": 0.0, "max": 10.0, "step": 5.0}
    assert normalized["measurements"]["trkr"]["coordinate"] == "measurement"
    assert normalized["measurements"]["signal_monitor"]["n_points"] == 360
    assert normalized["instruments"]["scanner"]["x"]["sample_um_per_unit"] == 582.0


def test_build_range_points_handles_positive_and_negative_steps():
    assert build_range_points(0.0, 0.3, 0.1) == [0.0, 0.1, 0.2, 0.3]
    assert build_range_points(3.0, 1.0, -1.0) == [3.0, 2.0, 1.0]


def test_build_range_points_rejects_zero_or_empty_ranges():
    with pytest.raises(ValueError, match="non-zero"):
        build_range_points(0.0, 1.0, 0.0)

    with pytest.raises(ValueError, match="No scan points"):
        build_range_points(1.0, 0.0, 1.0)


def test_output_path_respects_legacy_output_settings(tmp_path):
    settings = {
        "output_dir": str(tmp_path),
        "filename": "run_name",
        "auto_timestamp_suffix": False,
    }

    assert output_path(settings, "default.csv") == tmp_path / "run_name.csv"


def test_output_path_adds_csv_suffix_when_filename_contains_dot(tmp_path):
    settings = {
        "output_dir": str(tmp_path),
        "filename": "run.99",
        "auto_timestamp_suffix": False,
    }

    assert output_path(settings, "default.csv") == tmp_path / "run.99.csv"
