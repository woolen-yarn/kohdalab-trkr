//! Opt-in, headless real-app checks and an explicit frame-timing benchmark.
//!
//! These exercise the real `eframe::App`, rather than a copied widget.  They
//! deliberately use `App::with_context`, which owns a temporary configuration
//! directory, and never issue a device command.

use super::*;
use egui_kittest::{Harness, kittest::Queryable};

const SIZE: egui::Vec2 = egui::Vec2::new(1_440.0, 900.0);

fn app_harness() -> Harness<'static, App> {
    Harness::builder()
        .with_size(SIZE)
        .with_max_steps(1)
        .build_eframe(|cc| App::with_context(&cc.egui_ctx, false))
}

fn render(harness: &mut Harness<'_, App>) {
    // `App::update` deliberately requests periodic repaints for live hardware.
    // Fixed steps make these tests deterministic without waiting for that loop.
    harness.run_steps(2);
}

fn populate_synthetic_trace(app: &mut App, workflow: Workflow) -> usize {
    app.switch_workflow(workflow);
    let index = App::ix(workflow);
    let mut plan = app.plans[index].clone();
    plan.workflow = workflow;
    plan.interval_s = 0.01;
    if workflow == Workflow::SignalMonitor {
        plan.n_points = 401;
    } else {
        plan.fast_range = Range {
            min: -20.,
            max: 20.,
            step: 0.1,
        };
        if workflow.is_2d() {
            plan.slow_range = Range {
                min: -5.,
                max: 5.,
                step: 5.,
            };
        }
    }
    let targets = plan.targets().expect("valid synthetic evaluation plan");
    app.trace = targets
        .into_iter()
        .enumerate()
        .map(|(i, target)| Sample {
            recording: Default::default(),
            timing: Default::default(),
            captured_at: None,
            elapsed: i as f64 * plan.interval_s,
            signal: trkr_core::Signal::demo(
                target.corrected[0] + target.fast,
                target.corrected[1],
                target.corrected[2],
            ),
            position: trkr_motion::Position {
                physical: target.physical.map(Some),
                ..Default::default()
            },
            target,
        })
        .collect();
    app.plans[index] = plan.clone();
    app.trace_plans[index] = Some(plan);
    app.scan_progress[index] = (app.trace.len(), app.trace.len());
    app.active_workflow = workflow;
    app.trace.len()
}

#[test]
fn real_app_tab_click_preserves_idle_device_ownership() {
    let mut harness = app_harness();
    render(&mut harness);

    harness.get_by_label("TRKR").click();
    render(&mut harness);

    let app = harness.state();
    assert_eq!(app.workflow, Workflow::Trkr);
    assert!(!app.alignment_tab);
    assert!(
        !app.snapshot.running,
        "a tab click must not start acquisition"
    );
    assert!(app.geometry.contains_key("measurement_tabs"));
    assert!(app.geometry.contains_key("tab_trkr"));
}

#[test]
fn real_app_renders_every_measurement_workflow_at_the_same_frame_size() {
    let mut harness = app_harness();
    render(&mut harness);
    let expected_center_width = harness.state().geometry["center_panel"][2];
    for workflow in Workflow::ALL {
        harness.state_mut().switch_workflow(workflow);
        render(&mut harness);
        let app = harness.state();
        assert_eq!(app.workflow, workflow);
        assert_eq!(app.geometry["center_panel"][2], expected_center_width);
        assert!(
            app.geometry
                .contains_key(&format!("tab_{}", workflow.key()))
        );
        assert!(!app.snapshot.running);
    }
}

#[test]
fn real_app_keeps_fixed_side_column_contract_when_collapsed() {
    let mut harness = app_harness();
    render(&mut harness);
    let full = harness.state().geometry.clone();
    assert_eq!(full["left_panel"][2], 384.0);
    assert_eq!(full["right_panel"][2], 300.0);

    {
        let app = harness.state_mut();
        app.left_collapsed = true;
        app.right_collapsed = true;
    }
    render(&mut harness);
    let collapsed = &harness.state().geometry;
    assert_eq!(collapsed["left_panel"][2], 40.0);
    assert_eq!(collapsed["right_panel"][2], 27.0);
    assert!(collapsed["center_panel"][2] > full["center_panel"][2]);
}

#[test]
fn real_app_renders_measurement_settings_and_alignment_without_hardware() {
    let mut harness = app_harness();
    render(&mut harness);

    harness.state_mut().open_measurement_settings();
    render(&mut harness);
    assert!(harness.state().measurement_settings.is_some());
    assert!(!harness.state().snapshot.running);

    // A modal correctly consumes the Alignment click. Close it before testing
    // the tab's ordinary interaction path.
    harness.state_mut().measurement_settings = None;
    render(&mut harness);
    harness.get_by_label("Alignment").click();
    render(&mut harness);
    assert!(harness.state().alignment_tab);
    assert!(harness.state().geometry.contains_key("alignment_widget"));
    assert!(!harness.state().snapshot.running);

    // The existing preview is a safe stand-in for the multi-device scanner
    // panel: its constructor sets demo mode before it builds synthetic state.
    harness.state_mut().enable_alignment_preview(true);
    render(&mut harness);
    let app = harness.state();
    assert!(app.demo_viewer && app.source_demo);
    assert_eq!(app.inventory.scanners.len(), 4);
    assert_eq!(app.inventory.delays.len(), 2);
}

#[derive(serde::Serialize)]
struct TimingCase {
    screen: &'static str,
    frames_per_sample: usize,
    warmup_frames: usize,
    samples_ns: Vec<u128>,
}

/// Emits repeatable headless UI layout timings for an A/B build comparison.
///
/// This is intentionally ignored: it measures CPU wall time for `App::update`,
/// egui layout, and its AccessKit tree.  `egui_kittest::run_steps` does not
/// invoke the renderer, so it does not claim GPU, tessellation, native-window,
/// device-I/O, or end-to-end acquisition performance. Run it in a separate
/// process for each environment setting so `OnceLock`-controlled icon selection
/// cannot leak between the baseline and candidate.
#[test]
#[ignore = "comparison benchmark; run explicitly with OSS_EVAL_RESULTS=/absolute/result.json"]
fn headless_real_app_frame_timings() {
    const WARMUP: usize = 24;
    const FRAMES: usize = 120;
    const SAMPLES: usize = 9;
    type ScreenSetup = (&'static str, fn(&mut App));
    let cases: [ScreenSetup; 7] = [
        ("measurement-signal-monitor", |app| {
            populate_synthetic_trace(app, Workflow::SignalMonitor);
        }),
        ("measurement-trkr", |app| {
            populate_synthetic_trace(app, Workflow::Trkr);
        }),
        ("measurement-srkr", |app| {
            populate_synthetic_trace(app, Workflow::Srkr);
        }),
        ("measurement-strkr", |app| {
            populate_synthetic_trace(app, Workflow::Strkr);
        }),
        ("measurement-srkr-2d", |app| {
            populate_synthetic_trace(app, Workflow::Srkr2d);
        }),
        ("alignment-delay", |app| app.enable_alignment_preview(false)),
        ("alignment-scanner", |app| {
            app.enable_alignment_preview(true)
        }),
    ];
    let mut results = Vec::with_capacity(cases.len());
    for (screen, setup) in cases {
        let mut harness = app_harness();
        setup(harness.state_mut());
        harness.run_steps(WARMUP);
        let mut samples_ns = Vec::with_capacity(SAMPLES);
        for _ in 0..SAMPLES {
            let started = Instant::now();
            harness.run_steps(FRAMES);
            samples_ns.push(started.elapsed().as_nanos());
        }
        results.push(TimingCase {
            screen,
            frames_per_sample: FRAMES,
            warmup_frames: WARMUP,
            samples_ns,
        });
    }

    if let Ok(destination) = std::env::var("OSS_EVAL_RESULTS") {
        let document = serde_json::json!({
            "kind": "headless-egui-frame-wall-time",
            "notes": [
                "Real App::update with isolated settings and no device commands.",
                "CPU wall time includes App::update, egui layout, and AccessKit; GPU, tessellation, native-window, and hardware I/O are excluded.",
                "Run each icon/profiler variant in a separate process."
            ],
            "cases": results,
        });
        std::fs::write(
            destination,
            serde_json::to_vec_pretty(&document).expect("serialize timing report"),
        )
        .expect("write timing report");
    }
}
