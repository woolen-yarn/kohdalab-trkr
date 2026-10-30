#[test]
fn noncanonical_only_devices_are_never_selected_by_map_order() {
    for group in ["lockin", "delay_stage", "scanner"] {
        let mut settings = Settings::default();
        let key = match group {
            "lockin" => "main",
            "delay_stage" => "t",
            _ => "x",
        };
        let device = settings.raw["instruments"][group][key].clone();
        settings.raw["instruments"][group] = json!({"other":device});
        assert!(
            settings.hardware().is_err(),
            "silently selected {group}.other"
        );
    }
    let mut settings = Settings::default();
    settings.raw["instruments"]["scanner"]
        .as_object_mut()
        .unwrap()
        .remove("x");
    let config = settings.hardware().unwrap();
    assert!(config.motion.x.is_none());
    assert!(config.motion.y.is_some());
}
#[test]
fn null_legacy_device_selectors_mean_unspecified_without_falling_back_to_alias() {
    let mut settings = Settings::default();
    settings.raw["measurements"]["srkr"]["lockin_key"] = Value::Null;
    settings.raw["measurements"]["srkr"]["lockin"] = json!("ignored-alias");
    settings.raw["measurements"]["srkr"]["scanner_keys"] = Value::Null;
    settings.raw["measurements"]["srkr"]["scanners"] = json!({"x":"ignored-alias"});
    assert!(settings.hardware().is_ok());
    settings.raw["measurements"]["srkr"]["scanner_keys"] = json!({"x":null,"y":null});
    assert!(settings.hardware().is_ok());
}
#[test]
fn app_bundle_uses_resources_and_portable_uses_executable_directory() {
    let root = tempfile::tempdir().unwrap();
    let macos = root.path().join("Contents/MacOS");
    std::fs::create_dir_all(root.path().join("Contents/Resources")).unwrap();
    std::fs::create_dir_all(&macos).unwrap();
    assert!(super::resources_at(&macos).ends_with("MacOS/../Resources"));
    let portable = root.path().join("portable");
    assert_eq!(super::resources_at(&portable), portable);
}

use super::*;
#[test]
fn profile_plan_roundtrip_preserves_custom_keys() {
    let mut s = Settings::default();
    s.raw["custom"] = json!({"keep":true});
    for workflow in Workflow::ALL {
        let p = s.plan(workflow).unwrap();
        s.set_plan(&p).unwrap();
        assert_eq!(s.plan(workflow).unwrap(), p);
    }
    assert_eq!(s.raw["custom"]["keep"], true);
}
#[test]
fn return_reset_choice_survives_profile_roundtrip() {
    let mut s = Settings::default();
    let mut p = s.plan(Workflow::Strkr).unwrap();
    p.return_fast_between_lines = false;
    s.set_plan(&p).unwrap();
    assert_eq!(s.plan(Workflow::Strkr).unwrap(), p);
}
#[test]
fn rejects_direct_usb_and_nonintegral_count() {
    assert!(parse_gpib("USB0::12::INSTR").is_err());
    assert!(parse_gpib("GPIB0::0::INSTR").is_err());
    let mut s = Settings::default();
    s.raw["measurements"]["signal_monitor"]["n_points"] = json!(1.5);
    assert!(s.plan(Workflow::SignalMonitor).is_err());
}
#[test]
fn legacy_trkr_coordinate_aliases_preserve_their_units() {
    let mut settings = Settings::default();
    for (alias, expected) in [
        ("ps", Coordinate::Measurement),
        ("t_ps", Coordinate::Measurement),
        ("mm", Coordinate::Interface),
        ("pos_mm", Coordinate::Interface),
        ("pulse", Coordinate::Instrument),
        ("control", Coordinate::Interface),
        ("device", Coordinate::Instrument),
    ] {
        settings.raw["measurements"]["trkr"]["coordinate"] = json!(alias);
        assert_eq!(
            settings.plan(Workflow::Trkr).unwrap().coordinate,
            expected,
            "{alias}"
        );
    }
}
#[test]
fn legacy_srkr_coordinate_aliases_follow_the_actual_scanner_unit() {
    let mut settings = Settings::default();
    settings.raw["instruments"]["scanner"]["x"]["controller"] = json!("CONEXCC");
    settings.raw["instruments"]["scanner"]["x"]["actuator"] = json!("TRA12CC");
    settings.raw["instruments"]["scanner"]["x"]["axis"] = json!(1);
    for (alias, expected) in [
        ("um", Coordinate::Measurement),
        ("sample_um", Coordinate::Measurement),
        ("mm", Coordinate::Interface),
        ("pos_mm", Coordinate::Interface),
        ("instrument", Coordinate::Interface),
        ("device", Coordinate::Interface),
    ] {
        settings.raw["measurements"]["srkr"]["coordinate"] = json!(alias);
        assert_eq!(
            settings.plan(Workflow::Srkr).unwrap().coordinate,
            expected,
            "{alias}"
        );
    }
    settings.raw["measurements"]["srkr"]["coordinate"] = json!("deg");
    assert!(
        settings
            .plan(Workflow::Srkr)
            .unwrap_err()
            .contains("does not match")
    );

    settings.raw["instruments"]["scanner"]["x"]["controller"] = json!("CONEXAGAP");
    settings.raw["instruments"]["scanner"]["x"]["actuator"] = json!("AG-M100D");
    settings.raw["instruments"]["scanner"]["x"]["axis"] = json!("U");
    settings.raw["measurements"]["srkr"]["coordinate"] = json!("pos_deg");
    assert_eq!(
        settings.plan(Workflow::Srkr).unwrap().coordinate,
        Coordinate::Interface
    );
    settings.raw["measurements"]["srkr"]["coordinate"] = json!("pos_mm");
    assert!(
        settings
            .plan(Workflow::Srkr)
            .unwrap_err()
            .contains("does not match")
    );
}
#[test]
fn non_measurement_2d_and_rotated_aliases_remain_rejected() {
    let mut settings = Settings::default();
    settings.raw["measurements"]["strkr"]["coordinate"] = json!("device");
    assert!(
        settings
            .plan(Workflow::Strkr)
            .unwrap_err()
            .contains("measurement coordinates")
    );
    settings.raw["measurements"]["srkr"]["scan"]["axis"] = json!("u");
    settings.raw["measurements"]["srkr"]["coordinate"] = json!("control");
    assert!(
        settings
            .plan(Workflow::Srkr)
            .unwrap_err()
            .contains("measurement coordinates")
    );
}
#[test]
fn explicit_legacy_device_selectors_require_the_portable_defaults() {
    let mut settings = Settings::default();
    settings.raw["measurements"]["trkr"]["lockin"] = json!("main");
    settings.raw["measurements"]["trkr"]["delay_stage_key"] = json!("t");
    settings.raw["measurements"]["srkr"]["scanners"] = json!({"x": "x", "y": "y"});
    assert!(settings.hardware().is_ok());

    settings.raw["measurements"]["trkr"]["lockin_key"] = json!("other");
    assert!(
        settings
            .hardware()
            .unwrap_err()
            .contains("supports only instruments.lockin.main")
    );
    settings.raw["measurements"]["trkr"]["lockin_key"] = json!("main");
    settings.raw["measurements"]["srkr"]["scanner_keys"] = json!({"x": "other"});
    assert!(
        settings
            .hardware()
            .unwrap_err()
            .contains("supports only instruments.scanner.x")
    );
}
#[test]
fn profile_alias_normalization_uses_the_actual_scanner_unit_and_canonical_wins() {
    let mut settings = Settings::default();
    let scanner = settings.raw["instruments"]["scanner"]["x"]
        .as_object_mut()
        .unwrap();
    scanner.insert("controller".into(), json!("CONEXAGAP"));
    scanner.insert("scanner_controller".into(), json!("CONEX-CC"));
    scanner.insert("actuator".into(), json!("AG-M100D"));
    scanner.insert("sample_um_per_unit".into(), json!(17.));
    scanner.insert("sample_um_per_actuator_mm".into(), json!(400.));
    scanner.insert("sample_um_per_actuator_deg".into(), json!(500.));
    scanner.insert(
            "software_hysteresis".into(),
            json!({"distance_um": 7., "pre_move_um": 9., "direction": "negative", "approach": "positive"}),
        );
    scanner.insert("unknown_metadata".into(), json!({"keep": true}));

    settings.normalize_profile_aliases();
    let scanner = &settings.raw["instruments"]["scanner"]["x"];
    assert_eq!(scanner["controller"], "CONEXAGAP");
    assert_eq!(scanner["sample_um_per_unit"], 17.);
    assert_eq!(scanner["software_hysteresis"]["distance_um"], 7.);
    assert_eq!(scanner["software_hysteresis"]["direction"], "negative");
    assert_eq!(scanner["unknown_metadata"]["keep"], true);
    for alias in [
        "scanner_controller",
        "sample_um_per_actuator_mm",
        "sample_um_per_actuator_deg",
    ] {
        assert!(scanner.get(alias).is_none(), "{alias}");
    }
    assert!(scanner["software_hysteresis"].get("pre_move_um").is_none());
    assert!(scanner["software_hysteresis"].get("approach").is_none());

    let mut legacy = Settings::default();
    let scanner = legacy.raw["instruments"]["scanner"]["x"]
        .as_object_mut()
        .unwrap();
    scanner.remove("sample_um_per_unit");
    scanner.insert("controller".into(), json!("CONEX-CC"));
    scanner.insert("actuator".into(), json!("TRA12CC"));
    scanner.insert("sample_um_per_actuator_mm".into(), json!(321.));
    scanner.insert("sample_um_per_actuator_deg".into(), json!(999.));
    scanner.insert(
        "software_hysteresis".into(),
        json!({"approach_distance_um": 12., "pre_move_um": 99.}),
    );
    legacy.normalize_profile_aliases();
    assert_eq!(
        legacy.raw["instruments"]["scanner"]["x"]["controller"],
        "CONEXCC"
    );
    assert_eq!(
        legacy.raw["instruments"]["scanner"]["x"]["sample_um_per_unit"],
        321.
    );
    assert_eq!(
        legacy.raw["instruments"]["scanner"]["x"]["software_hysteresis"]["distance_um"],
        12.
    );
}
#[test]
fn lockin_model_alias_normalizes_without_overriding_a_canonical_model() {
    let mut settings = Settings::default();
    let lockin = settings.raw["instruments"]["lockin"]["main"]
        .as_object_mut()
        .unwrap();
    lockin.remove("model");
    lockin.insert("lockin_model".into(), json!("sr830"));
    settings.normalize_profile_aliases();
    assert_eq!(
        settings.raw["instruments"]["lockin"]["main"]["model"],
        "sr830"
    );
    assert!(
        settings.raw["instruments"]["lockin"]["main"]
            .get("lockin_model")
            .is_none()
    );

    settings.raw["instruments"]["lockin"]["main"] =
        json!({"model":"SR7265", "lockin_model":"SR830"});
    settings.normalize_profile_aliases();
    assert_eq!(
        settings.raw["instruments"]["lockin"]["main"]["model"],
        "SR7265"
    );
}
#[test]
fn manual_device_selectors_cannot_silently_move_a_different_device() {
    for (field, selection) in [
        ("delay_stage_key", json!("other")),
        ("delay_stage", json!("other")),
        ("scanner_keys", json!({"x":"other"})),
        ("scanners", json!({"y":"other"})),
    ] {
        let mut settings = Settings::default();
        settings.raw["measurements"]["move_abs"][field] = selection;
        let error = settings.hardware().unwrap_err();
        assert!(error.contains("Manual Move"), "{field}: {error}");
    }
    let mut settings = Settings::default();
    settings.raw["measurements"]["move_abs"]["delay_stage_key"] = json!("t");
    settings.raw["measurements"]["move_abs"]["scanner_keys"] = json!({"x":"x","y":"y"});
    assert!(settings.hardware().is_ok());
}
#[test]
fn singular_measurement_root_preserves_ranges_zero_and_metadata_on_load() {
    let mut raw = Settings::default().raw;
    let measurements = raw.as_object_mut().unwrap().remove("measurements").unwrap();
    raw["measurement"] = measurements;
    raw["measurement"]["trkr"]["scan"] = json!({"min":17.,"max":23.,"step":3.});
    raw["measurement"]["move_abs"]["zero"] = json!({"t_ps":123.,"x_um":45.,"y_um":67.});
    raw["measurement"]["private_metadata"] = json!({"keep":true});
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), serde_json::to_vec(&raw).unwrap()).unwrap();
    let loaded = Settings::load(file.path()).unwrap();
    let plan = loaded.plan(Workflow::Trkr).unwrap();
    assert_eq!(
        plan.fast_range,
        Range {
            min: 17.,
            max: 23.,
            step: 3.
        }
    );
    assert_eq!(plan.zero, [123., 45., 67.]);
    assert_eq!(loaded.raw["measurements"]["private_metadata"]["keep"], true);
    assert!(loaded.raw.get("measurement").is_none());

    raw["measurements"] = Settings::default().raw["measurements"].clone();
    std::fs::write(file.path(), serde_json::to_vec(&raw).unwrap()).unwrap();
    assert_eq!(
        Settings::load(file.path())
            .unwrap()
            .plan(Workflow::Trkr)
            .unwrap()
            .zero,
        Settings::default().plan(Workflow::Trkr).unwrap().zero
    );
    raw.as_object_mut().unwrap().remove("measurements");
    raw["measurement"] = json!("invalid");
    std::fs::write(file.path(), serde_json::to_vec(&raw).unwrap()).unwrap();
    assert!(Settings::load(file.path()).is_err());
}
#[test]
fn measurement_coordinate_profiles_do_not_require_scanners() {
    let mut settings = Settings::default();
    settings.raw["instruments"]["scanner"] = json!({});
    assert!(settings.hardware().is_ok());
    for workflow in Workflow::ALL {
        assert!(settings.plan(workflow).is_ok(), "{}", workflow.name());
    }
}

#[test]
fn experiment_metadata_survives_profile_plan_roundtrip() {
    let mut settings = Settings::default();
    settings.raw["experiment"] =
        json!({"sample_id":"Kim-01","temperature":{"value":300,"unit":"K"}});
    let mut plan = settings.plan(Workflow::Trkr).unwrap();
    assert_eq!(plan.experiment["sample_id"], "Kim-01");
    plan.experiment
        .insert("note".into(), json!("unchanged optical alignment"));
    settings.set_plan(&plan).unwrap();
    assert_eq!(
        settings.plan(Workflow::Trkr).unwrap().experiment,
        plan.experiment
    );
    settings.raw["experiment"] = json!("invalid");
    assert!(settings.plan(Workflow::Trkr).is_err());
}

#[test]
fn user_config_paths_keep_production_and_demo_profiles_separate() {
    let root = tempfile::tempdir().unwrap();
    let paths = UserConfigPaths::from_kohdalab_dir(root.path().join(".kohdalab"));
    assert_eq!(
        paths.production(),
        root.path().join(".kohdalab/trkr/config.json")
    );
    assert_eq!(paths.demo(), root.path().join(".kohdalab/trkr/demo.json"));
    assert_ne!(paths.production(), paths.demo());
}

#[test]
fn config_home_selection_prefers_platform_native_environment() {
    use std::ffi::OsStr;
    assert_eq!(
        super::home_from_environment(
            Some(OsStr::new("C:/Users/operator")),
            Some(OsStr::new("/Users/operator")),
            true,
        ),
        std::path::PathBuf::from("C:/Users/operator")
    );
    assert_eq!(
        super::home_from_environment(
            Some(OsStr::new("C:/Users/operator")),
            Some(OsStr::new("/Users/operator")),
            false,
        ),
        std::path::PathBuf::from("/Users/operator")
    );
    assert_eq!(
        super::home_from_environment(None, Some(OsStr::new("/Users/operator")), true),
        std::path::PathBuf::from("/Users/operator")
    );
}

#[test]
fn startup_uses_only_the_trkr_specific_profile() {
    let root = tempfile::tempdir().unwrap();
    let paths = UserConfigPaths::from_kohdalab_dir(root.path().join(".kohdalab"));
    let StartupSettings::Loaded { path, .. } = load_startup(&paths).unwrap();
    assert_eq!(path, paths.production());
    assert!(paths.production().is_file());
    let raw: Value = serde_json::from_slice(&std::fs::read(paths.production()).unwrap()).unwrap();
    assert_eq!(raw["application"], APPLICATION);
    let first_bytes = std::fs::read(paths.production()).unwrap();
    let _ = load_startup(&paths).unwrap();
    assert_eq!(first_bytes, std::fs::read(paths.production()).unwrap());
    let production = paths.production();
    Settings::default().save(&production).unwrap();
    let StartupSettings::Loaded { settings, path } = load_startup(&paths).unwrap();
    assert_eq!(path, production);
    assert!(settings.hardware().is_ok());
}

#[test]
fn first_launch_does_not_replace_a_racing_or_invalid_profile() {
    let root = tempfile::tempdir().unwrap();
    let paths = UserConfigPaths::from_kohdalab_dir(root.path().join(".kohdalab"));
    let production = paths.production();
    std::fs::create_dir_all(production.parent().unwrap()).unwrap();
    let foreign = br#"{"application":"kohdalab-iv","operator":"keep"}"#;
    std::fs::write(&production, foreign).unwrap();
    let error = load_startup(&paths).unwrap_err();
    assert!(error.contains("belongs to kohdalab-iv"));
    assert_eq!(std::fs::read(&production).unwrap(), foreign);
}

#[test]
fn concurrent_first_launches_publish_one_complete_json() {
    let root = tempfile::tempdir().unwrap();
    let paths = UserConfigPaths::from_kohdalab_dir(root.path().join(".kohdalab"));
    let production = paths.production();
    let mut workers = Vec::new();
    for _ in 0..8 {
        let path = production.clone();
        workers.push(std::thread::spawn(move || load_startup(&UserConfigPaths::from_kohdalab_dir(
            path.parent().unwrap().parent().unwrap().to_path_buf(),
        ))));
    }
    for worker in workers {
        assert!(worker.join().unwrap().is_ok());
    }
    assert!(Settings::load(&production).is_ok());
}

#[test]
fn unusable_profile_parent_is_reported_without_creating_partial_data() {
    let root = tempfile::tempdir().unwrap();
    let parent = root.path().join(".kohdalab");
    std::fs::write(&parent, b"occupied").unwrap();
    let paths = UserConfigPaths::from_kohdalab_dir(parent);
    let error = load_startup(&paths).unwrap_err();
    assert!(!error.is_empty());
    assert!(!root.path().join(".kohdalab/trkr/config.json").exists());
}

#[test]
fn future_profile_is_preserved_and_reported() {
    let root = tempfile::tempdir().unwrap();
    let paths = UserConfigPaths::from_kohdalab_dir(root.path().join(".kohdalab"));
    let production = paths.production();
    std::fs::create_dir_all(production.parent().unwrap()).unwrap();
    let bytes = br#"{"application":"kohdalab-trkr","schema_version":999}"#;
    std::fs::write(&production, bytes).unwrap();
    let error = load_startup(&paths).unwrap_err();
    assert!(error.contains("Configuration is invalid"));
    assert_eq!(std::fs::read(production).unwrap(), bytes);
}

#[test]
fn startup_prefers_existing_new_profile_and_reports_invalid_files() {
    let root = tempfile::tempdir().unwrap();
    let paths = UserConfigPaths::from_kohdalab_dir(root.path().join(".kohdalab"));
    let production = paths.production();
    std::fs::create_dir_all(production.parent().unwrap()).unwrap();
    let mut current = Settings::default();
    current.raw["experiment"] = json!({"profile":"new"});
    current.save(&production).unwrap();
    let StartupSettings::Loaded { settings, path } = load_startup(&paths).unwrap();
    assert_eq!(path, production);
    assert_eq!(settings.raw["experiment"]["profile"], "new");

    std::fs::write(paths.production(), b"not json").unwrap();
    let error = load_startup(&paths).unwrap_err();
    assert!(error.contains("Configuration is invalid"));
    assert!(error.contains("config.json"));

    std::fs::remove_file(paths.production()).unwrap();
    std::fs::create_dir_all(paths.production()).unwrap();
    let error = load_startup(&paths).unwrap_err();
    assert!(error.contains("not a regular file"));
}

#[test]
fn demo_startup_uses_only_the_trkr_specific_demo_profile() {
    let root = tempfile::tempdir().unwrap();
    let paths = UserConfigPaths::from_kohdalab_dir(root.path().join(".kohdalab"));
    assert_eq!(load_demo_startup(&paths).unwrap(), None);
    std::fs::create_dir_all(paths.demo().parent().unwrap()).unwrap();
    std::fs::write(paths.demo(), br#"{"profile":{"name":"demo"}}"#).unwrap();
    assert_eq!(load_demo_startup(&paths).unwrap(), Some(paths.demo()));
    assert!(!paths.production().exists());
}

#[test]
fn rejects_foreign_or_iv_profiles_and_never_overwrites_them() {
    let root = tempfile::tempdir().unwrap();
    let foreign = root.path().join("foreign.json");
    std::fs::write(&foreign, br#"{"application":"kohdalab-iv"}"#).unwrap();
    let error = Settings::load(&foreign).unwrap_err();
    assert!(error.contains("belongs to kohdalab-iv"));
    let error = Settings::default().save(&foreign).unwrap_err();
    assert!(error.contains("Refusing to overwrite invalid configuration"));
    assert_eq!(
        std::fs::read(&foreign).unwrap(),
        br#"{"application":"kohdalab-iv"}"#
    );

    let iv = root.path().join("iv-markerless.json");
    std::fs::write(&iv, br#"{"source_model":"GS210","meter_model":"34411A"}"#).unwrap();
    assert!(Settings::load(&iv).unwrap_err().contains("KohdaLab IV"));
    assert!(Settings::default().save(&iv).is_err());

    std::fs::write(&iv, br#"{"instruments":{"source":{},"meter":{}}}"#).unwrap();
    assert!(Settings::load(&iv).unwrap_err().contains("KohdaLab IV"));

    let mut foreign_settings = Settings::default();
    foreign_settings.raw["application"] = json!("kohdalab-iv");
    assert!(
        foreign_settings
            .save(&root.path().join("new.json"))
            .is_err()
    );
}

#[test]
fn save_marks_new_trkr_profiles_without_using_demo_path() {
    let root = tempfile::tempdir().unwrap();
    let paths = UserConfigPaths::from_kohdalab_dir(root.path().join(".kohdalab"));
    Settings::default().save(&paths.production()).unwrap();
    let saved: Value = serde_json::from_slice(&std::fs::read(paths.production()).unwrap()).unwrap();
    assert_eq!(saved["application"], APPLICATION);
    assert!(!paths.demo().exists());
}

#[test]
fn save_refuses_empty_or_non_file_targets() {
    let root = tempfile::tempdir().unwrap();
    let empty = root.path().join("empty.json");
    std::fs::write(&empty, b"").unwrap();
    assert!(Settings::default().save(&empty).is_err());
    assert!(empty.is_file());

    let directory = root.path().join("directory.json");
    std::fs::create_dir(&directory).unwrap();
    assert!(Settings::default().save(&directory).is_err());
}
