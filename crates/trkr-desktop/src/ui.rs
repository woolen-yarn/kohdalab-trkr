mod demo;
mod eta;
use crate::{axis_choices, precision};
use crate::{
    config::Settings,
    engine::{self, Command, Device, Event, Sample, Snapshot, Source},
    lockin::Model,
};
use crate::{
    file_dialog::{self, Kind as DialogKind},
    presentation,
};
use eframe::egui::{self, Color32, Stroke, Vec2};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use trkr_core::{Axis, Coordinate, DisplayMode, Plan, Range, Workflow};

const PANEL: Color32 = Color32::from_rgb(26, 27, 32);
const BORDER: Color32 = Color32::from_rgb(56, 58, 67);
const TEXT: Color32 = Color32::from_rgb(235, 236, 241);
const MAX_TRACE: usize = 50_000;

#[derive(Clone, Debug, PartialEq, Eq)]
struct OutputProfile {
    dir: String,
    filename: String,
    auto_timestamp_suffix: bool,
}
#[derive(Clone, PartialEq)]
struct StageDraft {
    model: String,
    preset: String,
    port: String,
    direction: i32,
}
#[derive(Clone, PartialEq)]
struct ScannerDraft {
    model: String,
    actuator: String,
    port: String,
    axis: String,
    scale: f64,
    hysteresis: bool,
    hysteresis_distance: f64,
    hysteresis_direction: String,
}
struct ScanColumn<'a> {
    fast: bool,
    x: f32,
    width: f32,
    tc: Option<f64>,
    replay_wait: bool,
    hints: &'a [String; 3],
}
struct PendingDialog {
    kind: DialogKind,
    workflow: Workflow,
    source: Option<PathBuf>,
    receiver: std::sync::mpsc::Receiver<Result<Option<PathBuf>, String>>,
}
enum SmokeStage {
    Start,
    Wait,
    CaptureXy,
    CaptureRtheta,
    CaptureWorkflow {
        index: usize,
        frames: u8,
        requested: bool,
    },
    Done,
}
struct Smoke {
    output: PathBuf,
    success: Arc<AtomicBool>,
    started: Instant,
    stage: SmokeStage,
    requested: bool,
    rtheta_frames: u8,
    failure: Option<String>,
    layout_only: bool,
}
fn show_scan_theta(workflow: Workflow, axis: Axis) -> bool {
    if workflow == Workflow::Srkr2d {
        axis == Axis::U
    } else {
        matches!(axis, Axis::U | Axis::V)
    }
}
fn delay_step_hint(
    config: &trkr_motion::DelayConfig,
    cached: Option<&trkr_motion::DelayCalibration>,
) -> String {
    let Ok(normalized) = config.normalized() else {
        return "— ps".into();
    };
    let actual = cached
        .filter(|c| c.configuration.as_ref() == Some(&normalized))
        .and_then(|c| c.effective_ps_per_pulse);
    let hint = normalized.calibration_hint().ok();
    if let Some(step) = actual.or_else(|| hint.as_ref().and_then(|h| h.effective_ps_per_pulse)) {
        format!("≥ {} ps", App::significant(step, 5))
    } else if let Some(full) = hint.and_then(|h| h.full_step_ps) {
        format!("full {} ps", App::significant(full, 5))
    } else {
        "Connect for step".into()
    }
}
fn scan_unit(axis: Axis) -> &'static str {
    if axis == Axis::T { "ps" } else { "µm" }
}
fn edit(ui: &mut egui::Ui, v: &mut String) {
    ui.add(egui::TextEdit::singleline(v).desired_width(ui.available_width().clamp(80.0, 220.0)));
}
pub struct App {
    demo_viewer: bool,
    demo: demo::Replay,
    settings: Settings,
    plans: [Plan; 5],
    workflow: Workflow,
    source_demo: bool,
    model: Model,
    address: u8,
    lockin_resource: String,
    mode: DisplayMode,
    trkr_log_y: bool,
    gpib_resources: Vec<String>,
    gpib_refresh: Option<std::sync::mpsc::Receiver<Result<Vec<String>, String>>>,
    controller: engine::Controller,
    snapshot: Snapshot,
    live: crate::live::History,
    active_live_origin: engine::ReadingOrigin,
    trace: Vec<Sample>,
    outputs: [OutputProfile; 5],
    replay_path: String,
    replay_interval_s: f64,
    config_path: String,
    status: String,
    log: VecDeque<String>,
    stage_model: String,
    stage_preset: String,
    stage_port: String,
    stage_direction: i32,
    stage_configured: bool,
    stage_draft: Option<StageDraft>,
    scanner_model: [String; 2],
    scanner_actuator: [String; 2],
    scanner_port: [String; 2],
    scanner_axis: [String; 2],
    scanner_scale: [f64; 2],
    hysteresis: [bool; 2],
    hysteresis_distance: [f64; 2],
    hysteresis_direction: [String; 2],
    scanner_configured: [bool; 2],
    scanner_draft: [Option<ScannerDraft>; 2],
    available_ports: Vec<String>,
    port_refresh: Option<std::sync::mpsc::Receiver<Result<Vec<String>, String>>>,
    sensitivity: f64,
    time_constant: f64,
    coupling: String,
    slope: i64,
    ac_gain: f64,
    manual: [f64; 3],
    corrected_manual: [f64; 3],
    smoke: Option<Smoke>,
    plot_epoch: u64,
    central_width: f32,
    left_collapsed: bool,
    devices_scrolled_smoke: bool,
    right_collapsed: bool,
    right_extra_width: f32,
    right_was_maximized: bool,
    map_cache: presentation::MapCache,
    dialog: Option<PendingDialog>,
    initialize_axis: Option<usize>,
    plot_history: crate::plot_history::History,
    saved_traces: [Vec<Sample>; 5],
    trace_plans: [Option<Plan>; 5],
    scan_progress: [(usize, usize); 5],
    eta: eta::Estimate,
    output_paths: [Option<PathBuf>; 5],
    active_workflow: Workflow,
    last_settings: Instant,
    geometry: precision::Geometry,
    device_expanded: [bool; 4],
}
impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, demo_viewer: bool) -> Self {
        Self::with_context(&cc.egui_ctx, demo_viewer)
    }
    fn with_context(ctx: &egui::Context, demo_viewer: bool) -> Self {
        ctx.set_visuals(egui::Visuals::dark());
        precision::configure(ctx);
        let b = Plan::default();
        let mut p = std::array::from_fn(|_| b.clone());
        p[1].workflow = Workflow::Trkr;
        p[2].workflow = Workflow::Srkr;
        p[2].fast = Axis::X;
        p[3].workflow = Workflow::Strkr;
        p[4].workflow = Workflow::Srkr2d;
        p[4].fast = Axis::X;
        p[4].slow = Axis::Y;
        let mut app = Self {
            demo_viewer,
            demo: demo::Replay::default(),
            settings: Settings::default(),
            plans: p,
            workflow: Workflow::SignalMonitor,
            source_demo: false,
            model: Model::Sr7265,
            address: 12,
            lockin_resource: "GPIB0::12::INSTR".into(),
            mode: DisplayMode::Xy,
            trkr_log_y: false,
            gpib_resources: Vec::new(),
            gpib_refresh: None,
            controller: if demo_viewer {
                engine::controller_read_only()
            } else {
                engine::controller()
            },
            snapshot: Snapshot::default(),
            live: crate::live::History::default(),
            active_live_origin: engine::ReadingOrigin::Hardware,
            trace: Vec::new(),
            outputs: std::array::from_fn(|i| default_output(Workflow::ALL[i])),
            replay_path: String::new(),
            replay_interval_s: 0.1,
            config_path: config_path(),
            status: "Ready".into(),
            log: VecDeque::new(),
            stage_model: "SHOT302GS".into(),
            stage_preset: "SGSP46-500".into(),
            stage_port: "COM6".into(),
            stage_direction: 1,
            stage_configured: false,
            stage_draft: None,
            scanner_model: ["CONEXAGAP".into(), "CONEXAGAP".into()],
            scanner_actuator: ["AG-M100D".into(), "AG-M100D".into()],
            scanner_port: ["COM11".into(), "COM11".into()],
            scanner_axis: ["U".into(), "V".into()],
            scanner_scale: [582., 412.],
            hysteresis: [false, false],
            hysteresis_distance: [100.; 2],
            hysteresis_direction: ["negative".into(), "negative".into()],
            scanner_configured: [false; 2],
            scanner_draft: [None, None],
            available_ports: Vec::new(),
            port_refresh: None,
            sensitivity: 1e-3,
            time_constant: 0.5,
            coupling: "AC".into(),
            slope: 12,
            ac_gain: 1.,
            manual: [0.; 3],
            corrected_manual: [0.; 3],
            smoke: None,
            plot_epoch: 0,
            central_width: 0.0,
            left_collapsed: false,
            devices_scrolled_smoke: false,
            right_collapsed: false,
            right_extra_width: precision::Columns::RIGHT_WIDTH - 27.,
            right_was_maximized: false,
            map_cache: [None, None],
            dialog: None,
            initialize_axis: None,
            plot_history: crate::plot_history::History::default(),
            saved_traces: std::array::from_fn(|_| Vec::new()),
            trace_plans: std::array::from_fn(|_| None),
            scan_progress: [(0, 0); 5],
            eta: eta::Estimate::default(),
            output_paths: std::array::from_fn(|_| None),
            active_workflow: Workflow::SignalMonitor,
            last_settings: Instant::now(),
            geometry: precision::Geometry::new(),
            device_expanded: [false; 4],
        };
        if demo_viewer {
            app.config_path = config_default_path()
                .with_file_name("default_demo.json")
                .display()
                .to_string();
            app.source_demo = true;
            app.workflow = Workflow::Trkr;
            app.active_workflow = Workflow::Trkr;
            let demo_config = PathBuf::from(&app.config_path);
            let bundled =
                Some(crate::config::application_resources().join("config/default_demo.json"));
            let candidate = if demo_config.is_file() {
                Some(demo_config)
            } else {
                bundled.filter(|p| p.is_file())
            };
            if let Some(path) = candidate
                && let Ok(settings) = Settings::load(&path)
            {
                app.settings = settings;
                app.config_path = path.display().to_string();
            }
            app.hydrate();
            for plan in &mut app.plans {
                plan.interval_s = 0.1;
            }
            app.note("DEMO MODE — NO HARDWARE · Choose CSV → Load → Connect All → Start");
            return app;
        }
        let dedicated = PathBuf::from(&app.config_path);
        let fallback = config_default_path();
        let candidate = dedicated
            .is_file()
            .then_some(dedicated.as_path())
            .or_else(|| fallback.is_file().then_some(fallback.as_path()));
        if let Some(path) = candidate {
            match Settings::load(path) {
                Ok(settings) => {
                    app.settings = settings;
                    app.hydrate();
                    app.note(format!("Loaded configuration: {}", path.display()));
                }
                Err(error) => {
                    app.hydrate();
                    app.note(format!(
                        "Configuration is invalid ({}): {error}",
                        path.display()
                    ));
                }
            }
        } else {
            app.hydrate();
            app.note("No configuration file found; using built-in defaults");
        }
        app
    }
    pub fn enable_smoke(&mut self, output_dir: PathBuf, success: Arc<AtomicBool>) {
        // Captures must be reproducible and safe to publish. Use only the
        // bundled synthetic profile; never save or expose the operator's paths.
        self.settings = Settings::default();
        self.config_path = "config/default.json".into();
        self.hydrate();
        for output in &mut self.outputs {
            output.dir = "demo/runs".into();
        }
        self.log.clear();
        self.note("DEMO — synthetic data · no hardware");
        success.store(false, Ordering::Release);
        self.smoke = Some(Smoke {
            output: output_dir,
            success,
            started: Instant::now(),
            stage: SmokeStage::Start,
            requested: false,
            rtheta_frames: 0,
            failure: None,
            layout_only: false,
        });
    }
    pub fn enable_layout_capture(
        &mut self,
        output: PathBuf,
        success: Arc<AtomicBool>,
        state: &str,
    ) {
        self.enable_smoke(output, success);
        if matches!(state, "rotated" | "rotated-reverse") {
            self.plans[Self::ix(Workflow::Srkr)].fast = Axis::U;
            self.plans[Self::ix(Workflow::Strkr)].fast = Axis::U;
            self.plans[Self::ix(Workflow::Strkr)].slow = Axis::T;
            self.plans[Self::ix(Workflow::Srkr2d)].fast = Axis::U;
            self.plans[Self::ix(Workflow::Srkr2d)].slow = Axis::V;
        }
        if state == "rotated-reverse" {
            self.plans[Self::ix(Workflow::Srkr)].fast = Axis::V;
            self.plans[Self::ix(Workflow::Strkr)].fast = Axis::V;
            self.plans[Self::ix(Workflow::Srkr2d)].fast = Axis::V;
            self.plans[Self::ix(Workflow::Srkr2d)].slow = Axis::U;
        }
        self.device_expanded = [matches!(state, "devices-expanded" | "devices-scrolled"); 4];
        self.left_collapsed = state == "left-collapsed";
        self.devices_scrolled_smoke = state == "devices-scrolled";
        self.right_collapsed = state == "right-collapsed";
        if let Some(smoke) = &mut self.smoke {
            smoke.layout_only = true;
            smoke.stage = SmokeStage::CaptureWorkflow {
                index: 0,
                frames: 0,
                requested: false,
            };
        }
    }
    fn ix(w: Workflow) -> usize {
        w.index()
    }
    fn plan(&self) -> &Plan {
        &self.plans[Self::ix(self.workflow)]
    }
    fn pm(&mut self) -> &mut Plan {
        let i = Self::ix(self.workflow);
        &mut self.plans[i]
    }
    fn note(&mut self, s: impl Into<String>) {
        let s = s.into();
        self.status = s.clone();
        self.log.push_back(s);
        while self.log.len() > 200 {
            self.log.pop_front();
        }
    }
    fn path_set(root: &mut Value, path: &[&str], value: Value) {
        let mut at = root;
        for key in &path[..path.len() - 1] {
            if !at.is_object() {
                *at = json!({});
            }
            let object = at.as_object_mut().expect("object");
            at = object
                .entry((*key).to_string())
                .or_insert_with(|| json!({}));
        }
        if !at.is_object() {
            *at = json!({});
        }
        at.as_object_mut()
            .expect("object")
            .insert(path[path.len() - 1].to_string(), value);
    }
    fn raw_str(&self, path: &[&str], fallback: &str) -> String {
        let mut at = &self.settings.raw;
        for key in path {
            match at.get(*key) {
                Some(v) => at = v,
                None => return fallback.into(),
            }
        }
        at.as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| fallback.into())
    }
    fn raw_f64(&self, path: &[&str], fallback: f64) -> f64 {
        let mut at = &self.settings.raw;
        for key in path {
            match at.get(*key) {
                Some(v) => at = v,
                None => return fallback,
            }
        }
        at.as_f64().unwrap_or(fallback)
    }
    fn raw_bool(&self, path: &[&str], fallback: bool) -> bool {
        let mut at = &self.settings.raw;
        for key in path {
            match at.get(*key) {
                Some(v) => at = v,
                None => return fallback,
            }
        }
        at.as_bool().unwrap_or(fallback)
    }
    fn hydrate(&mut self) {
        self.settings.normalize_profile_aliases();
        for w in Workflow::ALL {
            if let Ok(p) = self.settings.plan(w) {
                self.plans[Self::ix(w)] = p;
            }
            self.hydrate_output(w);
        }
        for (i, axis) in ["t", "x", "y"].into_iter().enumerate() {
            self.manual[i] = self.raw_f64(&["measurements", "move_abs", "targets", axis], 0.);
            self.corrected_manual[i] = self.raw_f64(
                &[
                    "measurements",
                    "move_abs",
                    "targets",
                    &format!("{axis}_cor"),
                ],
                0.,
            );
        }
        let m = self.raw_str(&["instruments", "lockin", "main", "model"], "SR7265");
        self.model = Model::ALL
            .into_iter()
            .find(|x| x.name().eq_ignore_ascii_case(&m))
            .unwrap_or(Model::Sr7265);
        let resource = self.raw_str(
            &["instruments", "lockin", "main", "resource"],
            "GPIB0::12::INSTR",
        );
        self.lockin_resource = resource;
        self.address = Self::gpib_address(&self.lockin_resource).unwrap_or(12);
        self.stage_configured = self
            .settings
            .raw
            .pointer("/instruments/delay_stage/t")
            .is_some_and(Value::is_object);
        let motion = self
            .settings
            .hardware()
            .ok()
            .map(|hardware| hardware.motion);
        if let Some(delay) = motion
            .as_ref()
            .and_then(|motion| motion.delay.as_ref())
            .and_then(|delay| delay.normalized().ok())
        {
            self.stage_model = delay.controller;
            self.stage_preset = delay.stage.unwrap_or_else(|| "SGSP46-500".into());
            self.stage_port = delay.port;
            self.stage_direction = delay.direction;
        } else {
            self.stage_model = self.raw_str(
                &["instruments", "delay_stage", "t", "controller"],
                "SHOT302GS",
            );
            self.stage_preset =
                self.raw_str(&["instruments", "delay_stage", "t", "stage"], "SGSP46-500");
            self.stage_port = self.raw_str(&["instruments", "delay_stage", "t", "port"], "COM6");
            self.stage_direction =
                self.raw_f64(&["instruments", "delay_stage", "t", "direction"], 1.) as i32;
        }
        self.stage_draft = (!self.stage_configured).then(|| self.current_stage_draft());
        for (i, key) in ["x", "y"].into_iter().enumerate() {
            let b = ["instruments", "scanner", key];
            self.scanner_configured[i] = self
                .settings
                .raw
                .pointer(&format!("/instruments/scanner/{key}"))
                .is_some_and(Value::is_object);
            let scanner = motion
                .as_ref()
                .and_then(|motion| {
                    if i == 0 {
                        motion.x.as_ref()
                    } else {
                        motion.y.as_ref()
                    }
                })
                .and_then(|scanner| scanner.normalized().ok());
            if let Some(scanner) = scanner {
                self.scanner_model[i] = scanner.controller;
                self.scanner_actuator[i] = scanner
                    .actuator
                    .unwrap_or_else(|| self.raw_str(&[b[0], b[1], b[2], "actuator"], ""));
                self.scanner_port[i] = scanner.port;
                self.scanner_axis[i] = scanner.axis;
                self.scanner_scale[i] = scanner.sample_um_per_unit;
                self.hysteresis[i] = scanner.software_hysteresis.enabled;
                self.hysteresis_distance[i] =
                    scanner.software_hysteresis.distance_um.unwrap_or(100.);
                self.hysteresis_direction[i] = scanner
                    .software_hysteresis
                    .direction
                    .unwrap_or_else(|| "negative".into());
            } else {
                self.scanner_model[i] =
                    self.raw_str(&[b[0], b[1], b[2], "controller"], "CONEXAGAP");
                self.scanner_actuator[i] =
                    self.raw_str(&[b[0], b[1], b[2], "actuator"], "AG-M100D");
                self.scanner_port[i] = self.raw_str(&[b[0], b[1], b[2], "port"], "COM11");
                self.scanner_axis[i] = self
                    .settings
                    .raw
                    .pointer(&format!("/instruments/scanner/{key}/axis"))
                    .map(|v| {
                        v.as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| v.to_string())
                    })
                    .unwrap_or_else(|| if i == 0 { "U".into() } else { "V".into() });
                self.scanner_scale[i] = self.raw_f64(&[b[0], b[1], b[2], "sample_um_per_unit"], 1.);
                self.hysteresis[i] =
                    self.raw_bool(&[b[0], b[1], b[2], "software_hysteresis", "enabled"], false);
                self.hysteresis_distance[i] = self.raw_f64(
                    &[b[0], b[1], b[2], "software_hysteresis", "distance_um"],
                    100.,
                );
                self.hysteresis_direction[i] = self.raw_str(
                    &[b[0], b[1], b[2], "software_hysteresis", "direction"],
                    "negative",
                );
            }
            self.scanner_draft[i] =
                (!self.scanner_configured[i]).then(|| self.current_scanner_draft(i));
        }
    }
    fn hydrate_output(&mut self, workflow: Workflow) {
        let fallback = default_output(workflow);
        let profile = &mut self.outputs[Self::ix(workflow)];
        *profile = hydrated_output(&self.settings.raw, workflow, fallback);
    }
    fn sync_output(&mut self, workflow: Workflow) {
        let profile = self.outputs[Self::ix(workflow)].clone();
        let root = ["measurements", workflow.key(), "output"];
        Self::path_set(
            &mut self.settings.raw,
            &[root[0], root[1], root[2], "dir"],
            json!(profile.dir),
        );
        Self::path_set(
            &mut self.settings.raw,
            &[root[0], root[1], root[2], "filename"],
            json!(profile.filename),
        );
        Self::path_set(
            &mut self.settings.raw,
            &[root[0], root[1], root[2], "auto_timestamp_suffix"],
            json!(profile.auto_timestamp_suffix),
        );
        if let Some(settings) = self
            .settings
            .raw
            .pointer_mut(&format!("/measurements/{}", workflow.key()))
            .and_then(Value::as_object_mut)
        {
            for key in ["output_dir", "filename", "auto_timestamp_suffix"] {
                settings.remove(key);
            }
            if let Some(output) = settings.get_mut("output").and_then(Value::as_object_mut) {
                output.remove("output_dir");
            }
        }
    }
    fn sync_all_outputs(&mut self) {
        for workflow in Workflow::ALL {
            self.sync_output(workflow);
        }
    }
    fn hydrate_lockin(&mut self) {
        let value = |key: &str| self.snapshot.lockin.get(key).and_then(Value::as_f64);
        if let Some(v) = value("Sensitivity") {
            self.sensitivity = v;
        }
        if let Some(v) = value("Time Constant") {
            self.time_constant = v;
        }
        if let Some(v) = value("AC Gain") {
            self.ac_gain = v;
        }
        if let Some(v) = self.snapshot.lockin.get("Slope").and_then(Value::as_i64) {
            self.slope = v;
        }
        if let Some(v) = self.snapshot.lockin.get("Coupling").and_then(Value::as_str) {
            self.coupling = v.into();
        }
    }
    fn sync(&mut self) {
        self.settings.normalize_profile_aliases();
        Self::path_set(
            &mut self.settings.raw,
            &["instruments", "lockin", "main", "model"],
            json!(self.model.name()),
        );
        Self::path_set(
            &mut self.settings.raw,
            &["instruments", "lockin", "main", "resource"],
            json!(self.lockin_resource),
        );
        if self.stage_configured || self.stage_has_user_configuration() {
            self.stage_configured = true;
            self.stage_draft = None;
            Self::path_set(
                &mut self.settings.raw,
                &["instruments", "delay_stage", "t", "controller"],
                json!(self.stage_model),
            );
            Self::path_set(
                &mut self.settings.raw,
                &["instruments", "delay_stage", "t", "stage"],
                json!(self.stage_preset),
            );
            Self::path_set(
                &mut self.settings.raw,
                &["instruments", "delay_stage", "t", "port"],
                json!(self.stage_port),
            );
            Self::path_set(
                &mut self.settings.raw,
                &["instruments", "delay_stage", "t", "direction"],
                json!(self.stage_direction),
            );
        }
        for (i, key) in ["x", "y"].into_iter().enumerate() {
            if !self.scanner_configured[i] && !self.scanner_has_user_configuration(i) {
                continue;
            }
            self.scanner_configured[i] = true;
            self.scanner_draft[i] = None;
            let p = ["instruments", "scanner", key];
            Self::path_set(
                &mut self.settings.raw,
                &[p[0], p[1], p[2], "controller"],
                json!(self.scanner_model[i]),
            );
            Self::path_set(
                &mut self.settings.raw,
                &[p[0], p[1], p[2], "actuator"],
                json!(self.scanner_actuator[i]),
            );
            Self::path_set(
                &mut self.settings.raw,
                &[p[0], p[1], p[2], "port"],
                json!(self.scanner_port[i]),
            );
            let axis = if self.scanner_model[i] == "CONEXCC" {
                json!(self.scanner_axis[i].parse::<u8>().unwrap_or(1))
            } else {
                json!(self.scanner_axis[i])
            };
            Self::path_set(&mut self.settings.raw, &[p[0], p[1], p[2], "axis"], axis);
            Self::path_set(
                &mut self.settings.raw,
                &[p[0], p[1], p[2], "sample_um_per_unit"],
                json!(self.scanner_scale[i]),
            );
            Self::path_set(
                &mut self.settings.raw,
                &[p[0], p[1], p[2], "software_hysteresis", "enabled"],
                json!(self.hysteresis[i]),
            );
            Self::path_set(
                &mut self.settings.raw,
                &[p[0], p[1], p[2], "software_hysteresis", "distance_um"],
                json!(self.hysteresis_distance[i]),
            );
            Self::path_set(
                &mut self.settings.raw,
                &[p[0], p[1], p[2], "software_hysteresis", "direction"],
                json!(self.hysteresis_direction[i]),
            );
        }
        for (i, axis) in ["t", "x", "y"].into_iter().enumerate() {
            Self::path_set(
                &mut self.settings.raw,
                &["measurements", "move_abs", "targets", axis],
                json!(self.manual[i]),
            );
            Self::path_set(
                &mut self.settings.raw,
                &[
                    "measurements",
                    "move_abs",
                    "targets",
                    &format!("{axis}_cor"),
                ],
                json!(self.corrected_manual[i]),
            );
        }
        self.sync_all_outputs();
    }
    fn sync_all_plans(&mut self) -> Result<(), String> {
        self.sync();
        // The profile has one shared physical zero.  The currently selected
        // workflow is its authoritative editor; copying it before validation
        // prevents a later tab from silently overwriting that edit on Save.
        sync_shared_coordinates(&mut self.plans, self.workflow);
        for workflow in Workflow::ALL {
            let index = Self::ix(workflow);
            let plan = self.plans[index].clone();
            self.settings
                .set_plan(&plan)
                .map_err(|error| format!("{} profile: {error}", workflow.name()))?;
        }
        self.settings.hardware().map(|_| ())
    }
    fn config(&mut self) -> Result<engine::HardwareConfig, String> {
        self.sync();
        let p = self.plan().clone();
        self.settings.set_plan(&p)?;
        self.settings.hardware()
    }
    fn source(&self) -> Source {
        if self.source_demo {
            Source::Demo
        } else {
            Source::Gpib {
                model: self.model,
                address: self.address,
            }
        }
    }
    fn stage_has_user_configuration(&self) -> bool {
        self.stage_draft
            .as_ref()
            .is_some_and(|draft| draft != &self.current_stage_draft())
    }
    fn scanner_has_user_configuration(&self, index: usize) -> bool {
        self.scanner_draft[index]
            .as_ref()
            .is_some_and(|draft| draft != &self.current_scanner_draft(index))
    }
    fn current_stage_draft(&self) -> StageDraft {
        StageDraft {
            model: self.stage_model.clone(),
            preset: self.stage_preset.clone(),
            port: self.stage_port.clone(),
            direction: self.stage_direction,
        }
    }
    fn current_scanner_draft(&self, index: usize) -> ScannerDraft {
        ScannerDraft {
            model: self.scanner_model[index].clone(),
            actuator: self.scanner_actuator[index].clone(),
            port: self.scanner_port[index].clone(),
            axis: self.scanner_axis[index].clone(),
            scale: self.scanner_scale[index],
            hysteresis: self.hysteresis[index],
            hysteresis_distance: self.hysteresis_distance[index],
            hysteresis_direction: self.hysteresis_direction[index].clone(),
        }
    }
    fn configured_devices(&self) -> Vec<Device> {
        let mut devices = vec![Device::Lockin];
        if self.stage_configured || self.stage_has_user_configuration() {
            devices.push(Device::T);
        }
        if self.scanner_configured[0] || self.scanner_has_user_configuration(0) {
            devices.push(Device::X);
        }
        if self.scanner_configured[1] || self.scanner_has_user_configuration(1) {
            devices.push(Device::Y);
        }
        devices
    }
    fn connect(&mut self, d: Vec<Device>) {
        if self.demo_viewer {
            for device in d {
                self.snapshot.devices[match device {
                    Device::Lockin => 0,
                    Device::T => 1,
                    Device::X => 2,
                    Device::Y => 3,
                }] = true;
            }
            self.snapshot.transport = "CSV replay · no hardware".into();
            self.note("Demo connected · no hardware");
            return;
        }
        if self.source_demo {
            self.note("Select GPIB to connect hardware");
            return;
        }
        if d.contains(&Device::Lockin) {
            let Some(address) = Self::gpib_address(&self.lockin_resource) else {
                self.note("Lock-in resource must be GPIB0::<1-30>::INSTR");
                return;
            };
            self.address = address;
        }
        if d.contains(&Device::T) {
            self.stage_configured = true;
            self.stage_draft = None;
        }
        if d.contains(&Device::X) {
            self.scanner_configured[0] = true;
            self.scanner_draft[0] = None;
        }
        if d.contains(&Device::Y) {
            self.scanner_configured[1] = true;
            self.scanner_draft[1] = None;
        }
        let reset_live = d.contains(&Device::Lockin);
        let r = self.config().and_then(|c| {
            self.controller.send(Command::Connect {
                config: c,
                devices: d,
            })
        });
        match r {
            Ok(_) => {
                if reset_live {
                    self.live.clear();
                }
                self.note("Connecting…");
            }
            Err(e) => self.note(e),
        }
    }
    fn disconnect(&mut self, d: Vec<Device>) {
        if self.demo_viewer {
            self.demo_stop();
            self.snapshot.devices = [false; 4];
            self.live.clear();
            self.note("Demo disconnected");
            return;
        }
        if d.contains(&Device::Lockin) {
            self.live.clear();
        }
        if let Err(e) = self.controller.send(Command::Disconnect { devices: d }) {
            self.note(e)
        }
    }
    fn start(&mut self) {
        if self.demo_viewer {
            self.demo_start();
            return;
        }
        self.select_plot_axes();
        let profile = self.outputs[Self::ix(self.workflow)].clone();
        let suffix = if profile.auto_timestamp_suffix {
            format!("_{}", time::OffsetDateTime::now_utc().unix_timestamp())
        } else {
            "".into()
        };
        let source = self.source();
        let name = output_filename(&profile, matches!(source, Source::Demo));
        let out = Some(PathBuf::from(profile.dir.trim()).join(format!("{name}{suffix}.csv")));
        let plan = self.plan().clone();
        let r = self.config().and_then(|c| {
            self.controller.send(Command::Run {
                source,
                config: c,
                plan: plan.clone(),
                output: out.clone(),
            })
        });
        match r {
            Ok(_) => {
                self.active_live_origin = if self.source_demo {
                    engine::ReadingOrigin::Demo
                } else {
                    engine::ReadingOrigin::Hardware
                };
                self.trace.clear();
                self.plot_epoch = self.plot_epoch.wrapping_add(1);
                let index = Self::ix(self.workflow);
                self.saved_traces[index].clear();
                self.trace_plans[index] = Some(plan);
                self.output_paths[index] = out;
                self.active_workflow = self.workflow;
                // Disable axis edits immediately, including before the worker's first state.
                self.snapshot.running = true;
                self.scan_progress[index] = (0, 0);
                self.eta.start();
                self.note("Measurement started")
            }
            Err(e) => self.note(e),
        }
    }
    fn replay(&mut self) {
        let path = PathBuf::from(self.replay_path.trim());
        if self.replay_path.trim().is_empty() {
            self.note("Choose a CSV file to replay");
            return;
        }
        match std::env::current_exe().and_then(|exe| {
            std::process::Command::new(exe)
                .arg("--demo")
                .arg(path)
                .spawn()
        }) {
            Ok(_) => self.note("Opened CSV demo"),
            Err(error) => self.note(format!("Open demo: {error}")),
        }
    }
    fn apply_snapshot(&mut self, s: Snapshot) {
        let completed_run = self.snapshot.running && !s.running;
        self.live
            .connection_changed(self.snapshot.devices[0], s.devices[0]);
        if self.snapshot.running && !s.running && s.total == 0 && s.phase == "Complete" {
            let action = if self.snapshot.phase.starts_with("Initializing") {
                "Initialization complete"
            } else {
                "Move complete"
            };
            self.note(action);
        }
        self.snapshot = s;
        if (self.snapshot.running || completed_run) && self.snapshot.total > 0 {
            self.scan_progress[Self::ix(self.active_workflow)] =
                (self.snapshot.rows, self.snapshot.total);
        }
        self.hydrate_lockin();
        if !self.snapshot.running
            && self.snapshot.devices[0]
            && !self.source_demo
            && self.snapshot.signal_origin == Some(engine::ReadingOrigin::Hardware)
            && (self.live.origin() != Some(engine::ReadingOrigin::Hardware)
                || self.live.signal().is_none())
            && let Some(signal) = self.snapshot.signal
        {
            self.live.push(signal, engine::ReadingOrigin::Hardware);
        }
    }
    fn receive(&mut self) {
        for _ in 0..512 {
            match self.controller.events.try_recv() {
                Ok(Event::Sample(s)) => {
                    self.snapshot.signal = Some(s.signal);
                    self.snapshot.position = s.position.clone();
                    self.snapshot.rows = self.snapshot.rows.saturating_add(1);
                    self.scan_progress[Self::ix(self.active_workflow)].0 = self.snapshot.rows;
                    self.eta.observe(
                        self.snapshot.rows,
                        s.elapsed,
                        s.captured_at.unwrap_or_else(Instant::now),
                    );
                    if self.active_live_origin == engine::ReadingOrigin::Replay {
                        self.live.push(s.signal, self.active_live_origin);
                    }
                    let trace = if self.active_workflow == self.workflow {
                        &mut self.trace
                    } else {
                        &mut self.saved_traces[Self::ix(self.active_workflow)]
                    };
                    trace.push(s);
                    if trace.len() > MAX_TRACE {
                        trace.drain(..MAX_TRACE / 2);
                    }
                }
                Ok(Event::Live(reading)) => {
                    self.snapshot.signal = Some(reading.signal);
                    self.snapshot.signal_origin = Some(reading.origin);
                    self.snapshot.position = reading.position;
                    self.live.push(reading.signal, reading.origin);
                }
                Ok(Event::Phase(phase)) => self.snapshot.phase = phase,
                Ok(Event::State(s)) => self.apply_snapshot(*s),
                Ok(Event::Finished(r)) => match r {
                    Ok((n, p)) => self.note(format!(
                        "Finished · {n} rows{}",
                        p.map(|x| format!(" · {}", x.display())).unwrap_or_default()
                    )),
                    Err(e) => self.note(e),
                },
                Ok(Event::Error(e)) => self.note(e),
                Err(_) => break,
            }
        }
    }
    fn smoke_json(&self, smoke: &Smoke, pass: bool, error: Option<String>) -> Result<(), String> {
        std::fs::create_dir_all(&smoke.output).map_err(|e| e.to_string())?;
        let labels = self.mode.labels();
        let data = json!({
            "pass": pass,
            "rows": self.trace.len(),
            "plot_count": presentation::plot_count(self.workflow),
            "layout": "compact-fixed",
            "layouts": Workflow::ALL.map(|workflow|json!({"workflow":workflow.key(),"plot_count":presentation::plot_count(workflow),"screenshot":format!("layout_{}.png",workflow.key())})),
            "central_width_px":self.central_width,
            "mode_labels": labels,
            "last_signal": self.trace.last().map(|sample| sample.signal),
            "error": error,
        });
        std::fs::write(
            smoke.output.join("smoke.json"),
            serde_json::to_vec_pretty(&data).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }
    fn save_smoke_screenshot(
        &self,
        ctx: &egui::Context,
        smoke: &Smoke,
        name: &str,
    ) -> Result<bool, String> {
        for event in ctx.input(|input| input.raw.events.clone()) {
            if let egui::Event::Screenshot { image, .. } = event {
                std::fs::create_dir_all(&smoke.output).map_err(|e| e.to_string())?;
                let bytes: Vec<u8> = image
                    .pixels
                    .iter()
                    .flat_map(|pixel| pixel.to_array())
                    .collect();
                let rgba =
                    image::RgbaImage::from_raw(image.width() as u32, image.height() as u32, bytes)
                        .ok_or("invalid screenshot pixels")?;
                rgba.save(smoke.output.join(name))
                    .map_err(|e| e.to_string())?;
                std::fs::write(
                    smoke.output.join(name).with_extension("json"),
                    serde_json::to_vec_pretty(
                        &json!({"workflow":self.workflow.key(),"geometry":self.geometry}),
                    )
                    .map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                return Ok(true);
            }
        }
        Ok(false)
    }
    fn smoke_tick(&mut self, ctx: &egui::Context) {
        let Some(mut smoke) = self.smoke.take() else {
            return;
        };
        let mut close = false;
        if smoke.started.elapsed() > Duration::from_secs(20) {
            smoke.failure = Some("timed out after 20 seconds".into());
        }
        match smoke.stage {
            SmokeStage::Start => {
                self.source_demo = true;
                self.switch_workflow(Workflow::Trkr);
                self.active_workflow = Workflow::Trkr;
                let plan = self.pm();
                plan.workflow = Workflow::Trkr;
                plan.fast = Axis::T;
                plan.fast_range = Range {
                    min: -50.,
                    max: 300.,
                    step: 5.,
                };
                plan.interval_s = 0.01;
                let run_plan = self.plan().clone();
                let output = smoke.output.join("trkr_gui_smoke.csv");
                self.trace_plans[Self::ix(self.workflow)] = Some(run_plan.clone());
                self.output_paths[Self::ix(self.workflow)] = Some(output.clone());
                let launch = std::fs::create_dir_all(&smoke.output)
                    .map_err(|e| e.to_string())
                    .and_then(|_| self.config())
                    .and_then(|config| {
                        self.controller.send(Command::Run {
                            source: Source::Demo,
                            config,
                            plan: run_plan,
                            output: Some(output),
                        })
                    });
                match launch {
                    Ok(()) => {
                        self.trace.clear();
                        smoke.stage = SmokeStage::Wait;
                    }
                    Err(e) => smoke.failure = Some(e),
                }
            }
            SmokeStage::Wait => {
                if self.central_width < 400.0 {
                    smoke.failure = Some(format!(
                        "central plot area is too narrow for validation: {:.0} px",
                        self.central_width
                    ));
                } else if !self.trace.is_empty() && !self.snapshot.running {
                    self.mode = DisplayMode::Xy;
                    smoke.requested = true;
                    smoke.stage = SmokeStage::CaptureXy;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                }
            }
            SmokeStage::CaptureXy => match self.save_smoke_screenshot(ctx, &smoke, "xy.png") {
                Ok(true) => {
                    self.mode = DisplayMode::RTheta;
                    smoke.requested = false;
                    smoke.rtheta_frames = 0;
                    smoke.stage = SmokeStage::CaptureRtheta;
                }
                Ok(false) => {}
                Err(e) => smoke.failure = Some(e),
            },
            SmokeStage::CaptureRtheta => {
                smoke.rtheta_frames = smoke.rtheta_frames.saturating_add(1);
                if smoke.rtheta_frames >= 3 && !smoke.requested {
                    smoke.requested = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                }
                if smoke.requested {
                    match self.save_smoke_screenshot(ctx, &smoke, "rtheta.png") {
                        Ok(true) => {
                            smoke.stage = SmokeStage::CaptureWorkflow {
                                index: 0,
                                frames: 0,
                                requested: false,
                            }
                        }
                        Ok(false) => {}
                        Err(e) => smoke.failure = Some(e),
                    }
                }
            }
            SmokeStage::CaptureWorkflow {
                index,
                mut frames,
                mut requested,
            } => {
                let workflow = Workflow::ALL[index];
                if frames == 0 {
                    // Public documentation captures show only synthetic status,
                    // never the temporary absolute CSV destination.
                    self.log.clear();
                    self.note("DEMO — synthetic data · no hardware");
                    self.switch_workflow(workflow);
                    if !smoke.layout_only && workflow != Workflow::Trkr {
                        let mut plan = self.plan().clone();
                        plan.fast_range = Range {
                            min: -2.,
                            max: 2.,
                            step: 1.,
                        };
                        plan.slow_range = Range {
                            min: -1.,
                            max: 1.,
                            step: 1.,
                        };
                        plan.n_points = 20;
                        plan.interval_s = 0.01;
                        if let Ok(targets) = plan.targets() {
                            self.trace = targets
                                .into_iter()
                                .enumerate()
                                .map(|(i, target)| Sample {
                                    recording: Default::default(),
                                    timing: Default::default(),
                                    captured_at: None,
                                    elapsed: i as f64 * 0.01,
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
                            self.trace_plans[Self::ix(workflow)] = Some(plan);
                        }
                    }
                }
                frames = frames.saturating_add(1);
                if frames >= 3 && !requested {
                    requested = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                }
                let name = format!("layout_{}.png", workflow.key());
                match self.save_smoke_screenshot(ctx, &smoke, &name) {
                    Ok(true) if requested => {
                        if index + 1 == Workflow::ALL.len() {
                            self.switch_workflow(Workflow::Trkr);
                            smoke.stage = SmokeStage::Done;
                        } else {
                            smoke.stage = SmokeStage::CaptureWorkflow {
                                index: index + 1,
                                frames: 0,
                                requested: false,
                            };
                        }
                    }
                    Err(error) => smoke.failure = Some(error),
                    _ => {
                        smoke.stage = SmokeStage::CaptureWorkflow {
                            index,
                            frames,
                            requested,
                        }
                    }
                }
            }
            SmokeStage::Done => {}
        }
        if let Some(error) = smoke.failure.take() {
            let _ = self.smoke_json(&smoke, false, Some(error));
            smoke.success.store(false, Ordering::Release);
            close = true;
        } else if matches!(smoke.stage, SmokeStage::Done) {
            match self.smoke_json(&smoke, true, None) {
                Ok(()) => smoke.success.store(true, Ordering::Release),
                Err(_) => smoke.success.store(false, Ordering::Release),
            }
            close = true;
        }
        if close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            self.smoke = Some(smoke);
        }
    }
    fn device_height(&self, index: usize) -> f32 {
        if self.device_expanded[index] {
            [348., 336., 368., 368.][index]
        } else if index == 0 {
            234.
        } else {
            158.
        }
    }
    fn significant(value: f64, digits: usize) -> String {
        if !value.is_finite() {
            return "-".into();
        }
        if value == 0. {
            return "0".into();
        }
        let exponent = value.abs().log10().floor() as i32;
        let text = if exponent >= digits as i32 || exponent < -4 {
            format!("{:.*e}", digits.saturating_sub(1), value)
        } else {
            format!(
                "{:.*}",
                (digits as i32 - 1 - exponent).max(0) as usize,
                value
            )
        };
        if let Some((mantissa, exponent)) = text.split_once('e') {
            let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
            let exponent = exponent.parse::<i32>().unwrap_or_default();
            format!("{mantissa}e{exponent}")
        } else if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            text
        }
    }
    fn time_constant_text(value: f64) -> String {
        if value < 1. {
            format!("{} ms", Self::significant(value * 1e3, 3))
        } else {
            format!("{} s", Self::significant(value, 3))
        }
    }
    fn gpib_address(resource: &str) -> Option<u8> {
        let mut parts = resource.split("::");
        let (Some(interface), Some(address), Some(instrument), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return None;
        };
        let parsed = address.parse::<u8>().ok()?;
        (interface == "GPIB0" && instrument == "INSTR" && address == parsed.to_string())
            .then_some(parsed)
            .filter(|address| (1..=30).contains(address))
    }
    fn refresh_gpib(&mut self) {
        if self.demo_viewer || self.gpib_refresh.is_some() || self.snapshot.running {
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(trkr_transport::available_gpib_resources());
        });
        self.gpib_refresh = Some(receiver);
        self.note("Searching GPIB resources…");
    }
    fn poll_gpib_refresh(&mut self) {
        let Some(result) = self.gpib_refresh.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return;
        };
        self.gpib_refresh = None;
        match result {
            Ok(mut resources) => {
                resources.sort();
                resources.dedup();
                let count = resources.len();
                if !self.lockin_resource.is_empty() && !resources.contains(&self.lockin_resource) {
                    resources.insert(0, self.lockin_resource.clone());
                }
                if self.lockin_resource.is_empty()
                    && let Some(first) = resources.first()
                {
                    self.lockin_resource = first.clone();
                }
                self.gpib_resources = resources;
                self.note(format!("Found {count} GPIB resource(s)"));
            }
            Err(error) => self.note(error),
        }
    }
    fn refresh_ports(&mut self) {
        if self.demo_viewer {
            return;
        }
        if self.port_refresh.is_some() {
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(trkr_motion::available_ports());
        });
        self.port_refresh = Some(receiver);
        self.note("Refreshing serial ports…");
    }
    fn poll_port_refresh(&mut self) {
        let result = self
            .port_refresh
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok());
        let Some(result) = result else {
            return;
        };
        self.port_refresh = None;
        match result {
            Ok(ports) => {
                self.available_ports = ports;
                self.note(if self.available_ports.is_empty() {
                    "No serial ports found".to_owned()
                } else {
                    format!("Found {} serial port(s)", self.available_ports.len())
                });
            }
            Err(error) => self.note(error),
        }
    }
    fn lockin(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let phase = device_phase(
            self.snapshot.devices[0],
            self.snapshot.running,
            &self.snapshot.phase,
            Device::Lockin,
            self.active_plan(),
        );
        let caption = device_caption(self.snapshot.devices[0], self.model.name(), phase);
        self.poll_gpib_refresh();
        let sensitivity = self
            .snapshot
            .lockin
            .get("Sensitivity")
            .and_then(Value::as_f64);
        let (voltage_scale, voltage_unit) = sensitivity
            .map(presentation::voltage_scale_from_sensitivity)
            .unwrap_or_else(|| self.demo_voltage_scale());
        let overload = self
            .snapshot
            .lockin
            .get("Overload")
            .and_then(|value| {
                value
                    .as_bool()
                    .or_else(|| value.get("input_overload").and_then(Value::as_bool))
                    .or_else(|| value.get("overload").and_then(Value::as_bool))
            })
            .map(|overloaded| if overloaded { "OVERLOAD" } else { "-" })
            .unwrap_or("-");
        let time_constant = self
            .snapshot
            .lockin
            .get("Time Constant")
            .and_then(Value::as_f64)
            .map(Self::time_constant_text)
            .unwrap_or_else(|| "-".into());
        let ref_freq = self
            .snapshot
            .lockin
            .get("Ref. Freq")
            .or_else(|| self.snapshot.lockin.get("Frequency"))
            .and_then(Value::as_f64)
            .map(|value| format!("{} Hz", Self::significant(value, 6)))
            .unwrap_or_else(|| "-".into());
        let mut connect = false;
        let mut disconnect = false;
        let mut read_settings = false;
        {
            let mut form = precision::Form::new(ui, &mut self.geometry, rect.min);
            form.enabled = !self.demo_viewer;
            let w = rect.width();
            form.group(
                "lockin_group",
                precision::rect(0., 0., w, rect.height()),
                "Lock-in",
            );
            form.toggle(
                "lockin_device",
                precision::rect(11., 27., 71., 24.),
                &mut self.device_expanded[0],
                "Device",
            );
            form.value(
                "lockin_name",
                precision::rect(102., 27., w - 212., 24.),
                caption,
                if phase.is_some() {
                    Color32::from_rgb(109, 219, 210)
                } else {
                    TEXT
                },
            );
            form.ui.painter().circle_filled(
                rect.min + Vec2::new(93., 39.),
                3.,
                if self.snapshot.devices[0] {
                    Color32::from_rgb(100, 196, 146)
                } else {
                    Color32::GRAY
                },
            );
            if self.device_expanded[0] {
                let models = Model::ALL.map(|model| (model, model.name()));
                form.label(
                    "lockin_model_label",
                    precision::rect(20., 66., 48., 26.),
                    "Model",
                );
                form.combo(
                    "lockin_model_combo",
                    precision::rect(74., 66., w - 94., 26.),
                    &mut self.model,
                    &models,
                );
                form.label(
                    "lockin_resource_label",
                    precision::rect(20., 98., 48., 26.),
                    "Resource",
                );
                let resources = &self.gpib_resources;
                form.editable_combo(
                    "lockin_resource_combo",
                    precision::rect(74., 98., w - 181., 26.),
                    &mut self.lockin_resource,
                    resources,
                );
                read_settings = form.button(
                    "lockin_refresh_button",
                    precision::rect(w - 101., 98., 81., 26.),
                    "Refresh",
                    self.gpib_refresh.is_none() && !self.snapshot.running,
                );
                let button_width = (w - 100.) * 0.5;
                connect = form.button(
                    "lockin_connect_button",
                    precision::rect(74., 130., button_width, 26.),
                    "Connect",
                    !self.snapshot.devices[0],
                );
                disconnect = form.button(
                    "lockin_disconnect_button",
                    precision::rect(80. + button_width, 130., button_width, 26.),
                    "Disconnect",
                    self.snapshot.devices[0],
                );
            }
            let extra = if self.device_expanded[0] { 114. } else { 0. };
            form.enabled = true;
            form.combo(
                "signal_mode_combo",
                precision::rect(w - 103., 27., 92., 24.),
                &mut self.mode,
                &[(DisplayMode::Xy, "X / Y"), (DisplayMode::RTheta, "R / θ")],
            );
            let origin = self.live.origin();
            let live = self.live.recent()
                && match origin {
                    Some(engine::ReadingOrigin::Hardware) => self.snapshot.devices[0],
                    Some(_) => self.snapshot.running,
                    None => false,
                };
            let half = (w - 28.) * 0.5;
            for channel in 0..2 {
                let x = 11. + channel as f32 * (half + 6.);
                let area = form.at(
                    &format!("lockin_live_{channel}"),
                    precision::rect(x, 54. + extra, half, 140.),
                );
                crate::live_view::draw(
                    form.ui,
                    area,
                    &self.live,
                    self.mode,
                    channel,
                    match origin {
                        Some(engine::ReadingOrigin::Hardware) => self.snapshot.devices[0],
                        Some(_) => self.snapshot.running,
                        None => false,
                    },
                );
            }
            let sens = sensitivity
                .map(|v| format!("{} {voltage_unit}", Self::significant(v * voltage_scale, 3)))
                .unwrap_or_else(|| "-".into());
            let state = if overload == "OVERLOAD" {
                "OVERLOAD"
            } else if live {
                match origin {
                    Some(engine::ReadingOrigin::Demo) => "SIM",
                    Some(engine::ReadingOrigin::Replay) => "REPLAY",
                    _ => "LIVE",
                }
            } else if self.live.signal().is_some() {
                "HELD"
            } else {
                "-"
            };
            for (index, (name, value)) in [
                ("Sens", sens),
                ("TC", time_constant),
                ("Freq", ref_freq),
                ("Status", state.into()),
            ]
            .into_iter()
            .enumerate()
            {
                let x = 11. + (index % 2) as f32 * (half + 6.);
                let y = 198. + extra + (index / 2) as f32 * 13.;
                for (key, local, text, color) in [
                    (
                        format!("lockin_status_{index}_label"),
                        precision::rect(x, y, 39., 13.),
                        name.to_owned(),
                        Color32::GRAY,
                    ),
                    (
                        [
                            "sensitivity_label",
                            "tc_label",
                            "freq_label",
                            "overload_label",
                        ][index]
                            .into(),
                        precision::rect(x + 42., y, half - 42., 13.),
                        value,
                        if index == 3 && state == "OVERLOAD" {
                            Color32::from_rgb(235, 100, 100)
                        } else {
                            TEXT
                        },
                    ),
                ] {
                    let r = form.at(&key, local);
                    form.ui
                        .painter()
                        .with_clip_rect(r.intersect(form.ui.clip_rect()))
                        .text(
                            r.left_center(),
                            egui::Align2::LEFT_CENTER,
                            text,
                            egui::FontId::proportional(10.),
                            color,
                        );
                }
            }
        }
        if connect {
            match Self::gpib_address(&self.lockin_resource) {
                Some(address) => {
                    self.address = address;
                    self.source_demo = false;
                    self.connect(vec![Device::Lockin]);
                }
                None => self.note("Lock-in resource must be GPIB0::<1-30>::INSTR"),
            }
        }
        if disconnect {
            self.disconnect(vec![Device::Lockin]);
        }
        if read_settings {
            self.refresh_gpib();
        }
    }
    fn motion(&mut self, ui: &mut egui::Ui, i: usize, title: &str, d: Device, rect: egui::Rect) {
        let phase = device_phase(
            self.snapshot.devices[i + 1],
            self.snapshot.running,
            &self.snapshot.phase,
            d,
            self.active_plan(),
        );
        self.poll_port_refresh();
        let unit = if i == 0 { "ps" } else { "um" };
        let axis = ["t", "x", "y"][i];
        let physical = self.snapshot.position.physical[i];
        let zero = self.plan().zero[i];
        let corrected = physical.map(|current| current - zero);
        let mut zero_edit = zero;
        let value = |value: Option<f64>| {
            value
                .map(|number| format!("{number:.3}"))
                .unwrap_or_else(|| "-".into())
        };
        let mut connect = false;
        let mut disconnect = false;
        let mut initialize = false;
        let mut use_current = false;
        let mut move_to = None;
        let mut move_corrected = false;
        let mut refresh = false;
        {
            let mut form = precision::Form::new(ui, &mut self.geometry, rect.min);
            form.enabled = !self.demo_viewer;
            let w = rect.width();
            form.group(
                &format!("motion_{axis}_group"),
                precision::rect(0., 0., w, rect.height()),
                title,
            );
            form.toggle(
                &format!("{axis}_device"),
                precision::rect(11., 27., 71., 24.),
                &mut self.device_expanded[i + 1],
                "Device",
            );
            let instrument_name = if !self.snapshot.devices[i + 1] {
                "----".into()
            } else if i == 0 {
                self.stage_preset.clone()
            } else {
                match self.scanner_model[i - 1].as_str() {
                    "CONEXCC" => "CONEX-CC".into(),
                    "CONEXAGAP" => "CONEX-AGAP".into(),
                    other => other.into(),
                }
            };
            form.value(
                &format!("{axis}_device_name"),
                precision::rect(102., 27., w - 113., 24.),
                device_caption(self.snapshot.devices[i + 1], &instrument_name, phase),
                if phase.is_some() {
                    Color32::from_rgb(109, 219, 210)
                } else {
                    TEXT
                },
            );
            form.ui.painter().circle_filled(
                rect.min + Vec2::new(93., 39.),
                3.,
                if self.snapshot.devices[i + 1] {
                    Color32::from_rgb(100, 196, 146)
                } else {
                    Color32::GRAY
                },
            );
            if self.device_expanded[i + 1] {
                if i == 0 {
                    let controllers =
                        ["SHOT302GS", "GSC01", "GSC01A"].map(|name| (name.to_owned(), name));
                    let stages = ["SGSP46-500", "OSMS20-35", "OSMS26-300", "OSMS33-600"]
                        .map(|name| (name.to_owned(), name));
                    let mut direction = self.stage_direction as f64;
                    form.label(
                        "t_controller_label",
                        precision::rect(20., 66., 52., 26.),
                        "Controller",
                    );
                    form.combo(
                        "t_controller_combo",
                        precision::rect(78., 66., w - 98., 26.),
                        &mut self.stage_model,
                        &controllers,
                    );
                    form.label(
                        "t_stage_label",
                        precision::rect(20., 98., 52., 26.),
                        "Stage",
                    );
                    form.combo(
                        "t_stage_combo",
                        precision::rect(78., 98., w - 98., 26.),
                        &mut self.stage_preset,
                        &stages,
                    );
                    form.label(
                        "t_direction_label",
                        precision::rect(20., 130., 52., 26.),
                        "Direction",
                    );
                    form.number(
                        "t_direction_spin",
                        precision::rect(78., 130., w - 98., 26.),
                        &mut direction,
                        1.,
                        0,
                        true,
                    );
                    self.stage_direction = direction.round().clamp(0., 1.) as i32;
                    form.label("t_port_label", precision::rect(20., 162., 52., 26.), "Port");
                    form.editable_combo(
                        "t_port_combo",
                        precision::rect(78., 162., w - 185., 26.),
                        &mut self.stage_port,
                        &self.available_ports,
                    );
                    refresh = form.button(
                        "t_port_refresh_button",
                        precision::rect(w - 101., 162., 81., 26.),
                        "Refresh",
                        !self.snapshot.running,
                    );
                    let width = (w - 110.) / 3.;
                    connect = form.button(
                        "t_connect_button",
                        precision::rect(78., 194., width, 26.),
                        "Connect",
                        !self.snapshot.devices[i + 1],
                    );
                    disconnect = form.button(
                        "t_disconnect_button",
                        precision::rect(84. + width, 194., width, 26.),
                        "Disconnect",
                        self.snapshot.devices[i + 1],
                    );
                    initialize = form.button(
                        "t_initialize_button",
                        precision::rect(90. + width * 2., 194., width, 26.),
                        "Initialize",
                        !self.source_demo && !self.snapshot.running,
                    );
                } else {
                    let scanner = i - 1;
                    let controllers = ["CONEXAGAP", "CONEXCC"].map(|name| (name.to_owned(), name));
                    form.label(
                        &format!("{axis}_controller_label"),
                        precision::rect(20., 66., 88., 26.),
                        "Controller",
                    );
                    form.combo(
                        &format!("{axis}_controller_combo"),
                        precision::rect(114., 66., w - 134., 26.),
                        &mut self.scanner_model[scanner],
                        &controllers,
                    );
                    form.label(
                        &format!("{axis}_actuator_label"),
                        precision::rect(20., 98., 88., 26.),
                        "Actuator",
                    );
                    form.text(
                        &format!("{axis}_actuator_combo"),
                        precision::rect(114., 98., w - 134., 26.),
                        &mut self.scanner_actuator[scanner],
                    );
                    form.label(
                        &format!("{axis}_axis_label"),
                        precision::rect(20., 130., 88., 26.),
                        "Axis",
                    );
                    if self.scanner_model[scanner] == "CONEXCC" {
                        let mut controller_axis =
                            self.scanner_axis[scanner].parse::<f64>().unwrap_or(1.);
                        form.number(
                            &format!("{axis}_axis_spin"),
                            precision::rect(114., 130., w - 134., 26.),
                            &mut controller_axis,
                            1.,
                            0,
                            true,
                        );
                        self.scanner_axis[scanner] = controller_axis.round().max(1.).to_string();
                    } else {
                        form.text(
                            &format!("{axis}_axis_spin"),
                            precision::rect(114., 130., w - 134., 26.),
                            &mut self.scanner_axis[scanner],
                        );
                    }
                    form.label(
                        &format!("{axis}_scale_label"),
                        precision::rect(20., 162., 88., 26.),
                        "Scale (um/unit)",
                    );
                    form.number(
                        &format!("{axis}_scale_spin"),
                        precision::rect(114., 162., w - 134., 26.),
                        &mut self.scanner_scale[scanner],
                        1.,
                        3,
                        !self.snapshot.running,
                    );
                    form.label(
                        &format!("{axis}_port_label"),
                        precision::rect(20., 194., 88., 26.),
                        "Port",
                    );
                    form.editable_combo(
                        &format!("{axis}_port_combo"),
                        precision::rect(114., 194., w - 221., 26.),
                        &mut self.scanner_port[scanner],
                        &self.available_ports,
                    );
                    refresh = form.button(
                        &format!("{axis}_port_refresh_button"),
                        precision::rect(w - 101., 194., 81., 26.),
                        "Refresh",
                        !self.snapshot.running,
                    );
                    let width = (w - 146.) / 3.;
                    connect = form.button(
                        &format!("{axis}_connect_button"),
                        precision::rect(114., 226., width, 26.),
                        "Connect",
                        !self.snapshot.devices[i + 1],
                    );
                    disconnect = form.button(
                        &format!("{axis}_disconnect_button"),
                        precision::rect(120. + width, 226., width, 26.),
                        "Disconnect",
                        self.snapshot.devices[i + 1],
                    );
                    initialize = form.button(
                        &format!("{axis}_initialize_button"),
                        precision::rect(126. + width * 2., 226., width, 26.),
                        "Initialize",
                        !self.source_demo && !self.snapshot.running,
                    );
                }
            }
            let rows_y = if self.device_expanded[i + 1] {
                if i == 0 { 235. } else { 267. }
            } else {
                57.
            };
            let numeric_width = if w >= 470. {
                (w * 0.23).round()
            } else if w < 350. {
                81.
            } else {
                93.
            };
            let button_width = if w >= 470. {
                (w * 0.2286).round()
            } else if self.device_expanded[i + 1] {
                85.
            } else {
                81.
            };
            let input_x = w - 11. - button_width - 6. - numeric_width;
            let button_x = input_x + numeric_width + 6.;
            for row in 0..3 {
                let y = rows_y + row as f32 * 32.;
                let (label, current) = match row {
                    0 => (format!("{axis} ({unit})"), value(physical)),
                    1 => (format!("offset ({unit})"), format!("{zero_edit:.3}")),
                    _ => (format!("{axis}_cor ({unit})"), value(corrected)),
                };
                form.label(
                    &format!("{axis}_row_{row}_label"),
                    precision::rect(11., y, 70., 26.),
                    label,
                );
                form.value(
                    &format!("{axis}_row_{row}_value"),
                    precision::rect(86., y, input_x - 92., 26.),
                    current,
                    TEXT,
                );
                match row {
                    0 => form.number(
                        &format!("move_{axis}_spin"),
                        precision::rect(input_x, y, numeric_width, 26.),
                        &mut self.manual[i],
                        1.,
                        3,
                        self.snapshot.devices[i + 1] && !self.snapshot.running,
                    ),
                    1 => form.number(
                        &format!("{axis}_zero_spin"),
                        precision::rect(input_x, y, numeric_width, 26.),
                        &mut zero_edit,
                        1.,
                        3,
                        self.snapshot.devices[i + 1] && !self.snapshot.running,
                    ),
                    _ => form.number(
                        &format!("{axis}_cor_spin"),
                        precision::rect(input_x, y, numeric_width, 26.),
                        &mut self.corrected_manual[i],
                        1.,
                        3,
                        self.snapshot.devices[i + 1] && !self.snapshot.running,
                    ),
                }
                let text = if row == 1 { "Use Current" } else { "Move" };
                let enabled = if row == 1 {
                    physical.is_some() && !self.snapshot.running
                } else {
                    self.snapshot.devices[i + 1] && !self.source_demo && !self.snapshot.running
                };
                let clicked = form.button(
                    &match row {
                        0 => format!("move_{axis}_button"),
                        1 => format!("use_current_{axis}_button"),
                        _ => format!("{axis}_cor_button"),
                    },
                    precision::rect(button_x, y, button_width, 26.),
                    text,
                    enabled,
                );
                if row == 0 && clicked {
                    move_to = Some(self.manual[i]);
                }
                if row == 1 && clicked {
                    use_current = true;
                }
                if row == 2 && clicked {
                    move_corrected = true;
                }
            }
        }
        if refresh {
            self.refresh_ports();
        }
        self.pm().zero[i] = zero_edit;
        if use_current && let Some(current) = physical {
            self.pm().zero[i] = current;
        }
        if move_corrected {
            self.manual[i] = self.plan().zero[i] + self.corrected_manual[i];
            move_to = Some(self.manual[i]);
        }
        if let Some(target) = move_to {
            self.move_axis(i, target);
        }
        if connect {
            self.source_demo = self.demo_viewer;
            self.connect(vec![d]);
        }
        if disconnect {
            self.disconnect(vec![d]);
        }
        if initialize {
            self.initialize_axis = Some(i);
        }
    }
    fn initialize_confirmation(&mut self, ctx: &egui::Context) {
        let Some(axis) = self.initialize_axis else {
            return;
        };
        let name = ["Stage (t)", "Scanner X", "Scanner Y"][axis];
        let homes = initialization_homes(axis, &self.scanner_model);
        egui::Modal::new(egui::Id::new("initialize-motion")).show(ctx, |ui| {
            ui.heading(format!("Initialize {name}"));
            ui.label(if homes {
                "Home the axis, then move to instrument zero (not the experiment zero). The optical position will change."
            } else {
                "Enable closed-loop operation, then move the CONEX-AGAP scanner to instrument zero. The optical position will change."
            });
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    self.initialize_axis = None;
                }
                if ui.add_enabled(!self.snapshot.running, egui::Button::new(if homes { "Home & initialize" } else { "Initialize & zero" })).clicked() {
                    self.initialize_axis = None;
                    match self.config().and_then(|config| self.controller.send(Command::Initialize {
                        config, axis, home: homes, move_to_origin: true,
                    })) {
                        Ok(_) => self.note(format!("Initializing {name}{}…", if homes { " · homing" } else { "" })),
                        Err(error) => self.note(error),
                    }
                }
            });
        });
    }
    fn move_axis(&mut self, axis: usize, target: f64) {
        let targets = std::array::from_fn(|i| if i == axis { Some(target) } else { None });
        let zero = [0.; 3];
        match self.config().and_then(|config| {
            self.controller.send(Command::Move {
                config,
                targets,
                coordinate: Coordinate::Measurement,
                zero,
                approach: engine::spatial_approach(),
            })
        }) {
            Ok(_) => self.note("Moving…"),
            Err(error) => self.note(error),
        }
    }
    fn select_plot_axes(&mut self) {
        if self.demo_viewer {
            return;
        }
        let index = Self::ix(self.workflow);
        let desired = self.plans[index].clone();
        if self.trace_plans[index]
            .as_ref()
            .is_some_and(|plan| crate::plot_history::History::same_selection(plan, &desired))
        {
            return;
        }
        let current = self.trace_plans[index]
            .take()
            .map(|plan| crate::plot_history::Trace {
                plan,
                samples: std::mem::take(&mut self.trace),
                progress: self.scan_progress[index],
                output: self.output_paths[index].take(),
            });
        let selected = self.plot_history.select(current, &desired);
        self.trace = selected.samples;
        self.trace_plans[index] = Some(selected.plan);
        self.scan_progress[index] = selected.progress;
        self.output_paths[index] = selected.output;
        self.plot_epoch = self.plot_epoch.wrapping_add(1);
        self.map_cache = [None, None];
    }
    fn switch_workflow(&mut self, workflow: Workflow) {
        if workflow == self.workflow {
            return;
        }
        let previous = self.workflow;
        if !self.demo_viewer {
            sync_shared_coordinates(&mut self.plans, previous);
            self.sync_output(previous);
        }
        self.saved_traces[Self::ix(previous)] = std::mem::take(&mut self.trace);
        self.trace = std::mem::take(&mut self.saved_traces[Self::ix(workflow)]);
        self.workflow = workflow;
        self.select_plot_axes();
        if self.demo_viewer {
            self.demo_output(workflow);
        } else {
            self.hydrate_output(workflow);
        }
        self.map_cache = [None, None];
    }
    fn begin_dialog(&mut self, kind: DialogKind, initial: String) {
        if self.dialog.is_none() {
            self.dialog = Some(PendingDialog {
                kind,
                workflow: self.workflow,
                source: self.output_paths[Self::ix(self.workflow)].clone(),
                receiver: file_dialog::open(kind, &initial),
            });
        }
    }
    fn poll_dialog(&mut self) {
        let result = self
            .dialog
            .as_ref()
            .and_then(|dialog| match dialog.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Some(Err("File dialog closed unexpectedly".into()))
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
            });
        if let Some(result) = result {
            let dialog = self.dialog.take().expect("pending dialog");
            match result {
                Ok(Some(path)) => match dialog.kind {
                    DialogKind::Config => self.config_path = path.display().to_string(),
                    DialogKind::Directory => {
                        self.outputs[Self::ix(dialog.workflow)].dir = path.display().to_string()
                    }
                    DialogKind::ReplayCsv => {
                        self.demo_select(dialog.workflow, path);
                    }
                    DialogKind::SaveCsv => {
                        let result = dialog
                            .source
                            .as_ref()
                            .ok_or_else(|| "No recorded CSV available".to_string())
                            .and_then(|source| {
                                copy_complete_csv(source, &path, self.snapshot.running)
                            });
                        match result {
                            Ok(()) => self.note(format!("Saved snapshot: {}", path.display())),
                            Err(error) => self.note(error),
                        }
                    }
                },
                Ok(None) => {}
                Err(error) => self.note(error),
            }
        }
    }
    fn measure(&mut self, ui: &mut egui::Ui, area: egui::Rect) {
        use precision::{Form, rect};
        if !self.demo_viewer {
            self.sync();
        }
        let mut plan = self.plan().clone();
        let running = self.snapshot.running;
        let mut selected = self.workflow;
        let tab_widths = [102., 51., 51., 57., 68.];
        let mut x = area.min.x;
        for (i, workflow) in Workflow::ALL.into_iter().enumerate() {
            let r = rect(x, area.min.y, tab_widths[i], 24.);
            precision::record(&mut self.geometry, &format!("tab_{}", workflow.key()), r);
            if ui
                .scope(|ui| {
                    if self.demo_viewer && (running || workflow == Workflow::SignalMonitor) {
                        ui.disable();
                    }
                    ui.put(
                        r,
                        egui::Button::new(workflow.name())
                            .selected(workflow == selected)
                            .min_size(r.size())
                            .corner_radius(6),
                    )
                    .clicked()
                })
                .inner
            {
                selected = workflow;
            }
            x += tab_widths[i];
        }
        self.switch_workflow(selected);
        if selected != plan.workflow {
            plan = self.plan().clone();
        }
        precision::record(&mut self.geometry, "measurement_tabs", area);
        let page = rect(
            area.min.x + 2.,
            area.min.y + 26.,
            area.width() - 6.,
            area.height() - 30.,
        );
        ui.painter().rect(
            page,
            10.,
            PANEL,
            Stroke::new(1_f32, BORDER),
            egui::StrokeKind::Inside,
        );
        // The measurement panel deliberately stays useful in the fixed-width
        // centre column.  Keep a readable output field, but reserve the rest
        // for the scan controls instead of stretching labels and inputs.
        let side = 250_f32.min((page.width() - 8. - 216.).max(180.));
        let settings = page.width() - 24. - side;
        let side_x = settings + 12.;
        let tc = self
            .snapshot
            .lockin
            .get("Time Constant")
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite() && *v >= 0.)
            .map(|v| v * 4.);

        let hints_fast = self.scan_hints(plan.fast, &plan);
        let hints_slow = self.scan_hints(plan.slow, &plan);
        let scan_column_width = ((settings - 24.) * 0.5).round();
        let (start, stop, save, browse);
        {
            let mut form = Form::new(ui, &mut self.geometry, page.min);
            form.enabled = !running && !self.demo_viewer;
            if plan.workflow == Workflow::SignalMonitor {
                form.label(
                    "signal_interval_label",
                    rect(9., 9., 58., 26.),
                    "Interval (s)",
                );
                form.number(
                    "signal_interval_spin",
                    rect(73., 9., 120., 26.),
                    &mut plan.interval_s,
                    1.,
                    2,
                    true,
                );
                form.label("signal_points_label", rect(9., 41., 58., 26.), "Points");
                let mut points = plan.n_points as f64;
                form.number(
                    "signal_points_spin",
                    rect(73., 41., 120., 26.),
                    &mut points,
                    1.,
                    0,
                    true,
                );
                plan.n_points = points.clamp(1., trkr_core::MAX_TOTAL_POINTS as f64) as usize;
            } else if !plan.workflow.is_2d() {
                let prefix = plan.workflow.key();
                let label = 66.;
                let field = 9. + label + 6.;
                let input = 88.;
                form.label(
                    &format!("{prefix}_axis_label"),
                    rect(9., 9., label, 26.),
                    "Axis",
                );
                let options = if plan.workflow == Workflow::Trkr {
                    vec![(Axis::T, "t")]
                } else {
                    vec![
                        (Axis::X, "x"),
                        (Axis::Y, "y"),
                        (Axis::U, "u"),
                        (Axis::V, "v"),
                    ]
                };
                form.combo(
                    &format!("{prefix}_axis_combo"),
                    rect(field, 9., 60., 26.),
                    &mut plan.fast,
                    &options,
                );
                if matches!(plan.fast, Axis::U | Axis::V) {
                    let theta_x = field + 60. + 4.;
                    form.label("spatial_theta_label", rect(theta_x, 9., 32., 26.), "θ (°)");
                    form.number(
                        "spatial_theta_spin",
                        rect(theta_x + 36., 9., 68., 26.),
                        &mut plan.theta_deg,
                        1.,
                        2,
                        true,
                    );
                }
                let unit = scan_unit(plan.fast);
                let labels = [
                    format!("Min ({unit})"),
                    format!("Max ({unit})"),
                    format!("Step ({unit})"),
                ];
                for (row, (key, value)) in [
                    ("min", &mut plan.fast_range.min),
                    ("max", &mut plan.fast_range.max),
                    ("step", &mut plan.fast_range.step),
                ]
                .into_iter()
                .enumerate()
                {
                    let y = 41. + row as f32 * 32.;
                    form.label(
                        &format!("{prefix}_{key}_label"),
                        rect(9., y, label, 26.),
                        &labels[row],
                    );
                    form.number(
                        &format!("{prefix}_{key}_spin"),
                        rect(field, y, input, 26.),
                        value,
                        1.,
                        3,
                        true,
                    );
                    form.value(
                        &format!("{prefix}_{key}_hint"),
                        rect(
                            field + input + 6.,
                            y,
                            scan_column_width - label - input - 12.,
                            26.,
                        ),
                        &hints_fast[row],
                        Color32::GRAY,
                    );
                }
                form.label(
                    &format!("{prefix}_wait_label"),
                    rect(9., 137., label, 26.),
                    "Wait (s)",
                );
                form.enabled = !running;
                form.number(
                    &format!("{prefix}_wait_spin"),
                    rect(field, 137., 72., 26.),
                    &mut plan.interval_s,
                    1.,
                    2,
                    true,
                );
                form.enabled = !running && !self.demo_viewer;
                if form.button(
                    &format!("{prefix}_tc_button"),
                    rect(field + 78., 137., 76., 26.),
                    "TC × 4",
                    tc.is_some(),
                ) && let Some(wait) = tc
                {
                    plan.interval_s = wait;
                }
            } else {
                let column = ((settings - 18. - 6.) * 0.5).round();
                for fast in [true, false] {
                    let origin = if fast { 9. } else { 9. + column + 6. };
                    let width = if fast {
                        column
                    } else {
                        settings - 18. - 6. - column
                    };
                    Self::precise_scan_column(
                        &mut form,
                        &mut plan,
                        ScanColumn {
                            fast,
                            x: origin,
                            width,
                            tc,
                            replay_wait: self.demo_viewer && !running,
                            hints: if fast { &hints_fast } else { &hints_slow },
                        },
                    );
                }
                if let Some(warning) =
                    axis_choices::pair_warning(plan.workflow, plan.fast, plan.slow)
                {
                    form.value(
                        "invalid_axis_pair",
                        rect(9., 169., settings - 18., 16.),
                        warning,
                        Color32::from_rgb(245, 183, 82),
                    );
                }
            }
            form.enabled = !running && !self.demo_viewer;
            form.group(
                "output_group",
                rect(side_x, 12., side, 96.),
                if self.demo_viewer {
                    "Replay CSV"
                } else {
                    "Output"
                },
            );
            let output = &mut self.outputs[Self::ix(self.workflow)];
            form.label(
                "output_directory_label",
                rect(side_x + 11., 39., 24., 26.),
                "Dir",
            );
            form.text(
                "output_dir_edit",
                rect(side_x + 41., 39., side - 112., 26.),
                &mut output.dir,
            );
            form.enabled = !running;
            browse = form.button(
                "output_browse_button",
                rect(side_x + side - 65., 39., 54., 26.),
                "…",
                true,
            );
            form.enabled = !running && !self.demo_viewer;
            form.label(
                "output_file_label",
                rect(side_x + 11., 71., 24., 26.),
                "File",
            );
            form.text(
                "output_name_edit",
                rect(side_x + 41., 71., side - 112., 26.),
                &mut output.filename,
            );
            form.check(
                "auto_suffix_check",
                rect(side_x + side - 65., 72., 54., 24.),
                &mut output.auto_timestamp_suffix,
                "Auto",
            );
            form.group("run_group", rect(side_x, 120., side, 64.), "Run");
            let usable = side - 22. - 12.;
            let first = (usable / 3.).round();
            let second = (usable * 2. / 3.).round() - first;
            let third = usable - first - second;
            form.enabled = true;
            start = form.button(
                "start_button",
                rect(side_x + 11., 147., first, 26.),
                "Start",
                !running
                    && (!plan.workflow.is_2d()
                        || axis_choices::valid_pair(plan.workflow, plan.fast, plan.slow))
                    && (!self.demo_viewer
                        || (self.snapshot.devices[0]
                            && self.demo.datasets[Self::ix(self.workflow)].is_some()
                            && self.demo.pending.is_none()))
                    && (self.source_demo
                        || (self.snapshot.devices[0]
                            && engine::active_axes(&plan)
                                .iter()
                                .enumerate()
                                .all(|(i, needed)| !needed || self.snapshot.devices[i + 1]))),
            );
            stop = form.button(
                "stop_button",
                rect(side_x + 17. + first, 147., second, 26.),
                "Stop",
                running,
            );
            save = form.button(
                "save_rows_button",
                rect(side_x + 23. + first + second, 147., third, 26.),
                if self.demo_viewer { "Load" } else { "Save Now" },
                if self.demo_viewer {
                    !running
                        && self.demo.pending.is_none()
                        && !self.demo.paths[Self::ix(self.workflow)]
                            .as_os_str()
                            .is_empty()
                } else {
                    self.output_paths[Self::ix(self.workflow)].is_some()
                },
            );
            precision::record(
                form.geometry,
                "output_run_widget",
                rect(side_x, 0., side, page.height()).translate(page.min.to_vec2()),
            );
        }
        self.plans[Self::ix(self.workflow)] = plan;
        self.select_plot_axes();
        if browse {
            self.begin_dialog(
                if self.demo_viewer {
                    DialogKind::ReplayCsv
                } else {
                    DialogKind::Directory
                },
                if self.demo_viewer {
                    self.demo.paths[Self::ix(self.workflow)]
                        .display()
                        .to_string()
                } else {
                    self.outputs[Self::ix(self.workflow)].dir.clone()
                },
            );
        }
        if start {
            self.start();
        }
        if stop {
            if self.demo_viewer {
                self.demo_stop();
            } else {
                self.controller.stop();
                self.note("Stopping…");
            }
        }
        if save && self.demo_viewer {
            self.demo_load(
                self.demo.paths[Self::ix(self.workflow)].clone(),
                Some(self.workflow),
            );
        }
        if save && !self.demo_viewer {
            let out = &self.outputs[Self::ix(self.workflow)];
            self.begin_dialog(
                DialogKind::SaveCsv,
                PathBuf::from(&out.dir)
                    .join(format!("{}_snapshot.csv", out.filename))
                    .display()
                    .to_string(),
            );
        }
    }
    fn precise_scan_column(form: &mut precision::Form<'_>, p: &mut Plan, column: ScanColumn<'_>) {
        let ScanColumn {
            fast,
            x,
            width,
            tc,
            replay_wait,
            hints,
        } = column;
        use precision::rect;
        let prefix = p.workflow.key();
        let role = if fast { "fast" } else { "slow" };
        // All tabs reserve the same two-column scan area. Units and limits
        // stay visible beside each numeric field without shifting Output/Run.
        let label = 66.;
        let field = x + label + 6.;
        let input = 88.;
        form.label(
            &format!("{prefix}_{role}_label"),
            rect(x, 9., label, 26.),
            if fast { "Fast" } else { "Slow" },
        );
        let choices = if fast {
            axis_choices::fast_options(p.workflow)
        } else {
            axis_choices::slow_options(p.workflow, p.fast)
        };
        let options: Vec<_> = choices
            .iter()
            .copied()
            .map(|axis| (axis, axis.name()))
            .collect();
        let previous = if fast { p.fast } else { p.slow };
        let axis_input = 60.;
        if fast {
            form.combo(
                &format!("{prefix}_{role}_axis_combo"),
                rect(field, 9., axis_input, 26.),
                &mut p.fast,
                &options,
            );
            if p.fast != previous {
                let chosen = p.fast;
                p.fast = previous;
                axis_choices::apply_fast_choice(p, chosen);
            }
        } else {
            form.combo(
                &format!("{prefix}_{role}_axis_combo"),
                rect(field, 9., axis_input, 26.),
                &mut p.slow,
                &options,
            );
        }
        let selected = if fast { p.fast } else { p.slow };
        if show_scan_theta(p.workflow, selected) {
            let theta_x = field + axis_input + 4.;
            form.label(
                &format!("{prefix}_{role}_theta_label"),
                rect(theta_x, 9., 32., 26.),
                "θ (°)",
            );
            form.number(
                &format!("{prefix}_{role}_theta"),
                rect(theta_x + 36., 9., 68., 26.),
                &mut p.theta_deg,
                1.,
                2,
                true,
            );
        }
        let unit = scan_unit(selected);
        let r = if fast {
            &mut p.fast_range
        } else {
            &mut p.slow_range
        };
        for (row, (key, value)) in [
            ("min", &mut r.min),
            ("max", &mut r.max),
            ("step", &mut r.step),
        ]
        .into_iter()
        .enumerate()
        {
            let y = 41. + row as f32 * 32.;
            form.label(
                &format!("{prefix}_{role}_{key}_label"),
                rect(x, y, label, 26.),
                format!(
                    "{} ({unit})",
                    match key {
                        "min" => "Min",
                        "max" => "Max",
                        _ => "Step",
                    }
                ),
            );
            form.number(
                &format!("{prefix}_{role}_{key}"),
                rect(field, y, input, 26.),
                value,
                1.,
                3,
                true,
            );
            form.value(
                &format!("{prefix}_{role}_{key}_hint"),
                rect(field + input + 6., y, width - label - input - 12., 26.),
                &hints[row],
                Color32::GRAY,
            );
        }
        if fast {
            form.label(
                &format!("{prefix}_wait_label"),
                rect(x, 137., label, 26.),
                "Wait (s)",
            );
            let enabled = form.enabled;
            form.enabled |= replay_wait;
            form.number(
                &format!("{prefix}_wait_spin"),
                rect(field, 137., 72., 26.),
                &mut p.interval_s,
                1.,
                2,
                true,
            );
            form.enabled = enabled;
            if form.button(
                &format!("{prefix}_tc_button"),
                rect(field + 78., 137., 76., 26.),
                "TC × 4",
                tc.is_some(),
            ) && let Some(wait) = tc
            {
                p.interval_s = wait;
            }
        }
    }
    fn scan_hints(&self, axis: Axis, plan: &Plan) -> [String; 3] {
        let unit = scan_unit(axis);
        if self.demo_viewer {
            let r = if axis == plan.fast {
                plan.fast_range
            } else {
                plan.slow_range
            };
            return [
                format!("≥ {} {unit}", Self::significant(r.min, 5)),
                format!("≤ {} {unit}", Self::significant(r.max, 5)),
                format!("≥ {} {unit}", Self::significant(r.step, 5)),
            ];
        }
        if matches!(axis, Axis::U | Axis::V) {
            return std::array::from_fn(|_| "checks x/y".into());
        }
        let config = self.settings.hardware().ok();
        let t_step_hint = config
            .as_ref()
            .and_then(|c| c.motion.delay.as_ref())
            .map(|d| delay_step_hint(d, self.snapshot.delay_calibration.as_ref()));
        let limits = config.and_then(|c| {
            if axis == Axis::T {
                let mut d = c.motion.delay?.normalized().ok()?;
                if let Some(cached) = self
                    .snapshot
                    .delay_calibration
                    .as_ref()
                    .filter(|c| c.configuration.as_ref() == Some(&d))
                {
                    d.pos_um_per_pulse = cached.effective_pos_um_per_pulse.or(d.pos_um_per_pulse);
                }
                let low_mm = d
                    .min_pulse
                    .zip(d.pos_um_per_pulse)
                    .map(|(p, u)| p as f64 * u / 1000.)
                    .unwrap_or(0.);
                let high_mm = d
                    .max_pulse
                    .zip(d.pos_um_per_pulse)
                    .map(|(p, u)| p as f64 * u / 1000.)
                    .or(d.travel_mm)?;
                let a = d.measurement_ps_from_mm(low_mm).ok()? - plan.zero[0];
                let b = d.measurement_ps_from_mm(high_mm).ok()? - plan.zero[0];
                let step = d.pos_um_per_pulse.map(|u| 2. * u / 1000. / 0.299792458);
                Some((a.min(b), a.max(b), step))
            } else if matches!(axis, Axis::X | Axis::Y) {
                let i = if axis == Axis::X { 1 } else { 2 };
                let s = if i == 1 { c.motion.x? } else { c.motion.y? }
                    .normalized()
                    .ok()?;
                let a = s.sample_um_from_raw(s.min_pos?) - plan.zero[i];
                let b = s.sample_um_from_raw(s.max_pos?) - plan.zero[i];
                Some((
                    a.min(b),
                    a.max(b),
                    s.resolution
                        .or(s.min_step)
                        .map(|r| (r * s.sample_um_per_unit).abs()),
                ))
            } else {
                None
            }
        });
        let fmt = |v: Option<f64>| {
            v.map(|v| Self::significant(v, 5))
                .unwrap_or_else(|| "—".into())
        };
        if let Some((min, max, step)) = limits {
            [
                format!("≥ {} {unit}", fmt(Some(min))),
                format!("≤ {} {unit}", fmt(Some(max))),
                if axis == Axis::T {
                    t_step_hint.unwrap_or_else(|| format!("≥ {} {unit}", fmt(step)))
                } else {
                    format!("≥ {} {unit}", fmt(step))
                },
            ]
        } else {
            std::array::from_fn(|_| format!("— {unit}"))
        }
    }

    fn plots(&mut self, ui: &mut egui::Ui, area: egui::Rect) {
        let plan = self.trace_plans[Self::ix(self.workflow)]
            .as_ref()
            .unwrap_or_else(|| self.plan())
            .clone();
        let (scale, unit) = self
            .snapshot
            .lockin
            .get("Sensitivity")
            .and_then(Value::as_f64)
            .map(presentation::voltage_scale_from_sensitivity)
            .unwrap_or_else(|| self.demo_voltage_scale());
        presentation::plots(
            ui,
            &self.trace,
            &plan,
            self.mode,
            &mut self.map_cache,
            &mut self.geometry,
            presentation::PlotViewport {
                area,
                epoch: self.plot_epoch,
                y_log: self.trkr_log_y,
                voltage_scale: scale,
                voltage_unit: unit,
                history: &self.plot_history,
            },
        );
    }

    fn session(&mut self, ui: &mut egui::Ui, area: egui::Rect) {
        use precision::{Form, rect};
        let (browse, load, save, connect, disconnect);
        {
            let mut f = Form::new(ui, &mut self.geometry, area.min);
            f.enabled = !self.snapshot.running;
            let inner = area.width() - 22.;
            let half = ((inner - 8.) / 2.).round();
            let browse_width = 70.;
            f.group("session_group", rect(0., 0., area.width(), 128.), "Config");
            f.text(
                "config_path",
                rect(11., 27., inner - browse_width - 8., 26.),
                &mut self.config_path,
            );
            browse = f.button(
                "browse_button",
                rect(area.width() - 11. - browse_width, 27., browse_width, 26.),
                "Browse",
                true,
            );
            load = f.button("load_button", rect(11., 59., half, 26.), "Load", true);
            save = f.button(
                "save_button",
                rect(19. + half, 59., inner - 8. - half, 26.),
                "Save",
                !self.demo_viewer,
            );
            connect = f.button(
                "connect_button",
                rect(11., 91., half, 26.),
                "All Connect",
                !self.snapshot.running,
            );
            disconnect = f.button(
                "disconnect_button",
                rect(19. + half, 91., inner - 8. - half, 26.),
                "All Disconnect",
                !self.snapshot.running,
            );
        }
        ui.interact(
            area,
            egui::Id::new("session-mode-menu"),
            egui::Sense::hover(),
        )
        .context_menu(|ui| {
            if !self.demo_viewer && ui.checkbox(&mut self.source_demo, "Demo mode").changed() {
                self.live.clear();
            }
            ui.label("GPIB hardware is selected when Connect is pressed.");
        });
        if browse {
            self.begin_dialog(DialogKind::Config, self.config_path.clone());
        }
        if load && self.demo_viewer {
            self.demo_load_config();
        }
        if load && !self.demo_viewer {
            match Settings::load(std::path::Path::new(&self.config_path)) {
                Ok(s) => {
                    self.settings = s;
                    self.hydrate();
                    self.note("Configuration loaded");
                }
                Err(e) => self.note(e),
            }
        }
        if save {
            match self
                .sync_all_plans()
                .and_then(|_| self.settings.save(std::path::Path::new(&self.config_path)))
            {
                Ok(()) => self.note("Configuration saved"),
                Err(e) => self.note(e),
            }
        }
        if connect {
            self.source_demo = false;
            self.connect(self.configured_devices());
        }
        if disconnect {
            self.disconnect(vec![Device::Lockin, Device::T, Device::X, Device::Y]);
        }
    }
    fn toggle_right_panel(&mut self, ctx: &egui::Context, viewport: egui::Rect) {
        let columns = precision::Columns::new(viewport, self.right_collapsed, self.left_collapsed);
        if !self.right_collapsed {
            self.right_was_maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            if self.right_was_maximized {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
            }
        }
        let width = if self.right_collapsed {
            viewport.width() + self.right_extra_width
        } else {
            self.right_extra_width = (columns.right.width() - 27.).max(0.);
            viewport.width() - self.right_extra_width
        };
        self.right_collapsed = !self.right_collapsed;
        let minimum = precision::Columns::minimum_width(self.right_collapsed, self.left_collapsed);
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(Vec2::new(
            minimum, 700.,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(
            width.max(minimum),
            viewport.height(),
        )));
        if !self.right_collapsed && self.right_was_maximized {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            self.right_was_maximized = false;
        }
    }
    fn active_plan(&self) -> &Plan {
        let index = Self::ix(self.active_workflow);
        self.trace_plans[index]
            .as_ref()
            .unwrap_or(&self.plans[index])
    }
    fn eta_label(&self, now: Instant) -> String {
        if !self.snapshot.running
            || self.active_workflow != self.workflow
            || self.snapshot.total == 0
        {
            return "-".into();
        }
        let total = self.scan_progress[Self::ix(self.workflow)].1;
        if total == 0 {
            return "Estimating…".into();
        }
        if self.snapshot.rows >= total {
            return if self.demo_viewer {
                "Looping…"
            } else if self.snapshot.phase.starts_with("Returning") {
                "Returning…"
            } else {
                "Finishing…"
            }
            .into();
        }
        self.eta
            .remaining(total, now)
            .map(|seconds| format!("~{:.0} s", seconds.ceil()))
            .unwrap_or_else(|| "Estimating…".into())
    }
    fn plot_toolbar(&mut self, ui: &mut egui::Ui, area: egui::Rect) {
        let eta = self.eta_label(Instant::now());
        let mut f = precision::Form::new(ui, &mut self.geometry, area.min);
        f.label("plot_label", precision::rect(0., 0., 28., 20.), "Plot");
        f.label("step_label", precision::rect(48., 0., 28., 20.), "Step");
        let (completed, total) = self.scan_progress[Self::ix(self.workflow)];
        let value = step_progress(completed, total);
        let width = if total == 0 {
            5.
        } else {
            (value.chars().count() as f32 * 7.).max(5.)
        };
        f.label(
            "point_label",
            precision::rect(86., 0., width.max(44.), 20.),
            value,
        );
        f.label(
            "eta_title",
            precision::rect(154. + (width - 44.).max(0.), 0., 28., 20.),
            "ETA",
        );
        f.label(
            "eta_label",
            precision::rect(192. + (width - 44.).max(0.), 0., 100., 20.),
            eta,
        );
        if self.workflow == Workflow::Trkr {
            let x = area.width() - 172.;
            f.label("y_scale_label", precision::rect(x, 0., 14., 16.), "Y");
            f.combo(
                "trkr_y_scale",
                precision::rect(x + 20., 0., 152., 18.),
                &mut self.trkr_log_y,
                &[(false, "Linear"), (true, "Log (y > 0)")],
            );
        }
        if self.demo_viewer {
            f.value(
                "demo_badge",
                precision::rect(
                    316. + (width - 44.).max(0.),
                    0.,
                    (area.width() - 500. - (width - 44.).max(0.)).max(100.),
                    20.,
                ),
                "DEMO · NO HARDWARE",
                Color32::from_rgb(255, 115, 115),
            );
        }
        ui.interact(
            precision::rect(154. + (width - 44.).max(0.), 0., 138., 20.)
                .translate(area.min.to_vec2()),
            egui::Id::new("eta-help"),
            egui::Sense::hover(),
        ).on_hover_text("Approximate time to finish acquiring points, based on the observed run pace, including motion, compensation, reads and CSV writes. Startup and line changes can shift the estimate. Return-to-zero is not timed; it is shown separately. Replay estimates refer to the current loop, not the CSV's original timestamps.");
        ui.interact(
            area,
            egui::Id::new("plot-options-menu"),
            egui::Sense::hover(),
        )
        .context_menu(|ui| {
            if ui.button("Clear plot").clicked() {
                self.plot_epoch = self.plot_epoch.wrapping_add(1);
                self.trace.clear();
                self.plot_history.clear(self.workflow);
                self.map_cache = [None, None];
                ui.close();
            }
            ui.collapsing("Replay CSV", |ui| {
                edit(ui, &mut self.replay_path);
                ui.add(egui::DragValue::new(&mut self.replay_interval_s).prefix("Interval (s) "));
                if ui.button("Replay").clicked() {
                    self.replay();
                    ui.close();
                }
            });
        });
    }
    fn snapshot_table(&self, ui: &mut egui::Ui) {
        let area = ui.max_rect();
        let border = 2_f32;
        let header_height = 24_f32;
        // Measured Windows Qt resizeColumnsToContents() for the standard fields.
        let first_column = if self.trace.is_empty() { 100. } else { 101. };
        let inner = area.shrink(border);
        let needs_scroll = !self.trace.is_empty()
            && (engine::FIELDS.len() - 1) as f32 * 30. > inner.height() - header_height;
        let header = egui::Rect::from_min_size(
            inner.min,
            Vec2::new(
                inner.width() - if needs_scroll { 12. } else { 0. },
                header_height.min(inner.height()),
            ),
        );
        let body = egui::Rect::from_min_max(egui::pos2(inner.min.x, header.max.y), inner.max);
        ui.painter().rect_stroke(
            area,
            0.,
            Stroke::new(1_f32, BORDER),
            egui::StrokeKind::Inside,
        );
        ui.painter()
            .rect_filled(header, 0., Color32::from_rgb(45, 46, 52));
        ui.painter().line_segment(
            [
                egui::pos2(inner.min.x + first_column, inner.min.y),
                egui::pos2(inner.min.x + first_column, inner.max.y),
            ],
            Stroke::new(1_f32, BORDER),
        );
        ui.painter().line_segment(
            [
                egui::pos2(inner.min.x, header.max.y),
                egui::pos2(inner.max.x, header.max.y),
            ],
            Stroke::new(1_f32, BORDER),
        );
        let header_font = egui::FontId::proportional(12.);
        ui.painter().text(
            egui::pos2(inner.min.x + first_column * 0.5, header.center().y),
            egui::Align2::CENTER_CENTER,
            "Field",
            header_font.clone(),
            TEXT,
        );
        ui.painter().text(
            egui::pos2(
                inner.min.x + first_column + (header.width() - first_column) * 0.5,
                header.center().y,
            ),
            egui::Align2::CENTER_CENTER,
            "Value",
            header_font,
            TEXT,
        );
        let format_value = |key: &str, value: &str| {
            if value.is_empty() {
                return "-".into();
            }
            let Ok(value) = value.parse::<f64>() else {
                return value.into();
            };
            match key {
                "X_V" | "Y_V" | "R_V" => format!("{value:.6e}"),
                "Theta_deg" => format!("{value:.3}"),
                "delay_stage_pulse" => format!("{value:.0}"),
                "t_ps"
                | "t_cor_ps"
                | "x_um"
                | "x_cor_um"
                | "y_um"
                | "y_cor_um"
                | "elapsed_s"
                | "acquisition_elapsed_s" => {
                    format!("{value:.3}")
                }
                "delay_stage_mm" | "x_scanner_mm" | "x_scanner_deg" | "y_scanner_mm"
                | "y_scanner_deg" => format!("{value:.6}"),
                _ => format!("{value:.6}"),
            }
        };
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(body)
                .layout(egui::Layout::top_down(egui::Align::Min)),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("snapshot-table-body")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let row_width = ui.available_width();
                        ui.set_min_width(row_width);
                        ui.spacing_mut().item_spacing.y = 0.;
                        if let Some(sample) = self.trace.last() {
                            let plan = self.trace_plans[Self::ix(self.workflow)]
                                .as_ref()
                                .unwrap_or_else(|| self.plan());
                            if let Ok(values) = engine::row(plan, sample) {
                                for (key, value) in engine::FIELDS.iter().zip(values) {
                                    // Sample does not carry its original timestamp. The recorded
                                    // CSV does; do not invent a new timestamp for this display.
                                    if *key == "timestamp" {
                                        continue;
                                    }
                                    let (row, _) = ui.allocate_exact_size(
                                        Vec2::new(row_width, 30.),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().line_segment(
                                        [
                                            egui::pos2(row.min.x, row.max.y),
                                            egui::pos2(row.max.x, row.max.y),
                                        ],
                                        Stroke::new(0.5_f32, BORDER.gamma_multiply(0.25)),
                                    );
                                    ui.painter().text(
                                        row.min + Vec2::new(4., row.height() * 0.5),
                                        egui::Align2::LEFT_CENTER,
                                        if *key == "acquisition_elapsed_s" {
                                            "acq_elapsed_s"
                                        } else {
                                            *key
                                        },
                                        egui::FontId::proportional(12.),
                                        TEXT,
                                    );
                                    ui.painter().text(
                                        egui::pos2(row.min.x + first_column + 4., row.center().y),
                                        egui::Align2::LEFT_CENTER,
                                        format_value(key, &value),
                                        egui::FontId::proportional(12.),
                                        TEXT,
                                    );
                                }
                            }
                        }
                    });
            },
        );
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.receive();
        self.poll_dialog();
        self.initialize_confirmation(ctx);
        self.demo_tick();
        if !self.demo_viewer
            && !self.snapshot.running
            && self.snapshot.devices[0]
            && self.last_settings.elapsed() >= Duration::from_secs(5)
        {
            let _ = self.controller.send(Command::LockinSettings);
            self.last_settings = Instant::now();
        }
        ctx.request_repaint_after(Duration::from_millis(
            if self.snapshot.devices[0] || (self.demo_viewer && self.snapshot.running) {
                16
            } else {
                75
            },
        ));
        self.geometry.clear();
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::from_rgb(18, 19, 23)))
            .show(ctx, |ui| {
                let viewport = ui.max_rect();
                let minimum =
                    precision::Columns::minimum_width(self.right_collapsed, self.left_collapsed);
                ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(Vec2::new(
                    minimum, 700.,
                )));
                if viewport.width() < minimum - 1. {
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(
                        minimum,
                        viewport.height(),
                    )));
                }
                let columns =
                    precision::Columns::new(viewport, self.right_collapsed, self.left_collapsed);
                columns.record(&mut self.geometry);
                self.central_width = columns.plots.width();
                precision::record(&mut self.geometry, "central_widget", viewport);
                if !self.left_collapsed {
                    let content = egui::Rect::from_min_max(
                        columns.left.min + Vec2::new(2., 2.),
                        egui::pos2(columns.left_toggle.min.x - 4., columns.left.max.y - 2.),
                    );
                    let origin = content.min + Vec2::new(9., 9.);
                    let card_width = content.width() - 18.;
                    ui.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| {
                        ui.set_clip_rect(ui.clip_rect().intersect(content));
                        self.session(
                            ui,
                            egui::Rect::from_min_size(origin, Vec2::new(card_width, 128.)),
                        );
                        let device_area = egui::Rect::from_min_max(
                            egui::pos2(content.min.x, origin.y + 136.),
                            content.max,
                        );
                        precision::record(&mut self.geometry, "device_scroll_area", device_area);
                        ui.scope_builder(
                            egui::UiBuilder::new()
                                .max_rect(device_area)
                                .layout(egui::Layout::top_down(egui::Align::Min)),
                            |ui| {
                                ui.set_clip_rect(ui.clip_rect().intersect(device_area));
                                ui.spacing_mut().scroll = egui::style::ScrollStyle::floating();
                                let mut scroll = egui::ScrollArea::vertical()
                                    .id_salt("devices-scroll")
                                    .scroll_bar_visibility(
                                        egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                                    )
                                    .auto_shrink([false, false])
                                    .max_height(device_area.height());
                                if self.devices_scrolled_smoke {
                                    scroll = scroll.vertical_scroll_offset(200.);
                                }
                                scroll.show(ui, |ui| {
                                    if self.snapshot.running && !self.demo_viewer {
                                        ui.disable();
                                    }
                                    let available = ui.max_rect();
                                    let origin = available.min + Vec2::new(9., 0.);
                                    let mut y = origin.y;
                                    self.lockin(
                                        ui,
                                        precision::rect(
                                            origin.x,
                                            y,
                                            card_width,
                                            self.device_height(0),
                                        ),
                                    );
                                    y += self.device_height(0) + 8.;
                                    for (i, title, d) in [
                                        (0, "Delay Stage", Device::T),
                                        (1, "Scanner X", Device::X),
                                        (2, "Scanner Y", Device::Y),
                                    ] {
                                        let height = self.device_height(i + 1);
                                        self.motion(
                                            ui,
                                            i,
                                            title,
                                            d,
                                            precision::rect(origin.x, y, card_width, height),
                                        );
                                        y += height + 8.;
                                    }
                                    ui.set_min_size(Vec2::new(available.width(), y - origin.y));
                                });
                            },
                        );
                    });
                }
                ui.painter().line_segment(
                    [
                        columns.left.right_top() + Vec2::new(4., 0.),
                        columns.left.right_bottom() + Vec2::new(4., 0.),
                    ],
                    Stroke::new(1_f32, BORDER),
                );
                if ui
                    .put(
                        columns.left_toggle,
                        egui::Button::new(if self.left_collapsed { ">" } else { "<" })
                            .min_size(columns.left_toggle.size())
                            .corner_radius(4),
                    )
                    .on_hover_text(if self.left_collapsed {
                        "Show devices"
                    } else {
                        "Hide devices"
                    })
                    .clicked()
                {
                    self.left_collapsed = !self.left_collapsed;
                }
                if ui
                    .put(
                        columns.toggle,
                        egui::Button::new(if self.right_collapsed { "<" } else { ">" })
                            .min_size(columns.toggle.size())
                            .corner_radius(4),
                    )
                    .clicked()
                {
                    self.toggle_right_panel(ctx, viewport);
                }
                if !self.right_collapsed {
                    ui.painter().rect(
                        columns.log,
                        8.,
                        Color32::from_rgb(18, 19, 23),
                        Stroke::new(1_f32, BORDER),
                        egui::StrokeKind::Inside,
                    );
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(columns.log.shrink(4.))
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                        |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("log-scroll")
                                .stick_to_bottom(true)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    for line in &self.log {
                                        ui.add(egui::Label::new(line).wrap());
                                    }
                                });
                        },
                    );
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(columns.snapshot)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                        |ui| self.snapshot_table(ui),
                    );
                }
                self.measure(ui, columns.measurement);
                self.plot_toolbar(ui, columns.toolbar);
                precision::record(
                    &mut self.geometry,
                    match self.workflow {
                        Workflow::Srkr => "srkr_plot_widget",
                        Workflow::Strkr | Workflow::Srkr2d => "scan2d_plot_widget",
                        _ => "standard_plot_widget",
                    },
                    columns.plots,
                );
                self.plots(ui, columns.plots);
            });
        self.smoke_tick(ctx);
        self.demo_smoke_tick(ctx);
    }
}
fn step_progress(completed: usize, total: usize) -> String {
    if total == 0 {
        "-".into()
    } else {
        format!("{completed}/{total}")
    }
}
fn device_caption(connected: bool, name: &str, phase: Option<&str>) -> String {
    if !connected {
        "----".into()
    } else if let Some(phase) = phase {
        format!("{name} · {phase}")
    } else {
        name.into()
    }
}
fn initialization_homes(axis: usize, controllers: &[String; 2]) -> bool {
    axis == 0
        || controllers[axis - 1]
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect::<String>()
            .eq_ignore_ascii_case("CONEXCC")
}
fn device_phase(
    connected: bool,
    running: bool,
    phase: &str,
    device: Device,
    plan: &Plan,
) -> Option<&'static str> {
    if !connected || !running {
        return None;
    }
    if matches!(device, Device::Lockin) {
        return if phase.starts_with("Reading") {
            Some("Reading")
        } else if phase.starts_with("Waiting") {
            Some("Waiting")
        } else {
            None
        };
    }
    let axis = match device {
        Device::T => 0,
        Device::X => 1,
        Device::Y => 2,
        Device::Lockin => unreachable!(),
    };
    if let Some((label, moving)) = phase
        .strip_prefix("Moving ")
        .map(|axes| ("Moving", axes))
        .or_else(|| {
            phase
                .strip_prefix("Initializing ")
                .map(|axes| ("Initializing", axes))
        })
        .or_else(|| {
            phase
                .strip_prefix("Returning ")
                .map(|axes| ("Returning", axes))
        })
    {
        let moving = moving.split_whitespace().next().unwrap_or("");
        return moving
            .split('/')
            .any(|name| match name {
                "t" => axis == 0,
                "x" => axis == 1,
                "y" => axis == 2,
                "u" | "v" => axis > 0,
                _ => false,
            })
            .then_some(label);
    }
    if engine::active_axes(plan)[axis] {
        if phase.starts_with("Waiting") {
            Some("Waiting")
        } else if phase.starts_with("Returning") {
            Some("Returning")
        } else {
            None
        }
    } else {
        None
    }
}
fn copy_complete_csv(
    source: &std::path::Path,
    destination: &std::path::Path,
    run_active: bool,
) -> Result<(), String> {
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let source_metadata_path = source.with_extension("run.json");
    let mut metadata = if source_metadata_path.exists() {
        Some(
            serde_json::from_slice::<Value>(
                &std::fs::read(&source_metadata_path).map_err(|e| e.to_string())?,
            )
            .map_err(|e| format!("Save snapshot metadata: {e}"))?,
        )
    } else {
        None
    };
    let source_audit_path = source_audit_path(source, metadata.as_ref())?;
    if let Some(path) = &source_audit_path
        && !path.exists()
    {
        return Err(format!(
            "Save snapshot: declared readings audit is missing: {}",
            path.display()
        ));
    }

    let destination_metadata = destination.with_extension("run.json");
    let raw_format = source_audit_path
        .as_ref()
        .is_some_and(|path| path.extension().is_some_and(|ext| ext == "csv"));
    let destination_audit = destination.with_extension(if raw_format {
        "raw.csv"
    } else {
        "readings.jsonl"
    });
    // Check every related destination before staging.  This also avoids
    // combining a legacy CSV snapshot with sidecars from another run.
    for path in [
        destination,
        &destination_metadata,
        &destination_audit,
        &destination.with_extension(if raw_format {
            "readings.jsonl"
        } else {
            "raw.csv"
        }),
    ] {
        if path.exists() {
            return Err(format!("Save snapshot: {} already exists", path.display()));
        }
    }

    let (csv, rows) = stage_complete_csv(source, parent)?;
    let audit = source_audit_path
        .as_deref()
        .map(|path| {
            stage_complete_audit(
                path,
                parent,
                rows,
                !run_active
                    && metadata
                        .as_ref()
                        .is_some_and(|value| value["status"] == "error"),
            )
        })
        .transpose()?;
    if let Some(metadata) = &mut metadata
        && raw_format
    {
        metadata["file_hashes"] = json!({"csv_sha256":crate::raw_csv::file_sha256(csv.path())?,
            "raw_csv_sha256":audit.as_ref().map(|file|crate::raw_csv::file_sha256(file.path())).transpose()?});
    }
    let metadata = metadata
        .as_mut()
        .map(|metadata| {
            stage_snapshot_metadata(
                metadata,
                parent,
                rows,
                run_active,
                audit.is_some().then_some(&destination_audit),
            )
        })
        .transpose()?;

    let mut staged = vec![(destination.to_path_buf(), csv)];
    if let Some(metadata) = metadata {
        staged.push((destination_metadata, metadata));
    }
    if let Some(audit) = audit {
        staged.push((destination_audit, audit));
    }
    persist_snapshot_noclobber(staged)
}

fn source_audit_path(
    source: &std::path::Path,
    metadata: Option<&Value>,
) -> Result<Option<PathBuf>, String> {
    let default_path = if source.with_extension("raw.csv").exists() {
        source.with_extension("raw.csv")
    } else {
        source.with_extension("readings.jsonl")
    };
    let Some(metadata) = metadata else {
        return Ok(default_path.exists().then_some(default_path));
    };
    let declared = metadata
        .pointer("/raw_csv/filename")
        .or_else(|| metadata.get("readings_file"))
        .and_then(Value::as_str);
    let Some(declared) = declared else {
        return Ok(default_path.exists().then_some(default_path));
    };
    let declared_path = std::path::Path::new(declared);
    if declared_path.is_absolute()
        || declared_path.file_name().is_none()
        || declared_path != declared_path.file_name().unwrap()
    {
        return Err("Save snapshot: readings_file must be a basename".into());
    }
    let path = source
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(declared_path);
    // Any explicit declaration must remain valid in the saved copy.
    Ok(Some(path))
}

fn stage_complete_csv(
    source: &std::path::Path,
    parent: &std::path::Path,
) -> Result<(tempfile::NamedTempFile, usize), String> {
    use std::io::{BufRead, Read, Write};
    let input = std::fs::File::open(source).map_err(|e| e.to_string())?;
    let length = input.metadata().map_err(|e| e.to_string())?.len();
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|e| format!("Save snapshot: {e}"))?;
    let mut reader = std::io::BufReader::new(input.take(length));
    let mut line = Vec::new();
    let mut complete_lines = 0usize;
    // Native fields are single-line. Capture the starting file length and omit
    // an in-progress final row while preserving original acquisition timestamps.
    loop {
        line.clear();
        if reader
            .read_until(b'\n', &mut line)
            .map_err(|e| e.to_string())?
            == 0
        {
            break;
        }
        if !line.ends_with(b"\n") {
            break;
        }
        temporary.write_all(&line).map_err(|e| e.to_string())?;
        complete_lines += 1;
    }
    temporary
        .flush()
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    Ok((temporary, complete_lines.saturating_sub(1)))
}

fn stage_complete_audit(
    source: &std::path::Path,
    parent: &std::path::Path,
    csv_rows: usize,
    preserve_failed_attempts: bool,
) -> Result<tempfile::NamedTempFile, String> {
    use std::io::{BufRead, Read, Write};
    if source
        .extension()
        .is_some_and(|extension| extension == "csv")
    {
        let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        crate::raw_csv::snapshot(source, &mut temporary, csv_rows, preserve_failed_attempts)?;
        temporary
            .flush()
            .and_then(|()| temporary.as_file().sync_all())
            .map_err(|e| e.to_string())?;
        return Ok(temporary);
    }
    let input = std::fs::File::open(source).map_err(|e| e.to_string())?;
    let length = input.metadata().map_err(|e| e.to_string())?.len();
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|e| format!("Save snapshot: {e}"))?;
    let mut reader = std::io::BufReader::new(input.take(length));
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader
            .read_until(b'\n', &mut line)
            .map_err(|e| e.to_string())?
            == 0
        {
            break;
        }
        if !line.ends_with(b"\n") {
            break;
        }
        let record: Value = serde_json::from_slice(&line)
            .map_err(|error| format!("Save snapshot audit: {error}"))?;
        // A finalized failed run may have a valid raw reading whose CSV write
        // failed. Preserve that evidence even though there is no matching row.
        if preserve_failed_attempts || audit_record_is_in_snapshot(&record, csv_rows) {
            temporary.write_all(&line).map_err(|e| e.to_string())?;
        }
    }
    temporary
        .flush()
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    Ok(temporary)
}

fn audit_record_is_in_snapshot(record: &Value, csv_rows: usize) -> bool {
    // The source can finish after the CSV length was captured. Do not attach
    // a later terminal count to an earlier snapshot of complete CSV rows.
    if record.get("event").and_then(Value::as_str) == Some("finished")
        && record
            .get("rows")
            .and_then(Value::as_u64)
            .is_some_and(|rows| rows > csv_rows as u64)
    {
        return false;
    }
    let Some(point_index) = record.get("point_index").and_then(Value::as_u64) else {
        // Context, schema/header, and other non-point provenance describe the
        // complete snapshot regardless of the current acquisition row.
        return true;
    };
    let status = record.get("status").and_then(Value::as_str);
    if status == Some("error") {
        point_index <= csv_rows.saturating_add(1) as u64
    } else {
        point_index <= csv_rows as u64
    }
}

fn stage_snapshot_metadata(
    metadata: &mut Value,
    parent: &std::path::Path,
    rows: usize,
    run_active: bool,
    destination_audit: Option<&PathBuf>,
) -> Result<tempfile::NamedTempFile, String> {
    use std::io::Write;
    let object = metadata
        .as_object_mut()
        .ok_or_else(|| "Save snapshot metadata must be a JSON object".to_string())?;
    let source_status = object
        .get("status")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let status = if run_active
        || !matches!(
            source_status.as_deref(),
            Some("complete" | "cancelled" | "error")
        ) {
        "snapshot".to_owned()
    } else {
        source_status.expect("matched status")
    };
    object.insert("rows".into(), json!(rows));
    object.insert("status".into(), json!(status));
    if let Some(path) = destination_audit {
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "Save snapshot readings filename must be UTF-8".to_string())?;
        if let Some(raw) = object.get_mut("raw_csv") {
            raw["filename"] = json!(filename);
            raw["path"] = json!(path.display().to_string());
        } else {
            object.insert("readings_file".into(), json!(filename));
        }
    }
    let bytes = serde_json::to_vec_pretty(metadata).map_err(|e| e.to_string())?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|e| format!("Save snapshot: {e}"))?;
    temporary.write_all(&bytes).map_err(|e| e.to_string())?;
    temporary
        .flush()
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    Ok(temporary)
}

fn persist_snapshot_noclobber(
    staged: Vec<(PathBuf, tempfile::NamedTempFile)>,
) -> Result<(), String> {
    let mut created = Vec::new();
    for (destination, temporary) in staged {
        match temporary.persist_noclobber(&destination) {
            Ok(_) => created.push(destination),
            Err(error) => {
                // Do not delete paths by name: another process may have
                // replaced them after activation. Preserve partial evidence.
                return Err(format!(
                    "Save snapshot: {error}; partial snapshot files retained: {created:?}"
                ));
            }
        }
    }
    Ok(())
}

fn default_output(workflow: Workflow) -> OutputProfile {
    let filename = workflow.default_filename();
    OutputProfile {
        dir: output_dir(),
        filename: filename.into(),
        auto_timestamp_suffix: true,
    }
}
/// Read both the nested output object written by the GUI and the flat fields
/// written by legacy Python profiles.  Empty nested strings deliberately fall
/// back, while an explicit nested `false` remains authoritative.
fn hydrated_output(raw: &Value, workflow: Workflow, fallback: OutputProfile) -> OutputProfile {
    let settings = raw.pointer(&format!("/measurements/{}", workflow.key()));
    let output = settings.and_then(|value| value.get("output"));
    let nonempty = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
    };
    let nested = |key: &str| output.and_then(|value| value.get(key));
    OutputProfile {
        dir: nonempty(nested("dir"))
            .or_else(|| nonempty(nested("output_dir")))
            .or_else(|| nonempty(settings.and_then(|value| value.get("output_dir"))))
            .unwrap_or(fallback.dir),
        filename: nonempty(nested("filename"))
            .or_else(|| nonempty(settings.and_then(|value| value.get("filename"))))
            .unwrap_or(fallback.filename),
        auto_timestamp_suffix: nested("auto_timestamp_suffix")
            .and_then(Value::as_bool)
            .or_else(|| {
                settings
                    .and_then(|value| value.get("auto_timestamp_suffix"))
                    .and_then(Value::as_bool)
            })
            .unwrap_or(fallback.auto_timestamp_suffix),
    }
}
fn output_filename(profile: &OutputProfile, demo: bool) -> String {
    let fallback = "trkr_run";
    let name = profile.filename.trim();
    let name = if name.is_empty() { fallback } else { name };
    if demo && !name.to_ascii_lowercase().starts_with("demo_") {
        format!("demo_{name}")
    } else {
        name.to_owned()
    }
}
fn workflow_index(workflow: Workflow) -> usize {
    workflow.index()
}
fn sync_shared_coordinates(plans: &mut [Plan; 5], selected: Workflow) {
    let selected = &plans[workflow_index(selected)];
    let zero = selected.zero;
    let theta_deg = selected.theta_deg;
    for plan in plans {
        plan.zero = zero;
        plan.theta_deg = theta_deg;
    }
}
fn output_dir() -> String {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .map(|home| home.join("Documents/KohdaLab TRKR/runs"))
        .unwrap_or_else(|| PathBuf::from("runs"))
        .display()
        .to_string()
}
fn config_path() -> String {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(".kohdalab/config/trkr-next.json")
        .display()
        .to_string()
}
fn config_default_path() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(".kohdalab/config/default.json")
}

#[cfg(test)]
#[path = "ui/tests.rs"]
mod ui_tests;
