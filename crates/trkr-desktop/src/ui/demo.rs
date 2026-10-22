//! The Python replay GUI's hardware-free successor.
use super::*;
use crate::{
    replay_data::{self, Dataset},
    replay_player::Player,
};
use std::sync::mpsc::{self, Receiver};

type Loaded = Result<Vec<(PathBuf, Dataset)>, String>;
#[derive(Default)]
pub(super) struct Replay {
    pub paths: [PathBuf; 5],
    pub datasets: [Option<Arc<Dataset>>; 5],
    pub pending: Option<Receiver<Loaded>>,
    expected: Option<Workflow>,
    discover: bool,
    folder: Option<PathBuf>,
    magnitude: [f64; 5],
    pub player: Player,
    smoke: Option<DemoSmoke>,
    clock: Option<Instant>,
}
impl App {
    pub fn open_demo(&mut self, path: Option<PathBuf>) {
        if let Some(path) = path {
            if path.is_dir() {
                self.demo_discover(path);
            } else {
                self.demo_load(path, None);
            }
        } else {
            let dir = Some(crate::config::application_resources().join("demo_csv"))
                .filter(|p| p.is_dir())
                .unwrap_or_else(|| PathBuf::from("demo_csv"));
            self.demo_discover(dir);
        }
    }
    fn demo_discover(&mut self, dir: PathBuf) {
        if self.demo.pending.is_some() {
            return;
        }
        self.demo.folder = Some(dir.clone());
        let (tx, rx) = mpsc::channel();
        self.demo.pending = Some(rx);
        self.demo.expected = None;
        self.demo.discover = true;
        std::thread::spawn(move || {
            let result = (|| {
                let mut paths = std::fs::read_dir(&dir)
                    .map_err(|e| format!("Demo CSV folder: {e}"))?
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("csv")))
                    .collect::<Vec<_>>();
                paths.sort();
                let mut loaded = Vec::new();
                for path in paths {
                    let data =
                        replay_data::load(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                    if data.plan.workflow == Workflow::SignalMonitor {
                        continue;
                    }
                    if loaded
                        .iter()
                        .any(|(_, d): &(PathBuf, Dataset)| d.plan.workflow == data.plan.workflow)
                    {
                        return Err(format!(
                            "Multiple {} CSVs in folder; choose one with Browse",
                            data.plan.workflow.name()
                        ));
                    }
                    loaded.push((path, data));
                }
                Ok(loaded)
            })();
            let _ = tx.send(result);
        });
    }
    pub(super) fn demo_select(&mut self, workflow: Workflow, path: PathBuf) {
        let i = Self::ix(workflow);
        self.demo.paths[i] = path;
        self.demo.datasets[i] = None;
        self.saved_traces[i].clear();
        self.trace_plans[i] = None;
        self.scan_progress[i] = (0, 0);
        if workflow == self.workflow {
            self.trace.clear();
            self.map_cache = [None, None];
        }
        self.demo_output(workflow);
    }
    pub(super) fn demo_output(&mut self, workflow: Workflow) {
        let i = Self::ix(workflow);
        let path = &self.demo.paths[i];
        self.outputs[i].dir = path
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        self.outputs[i].filename = path
            .file_name()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.outputs[i].auto_timestamp_suffix = false;
    }
    pub(super) fn demo_load(&mut self, path: PathBuf, expected: Option<Workflow>) {
        if self.demo.pending.is_some() || self.snapshot.running {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.demo.pending = Some(rx);
        self.demo.expected = expected;
        if let Some(w) = expected {
            self.demo.datasets[Self::ix(w)] = None;
        }
        self.demo.discover = false;
        self.note("Loading replay CSV…");
        std::thread::spawn(move || {
            let result = replay_data::load(&path).and_then(|data| {
                if data.plan.workflow == Workflow::SignalMonitor {
                    return Err(
                        "Signal Monitor is unavailable in the Python-compatible replay demo".into(),
                    );
                }
                if expected.is_some_and(|w| w != data.plan.workflow) {
                    return Err(format!(
                        "CSV contains {}; choose the matching tab",
                        data.plan.workflow.name()
                    ));
                }
                Ok(vec![(path, data)])
            });
            let _ = tx.send(result);
        });
    }
    pub(super) fn demo_load_config(&mut self) {
        match Settings::load(std::path::Path::new(&self.config_path)) {
            Ok(settings) => {
                self.settings = settings;
                self.hydrate();
                // Loaded datasets retain their CSV coordinates, not hardware profile ranges.
                for i in 0..5 {
                    if let Some(data) = &self.demo.datasets[i] {
                        let wait = self.plans[i].interval_s;
                        self.plans[i] = data.plan.clone();
                        self.plans[i].interval_s = wait;
                    }
                }
                for workflow in Workflow::ALL {
                    self.demo_output(workflow);
                }
                self.note("Demo configuration loaded · no hardware");
                if let Some(dir) = self.demo.folder.clone() {
                    self.demo_discover(dir);
                }
            }
            Err(e) => self.note(e),
        }
    }
    pub(super) fn demo_start(&mut self) {
        let i = Self::ix(self.workflow);
        if !self.snapshot.devices[0]
            || self.demo.datasets[i].is_none()
            || self.demo.pending.is_some()
        {
            return;
        }
        if !self.plans[i].interval_s.is_finite() || self.plans[i].interval_s < 0. {
            self.note("Wait must be finite and non-negative");
            return;
        }
        self.plot_epoch = self.plot_epoch.wrapping_add(1);
        self.demo.player = Player::default();
        self.demo.clock = Some(Instant::now());
        self.eta.start();
        self.trace.clear();
        self.map_cache = [None, None];
        self.live.clear();
        self.trace_plans[i] = Some(self.plan().clone());
        self.output_paths[i] = None;
        self.active_workflow = self.workflow;
        self.active_live_origin = engine::ReadingOrigin::Replay;
        self.snapshot.running = true;
        self.snapshot.rows = 0;
        self.snapshot.total = self.demo.datasets[i].as_ref().unwrap().samples.len();
        self.scan_progress[i] = (0, self.snapshot.total);
        self.note("Replay started · loops until Stop");
    }
    pub(super) fn demo_stop(&mut self) {
        self.demo.clock = None;
        self.snapshot.running = false;
        self.snapshot.phase = "Stopped".into();
        self.note("Replay stopped");
    }
    pub(super) fn demo_tick(&mut self) {
        if !self.demo_viewer {
            return;
        }
        let result = self
            .demo
            .pending
            .as_ref()
            .and_then(|rx| match rx.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => Some(Err("CSV loader stopped".into())),
                Err(mpsc::TryRecvError::Empty) => None,
            });
        if let Some(result) = result {
            self.demo.pending = None;
            match result {
                Ok(loaded) => {
                    let discover = self.demo.discover;
                    for (path, data) in loaded {
                        let workflow = data.plan.workflow;
                        let i = Self::ix(workflow);
                        self.demo_select(workflow, path);
                        if !discover {
                            if self.demo.expected.is_none() {
                                self.switch_workflow(workflow);
                            }
                            let wait = self.plans[i].interval_s;
                            self.plans[i] = data.plan.clone();
                            self.plans[i].interval_s = wait;
                            self.note(format!(
                                "Loaded {} · {} points",
                                workflow.name(),
                                data.samples.len()
                            ));
                            self.demo.magnitude[i] = data
                                .samples
                                .iter()
                                .map(|s| {
                                    s.signal
                                        .x_v
                                        .abs()
                                        .max(s.signal.y_v.abs())
                                        .max(s.signal.r_v.abs())
                                })
                                .fold(0_f64, f64::max);
                            self.demo.datasets[i] = Some(Arc::new(data));
                        }
                    }
                    if discover {
                        self.note("Demo CSVs ready · select a tab and press Load");
                    }
                }
                Err(error) => self.note(error),
            }
        }
        let Some(clock) = self.demo.clock else {
            return;
        };
        let i = Self::ix(self.workflow);
        let Some(data) = self.demo.datasets[i].clone() else {
            self.demo_stop();
            return;
        };
        let batch = self.demo.player.tick(
            clock.elapsed(),
            self.plans[i].interval_s,
            data.samples.len(),
        );
        if batch.clear {
            self.eta.start();
            self.trace.clear();
            self.map_cache = [None, None];
            self.snapshot.rows = 0;
        }
        let phase = if batch.phase == "Moving" {
            let cursor = self.demo.player.cursor;
            let changed = data
                .samples
                .get(cursor)
                .and_then(|s| s.target.slow)
                .is_some_and(|slow| {
                    cursor == 0 || data.samples[cursor - 1].target.slow != Some(slow)
                });
            let axis =
                if self
                    .demo
                    .player
                    .slow_motion(clock.elapsed(), self.plans[i].interval_s, changed)
                {
                    data.plan.slow
                } else {
                    data.plan.fast
                };
            format!("Moving {}", axis.name())
        } else {
            batch.phase.to_string()
        };
        self.snapshot.phase = format!("{phase} · loop {}", self.demo.player.cycle + 1);
        for row in batch.rows {
            let sample = data.samples[row].clone();
            self.snapshot.signal = Some(sample.signal);
            self.snapshot.signal_origin = Some(engine::ReadingOrigin::Replay);
            self.snapshot.position = sample.position.clone();
            self.live.push(sample.signal, engine::ReadingOrigin::Replay);
            self.trace.push(sample);
            self.snapshot.rows += 1;
        }
        self.scan_progress[i] = (self.snapshot.rows, self.snapshot.total);
        if self.snapshot.rows > 0 {
            self.eta.observe(
                self.snapshot.rows,
                clock.elapsed().as_secs_f64(),
                Instant::now(),
            );
        }
    }
}

struct DemoSmoke {
    output: PathBuf,
    started: Instant,
    workflow: usize,
    stage: u8,
    frames: u8,
    requested: bool,
    reports: Vec<Value>,
    resize_widths: Vec<f32>,
}
impl App {
    pub fn enable_demo_smoke(&mut self, output: PathBuf) {
        self.demo.smoke = Some(DemoSmoke {
            output,
            started: Instant::now(),
            workflow: 1,
            stage: 0,
            frames: 0,
            requested: false,
            reports: Vec::new(),
            resize_widths: Vec::new(),
        });
    }
    pub(super) fn demo_smoke_tick(&mut self, ctx: &egui::Context) {
        let Some(mut smoke) = self.demo.smoke.take() else {
            return;
        };
        let result = self.demo_smoke_step(ctx, &mut smoke);
        if let Err(error) = result {
            let _ = std::fs::write(
                smoke.output.join("demo-smoke.json"),
                json!({"success":false,"error":error}).to_string(),
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if smoke.workflow > 4 && smoke.stage == 9 {
            let _ = std::fs::write(
                smoke.output.join("demo-smoke.json"),
                serde_json::to_vec_pretty(
                    &json!({"success":true,"hardware_commands":0,"workflows":smoke.reports,"sidebar_window_widths":smoke.resize_widths,"log_view_verified":true}),
                )
                .unwrap(),
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            self.demo.smoke = Some(smoke);
        }
    }
    fn demo_smoke_step(
        &mut self,
        ctx: &egui::Context,
        smoke: &mut DemoSmoke,
    ) -> Result<(), String> {
        if smoke.started.elapsed() > Duration::from_secs(45) {
            return Err(format!(
                "Demo smoke timed out at workflow {} stage {}",
                smoke.workflow, smoke.stage
            ));
        }
        if smoke.workflow > 4 {
            smoke.frames = smoke.frames.saturating_add(1);
            let viewport = ctx.content_rect();
            if smoke.stage == 0 {
                smoke.resize_widths.push(viewport.width());
                self.toggle_right_panel(ctx, viewport);
                smoke.stage = 6;
                smoke.frames = 0;
            } else if smoke.stage == 6
                && smoke.frames >= 3
                && viewport.width() < smoke.resize_widths[0] - 100.
            {
                smoke.resize_widths.push(viewport.width());
                self.toggle_right_panel(ctx, viewport);
                smoke.stage = 7;
                smoke.frames = 0;
            } else if smoke.stage == 7
                && smoke.frames >= 3
                && (viewport.width() - smoke.resize_widths[0]).abs() <= 1.
            {
                smoke.resize_widths.push(viewport.width());
                smoke.stage = 9;
            } else if smoke.frames >= 120 {
                return Err("Window width did not follow sidebar toggle".into());
            }
            return Ok(());
        }
        let workflow = Workflow::ALL[smoke.workflow];
        match smoke.stage {
            0 => {
                if self.demo.pending.is_none() {
                    let path = self.demo.paths[smoke.workflow].clone();
                    if !path.is_file() {
                        return Err(format!("Missing demo CSV for {}", workflow.name()));
                    }
                    self.switch_workflow(workflow);
                    self.demo_load(path, Some(workflow));
                    smoke.stage = 1;
                }
            }
            1 => {
                if self.demo.pending.is_none() {
                    if self.demo.datasets[smoke.workflow].is_none() {
                        return Err(self.status.clone());
                    }
                    self.plans[smoke.workflow].interval_s = 0.;
                    self.connect(vec![Device::Lockin, Device::T, Device::X, Device::Y]);
                    self.demo_start();
                    smoke.stage = 2;
                }
            }
            2 => {
                let count = self.demo.datasets[smoke.workflow]
                    .as_ref()
                    .unwrap()
                    .samples
                    .len();
                if self.demo.player.cycle >= 2 && self.trace.len() == count {
                    self.demo_stop();
                    let dataset = self.demo.datasets[smoke.workflow].as_ref().unwrap();
                    if self.trace.iter().zip(&dataset.samples).any(|(a, b)| {
                        a.target.fast != b.target.fast
                            || a.target.slow != b.target.slow
                            || a.target.line != b.target.line
                            || a.signal.x_v != b.signal.x_v
                            || a.signal.y_v != b.signal.y_v
                            || a.signal.r_v != b.signal.r_v
                            || a.signal.theta_deg != b.signal.theta_deg
                    }) {
                        return Err("Replay changed CSV signals or scan ordering".into());
                    }
                    smoke.reports.push(json!({"workflow":workflow.key(),"rows":count,"loops":self.demo.player.cycle+1,"stopped":!self.snapshot.running,"fast":self.plan().fast.name(),"slow":self.plan().slow.name(),"plot_count":presentation::plot_count(workflow)}));
                    self.mode = DisplayMode::Xy;
                    smoke.stage = 3;
                    smoke.frames = 0;
                }
            }
            3..=5 => {
                smoke.frames += 1;
                if smoke.frames < 3 {
                    return Ok(());
                }
                let (scale, _) = self.demo_voltage_scale();
                presentation::verify_visible_data(
                    ctx,
                    &self.trace,
                    self.plan(),
                    self.mode,
                    scale,
                    self.plot_epoch,
                    self.trkr_log_y,
                )?;
                if !smoke.requested {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                        egui::UserData::default(),
                    ));
                    smoke.requested = true;
                } else {
                    for event in ctx.input(|i| i.raw.events.clone()) {
                        if let egui::Event::Screenshot { image, .. } = event {
                            let pixels = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                            let rgba = image::RgbaImage::from_raw(
                                image.width() as u32,
                                image.height() as u32,
                                pixels,
                            )
                            .ok_or("Invalid screenshot")?;
                            let name = format!(
                                "{}_{}.png",
                                workflow.key(),
                                if smoke.stage == 3 {
                                    "xy"
                                } else if smoke.stage == 5 {
                                    "log"
                                } else {
                                    "rtheta"
                                }
                            );
                            rgba.save(smoke.output.join(name))
                                .map_err(|e| e.to_string())?;
                            smoke.requested = false;
                            smoke.frames = 0;
                            if smoke.stage == 3 {
                                self.mode = DisplayMode::RTheta;
                                smoke.stage = 4;
                            } else if workflow == Workflow::Trkr && smoke.stage == 4 {
                                self.trkr_log_y = true;
                                smoke.stage = 5;
                            } else {
                                self.trkr_log_y = false;
                                smoke.workflow += 1;
                                smoke.stage = 0;
                            }
                            break;
                        }
                    }
                }
            }
            _ => unreachable!(),
        }
        Ok(())
    }
}
impl App {
    pub(super) fn demo_voltage_scale(&self) -> (f64, &'static str) {
        if !self.demo_viewer {
            return (1., "V");
        }
        presentation::voltage_scale_from_sensitivity(self.demo.magnitude[Self::ix(self.workflow)])
    }
}
