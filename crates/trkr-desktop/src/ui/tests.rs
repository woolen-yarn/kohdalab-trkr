use super::*;

#[test]
fn terminal_progress_survives_missed_samples_and_restored_axes_ignore_idle_progress() {
    let mut app = App::with_context(&egui::Context::default(), false);
    app.switch_workflow(Workflow::Srkr);
    app.active_workflow = Workflow::Srkr;
    let index = App::ix(Workflow::Srkr);
    app.plans[index].fast = Axis::X;
    app.select_plot_axes();
    // Only one display sample reached the UI, but all 41 rows reached disk.
    app.scan_progress[index] = (1, 41);
    app.snapshot.running = true;
    app.apply_snapshot(Snapshot {
        rows: 41,
        total: 41,
        phase: "Complete".into(),
        ..Default::default()
    });
    assert_eq!(app.scan_progress[index], (41, 41));
    app.pm().fast = Axis::Y;
    app.select_plot_axes();
    app.scan_progress[index] = (3, 3);
    app.pm().fast = Axis::X;
    app.select_plot_axes();
    app.apply_snapshot(Snapshot {
        rows: 3,
        total: 3,
        ..Default::default()
    });
    assert_eq!(app.scan_progress[index], (41, 41));
}
#[test]
fn axes_restore_results_progress_and_output_without_idle_state_overwriting_them() {
    for workflow in [Workflow::Srkr, Workflow::Strkr, Workflow::Srkr2d] {
        let mut app = App::with_context(&egui::Context::default(), false);
        app.settings = Settings::default();
        app.source_demo = true;
        app.plans = Workflow::ALL.map(|workflow| Plan {
            workflow,
            fast: if matches!(workflow, Workflow::Trkr | Workflow::Strkr) {
                Axis::T
            } else {
                Axis::X
            },
            slow: Axis::Y,
            fast_range: Range {
                min: 0.,
                max: 1.,
                step: 1.,
            },
            slow_range: Range {
                min: 0.,
                max: 1.,
                step: 1.,
            },
            interval_s: 0.,
            ..Default::default()
        });
        app.switch_workflow(workflow);
        let dir = tempfile::tempdir().unwrap();
        let index = App::ix(workflow);
        app.outputs[index].dir = dir.path().display().to_string();
        let initial = app.plan().clone();
        for run in 0..2 {
            if run == 1 {
                match workflow {
                    Workflow::Srkr => app.pm().fast = Axis::Y,
                    Workflow::Strkr => app.pm().slow = Axis::X,
                    Workflow::Srkr2d => {
                        app.pm().fast = Axis::Y;
                        app.pm().slow = Axis::X;
                    }
                    _ => unreachable!(),
                }
            }
            app.select_plot_axes();
            assert!(app.trace.is_empty());
            app.outputs[index].filename = format!("retained-{run}");
            app.start();
            let deadline = Instant::now() + Duration::from_secs(5);
            while app.snapshot.running {
                assert!(Instant::now() < deadline, "simulated scan timed out");
                app.receive();
                std::thread::sleep(Duration::from_millis(5));
            }
            assert_eq!(app.trace.len(), app.plan().targets().unwrap().len());
            assert!(app.output_paths[index].as_ref().unwrap().is_file());
            assert_eq!(app.scan_progress[index], (app.trace.len(), app.trace.len()));
        }
        let second = app.plan().clone();
        let second_len = app.trace.len();
        app.plans[index] = initial;
        app.select_plot_axes();
        assert!(!app.trace.is_empty());
        let first_len = app.trace.len();
        assert!(
            app.output_paths[index]
                .as_ref()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("retained-0")
        );
        // A worker idle snapshot still describes the last run, not this restored view.
        app.receive();
        assert_eq!(app.scan_progress[index], (first_len, first_len));
        app.switch_workflow(Workflow::Trkr);
        app.switch_workflow(workflow);
        assert_eq!(app.trace.len(), first_len);
        app.plans[index] = second;
        app.select_plot_axes();
        assert_eq!(app.trace.len(), second_len);
        assert!(
            app.output_paths[index]
                .as_ref()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("retained-1")
        );
    }
}

#[test]
fn delay_step_hint_uses_actual_microstep_and_rejects_stale_identity() {
    let settings = Settings::default();
    let config = settings.hardware().unwrap().motion.delay.unwrap();
    assert!(delay_step_hint(&config, None).starts_with("full "));
    let mut actual = config.calibration_hint().unwrap();
    actual.effective_ps_per_pulse = Some(0.001);
    actual.effective_pos_um_per_pulse = Some(0.149896229);
    actual.microstep_division = Some(100);
    assert_eq!(delay_step_hint(&config, Some(&actual)), "≥ 0.001 ps");
    let mut other = config.clone();
    other.controller_axis = 2;
    other.axis_count = 2;
    assert!(delay_step_hint(&other, Some(&actual)).starts_with("full "));
}
#[test]
fn srkr_2d_exposes_one_shared_theta_on_u_for_both_axis_orders() {
    for (fast, slow) in [(Axis::U, Axis::V), (Axis::V, Axis::U)] {
        assert_eq!(
            [fast, slow]
                .into_iter()
                .filter(|a| show_scan_theta(Workflow::Srkr2d, *a))
                .count(),
            1
        );
        assert!(show_scan_theta(Workflow::Srkr2d, Axis::U));
        assert!(!show_scan_theta(Workflow::Srkr2d, Axis::V));
    }
    assert!(show_scan_theta(Workflow::Strkr, Axis::V));
    assert!(!show_scan_theta(Workflow::Strkr, Axis::T));
    assert_eq!(scan_unit(Axis::T), "ps");
    assert_eq!(scan_unit(Axis::U), "µm");
}
#[test]
fn save_now_preserves_complete_rows_and_refuses_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.csv");
    let destination = dir.path().join("snapshot.csv");
    std::fs::write(&source, "timestamp,X_V\n2026-10-19T00:00:00Z,0.1\npartial").unwrap();
    copy_complete_csv(&source, &destination).unwrap();
    assert_eq!(
        std::fs::read_to_string(&destination).unwrap(),
        "timestamp,X_V\n2026-10-19T00:00:00Z,0.1\n"
    );
    assert!(copy_complete_csv(&source, &destination).is_err());
    assert_eq!(
        std::fs::read_to_string(source).unwrap(),
        "timestamp,X_V\n2026-10-19T00:00:00Z,0.1\npartial"
    );
}
#[test]
fn step_uses_acquired_count_and_total_even_if_plot_history_is_shorter() {
    assert_eq!(step_progress(9, 41), "9/41");
    assert_eq!(step_progress(50_001, 100_000), "50001/100000");
    assert_eq!(step_progress(0, 0), "-");
}
#[test]
fn initialization_distinguishes_homing_and_agap_enable_with_controller_aliases() {
    for cc in ["CONEXCC", "CONEX-CC", "conex_cc", "CONEX CC"] {
        let controllers = [cc.into(), "CONEXAGAP".into()];
        assert!(initialization_homes(0, &controllers));
        assert!(initialization_homes(1, &controllers));
        assert!(!initialization_homes(2, &controllers));
    }
}
#[test]
fn device_headers_show_only_connected_names_and_relevant_motion() {
    let plan = Plan {
        workflow: Workflow::Strkr,
        fast: Axis::T,
        slow: Axis::U,
        ..Default::default()
    };
    assert_eq!(device_caption(false, "SR7265", Some("Reading")), "----");
    assert_eq!(
        device_caption(true, "SR7265", Some("Reading")),
        "SR7265 · Reading"
    );
    assert_eq!(
        device_phase(true, true, "Moving t", Device::T, &plan),
        Some("Moving")
    );
    assert_eq!(device_phase(true, true, "Moving t", Device::X, &plan), None);
    assert_eq!(
        device_phase(true, true, "Returning x", Device::X, &plan),
        Some("Returning")
    );
    assert_eq!(
        device_phase(true, true, "Returning x", Device::T, &plan),
        None
    );

    assert_eq!(
        device_phase(true, true, "Moving y", Device::Y, &plan),
        Some("Moving")
    );
    assert_eq!(
        device_phase(true, true, "Initializing x", Device::X, &plan),
        Some("Initializing")
    );
    assert_eq!(
        device_phase(true, true, "Initializing x", Device::Y, &plan),
        None
    );

    for device in [Device::X, Device::Y] {
        assert_eq!(
            device_phase(true, true, "Moving u · loop 1", device, &plan),
            Some("Moving")
        );
        assert_eq!(
            device_phase(true, true, "Waiting", device, &plan),
            Some("Waiting")
        );
    }
    assert_eq!(
        device_phase(true, true, "Reading", Device::Lockin, &plan),
        Some("Reading")
    );
    assert_eq!(
        device_phase(true, false, "Moving t", Device::T, &plan),
        None
    );
    assert_eq!(
        device_phase(false, true, "Moving t", Device::T, &plan),
        None
    );
}
#[test]
fn demo_output_name_is_marked_without_double_prefix() {
    let normal = OutputProfile {
        dir: "runs".into(),
        filename: "trkr_run".into(),
        auto_timestamp_suffix: true,
    };
    assert_eq!(output_filename(&normal, true), "demo_trkr_run");
    assert_eq!(output_filename(&normal, false), "trkr_run");
    let marked = OutputProfile {
        filename: "Demo_TRKR".into(),
        ..normal
    };
    assert_eq!(output_filename(&marked, true), "Demo_TRKR");
}

#[test]
fn output_hydration_accepts_legacy_flat_fields_and_nested_priority() {
    let mut raw = json!({
        "measurements": {
            "trkr": {
                "output_dir": "legacy-runs",
                "filename": "legacy-name",
                "auto_timestamp_suffix": true,
                "output": {
                    "dir": "nested-runs",
                    "output_dir": "ignored-runs",
                    "filename": "nested-name",
                    "auto_timestamp_suffix": false
                }
            }
        }
    });
    let fallback = default_output(Workflow::Trkr);
    assert_eq!(
        hydrated_output(&raw, Workflow::Trkr, fallback.clone()),
        OutputProfile {
            dir: "nested-runs".into(),
            filename: "nested-name".into(),
            auto_timestamp_suffix: false,
        }
    );

    raw["measurements"]["trkr"]["output"] = json!({
        "dir": "   ",
        "output_dir": "nested-legacy-runs",
        "filename": "",
    });
    assert_eq!(
        hydrated_output(&raw, Workflow::Trkr, fallback),
        OutputProfile {
            dir: "nested-legacy-runs".into(),
            filename: "legacy-name".into(),
            auto_timestamp_suffix: true,
        }
    );
}

#[test]
fn sync_output_canonicalizes_known_legacy_output_fields() {
    let mut app = App::with_context(&egui::Context::default(), false);
    app.settings.raw["measurements"]["trkr"] = json!({
        "output_dir": "flat-dir",
        "filename": "flat-name",
        "auto_timestamp_suffix": false,
        "output": {"output_dir": "nested-alias", "metadata": {"keep": true}}
    });
    app.hydrate_output(Workflow::Trkr);
    app.sync_output(Workflow::Trkr);
    let settings = &app.settings.raw["measurements"]["trkr"];
    assert!(settings.get("output_dir").is_none());
    assert!(settings.get("filename").is_none());
    assert!(settings.get("auto_timestamp_suffix").is_none());
    assert!(settings["output"].get("output_dir").is_none());
    assert_eq!(settings["output"]["dir"], "nested-alias");
    assert_eq!(settings["output"]["filename"], "flat-name");
    assert_eq!(settings["output"]["auto_timestamp_suffix"], false);
    assert_eq!(settings["output"]["metadata"]["keep"], true);
}

#[test]
fn ordinary_sync_does_not_create_missing_motion_devices() {
    let mut app = App::with_context(&egui::Context::default(), false);
    app.settings.raw["instruments"]["delay_stage"] = json!({});
    app.settings.raw["instruments"]["scanner"] = json!({});
    app.hydrate();
    app.sync();
    assert!(
        app.settings.raw["instruments"]["delay_stage"]
            .get("t")
            .is_none()
    );
    assert!(
        app.settings.raw["instruments"]["scanner"]
            .get("x")
            .is_none()
    );
    assert!(
        app.settings.raw["instruments"]["scanner"]
            .get("y")
            .is_none()
    );
    assert_eq!(app.configured_devices(), vec![Device::Lockin]);
}

#[test]
fn ordinary_sync_keeps_only_the_configured_y_scanner() {
    let mut app = App::with_context(&egui::Context::default(), false);
    let y = app.settings.raw["instruments"]["scanner"]["y"].clone();
    app.settings.raw["instruments"]["delay_stage"] = json!({});
    app.settings.raw["instruments"]["scanner"] = json!({"y": y});
    app.hydrate();
    app.sync();
    assert!(
        app.settings.raw["instruments"]["scanner"]
            .get("x")
            .is_none()
    );
    assert!(app.settings.raw["instruments"]["scanner"]["y"].is_object());
    assert_eq!(app.configured_devices(), vec![Device::Lockin, Device::Y]);
}

#[test]
fn editing_an_unconfigured_device_promotes_only_that_device_on_sync() {
    let mut app = App::with_context(&egui::Context::default(), false);
    app.settings.raw["instruments"]["delay_stage"] = json!({});
    app.settings.raw["instruments"]["scanner"] = json!({});
    app.hydrate();
    app.scanner_port[1] = "COM5".into();
    // All Connect builds its list before config() calls sync().
    assert_eq!(app.configured_devices(), vec![Device::Lockin, Device::Y]);
    app.sync();
    assert!(
        app.settings.raw["instruments"]["scanner"]
            .get("x")
            .is_none()
    );
    assert_eq!(
        app.settings.raw["instruments"]["scanner"]["y"]["port"],
        "COM5"
    );
    assert_eq!(app.configured_devices(), vec![Device::Lockin, Device::Y]);
}

#[test]
fn legacy_hardware_aliases_roundtrip_through_hydrate_and_sync() {
    let mut app = App::with_context(&egui::Context::default(), false);
    let delay = app.settings.raw["instruments"]["delay_stage"]["t"]
        .as_object_mut()
        .unwrap();
    delay.remove("controller");
    delay.insert("delay_stage_controller".into(), json!("SHOT-302GS"));

    let x = app.settings.raw["instruments"]["scanner"]["x"]
        .as_object_mut()
        .unwrap();
    x.remove("controller");
    x.remove("sample_um_per_unit");
    x.insert("scanner_controller".into(), json!("CONEX-CC"));
    x.insert("actuator".into(), json!("TRA12CC"));
    x.insert("axis".into(), json!(1));
    x.insert("sample_um_per_actuator_mm".into(), json!(321.));
    x.insert("sample_um_per_actuator_deg".into(), json!(999.));
    x.insert(
        "software_hysteresis".into(),
        json!({"enabled": true, "approach_distance_um": 37., "approach": "positive"}),
    );
    x.insert("vendor_metadata".into(), json!({"keep": true}));

    app.hydrate();
    assert_eq!(app.stage_model, "SHOT302GS");
    assert_eq!(app.scanner_model[0], "CONEXCC");
    assert_eq!(app.scanner_scale[0], 321.);
    assert_eq!(app.hysteresis_distance[0], 37.);
    assert_eq!(app.hysteresis_direction[0], "positive");

    app.sync();
    let delay = &app.settings.raw["instruments"]["delay_stage"]["t"];
    let x = &app.settings.raw["instruments"]["scanner"]["x"];
    assert_eq!(delay["controller"], "SHOT302GS");
    assert_eq!(x["controller"], "CONEXCC");
    assert_eq!(x["sample_um_per_unit"], 321.);
    assert_eq!(x["software_hysteresis"]["distance_um"], 37.);
    assert_eq!(x["software_hysteresis"]["direction"], "positive");
    assert_eq!(x["vendor_metadata"]["keep"], true);
    for alias in [
        "scanner_controller",
        "sample_um_per_actuator_mm",
        "sample_um_per_actuator_deg",
    ] {
        assert!(x.get(alias).is_none(), "{alias}");
    }
    assert!(delay.get("delay_stage_controller").is_none());
    assert!(
        x["software_hysteresis"]
            .get("approach_distance_um")
            .is_none()
    );
    assert!(x["software_hysteresis"].get("approach").is_none());
    assert!(app.settings.hardware().is_ok());
}

#[test]
fn lockin_model_alias_hydrates_case_insensitively_and_syncs_canonical_model() {
    let mut app = App::with_context(&egui::Context::default(), false);
    let lockin = app.settings.raw["instruments"]["lockin"]["main"]
        .as_object_mut()
        .unwrap();
    lockin.remove("model");
    lockin.insert("lockin_model".into(), json!("sr830"));
    app.hydrate();
    assert_eq!(app.model, Model::Sr830);
    app.sync();
    let lockin = &app.settings.raw["instruments"]["lockin"]["main"];
    assert_eq!(lockin["model"], "SR830");
    assert!(lockin.get("lockin_model").is_none());
}

#[test]
fn gpib_resource_requires_canonical_portable_address() {
    assert_eq!(App::gpib_address("GPIB0::0::INSTR"), None);
    assert_eq!(App::gpib_address("GPIB0::1::INSTR"), Some(1));
    assert_eq!(App::gpib_address("GPIB0::30::INSTR"), Some(30));
    assert_eq!(App::gpib_address("GPIB0::31::INSTR"), None);
    assert_eq!(App::gpib_address("GPIB0::01::INSTR"), None);
    assert_eq!(App::gpib_address("GPIB1::12::INSTR"), None);
}

#[test]
fn shared_coordinates_follow_selected_profile_without_changing_scan_settings() {
    let base = Plan::default();
    let mut plans = std::array::from_fn(|_| base.clone());
    plans[workflow_index(Workflow::Trkr)].workflow = Workflow::Trkr;
    plans[workflow_index(Workflow::Srkr)].workflow = Workflow::Srkr;
    plans[workflow_index(Workflow::Strkr)].workflow = Workflow::Strkr;
    plans[workflow_index(Workflow::Srkr2d)].workflow = Workflow::Srkr2d;
    plans[workflow_index(Workflow::Trkr)].fast_range = Range {
        min: -10.,
        max: 20.,
        step: 2.,
    };
    plans[workflow_index(Workflow::Srkr)].fast_range = Range {
        min: -31.,
        max: 27.,
        step: 0.5,
    };
    plans[workflow_index(Workflow::Srkr)].zero = [12., 34., 56.];
    plans[workflow_index(Workflow::Srkr)].theta_deg = 18.5;

    sync_shared_coordinates(&mut plans, Workflow::Srkr);

    for plan in &plans {
        assert_eq!(plan.zero, [12., 34., 56.]);
        assert_eq!(plan.theta_deg, 18.5);
    }
    assert_eq!(
        plans[workflow_index(Workflow::Trkr)].fast_range,
        Range {
            min: -10.,
            max: 20.,
            step: 2.,
        }
    );
    assert_eq!(
        plans[workflow_index(Workflow::Srkr)].fast_range,
        Range {
            min: -31.,
            max: 27.,
            step: 0.5,
        }
    );
}

#[test]
fn documentation_captures_use_synthetic_settings_without_operator_paths() {
    let mut app = App::with_context(&egui::Context::default(), false);
    app.config_path = "/operator/private/profile.json".into();
    app.settings.raw["profile"]["description"] = json!("private experiment");
    for output in &mut app.outputs {
        output.dir = "/operator/private/experiment".into();
    }
    app.note("private path from profile load");
    let success = Arc::new(AtomicBool::new(true));
    let directory = tempfile::tempdir().unwrap();
    app.enable_smoke(directory.path().join("capture"), success.clone());
    assert_eq!(app.config_path, "config/default.json");
    assert_eq!(app.settings.raw, Settings::default().raw);
    assert!(app.outputs.iter().all(|output| output.dir == "demo/runs"));
    assert_eq!(app.log.len(), 1);
    assert!(!app.log[0].contains("private"));
    assert!(!success.load(Ordering::Acquire));
    assert!(!directory.path().join("capture").exists());
}

#[test]
fn synthetic_layout_capture_keeps_explicit_rotated_axes() {
    for (state, fast, slow) in [
        ("rotated", Axis::U, Axis::V),
        ("rotated-reverse", Axis::V, Axis::U),
    ] {
        let mut app = App::with_context(&egui::Context::default(), false);
        let directory = tempfile::tempdir().unwrap();
        app.enable_layout_capture(
            directory.path().join("capture"),
            Arc::new(AtomicBool::new(false)),
            state,
        );
        assert_eq!(app.plans[Workflow::Srkr.index()].fast, fast);
        assert_eq!(app.plans[Workflow::Strkr.index()].fast, fast);
        assert_eq!(app.plans[Workflow::Strkr.index()].slow, Axis::T);
        assert_eq!(app.plans[Workflow::Srkr2d.index()].fast, fast);
        assert_eq!(app.plans[Workflow::Srkr2d.index()].slow, slow);
        assert!(app.smoke.as_ref().unwrap().layout_only);
    }
}
