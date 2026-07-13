from __future__ import annotations

import json
import math
import re

import pytest

from kohdalab.api.config import (
    CONFIG_PATH_ENV,
    CONFIG_STATE_DIR_ENV,
    DEFAULT_CONFIG_PATH_ENV,
    DEFAULT_CONFIG_PATH,
    LAST_CONFIG_STATE_PATH_ENV,
    MAX_SCAN_POINTS_PER_AXIS,
    build_range_points,
    config_state_dir,
    delay_stage_config_for,
    instrument_config,
    instrument_key,
    last_config_state_path,
    load_config,
    lockin_config_for,
    measurement_output_settings,
    measurement_settings,
    move_abs_settings,
    move_abs_zero,
    normalize_config,
    normalize_delay_stage_name,
    output_path,
    read_last_config_path,
    resolve_config_path,
    save_config,
    scan_settings,
    scanner_config_for,
    validate_config,
    with_csv_suffix,
    write_last_config_path,
    zero_for,
)


def test_packaged_default_config_exists_and_loads():
    assert DEFAULT_CONFIG_PATH.is_file()
    assert load_config()["profile"]["name"] == "default"


def test_packaged_default_config_matches_repository_sample():
    repository_sample = DEFAULT_CONFIG_PATH.parents[3] / "config" / "default.json"

    assert json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8")) == json.loads(
        repository_sample.read_text(encoding="utf-8")
    )


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
    assert normalized["measurements"]["trkr"]["scan"] == {
        "min": 0.0,
        "max": 10.0,
        "step": 5.0,
    }
    assert normalized["measurements"]["trkr"]["coordinate"] == "measurement"
    assert normalized["measurements"]["signal_monitor"]["n_points"] == 360
    assert normalized["instruments"]["scanner"]["x"]["sample_um_per_unit"] == 582.0


def test_normalize_config_canonicalizes_hardware_catalog_names():
    config = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    config["instruments"]["lockin"]["main"]["model"] = " sr7265 "
    config["instruments"]["delay_stage"]["t"].update(
        {"controller": " shot302gs ", "stage": "sgsp_46_500"}
    )
    config["instruments"]["scanner"]["x"].update(
        {"controller": " conexagap ", "actuator": "ag_m100d"}
    )

    normalized = normalize_config(config)

    assert normalized["instruments"]["lockin"]["main"]["model"] == "SR7265"
    assert normalized["instruments"]["delay_stage"]["t"]["controller"] == "SHOT302GS"
    assert normalized["instruments"]["delay_stage"]["t"]["stage"] == "SGSP46-500"
    assert normalized["instruments"]["scanner"]["x"]["controller"] == "CONEXAGAP"
    assert normalized["instruments"]["scanner"]["x"]["actuator"] == "AG-M100D"


def test_build_range_points_handles_positive_and_negative_steps():
    assert build_range_points(0.0, 0.3, 0.1) == [0.0, 0.1, 0.2, 0.3]
    assert build_range_points(3.0, 1.0, -1.0) == [3.0, 2.0, 1.0]


def test_build_range_points_rejects_zero_or_empty_ranges():
    with pytest.raises(ValueError, match="non-zero"):
        build_range_points(0.0, 1.0, 0.0)

    with pytest.raises(ValueError, match="No scan points"):
        build_range_points(1.0, 0.0, 1.0)


@pytest.mark.parametrize("value", [math.inf, -math.inf, math.nan])
def test_build_range_points_rejects_non_finite_values(value):
    with pytest.raises(ValueError, match="finite"):
        build_range_points(0.0, 1.0, value)


def test_build_range_points_rejects_excessive_point_count():
    with pytest.raises(ValueError, match="maximum"):
        build_range_points(0.0, float(MAX_SCAN_POINTS_PER_AXIS), 1.0)


def test_load_config_validates_by_default(tmp_path):
    invalid = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    invalid["measurements"]["signal_monitor"]["n_points"] = 0
    path = tmp_path / "invalid.json"
    path.write_text(json.dumps(invalid), encoding="utf-8")

    with pytest.raises(ValueError, match="n_points"):
        load_config(path)


@pytest.mark.parametrize(
    ("section", "value", "message"),
    [
        ("instruments", [], "'instruments' object"),
        ("measurements", [], "'measurements' object"),
    ],
)
def test_validate_config_rejects_non_object_top_level_sections(section, value, message):
    invalid = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    invalid[section] = value

    with pytest.raises(ValueError, match=message):
        validate_config(invalid)


@pytest.mark.parametrize(
    ("kind", "key"),
    [("lockin", "main"), ("delay_stage", "t"), ("scanner", "x")],
)
def test_validate_config_rejects_non_object_device_entries(kind, key):
    invalid = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    invalid["instruments"][kind][key] = []

    with pytest.raises(
        ValueError, match=rf"instruments\.{kind}\.{key} must be an object"
    ):
        validate_config(invalid)


@pytest.mark.parametrize(
    ("measurement", "field", "value", "message"),
    [
        ("signal_monitor", "n_points", True, "n_points must be an integer"),
        ("signal_monitor", "n_points", 1.5, "n_points must be an integer"),
        ("trkr", "scan", [], "scan must be an object"),
        ("srkr", "wait_s", math.nan, "wait_s must be finite"),
        ("strkr", "output", [], "output must be an object"),
    ],
)
def test_load_config_rejects_ambiguous_or_broken_measurement_values(
    measurement, field, value, message, tmp_path
):
    invalid = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    invalid["measurements"][measurement][field] = value
    path = tmp_path / "invalid-measurement.json"
    path.write_text(json.dumps(invalid), encoding="utf-8")

    with pytest.raises(ValueError, match=message):
        load_config(path)


@pytest.mark.parametrize("value", [0.0, math.nan])
def test_load_config_rejects_unsafe_scanner_scale(value, tmp_path):
    invalid = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    invalid["instruments"]["scanner"]["x"]["sample_um_per_unit"] = value
    path = tmp_path / "invalid-scanner-scale.json"
    path.write_text(json.dumps(invalid), encoding="utf-8")

    with pytest.raises(
        ValueError, match="sample_um_per_unit must be finite and non-zero"
    ):
        load_config(path)


@pytest.mark.parametrize("value", [math.inf, -math.inf, math.nan])
def test_load_config_rejects_non_finite_delay_stage_zero(value, tmp_path):
    invalid = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    invalid["instruments"]["delay_stage"]["t"]["zero_pos_mm"] = value
    path = tmp_path / "invalid-zero.json"
    path.write_text(json.dumps(invalid), encoding="utf-8")

    with pytest.raises(ValueError, match="zero_pos_mm must be finite"):
        load_config(path)


@pytest.mark.parametrize(
    ("settings", "message"),
    [
        ({"enabled": "yes"}, "enabled must be boolean"),
        (
            {"enabled": True, "distance_um": math.nan},
            "distance_um must be finite and non-negative",
        ),
        (
            {"enabled": True, "distance_um": -1.0},
            "distance_um must be finite and non-negative",
        ),
        (
            {"enabled": True, "distance_um": 1.0, "direction": "sideways"},
            "direction must be negative or positive",
        ),
    ],
)
def test_load_config_rejects_invalid_scanner_hysteresis(
    settings, message: str, tmp_path
):
    invalid = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    invalid["instruments"]["scanner"]["x"]["software_hysteresis"] = settings
    path = tmp_path / "invalid-hysteresis.json"
    path.write_text(json.dumps(invalid), encoding="utf-8")

    with pytest.raises(ValueError, match=message):
        load_config(path)


@pytest.mark.parametrize("field", ["min_pos", "max_pos", "origin_pos"])
def test_load_config_rejects_non_finite_scanner_positions(field: str, tmp_path):
    invalid = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    invalid["instruments"]["scanner"]["x"][field] = math.nan
    path = tmp_path / "invalid-scanner-position.json"
    path.write_text(json.dumps(invalid), encoding="utf-8")

    with pytest.raises(ValueError, match=rf"{field} must be finite"):
        load_config(path)


@pytest.mark.parametrize(
    ("minimum", "maximum", "origin", "message"),
    [
        (1.0, 1.0, 1.0, "max_pos must be greater"),
        (2.0, 1.0, 1.5, "max_pos must be greater"),
        (0.0, 1.0, 2.0, "origin_pos must be within"),
    ],
)
def test_load_config_rejects_invalid_scanner_position_range(
    minimum, maximum, origin, message, tmp_path
):
    invalid = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    scanner = invalid["instruments"]["scanner"]["x"]
    scanner.update({"min_pos": minimum, "max_pos": maximum, "origin_pos": origin})
    path = tmp_path / "invalid-scanner-range.json"
    path.write_text(json.dumps(invalid), encoding="utf-8")

    with pytest.raises(ValueError, match=message):
        load_config(path)


@pytest.mark.parametrize(
    ("kind", "field", "value", "message"),
    [
        ("lockin", "model", "UNKNOWN", "Unsupported.*model"),
        ("delay_stage", "controller", "UNKNOWN", "Unsupported.*controller"),
        ("delay_stage", "stage", "UNKNOWN", "Unsupported.*stage"),
        ("scanner", "controller", "UNKNOWN", "Unsupported.*controller"),
        ("scanner", "actuator", "UNKNOWN", "Unsupported.*actuator"),
    ],
)
def test_load_config_rejects_unknown_hardware_catalog_entries(
    kind, field, value, message, tmp_path
):
    invalid = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    key = {"lockin": "main", "delay_stage": "t", "scanner": "x"}[kind]
    invalid["instruments"][kind][key][field] = value
    path = tmp_path / "unknown-hardware.json"
    path.write_text(json.dumps(invalid), encoding="utf-8")

    with pytest.raises(ValueError, match=message):
        load_config(path)


@pytest.mark.parametrize(
    ("kind", "updates", "message"),
    [
        (
            "delay_stage",
            {"controller": "GSC01", "stage": "SGSP46-500"},
            "not compatible",
        ),
        (
            "scanner",
            {"controller": "CONEXCC", "actuator": "AG-M100D", "axis": 1},
            "not compatible",
        ),
    ],
)
def test_load_config_rejects_incompatible_hardware_combinations(
    kind, updates, message, tmp_path
):
    invalid = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    key = "t" if kind == "delay_stage" else "x"
    invalid["instruments"][kind][key].update(updates)
    path = tmp_path / "incompatible-hardware.json"
    path.write_text(json.dumps(invalid), encoding="utf-8")

    with pytest.raises(ValueError, match=message):
        load_config(path)


@pytest.mark.parametrize("axis", ["W", 0, 3, True])
def test_load_config_rejects_invalid_conexagap_axis(axis, tmp_path):
    invalid = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    invalid["instruments"]["scanner"]["x"]["axis"] = axis
    path = tmp_path / "invalid-axis.json"
    path.write_text(json.dumps(invalid), encoding="utf-8")

    with pytest.raises(ValueError, match="axis must be U/V or 1/2"):
        load_config(path)


def test_load_config_rejects_invalid_conexcc_axis(tmp_path):
    invalid = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    invalid["instruments"]["scanner"]["x"].update(
        {"controller": "CONEXCC", "actuator": "TRA12CC", "axis": "x"}
    )
    path = tmp_path / "invalid-conexcc-axis.json"
    path.write_text(json.dumps(invalid), encoding="utf-8")

    with pytest.raises(ValueError, match="axis must be a positive integer"):
        load_config(path)


@pytest.mark.parametrize("duplicate_kind", ["lockin", "delay_stage", "scanner"])
def test_load_config_rejects_duplicate_hardware_endpoints(
    duplicate_kind: str, tmp_path
):
    invalid = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    if duplicate_kind == "lockin":
        invalid["instruments"]["lockin"]["aux"] = dict(
            invalid["instruments"]["lockin"]["main"]
        )
    elif duplicate_kind == "delay_stage":
        invalid["instruments"]["delay_stage"]["aux"] = dict(
            invalid["instruments"]["delay_stage"]["t"]
        )
    else:
        invalid["instruments"]["scanner"]["x"]["axis"] = "V"
    path = tmp_path / "duplicate-endpoint.json"
    path.write_text(json.dumps(invalid), encoding="utf-8")

    with pytest.raises(ValueError, match="duplicates"):
        load_config(path)


def test_load_config_rejects_ambiguous_dotted_device_key(tmp_path):
    invalid = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    invalid["instruments"]["lockin"]["aux.main"] = invalid["instruments"]["lockin"].pop(
        "main"
    )
    path = tmp_path / "dotted-key.json"
    path.write_text(json.dumps(invalid), encoding="utf-8")

    with pytest.raises(ValueError, match="must not contain"):
        load_config(path)


@pytest.mark.parametrize(
    ("measurement", "field", "value", "message"),
    [
        ("signal_monitor", "lockin_key", "missing", "instruments.lockin"),
        ("trkr", "delay_stage_key", "missing", "instruments.delay_stage"),
    ],
)
def test_load_config_rejects_missing_measurement_device_reference(
    measurement,
    field,
    value,
    message,
    tmp_path,
):
    invalid = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    invalid["measurements"][measurement][field] = value
    path = tmp_path / "missing-measurement-device.json"
    path.write_text(json.dumps(invalid), encoding="utf-8")

    with pytest.raises(ValueError, match=rf"measurements\.{measurement}.*{message}"):
        load_config(path)


def test_load_config_rejects_missing_scanner_axis_reference(tmp_path):
    invalid = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    invalid["measurements"]["srkr"]["scanner_keys"] = {"x": "missing", "y": "y"}
    path = tmp_path / "missing-scanner-reference.json"
    path.write_text(json.dumps(invalid), encoding="utf-8")

    with pytest.raises(ValueError, match=r"measurements\.srkr.*instruments.scanner"):
        load_config(path)


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


def test_auto_timestamp_suffix_uses_microseconds_to_reduce_collisions(tmp_path):
    settings = {
        "output_dir": str(tmp_path),
        "filename": "run",
        "auto_timestamp_suffix": True,
    }

    assert re.fullmatch(
        r"run_\d{8}_\d{6}_\d{6}\.csv", output_path(settings, "default.csv").name
    )


@pytest.mark.parametrize(
    ("field", "value", "message"),
    [
        ("dir", "", "output.dir must not be empty"),
        ("filename", "", "output.filename must not be empty"),
        ("filename", "../run.csv", "file name, not a path"),
        ("filename", r"subdir\run.csv", "file name, not a path"),
        ("auto_timestamp_suffix", "yes", "auto_timestamp_suffix must be boolean"),
    ],
)
def test_load_config_rejects_invalid_output_settings(field, value, message, tmp_path):
    invalid = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    invalid["measurements"]["trkr"]["output"][field] = value
    path = tmp_path / "invalid-output.json"
    path.write_text(json.dumps(invalid), encoding="utf-8")

    with pytest.raises(ValueError, match=message):
        load_config(path)


def test_config_path_resolution_obeys_explicit_env_last_and_default_precedence(
    monkeypatch, tmp_path
):
    explicit = tmp_path / "explicit.json"
    env_path = tmp_path / "environment.json"
    last = tmp_path / "last.json"
    default = tmp_path / "default.json"
    for path in (env_path, last, default):
        path.write_text("{}", encoding="utf-8")
    state = write_last_config_path(last, tmp_path / "state" / "last.json")
    monkeypatch.setenv(CONFIG_PATH_ENV, str(env_path))
    monkeypatch.setenv(DEFAULT_CONFIG_PATH_ENV, str(default))

    resolution = resolve_config_path(explicit, last_state_path=state)
    assert (resolution.path, resolution.source) == (explicit, "explicit")
    assert resolution.candidates == [
        {"source": "explicit", "path": str(explicit), "exists": "False"}
    ]

    resolution = resolve_config_path(last_state_path=state)
    assert (resolution.path, resolution.source) == (env_path, CONFIG_PATH_ENV)

    monkeypatch.delenv(CONFIG_PATH_ENV)
    resolution = resolve_config_path(last_state_path=state)
    assert (resolution.path, resolution.source) == (last, "last")

    last.unlink()
    resolution = resolve_config_path(last_state_path=state)
    assert (resolution.path, resolution.source) == (default, "lab_default")
    assert [candidate["source"] for candidate in resolution.candidates] == [
        "last",
        "lab_default",
    ]


def test_config_path_resolution_reports_none_and_reads_legacy_state(
    monkeypatch, tmp_path
):
    legacy_state = tmp_path / "legacy-state"
    legacy_target = tmp_path / "legacy.json"
    legacy_state.write_text(f" {legacy_target} \n", encoding="utf-8")
    missing_default = tmp_path / "missing-default.json"
    monkeypatch.delenv(CONFIG_PATH_ENV, raising=False)
    monkeypatch.delenv(DEFAULT_CONFIG_PATH_ENV, raising=False)

    assert read_last_config_path(legacy_state) == legacy_target
    resolution = resolve_config_path(
        last_state_path=legacy_state,
        lab_default_path=missing_default,
    )

    assert resolution.path is None
    assert resolution.source == "none"
    assert [candidate["exists"] for candidate in resolution.candidates] == [
        "False",
        "False",
    ]
    assert read_last_config_path(tmp_path / "absent-state") is None


def test_normalize_config_migrates_arbitrary_legacy_scanner_scale():
    config = {
        "instruments": {
            "scanner": {
                "x": {
                    "sample_um_per_actuator_custom": 12.5,
                    "controller": " conex_cc ",
                    "actuator": " tra12_cc ",
                }
            }
        }
    }

    normalized = normalize_config(config)
    scanner = normalized["instruments"]["scanner"]["x"]

    assert scanner["sample_um_per_unit"] == 12.5
    assert scanner["controller"] == "CONEX_CC"
    assert scanner["actuator"] == "TRA12-CC"


def test_output_helpers_preserve_csv_case_and_nested_settings_precedence(tmp_path):
    settings = {
        "output_dir": "ignored",
        "filename": "ignored",
        "auto_timestamp_suffix": True,
        "output": {
            "dir": str(tmp_path),
            "filename": "Run.CSV",
            "auto_timestamp_suffix": False,
        },
    }

    assert with_csv_suffix("Run.CSV") == "Run.CSV"
    assert output_path(settings, "fallback.csv") == tmp_path / "Run.CSV"
    assert measurement_output_settings(
        {"measurements": {"trkr": settings}}, "trkr"
    ) == {
        "output_dir": str(tmp_path),
        "filename": "Run.CSV",
        "auto_timestamp_suffix": False,
    }


def test_instrument_selection_uses_measurement_keys_and_rejects_ambiguity():
    config = {
        "instruments": {
            "lockin": {"main": {"id": "main"}, "aux": {"id": "aux"}},
            "delay_stage": {"t": {"id": "t"}, "backup": {"id": "backup"}},
            "scanner": {"x": {"id": "x"}, "y": {"id": "y"}},
        },
        "measurements": {
            "trkr": {"lockin_key": "aux", "delay_stage_key": "backup"},
            "srkr": {"scanner_keys": {"x": "y"}},
        },
    }

    assert lockin_config_for(config, "trkr") == {"id": "aux"}
    assert delay_stage_config_for(config, "trkr") == {"id": "backup"}
    assert scanner_config_for(config, "x") == {"id": "y"}
    assert instrument_config(config, "scanner", "x") == {"id": "x"}

    with pytest.raises(ValueError, match="Multiple instruments.lockin"):
        instrument_key(config, "lockin")
    with pytest.raises(ValueError, match="Missing instruments.lockin.'missing'"):
        instrument_key(config, "lockin", "missing")


@pytest.mark.parametrize(
    ("mutation", "message"),
    [
        (lambda config: config["instruments"].update({"scanner": {}}), "scanner"),
        (
            lambda config: config["instruments"]["lockin"]["main"].pop("resource"),
            "lockin.main.resource",
        ),
        (
            lambda config: config["instruments"]["delay_stage"]["t"].pop("direction"),
            "delay_stage.t.direction",
        ),
        (
            lambda config: config["instruments"]["scanner"]["x"].pop(
                "sample_um_per_unit"
            ),
            "scanner.x.sample_um_per_unit",
        ),
    ],
)
def test_validate_config_rejects_missing_required_instrument_structure(
    mutation, message
):
    invalid = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    mutation(invalid)

    with pytest.raises(ValueError, match=message):
        validate_config(invalid)


def test_state_path_helpers_honor_environment_overrides(monkeypatch, tmp_path):
    state_dir = tmp_path / "state"
    explicit_state = tmp_path / "custom-last.json"
    monkeypatch.setenv(CONFIG_STATE_DIR_ENV, str(state_dir))

    assert config_state_dir() == state_dir
    assert last_config_state_path() == state_dir / "last_config.json"

    monkeypatch.setenv(LAST_CONFIG_STATE_PATH_ENV, str(explicit_state))
    assert last_config_state_path() == explicit_state


def test_state_path_helpers_use_home_defaults_without_environment(monkeypatch):
    monkeypatch.delenv(CONFIG_STATE_DIR_ENV, raising=False)
    monkeypatch.delenv(LAST_CONFIG_STATE_PATH_ENV, raising=False)

    assert config_state_dir().name == ".kohdalab"
    assert last_config_state_path() == config_state_dir() / "last_config.json"


@pytest.mark.parametrize("contents", ["", "{}", "null", '""'])
def test_read_last_config_path_returns_none_for_empty_state(contents, tmp_path):
    state = tmp_path / "last.json"
    state.write_text(contents, encoding="utf-8")

    assert read_last_config_path(state) is None


def test_delay_stage_name_normalization_handles_missing_and_legacy_names():
    assert normalize_delay_stage_name(None) is None
    assert normalize_delay_stage_name("") is None
    assert normalize_delay_stage_name(" sgsp_46-500 ") == "SGSP46-500"


def test_legacy_measurement_helpers_fail_closed_on_broken_nested_shapes():
    legacy = {
        "measurement": {
            "move_abs": {"zero": {"t_ps": 2.5}},
            "trkr": {
                "scan": "broken",
                "output": "broken",
                "output_dir": "/legacy",
                "filename": "legacy-run",
                "auto_timestamp_suffix": False,
            },
        }
    }

    assert measurement_settings(legacy, "trkr")["filename"] == "legacy-run"
    assert measurement_output_settings(legacy, "trkr") == {
        "output_dir": "/legacy",
        "filename": "legacy-run",
        "auto_timestamp_suffix": False,
    }
    assert scan_settings(legacy, "trkr") == {}
    assert move_abs_settings(legacy) == {"zero": {"t_ps": 2.5}}
    assert move_abs_zero(legacy) == {"t_ps": 2.5}
    assert zero_for(legacy, " T ") == 2.5
    assert zero_for(legacy, "x", default=7.0) == 7.0
    assert measurement_settings({"measurements": []}, "trkr") == {}


def test_move_abs_zero_rejects_broken_shape_by_returning_empty_mapping():
    config = {"measurements": {"move_abs": {"zero": "broken"}}}

    assert move_abs_zero(config) == {}
    assert zero_for(config, "y", default=4.0) == 4.0


def test_instrument_helpers_select_single_and_conventional_default_entries():
    config = {
        "instruments": {
            "lockin": {"main": {"id": "main"}, "aux": {"id": "aux"}},
            "delay_stage": {"t": {"id": "t"}, "aux": {"id": "aux"}},
            "scanner": {"x": {"id": "x"}, "y": {"id": "y"}},
        },
        "measurements": {
            "signal_monitor": {},
            "trkr": {},
            "srkr": {"scanners": "legacy-broken"},
        },
    }

    assert lockin_config_for(config, "signal_monitor") == {"id": "main"}
    assert delay_stage_config_for(config, "trkr") == {"id": "t"}
    assert scanner_config_for(config, "x") == {"id": "x"}
    assert instrument_key({"instruments": {"lockin": {"only": {}}}}, "lockin") == (
        "only"
    )


@pytest.mark.parametrize(
    ("config", "kind", "key", "message"),
    [
        ({}, "lockin", "main", "Missing instruments.lockin"),
        (
            {"instruments": {"lockin": {"main": "broken"}}},
            "lockin",
            "main",
            "Invalid instruments.lockin",
        ),
    ],
)
def test_instrument_config_rejects_missing_and_non_object_entries(
    config, kind, key, message
):
    with pytest.raises(ValueError, match=message):
        instrument_config(config, kind, key)


def test_instrument_key_rejects_missing_or_broken_device_collection():
    with pytest.raises(ValueError, match="Missing instruments.scanner"):
        instrument_key({"instruments": {"scanner": []}}, "scanner")
    with pytest.raises(ValueError, match="Missing instruments.scanner"):
        instrument_key({"instruments": {"scanner": {}}}, "scanner")


def test_save_and_load_without_validation_round_trips_normalized_config(tmp_path):
    path = tmp_path / "nested" / "minimal.json"

    assert save_config({"measurement": {}}, path, validate=False) == path
    loaded = load_config(path, validate=False)

    assert loaded["profile"]["name"] == "default"
    assert loaded["instruments"] == {}
    assert set(loaded["measurements"]) >= {"move_abs", "signal_monitor", "trkr"}


def test_output_naming_defaults_blank_name_and_directory():
    assert with_csv_suffix("   ") == "run.csv"
    generated = output_path({}, "fallback")
    assert generated.parent == DEFAULT_CONFIG_PATH.parents[3]
    assert re.fullmatch(r"fallback_\d{8}_\d{6}_\d{6}\.csv", generated.name)


@pytest.mark.parametrize(
    ("mutation", "message"),
    [
        (
            lambda config: config["measurements"].pop("strkr"),
            "measurements.strkr",
        ),
        (
            lambda config: config["instruments"]["delay_stage"]["t"].update(
                {"port": ""}
            ),
            "delay_stage.t.port must not be empty",
        ),
        (
            lambda config: config["instruments"]["scanner"]["x"].update({"axis": ""}),
            "scanner.x.axis must not be empty",
        ),
    ],
)
def test_validate_config_rejects_remaining_missing_and_empty_fields(mutation, message):
    invalid = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    mutation(invalid)

    with pytest.raises(ValueError, match=message):
        validate_config(invalid)


def test_build_range_points_single_point_and_custom_limit_edges():
    assert build_range_points(2.5, 2.5, 1.0, max_points=1) == [2.5]
    assert build_range_points(2.5, 2.5, -1.0, max_points=1) == [2.5]
    with pytest.raises(ValueError, match="maximum is 2"):
        build_range_points(0.0, 2.0, 1.0, max_points=2)


@pytest.mark.parametrize(
    ("start", "stop", "step"),
    [(math.nan, 1.0, 1.0), (0.0, math.inf, 1.0), (0.0, 1.0, -math.inf)],
)
def test_build_range_points_rejects_non_finite_values_in_every_position(
    start, stop, step
):
    with pytest.raises(ValueError, match="finite numbers"):
        build_range_points(start, stop, step)


def test_normalize_config_tolerates_non_object_legacy_device_entries():
    config = {
        "profile": "legacy-profile",
        "instruments": {
            "lockin": {"main": "legacy-lockin"},
            "delay_stage": {"t": "legacy-stage"},
            "scanner": {"x": "legacy-scanner"},
        },
        "measurements": {"trkr": "legacy-trkr"},
    }

    normalized = normalize_config(config)

    assert normalized["profile"] == "legacy-profile"
    assert normalized["instruments"] == config["instruments"]
    assert normalized["measurements"]["trkr"]["scan"]["step"] == 5.0


def test_scanner_selection_rejects_ambiguous_unknown_axis():
    config = {
        "instruments": {"scanner": {"left": {}, "right": {}}},
        "measurements": {"srkr": {}},
    }

    with pytest.raises(ValueError, match="Multiple instruments.scanner"):
        scanner_config_for(config, "z")


@pytest.mark.parametrize(
    ("mutation", "message"),
    [
        (
            lambda c: c["instruments"]["delay_stage"]["t"].update({"direction": 2}),
            "direction must be 0 or 1",
        ),
        (
            lambda c: c["instruments"]["scanner"]["x"].update(
                {"controller": "CONEXCC", "actuator": "TRA12CC", "axis": "01"}
            ),
            "axis must be a positive integer",
        ),
        (
            lambda c: c["instruments"]["scanner"]["x"].update(
                {"software_hysteresis": []}
            ),
            "software_hysteresis must be an object",
        ),
        (
            lambda c: c["measurements"]["move_abs"].update({"zero": []}),
            "move_abs.zero must be an object",
        ),
        (
            lambda c: c["measurements"]["move_abs"]["zero"].update({"x_um": math.nan}),
            "zero.x_um must be finite",
        ),
        (
            lambda c: c["measurements"]["signal_monitor"].update({"interval_s": -0.1}),
            "interval_s must be finite and non-negative",
        ),
        (
            lambda c: c["measurements"]["trkr"].update({"coordinate": "laboratory"}),
            "trkr.coordinate must be measurement",
        ),
        (
            lambda c: c["measurements"]["srkr"]["scan"].update({"axis": "z"}),
            "srkr.scan.axis must be 'x' or 'y'",
        ),
        (
            lambda c: c["measurements"]["srkr"].update({"coordinate": "laboratory"}),
            "srkr.coordinate must be measurement",
        ),
        (
            lambda c: c["measurements"]["strkr"].update({"wait_s": math.inf}),
            "strkr.wait_s must be finite",
        ),
        (
            lambda c: c["measurements"]["strkr"]["scan"].update(
                {"fast_axis": "x", "slow_axis": "y"}
            ),
            "strkr axes must combine t",
        ),
        (
            lambda c: c["measurements"]["srkr_2d"]["scan"]["ranges"].update({"x": []}),
            "ranges.x must be an object",
        ),
    ],
)
def test_validate_config_rejects_remaining_semantic_edges(mutation, message):
    invalid = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    mutation(invalid)

    with pytest.raises(ValueError, match=message):
        validate_config(invalid)


def test_build_range_points_rounds_repeating_decimal_coordinates():
    assert build_range_points(0.0, 0.999999999, 0.333333333) == [
        0.0,
        0.333333333,
        0.666666666,
        0.999999999,
    ]


@pytest.mark.parametrize(
    ("scanner", "expected"),
    [
        ({"sample_um_per_actuator_deg": 4.5}, 4.5),
        ({"controller": "CONEXCC"}, None),
    ],
)
def test_normalize_config_exercises_legacy_scale_fallback_endpoints(scanner, expected):
    normalized = normalize_config({"instruments": {"scanner": {"x": scanner}}})[
        "instruments"
    ]["scanner"]["x"]

    assert normalized.get("sample_um_per_unit") == expected


def test_resolve_config_path_uses_default_when_last_state_is_absent(tmp_path):
    default = tmp_path / "default.json"
    default.write_text("{}", encoding="utf-8")

    resolution = resolve_config_path(
        last_state_path=tmp_path / "absent-state.json",
        lab_default_path=default,
    )

    assert (resolution.path, resolution.source) == (default, "lab_default")
    assert [candidate["source"] for candidate in resolution.candidates] == [
        "lab_default"
    ]


def test_validate_config_accepts_canonical_conexcc_positive_integer_axis():
    config = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    config["instruments"]["scanner"]["x"].update(
        {"controller": "CONEXCC", "actuator": "TRA12CC", "axis": 1}
    )

    validate_config(config)


def test_validate_config_rejects_duplicate_2d_axes():
    config = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    config["measurements"]["srkr_2d"]["scan"].update(
        {"fast_axis": "x", "slow_axis": "x"}
    )

    with pytest.raises(ValueError, match="two different supported scan axes"):
        validate_config(config)


def test_validate_config_rejects_2d_total_point_count_over_limit():
    config = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    ranges = config["measurements"]["srkr_2d"]["scan"]["ranges"]
    ranges["x"] = {"min": 0.0, "max": 999.0, "step": 1.0}
    ranges["y"] = {"min": 0.0, "max": 1000.0, "step": 1.0}

    with pytest.raises(ValueError, match="would generate 1001000 points"):
        validate_config(config)


def test_save_config_with_validation_writes_loadable_configuration(tmp_path):
    config = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    output = tmp_path / "validated.json"

    assert save_config(config, output, validate=True) == output
    assert load_config(output)["profile"]["name"] == "default"


def test_instrument_selection_falls_back_to_only_nonstandard_key():
    config = {
        "instruments": {
            "lockin": {"only-lockin": {"id": "lockin"}},
            "delay_stage": {"only-stage": {"id": "stage"}},
            "scanner": {"only-scanner": {"id": "scanner"}},
        },
        "measurements": {"custom": {}, "srkr": {}},
    }

    assert lockin_config_for(config, "custom") == {"id": "lockin"}
    assert delay_stage_config_for(config, "custom") == {"id": "stage"}
    assert scanner_config_for(config, "z") == {"id": "scanner"}


def test_normalize_config_tolerates_non_object_instrument_collections():
    normalized = normalize_config(
        {
            "instruments": {
                "lockin": [],
                "delay_stage": [],
                "scanner": [],
            }
        }
    )

    assert normalized["instruments"] == {
        "lockin": [],
        "delay_stage": [],
        "scanner": [],
    }

    non_object = normalize_config({"instruments": []})
    assert non_object["instruments"] == []


def test_normalize_config_handles_partial_delay_stage_and_legacy_unit_miss():
    normalized = normalize_config(
        {
            "instruments": {
                "delay_stage": {
                    "stage-only": {"stage": "sgsp_46_500"},
                    "controller-only": {"controller": " gsc01 "},
                },
                "scanner": {
                    "x": {
                        "pos_unit": "custom",
                        "sample_um_per_actuator_deg": 6.5,
                    }
                },
            }
        }
    )

    stages = normalized["instruments"]["delay_stage"]
    assert stages["stage-only"]["stage"] == "SGSP46-500"
    assert stages["controller-only"]["controller"] == "GSC01"
    assert normalized["instruments"]["scanner"]["x"]["sample_um_per_unit"] == 6.5


def test_validate_config_accepts_scanner_range_without_optional_origin():
    config = json.loads(DEFAULT_CONFIG_PATH.read_text(encoding="utf-8"))
    scanner = config["instruments"]["scanner"]["x"]
    scanner.update({"min_pos": -1.0, "max_pos": 1.0})
    scanner.pop("origin_pos", None)

    validate_config(config)
