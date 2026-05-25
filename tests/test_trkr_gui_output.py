from __future__ import annotations

from pathlib import Path

from kohdalab.apps.trkr_gui_output import (
    build_output_path,
    normalize_output_settings,
    output_config_for_measurement,
    output_settings_from_fields,
)


def test_output_settings_from_fields_applies_defaults(tmp_path):
    settings = output_settings_from_fields(
        output_dir="",
        filename="",
        auto_timestamp_suffix=False,
        default_dir=tmp_path,
    )

    assert settings == {
        "output_dir": str(tmp_path),
        "filename": "trkr_run",
        "auto_timestamp_suffix": False,
    }


def test_normalize_output_settings_accepts_dir_alias(tmp_path):
    settings = normalize_output_settings(
        {"dir": str(tmp_path), "filename": "scan", "auto_timestamp_suffix": False}
    )

    assert settings == {
        "output_dir": str(tmp_path),
        "filename": "scan",
        "auto_timestamp_suffix": False,
    }


def test_build_output_path_adds_csv_suffix_without_timestamp(tmp_path):
    path = build_output_path(
        {
            "output_dir": str(tmp_path),
            "filename": "scan",
            "auto_timestamp_suffix": False,
        }
    )

    assert path == tmp_path / "scan.csv"


def test_build_output_path_adds_csv_suffix_to_non_csv_suffix_with_timestamp(tmp_path):
    path = build_output_path(
        {
            "output_dir": str(tmp_path),
            "filename": "scan.99",
            "auto_timestamp_suffix": True,
        }
    )

    assert path.parent == tmp_path
    assert path.suffix == ".csv"
    assert path.name.startswith("scan.99_")


def test_output_config_for_measurement_uses_api_output_shape(tmp_path):
    config = output_config_for_measurement(
        {
            "output_dir": Path(tmp_path),
            "filename": "signal.csv",
            "auto_timestamp_suffix": False,
        }
    )

    assert config == {
        "dir": str(tmp_path),
        "filename": "signal.csv",
        "auto_timestamp_suffix": False,
    }
