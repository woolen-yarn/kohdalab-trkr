//! One persistent worker owns instruments, motion, acquisition and incremental export.
use crate::lockin::{self, Model};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use trkr_core::{Axis, Coordinate, Plan, Signal, Target, Workflow};
use trkr_motion::{DelayCalibration, MotionConfig, MotionRig, Position};
use trkr_transport::Gpib;
pub const FIELDS: &[&str] = &[
    "timestamp",
    "measurement",
    "fast_axis",
    "slow_axis",
    "target_elapsed_s",
    "target_t_cor_ps",
    "target_x_cor_um",
    "target_y_cor_um",
    "target_u_cor_um",
    "target_v_cor_um",
    "elapsed_s",
    "t_cor_ps",
    "t_ps",
    "x_cor_um",
    "x_um",
    "y_cor_um",
    "y_um",
    "u_cor_um",
    "v_cor_um",
    "X_V",
    "Y_V",
    "R_V",
    "Theta_deg",
    "spatial_theta_deg",
    "coordinate",
    "delay_stage_mm",
    "delay_stage_pulse",
    "x_scanner_mm",
    "x_scanner_deg",
    "y_scanner_mm",
    "y_scanner_deg",
];

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Source {
    Demo,
    Gpib { model: Model, address: u8 },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HardwareConfig {
    pub model: Model,
    pub address: u8,
    pub motion: MotionConfig,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Device {
    Lockin,
    T,
    X,
    Y,
}
impl Device {
    fn index(self) -> usize {
        match self {
            Self::Lockin => 0,
            Self::T => 1,
            Self::X => 2,
            Self::Y => 3,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sample {
    pub elapsed: f64,
    pub target: Target,
    pub signal: Signal,
    pub position: Position,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadingOrigin {
    Hardware,
    Demo,
    Replay,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub devices: [bool; 4],
    pub transport: String,
    pub running: bool,
    pub phase: String,
    pub position: Position,
    pub signal: Option<Signal>,
    #[serde(default)]
    pub signal_origin: Option<ReadingOrigin>,
    pub lockin: Value,
    pub rows: usize,
    pub total: usize,
    /// Resolved delay scale for the currently connected rig.  It is cleared
    /// whenever the motion configuration or connection changes.
    #[serde(default)]
    pub delay_calibration: Option<DelayCalibration>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveReading {
    pub signal: Signal,
    pub position: Position,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    Sample(Sample),
    /// A display-only lock-in read. It never contributes a scan row or CSV
    /// record, so the GUI may update its live trace independently.
    Live(LiveReading),
    State(Box<Snapshot>),
    /// Advisory progress only; never blocks acquisition or adds measurement rows.
    Phase(String),
    Finished(Result<(usize, Option<PathBuf>), String>),
    Error(String),
}
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Command {
    Connect {
        config: HardwareConfig,
        devices: Vec<Device>,
    },
    Disconnect {
        devices: Vec<Device>,
    },
    Read,
    Move {
        config: HardwareConfig,
        targets: [Option<f64>; 3],
        coordinate: Coordinate,
        zero: [f64; 3],
        approach: [bool; 3],
    },
    Initialize {
        config: HardwareConfig,
        axis: usize,
        home: bool,
    },
    Run {
        source: Source,
        config: HardwareConfig,
        plan: Plan,
        output: Option<PathBuf>,
    },
    LockinSettings,
    SetLockin {
        setting: String,
        value: Value,
    },
    AutoLockin {
        action: String,
    },
    Replay {
        path: PathBuf,
        interval_s: f64,
    },
    Stop,
    Shutdown,
}
impl Command {
    fn is_job(&self) -> bool {
        matches!(
            self,
            Self::Run { .. } | Self::Move { .. } | Self::Initialize { .. } | Self::Replay { .. }
        )
    }
}
pub struct Controller {
    pub events: Receiver<Event>,
    commands: Sender<Command>,
    cancel: Arc<AtomicBool>,
    busy: Arc<AtomicBool>,
    read_only: bool,
    thread: Option<thread::JoinHandle<()>>,
}
impl Controller {
    pub fn send(&self, command: Command) -> Result<(), String> {
        if self.read_only && !matches!(command, Command::Stop | Command::Shutdown) {
            return Err("demo controller is read-only; hardware, acquisition, and file output commands are disabled".into());
        }
        if matches!(command, Command::Stop) {
            self.stop();
            return Ok(());
        }
        let job = command.is_job();
        if job {
            if self.busy.swap(true, Ordering::AcqRel) {
                return Err("An operation is already running".into());
            }
            self.cancel.store(false, Ordering::Release);
        }
        if self.commands.send(command).is_err() {
            if job {
                self.busy.store(false, Ordering::Release);
            }
            return Err("Instrument worker stopped".into());
        }
        Ok(())
    }
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Release);
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        self.stop();
        let _ = self.commands.send(Command::Shutdown);
        if let Some(handle) = self.thread.take() {
            while !handle.is_finished() {
                let _ = self.events.recv_timeout(Duration::from_millis(20));
            }
            let _ = handle.join();
        }
    }
}
pub fn controller() -> Controller {
    controller_with_mode(false)
}

/// Creates a controller for the CSV demo viewer.
///
/// The worker is still present so its shutdown lifecycle matches the normal
/// application, but it accepts only cancellation and shutdown. In particular,
/// no command can open an instrument, move a stage, or create an output file.
pub fn controller_read_only() -> Controller {
    controller_with_mode(true)
}

fn controller_with_mode(read_only: bool) -> Controller {
    let (tx, rx) = mpsc::channel();
    let (send, events) = mpsc::sync_channel(2048);
    let cancel = Arc::new(AtomicBool::new(false));
    let busy = Arc::new(AtomicBool::new(false));
    let token = cancel.clone();
    let working = busy.clone();
    let handle = thread::spawn(move || {
        let mut session = Session::default();
        let mut last_lockin = Instant::now() - Duration::from_millis(100);
        let mut last_position = Instant::now() - Duration::from_millis(500);
        loop {
            let now = Instant::now();
            let lockin_due = last_lockin + Duration::from_millis(100);
            let position_due = last_position + Duration::from_millis(500);
            let timeout = idle_refresh_timeout(
                session.gpib.is_some(),
                session.rig.is_some(),
                lockin_due,
                position_due,
                now,
            );
            let cmd = match rx.recv_timeout(timeout) {
                Ok(c) => c,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let now = Instant::now();
                    let mut changed = false;
                    if session.gpib.is_some() && now >= lockin_due {
                        last_lockin = now;
                        if let Err(e) = session.read_lockin() {
                            let _ = send.send(Event::Error(e));
                        } else if let Some(signal) = session.state.signal {
                            let _ = send.try_send(Event::Live(LiveReading {
                                signal,
                                position: session.state.position.clone(),
                            }));
                            changed = true;
                        }
                    }
                    if session.rig.is_some() && now >= position_due {
                        last_position = now;
                        if let Err(e) = session.read_position() {
                            let _ = send.send(Event::Error(e));
                        } else {
                            changed = true;
                        }
                    }
                    if changed {
                        let _ = send.send(Event::State(Box::new(session.state.clone())));
                    }
                    continue;
                }
            };
            if matches!(cmd, Command::Shutdown) {
                break;
            }
            let job = cmd.is_job();
            let acquisition = matches!(cmd, Command::Run { .. } | Command::Replay { .. });
            if job {
                session.state.running = true;
                session.state.phase = "Preparing".into();
                session.state.rows = 0;
                session.state.total = 0;
                let _ = send.send(Event::State(Box::new(session.state.clone())));
            }
            let result = session.execute(cmd, &token, &send);
            if let Err(e) = &result {
                if let Some(rig) = &mut session.rig {
                    let _ = rig.stop();
                }
                let _ = send.send(Event::Error(e.clone()));
            }
            if job {
                session.state.running = false;
                session.state.phase = if token.load(Ordering::Acquire) {
                    "Stopped"
                } else if result.is_ok() {
                    "Complete"
                } else {
                    "Error"
                }
                .into();
                working.store(false, Ordering::Release);
            }
            let _ = send.send(Event::State(Box::new(session.state.clone())));
            if acquisition {
                let _ = send.send(Event::Finished(result.map(|out| (session.state.rows, out))));
            }
        }
        if let Some(rig) = &mut session.rig {
            let _ = rig.stop();
        }
    });
    Controller {
        events,
        commands: tx,
        cancel,
        busy,
        read_only,
        thread: Some(handle),
    }
}
#[derive(Default)]
struct Session {
    gpib: Option<Gpib>,
    rig: Option<MotionRig>,
    config: Option<HardwareConfig>,
    state: Snapshot,
}
impl Session {
    fn connect(&mut self, config: HardwareConfig, devices: Vec<Device>) -> Result<(), String> {
        // Reopening serial handles never initiates motion or homing.
        if devices.contains(&Device::Lockin) {
            let changed = self
                .config
                .as_ref()
                .is_none_or(|c| c.model != config.model || c.address != config.address);
            if changed {
                self.gpib = None;
                self.state.devices[0] = false;
            }
            if self.gpib.is_none() {
                self.gpib = Some(Gpib::connect(config.address)?);
                self.state.transport = self
                    .gpib
                    .as_ref()
                    .map(|g| g.backend_name())
                    .unwrap_or_default()
                    .into();
                self.state.devices[0] = true;
            }
        }
        let mut mask = self.state.devices;
        for d in &devices {
            match d {
                Device::T if config.motion.delay.is_none() => {
                    return Err("No delay stage configured".into());
                }
                Device::X if config.motion.x.is_none() => {
                    return Err("No X scanner configured".into());
                }
                Device::Y if config.motion.y.is_none() => {
                    return Err("No Y scanner configured".into());
                }
                _ => {}
            }
        }
        for d in devices {
            mask[d.index()] = true;
        }
        let motion_changed = self
            .config
            .as_ref()
            .is_none_or(|c| c.motion != config.motion);
        if motion_changed || mask[1..] != self.state.devices[1..] {
            self.rig = None;
            self.state.delay_calibration = None;
            self.state.devices[1..].fill(false);
            let motion = filtered(&config.motion, &mask);
            if motion.delay.is_some() || motion.x.is_some() || motion.y.is_some() {
                self.rig = Some(MotionRig::open(&motion)?);
            }
            self.state.devices[1..].copy_from_slice(&mask[1..]);
            self.state.delay_calibration = self
                .rig
                .as_ref()
                .map(MotionRig::delay_calibration)
                .transpose()?
                .flatten();
        }
        self.config = Some(config);
        self.read()?;
        Ok(())
    }
    fn read(&mut self) -> Result<(), String> {
        self.read_lockin()?;
        self.read_position()
    }
    fn read_lockin(&mut self) -> Result<(), String> {
        if let Some(gpib) = &mut self.gpib {
            let model = self
                .config
                .as_ref()
                .ok_or("Missing lock-in configuration")?
                .model;
            self.state.signal = Some(lockin::read(model, gpib)?);
            self.state.signal_origin = Some(ReadingOrigin::Hardware);
        }
        Ok(())
    }
    fn read_position(&mut self) -> Result<(), String> {
        if let Some(rig) = &mut self.rig {
            self.state.position = rig.position()?;
        }
        Ok(())
    }
    fn execute(
        &mut self,
        cmd: Command,
        cancel: &AtomicBool,
        send: &SyncSender<Event>,
    ) -> Result<Option<PathBuf>, String> {
        match cmd {
            Command::Connect { config, devices } => self.connect(config, devices)?,
            Command::Disconnect { devices } => {
                let mut mask = self.state.devices;
                for d in devices {
                    mask[d.index()] = false;
                }
                if !mask[0] {
                    self.gpib = None;
                    self.state.devices[0] = false;
                    self.state.transport.clear();
                    self.state.signal = None;
                    self.state.signal_origin = None;
                    self.state.lockin = Value::Null;
                }
                if mask[1..] != self.state.devices[1..] {
                    self.rig = None;
                    self.state.delay_calibration = None;
                    self.state.devices[1..].fill(false);
                    if let Some(config) = &self.config {
                        let motion = filtered(&config.motion, &mask);
                        if motion.delay.is_some() || motion.x.is_some() || motion.y.is_some() {
                            self.rig = Some(MotionRig::open(&motion)?);
                        }
                    }
                    self.state.devices[1..].copy_from_slice(&mask[1..]);
                    self.state.position = self
                        .rig
                        .as_mut()
                        .map(|r| r.position())
                        .transpose()?
                        .unwrap_or_default();
                    self.state.delay_calibration = self
                        .rig
                        .as_ref()
                        .map(MotionRig::delay_calibration)
                        .transpose()?
                        .flatten();
                }
            }
            Command::Read => self.read()?,
            Command::Move {
                config,
                targets,
                coordinate,
                zero,
                approach,
            } => {
                self.connect(config, devices_for_targets(targets))?;
                let rig = self.rig.as_mut().ok_or("No motion devices selected")?;
                let physical = rig.to_physical(targets, coordinate_name(coordinate), zero)?;
                rig.validate_targets(&[physical])?;
                self.state.position = rig.move_to(physical, approach, cancel)?;
            }
            Command::Initialize { config, axis, home } => {
                if axis > 2 {
                    return Err("Unknown motion axis".into());
                }
                self.connect(config, vec![[Device::T, Device::X, Device::Y][axis]])?;
                self.rig
                    .as_mut()
                    .ok_or("No motion device")?
                    .initialize(axis, home, cancel)?;
                self.read()?;
            }
            Command::Run {
                source,
                config,
                plan,
                output,
            } => {
                self.acquire(source, config, &plan, output.as_deref(), cancel, send)?;
                return Ok(output);
            }
            Command::LockinSettings => {
                let model = self.config.as_ref().ok_or("Connect a lock-in first")?.model;
                self.state.lockin =
                    lockin::settings(self.gpib.as_mut().ok_or("Connect a lock-in first")?, model)?;
            }
            Command::SetLockin { setting, value } => {
                let setting = match setting.as_str() {
                    "sensitivity" => "Sensitivity",
                    "time_constant" => "Time Constant",
                    "coupling" => "Coupling",
                    "slope" => "Slope",
                    "ac_gain" => "AC Gain",
                    other => other,
                };
                let model = self.config.as_ref().ok_or("Connect a lock-in first")?.model;
                lockin::set_setting(
                    self.gpib.as_mut().ok_or("Connect a lock-in first")?,
                    model,
                    setting,
                    &value,
                )?;
            }
            Command::AutoLockin { action } => {
                let model = self.config.as_ref().ok_or("Connect a lock-in first")?.model;
                lockin::auto(
                    self.gpib.as_mut().ok_or("Connect a lock-in first")?,
                    model,
                    &action,
                )?;
            }
            Command::Replay { path, interval_s } => {
                self.replay(&path, interval_s, cancel, send)?;
                return Ok(Some(path));
            }
            Command::Stop | Command::Shutdown => {}
        }
        Ok(None)
    }
    fn set_phase(&mut self, phase: String, send: &mpsc::SyncSender<Event>) {
        self.state.phase = phase;
        // Status must never delay motion, settling, or the acquisition itself.
        let _ = send.try_send(Event::Phase(self.state.phase.clone()));
    }
    fn acquire(
        &mut self,
        source: Source,
        mut config: HardwareConfig,
        plan: &Plan,
        output: Option<&Path>,
        cancel: &AtomicBool,
        send: &SyncSender<Event>,
    ) -> Result<(), String> {
        let targets = plan.targets()?;
        if matches!(source, Source::Gpib { .. })
            && plan.workflow == Workflow::SignalMonitor
            && plan.interval_s < 0.01
        {
            return Err("Hardware monitor interval must be at least 0.01 s".into());
        }
        let mask = active_axes(plan);
        let mut physical: Vec<[Option<f64>; 3]> = targets
            .iter()
            .map(|t| std::array::from_fn(|i| mask[i].then_some(t.physical[i])))
            .collect();
        let mut returns = return_target(plan, targets.last().ok_or("Empty scan")?);
        if let Source::Gpib { model, address } = source {
            config.model = model;
            config.address = address;
            let mut devices = vec![Device::Lockin];
            for i in 0..3 {
                if mask[i] {
                    devices.push([Device::T, Device::X, Device::Y][i]);
                }
            }
            // Validate static bounds before opening any devices.
            if plan.coordinate == Coordinate::Measurement {
                config.motion.validate_targets(&physical)?;
                if let Some(r) = returns {
                    config.motion.validate_targets(&[std::array::from_fn(|i| {
                        r[i].map(|v| v + plan.zero[i])
                    })])?;
                }
            }
            self.connect(config, devices)?;
            if plan.workflow != Workflow::SignalMonitor {
                let rig = self.rig.as_mut().ok_or("Motion not connected")?;
                for (p, t) in physical.iter_mut().zip(&targets) {
                    *p = rig.to_physical(
                        std::array::from_fn(|i| mask[i].then_some(t.corrected[i])),
                        coordinate_name(plan.coordinate),
                        plan.zero,
                    )?;
                }
                if let Some(r) = returns.as_mut() {
                    *r = rig.to_physical(*r, coordinate_name(plan.coordinate), plan.zero)?;
                }
                rig.validate_targets(&physical)?;
                if let Some(r) = returns {
                    rig.validate_targets(&[r])?;
                }
                let mut moves = Vec::with_capacity(targets.len());
                for (index, target) in targets.iter().enumerate() {
                    let first = index == 0 || target.line != targets[index - 1].line;
                    if index > 0
                        && first
                        && plan.return_fast_between_lines
                        && !matches!(plan.fast, Axis::U | Axis::V)
                        && !matches!(plan.slow, Axis::U | Axis::V)
                    {
                        let reset = rig.to_physical(
                            line_reset(plan, &targets[index - 1]),
                            coordinate_name(plan.coordinate),
                            plan.zero,
                        )?;
                        moves.push((reset, [false; 3]));
                    }
                    moves.push((
                        physical[index],
                        std::array::from_fn(|i| mask[i] && index == 0),
                    ));
                }
                if let Some(r) = returns {
                    moves.push((r, [false; 3]));
                }
                rig.validate_moves(&moves)?;
            }
        }
        let mut csv = output.map(|p| create_csv(p, source, plan)).transpose()?;
        self.state.total = targets.len();
        self.state.rows = 0;
        self.state.phase = "Measuring".into();
        let _ = send.send(Event::State(Box::new(self.state.clone())));
        let start = Instant::now();
        let mut result = (|| {
            for (index, target) in targets.iter().enumerate() {
                if cancel.load(Ordering::Acquire) {
                    return Ok(());
                }
                if plan.workflow != Workflow::SignalMonitor {
                    let phase = if plan.workflow.is_2d()
                        && (index == 0 || target.line != targets[index - 1].line)
                    {
                        format!("Moving {}/{}", plan.fast.name(), plan.slow.name())
                    } else {
                        format!("Moving {}", plan.fast.name())
                    };
                    self.set_phase(phase, send);
                }
                if matches!(source, Source::Gpib { .. }) && plan.workflow != Workflow::SignalMonitor
                {
                    let rig = self.rig.as_mut().ok_or("Motion disconnected")?;
                    if index > 0
                        && target.line != targets[index - 1].line
                        && plan.return_fast_between_lines
                        && !matches!(plan.fast, Axis::U | Axis::V)
                        && !matches!(plan.slow, Axis::U | Axis::V)
                    {
                        let reset = line_reset(plan, &targets[index - 1]);
                        let reset =
                            rig.to_physical(reset, coordinate_name(plan.coordinate), plan.zero)?;
                        rig.validate_targets(&[reset])?;
                        rig.move_to(reset, [false; 3], cancel)?;
                    }
                    let first = index == 0 || target.line != targets[index - 1].line;
                    let step = step_axes(plan, first);
                    let motion =
                        std::array::from_fn(|i| if step[i] { physical[index][i] } else { None });
                    self.state.position = rig.move_to(
                        motion,
                        std::array::from_fn(|i| mask[i] && index == 0),
                        cancel,
                    )?;
                } else if plan.workflow != Workflow::SignalMonitor {
                    self.state.position.physical = physical[index];
                }
                let due = if plan.workflow == Workflow::SignalMonitor {
                    start + Duration::from_secs_f64(index as f64 * plan.interval_s)
                } else {
                    Instant::now() + Duration::from_secs_f64(plan.interval_s)
                };
                self.set_phase("Waiting".into(), send);
                if !wait_until(due, cancel) {
                    return Ok(());
                }
                self.set_phase("Reading".into(), send);
                let signal = match source {
                    Source::Gpib { model, .. } => {
                        if let Some(rig) = &mut self.rig {
                            self.state.position = rig.position()?;
                        }
                        lockin::read(model, self.gpib.as_mut().ok_or("Lock-in disconnected")?)?
                    }
                    Source::Demo => Signal::demo(
                        if plan.workflow == Workflow::SignalMonitor {
                            start.elapsed().as_secs_f64() * 10.
                        } else {
                            target.corrected[0]
                        },
                        target.corrected[1],
                        target.corrected[2],
                    ),
                };
                let sample = Sample {
                    elapsed: start.elapsed().as_secs_f64(),
                    target: *target,
                    signal,
                    position: if matches!(source, Source::Demo)
                        && plan.workflow == Workflow::SignalMonitor
                    {
                        Position::default()
                    } else {
                        self.state.position.clone()
                    },
                };
                if let Some(csv) = &mut csv {
                    csv.write_record(row(plan, &sample)?)
                        .map_err(|e| e.to_string())?;
                    csv.flush().map_err(|e| e.to_string())?;
                }
                self.state.signal = Some(signal);
                self.state.signal_origin = Some(match source {
                    Source::Demo => ReadingOrigin::Demo,
                    _ => ReadingOrigin::Hardware,
                });
                self.state.rows += 1;
                let _ = send.try_send(Event::Sample(sample));
            }
            if !cancel.load(Ordering::Acquire)
                && matches!(source, Source::Gpib { .. })
                && let (Some(rig), Some(r)) = (&mut self.rig, returns)
            {
                self.state.phase = "Returning".into();
                let _ = send.send(Event::State(Box::new(self.state.clone())));
                self.state.position = rig.move_to(r, [false; 3], cancel)?;
            }
            Ok(())
        })();
        if result.is_err() || cancel.load(Ordering::Acquire) {
            let stopped = self.rig.as_mut().map(|rig| rig.stop()).transpose();
            if let Err(error) = stopped {
                result = Err(format!(
                    "{}; STOP failed: {error}",
                    result
                        .as_ref()
                        .err()
                        .map(String::as_str)
                        .unwrap_or("Cancelled")
                ));
            } else if cancel.load(Ordering::Acquire) {
                result = Ok(());
            }
        }
        if let Some(csv) = &mut csv {
            csv.flush().map_err(|e| e.to_string())?;
        }
        if let Some(path) = output {
            finish_metadata(
                path,
                self.state.rows,
                if result.is_err() {
                    "error"
                } else if cancel.load(Ordering::Acquire) {
                    "cancelled"
                } else {
                    "complete"
                },
                result.as_ref().err(),
            )?;
        }
        result
    }
    fn replay(
        &mut self,
        path: &Path,
        interval: f64,
        cancel: &AtomicBool,
        send: &SyncSender<Event>,
    ) -> Result<(), String> {
        if !interval.is_finite() || !(0.0..=3600.).contains(&interval) {
            return Err("Invalid replay interval".into());
        }
        let mut reader = csv::Reader::from_path(path).map_err(|e| e.to_string())?;
        let headers = reader.headers().map_err(|e| e.to_string())?.clone();
        for key in ["X_V", "Y_V", "R_V", "Theta_deg"] {
            if !headers.iter().any(|v| v == key) {
                return Err(format!("Replay missing {key}"));
            }
        }
        for record in reader.records() {
            if cancel.load(Ordering::Acquire) {
                break;
            }
            let r = record.map_err(|e| e.to_string())?;
            let get = |key: &str| -> Option<f64> {
                headers
                    .iter()
                    .position(|s| s == key)
                    .and_then(|i| r.get(i))
                    .and_then(|s| s.parse().ok())
            };
            let required = |key| get(key).ok_or_else(|| format!("Replay invalid {key}"));
            let signal = Signal::new(
                required("X_V")?,
                required("Y_V")?,
                required("R_V")?,
                required("Theta_deg")?,
            )?;
            let target = Target {
                fast: get("target_t_cor_ps")
                    .or_else(|| get("target_x_cor_um"))
                    .or_else(|| get("target_elapsed_s"))
                    .unwrap_or(self.state.rows as f64),
                slow: get("target_y_cor_um"),
                corrected: [
                    get("t_cor_ps").unwrap_or(0.),
                    get("x_cor_um").unwrap_or(0.),
                    get("y_cor_um").unwrap_or(0.),
                ],
                physical: [
                    get("t_ps").unwrap_or(0.),
                    get("x_um").unwrap_or(0.),
                    get("y_um").unwrap_or(0.),
                ],
                line: 0,
            };
            let position = Position {
                physical: [get("t_ps"), get("x_um"), get("y_um")],
                ..Default::default()
            };
            self.state.signal = Some(signal);
            self.state.signal_origin = Some(ReadingOrigin::Replay);
            self.state.position = position.clone();
            self.state.rows += 1;
            let _ = send.try_send(Event::Sample(Sample {
                elapsed: get("elapsed_s").unwrap_or(0.),
                target,
                signal,
                position,
            }));
            if !wait_until(Instant::now() + Duration::from_secs_f64(interval), cancel) {
                break;
            }
        }
        Ok(())
    }
}
fn filtered(config: &MotionConfig, mask: &[bool; 4]) -> MotionConfig {
    MotionConfig {
        delay: mask[1].then(|| config.delay.clone()).flatten(),
        x: mask[2].then(|| config.x.clone()).flatten(),
        y: mask[3].then(|| config.y.clone()).flatten(),
    }
}
fn devices_for_targets(targets: [Option<f64>; 3]) -> Vec<Device> {
    targets
        .into_iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|_| [Device::T, Device::X, Device::Y][i]))
        .collect()
}
fn coordinate_name(c: Coordinate) -> &'static str {
    match c {
        Coordinate::Measurement => "measurement",
        Coordinate::Interface => "interface",
        Coordinate::Instrument => "instrument",
    }
}
pub(crate) fn active_axes(p: &Plan) -> [bool; 3] {
    let mut mask = [false; 3];
    if p.workflow == Workflow::SignalMonitor {
        return mask;
    }
    for axis in [Some(p.fast), p.workflow.is_2d().then_some(p.slow)]
        .into_iter()
        .flatten()
    {
        match axis {
            Axis::T => mask[0] = true,
            Axis::X => mask[1] = true,
            Axis::Y => mask[2] = true,
            Axis::U | Axis::V => {
                mask[1] = true;
                mask[2] = true;
            }
        }
    }
    mask
}
fn step_axes(p: &Plan, line_start: bool) -> [bool; 3] {
    let mut fast = p.clone();
    fast.workflow = if p.fast == Axis::T {
        Workflow::Trkr
    } else {
        Workflow::Srkr
    };
    let mut mask = active_axes(&fast);
    if line_start && p.workflow.is_2d() {
        let slow = Plan {
            workflow: if p.slow == Axis::T {
                Workflow::Trkr
            } else {
                Workflow::Srkr
            },
            fast: p.slow,
            ..p.clone()
        };
        let slow = active_axes(&slow);
        for i in 0..3 {
            mask[i] |= slow[i];
        }
    }
    mask
}
fn axis_target(p: &Plan, fast: f64, slow: Option<f64>) -> [Option<f64>; 3] {
    let mut xyz = [None; 3];
    let mut uv = [0.; 2];
    let mut rotate = false;
    for (axis, value) in [(p.fast, Some(fast)), (p.slow, slow)] {
        if let Some(value) = value {
            match axis {
                Axis::T => xyz[0] = Some(value),
                Axis::X => xyz[1] = Some(value),
                Axis::Y => xyz[2] = Some(value),
                Axis::U => {
                    uv[0] = value;
                    rotate = true
                }
                Axis::V => {
                    uv[1] = value;
                    rotate = true
                }
            }
        }
    }
    if rotate {
        let (x, y) = trkr_core::rotated_to_xy(uv[0], uv[1], p.theta_deg);
        xyz[1] = Some(x);
        xyz[2] = Some(y);
    }
    xyz
}
fn return_target(p: &Plan, last: &Target) -> Option<[Option<f64>; 3]> {
    if p.workflow == Workflow::SignalMonitor || !p.return_to_zero.into_iter().any(|x| x) {
        return None;
    }
    let fast = if p.return_to_zero[0] { 0. } else { last.fast };
    let slow = last.slow.map(|s| if p.return_to_zero[1] { 0. } else { s });
    let mut result = axis_target(p, fast, slow);
    if !matches!(p.fast, Axis::U | Axis::V) && !matches!(p.slow, Axis::U | Axis::V) {
        if !p.return_to_zero[0] {
            match p.fast {
                Axis::T => result[0] = None,
                Axis::X => result[1] = None,
                Axis::Y => result[2] = None,
                _ => {}
            }
        }
        if p.workflow.is_2d() && !p.return_to_zero[1] {
            match p.slow {
                Axis::T => result[0] = None,
                Axis::X => result[1] = None,
                Axis::Y => result[2] = None,
                _ => {}
            }
        }
    }
    Some(result)
}
fn line_reset(p: &Plan, last: &Target) -> [Option<f64>; 3] {
    let mut target = axis_target(p, p.fast_range.min, None);
    if matches!(p.fast, Axis::U | Axis::V) {
        target = axis_target(p, p.fast_range.min, last.slow);
    }
    target
}
/// Returns the next receive timeout for connected idle devices only. With no
/// devices, retain a bounded command wait instead of spinning on a stale due
/// time from an absent device.
fn idle_refresh_timeout(
    lockin_connected: bool,
    motion_connected: bool,
    lockin_due: Instant,
    position_due: Instant,
    now: Instant,
) -> Duration {
    let next = match (lockin_connected, motion_connected) {
        (true, true) => lockin_due.min(position_due),
        (true, false) => lockin_due,
        (false, true) => position_due,
        (false, false) => return Duration::from_millis(500),
    };
    next.saturating_duration_since(now)
}
fn wait_until(deadline: Instant, cancel: &AtomicBool) -> bool {
    while !cancel.load(Ordering::Acquire) {
        let now = Instant::now();
        if now >= deadline {
            return true;
        }
        thread::sleep((deadline - now).min(Duration::from_millis(20)));
    }
    false
}
fn finish_metadata(
    path: &Path,
    rows: usize,
    status: &str,
    error: Option<&String>,
) -> Result<(), String> {
    let path = path.with_extension("run.json");
    let mut meta: Value = serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    meta["rows"] = serde_json::json!(rows);
    meta["status"] = serde_json::json!(status);
    meta["error"] = serde_json::json!(error);
    meta["finished_at"] = serde_json::json!(utc_now()?);
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&meta).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
fn create_csv(path: &Path, source: Source, plan: &Plan) -> Result<csv::Writer<File>, String> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let metadata_path = path.with_extension("run.json");
    if metadata_path == path {
        return Err("CSV path must not have the .run.json extension".into());
    }
    let metadata = serde_json::json!({
        "application": "kohdalab-trkr", "version": env!("CARGO_PKG_VERSION"),
        "simulation": matches!(source, Source::Demo), "source": format!("{source:?}"),
        "plan": plan, "timestamp": utc_now()?,
    });
    let bytes = serde_json::to_vec_pretty(&metadata).map_err(|e| e.to_string())?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("Create {}: {e}", path.display()))?;
    let mut metadata_created = false;
    let result = (|| {
        use std::io::Write;
        let mut meta = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&metadata_path)
            .map_err(|e| format!("Create run metadata: {e}"))?;
        metadata_created = true;
        meta.write_all(&bytes).map_err(|e| e.to_string())?;
        let mut writer = csv::Writer::from_writer(file);
        writer.write_record(FIELDS).map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())?;
        Ok(writer)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(path);
        if metadata_created {
            let _ = std::fs::remove_file(metadata_path);
        }
    }
    result
}

fn utc_now() -> Result<String, String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|e| e.to_string())
}

pub(crate) fn row(plan: &Plan, sample: &Sample) -> Result<Vec<String>, String> {
    let mut result = vec![String::new(); FIELDS.len()];
    let mut put =
        |key: &str, value: String| result[FIELDS.iter().position(|&k| k == key).unwrap()] = value;
    put("timestamp", utc_now()?);
    put("measurement", plan.workflow.key().into());
    put("elapsed_s", sample.elapsed.to_string());
    let monitor = plan.workflow == Workflow::SignalMonitor;
    if monitor {
        put("target_elapsed_s", sample.target.fast.to_string());
    } else {
        put("fast_axis", plan.fast.name().into());
        if plan.workflow.is_2d() {
            put("slow_axis", plan.slow.name().into());
        }
        for (axis, value) in [
            (plan.fast, Some(sample.target.fast)),
            (plan.slow, sample.target.slow),
        ] {
            if let Some(value) = value {
                put(
                    &format!(
                        "target_{}_cor_{}",
                        axis.name(),
                        if axis == trkr_core::Axis::T {
                            "ps"
                        } else {
                            "um"
                        }
                    ),
                    value.to_string(),
                );
            }
        }
        for (i, axis) in ["t", "x", "y"].iter().enumerate() {
            let unit = if i == 0 { "ps" } else { "um" };
            if let Some(value) = sample.position.physical[i] {
                put(&format!("{axis}_{unit}"), value.to_string());
                put(
                    &format!("{axis}_cor_{unit}"),
                    (value - plan.zero[i]).to_string(),
                );
            }
        }
        let (s, c) = plan.theta_deg.to_radians().sin_cos();
        let [_, x, y] = sample.target.corrected;
        put("u_cor_um", (x * c + y * s).to_string());
        put("v_cor_um", (-x * s + y * c).to_string());
        put("spatial_theta_deg", plan.theta_deg.to_string());
        put("coordinate", coordinate_name(plan.coordinate).into());
    }
    for (key, value) in [
        ("delay_stage_mm", sample.position.delay_stage_mm),
        ("x_scanner_mm", sample.position.x_scanner_mm),
        ("x_scanner_deg", sample.position.x_scanner_deg),
        ("y_scanner_mm", sample.position.y_scanner_mm),
        ("y_scanner_deg", sample.position.y_scanner_deg),
    ] {
        if let Some(value) = value {
            put(key, value.to_string());
        }
    }
    if let Some(value) = sample.position.delay_stage_pulse {
        put("delay_stage_pulse", value.to_string());
    }
    put("X_V", format!("{:.6e}", sample.signal.x_v));
    put("Y_V", format!("{:.6e}", sample.signal.y_v));
    put("R_V", format!("{:.6e}", sample.signal.r_v));
    put("Theta_deg", sample.signal.theta_deg.to_string());
    Ok(result)
}

pub fn demo_csv(plan: Plan, output: &Path) -> Result<usize, String> {
    let mut plan = plan;
    plan.interval_s = 0.0;
    let targets = plan.targets()?;
    let mut writer = create_csv(output, Source::Demo, &plan)?;
    for target in &targets {
        let p = target.corrected;
        let sample = Sample {
            elapsed: 0.0,
            target: *target,
            signal: Signal::demo(p[0] + target.fast, p[1], p[2]),
            position: Position {
                physical: target.physical.map(Some),
                ..Default::default()
            },
        };
        writer
            .write_record(row(&plan, &sample)?)
            .map_err(|e| e.to_string())?;
    }
    writer.flush().map_err(|e| e.to_string())?;
    finish_metadata(output, targets.len(), "complete", None)?;
    Ok(targets.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> HardwareConfig {
        HardwareConfig {
            model: Model::Sr830,
            address: 12,
            motion: MotionConfig::default(),
        }
    }
    #[test]
    fn idle_refresh_waits_only_for_connected_devices() {
        let now = Instant::now();
        let lockin_due = now + Duration::from_millis(100);
        let position_due = now + Duration::from_millis(500);
        assert_eq!(
            idle_refresh_timeout(false, false, lockin_due, position_due, now),
            Duration::from_millis(500)
        );
        assert_eq!(
            idle_refresh_timeout(true, false, lockin_due, position_due, now),
            Duration::from_millis(100)
        );
        assert_eq!(
            idle_refresh_timeout(false, true, lockin_due, position_due, now),
            Duration::from_millis(500)
        );
        assert_eq!(
            idle_refresh_timeout(
                true,
                true,
                now + Duration::from_millis(600),
                now + Duration::from_millis(200),
                now,
            ),
            Duration::from_millis(200)
        );
    }
    #[test]
    fn active_axes_never_moves_unselected_devices() {
        let mut p = Plan {
            workflow: Workflow::Trkr,
            ..Default::default()
        };
        assert_eq!(active_axes(&p), [true, false, false]);
        p.workflow = Workflow::Srkr;
        p.fast = Axis::X;
        assert_eq!(active_axes(&p), [false, true, false]);
        p.fast = Axis::U;
        assert_eq!(active_axes(&p), [false, true, true]);
        p.workflow = Workflow::Strkr;
        p.fast = Axis::T;
        p.slow = Axis::V;
        assert_eq!(active_axes(&p), [true, true, true]);
    }
    #[test]
    fn slow_axis_moves_only_on_line_boundaries() {
        let p = Plan {
            workflow: Workflow::Strkr,
            fast: Axis::T,
            slow: Axis::X,
            ..Default::default()
        };
        assert_eq!(step_axes(&p, false), [true, false, false]);
        assert_eq!(step_axes(&p, true), [true, true, false]);
        let p = Plan {
            fast: Axis::U,
            slow: Axis::T,
            ..p
        };
        assert_eq!(step_axes(&p, false), [false, true, true]);
        assert_eq!(step_axes(&p, true), [true, true, true]);
    }
    #[test]
    fn reset_returns_only_fast_axis_to_first_scan_point() {
        let p = Plan {
            workflow: Workflow::Strkr,
            fast: Axis::T,
            slow: Axis::X,
            fast_range: trkr_core::Range {
                min: 50.,
                max: 100.,
                step: 25.,
            },
            ..Default::default()
        };
        let last = *p.targets().unwrap().last().unwrap();
        assert_eq!(line_reset(&p, &last), [Some(50.), None, None]);
    }
    #[test]
    fn rotated_return_preserves_unreturned_virtual_axis() {
        let p = Plan {
            workflow: Workflow::Srkr2d,
            fast: Axis::U,
            slow: Axis::V,
            theta_deg: 90.,
            return_to_zero: [true, false],
            ..Default::default()
        };
        let last = *p.targets().unwrap().last().unwrap();
        let r = return_target(&p, &last).unwrap();
        assert!((r[1].unwrap() + last.slow.unwrap()).abs() < 1e-9);
        assert!(r[2].unwrap().abs() < 1e-9);
    }
    #[test]
    fn nonzero_origin_is_added_once() {
        let p = Plan {
            workflow: Workflow::Trkr,
            zero: [-122., 61., 477.],
            ..Default::default()
        };
        let last = *p.targets().unwrap().last().unwrap();
        let r = return_target(&p, &last).unwrap();
        assert_eq!(r, [Some(0.), None, None]);
        let physical: [Option<f64>; 3] = std::array::from_fn(|i| r[i].map(|v| v + p.zero[i]));
        assert_eq!(physical, [Some(-122.), None, None]);
    }
    #[test]
    fn metadata_collision_preserves_existing_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.csv");
        let meta = path.with_extension("run.json");
        std::fs::write(&meta, "existing").unwrap();
        assert!(create_csv(&path, Source::Demo, &Plan::default()).is_err());
        assert!(!path.exists());
        assert_eq!(std::fs::read_to_string(meta).unwrap(), "existing");
    }
    #[test]
    fn monitor_retains_components_without_inventing_positions() {
        let p = Plan::default();
        let sample = Sample {
            elapsed: 1.,
            target: p.targets().unwrap()[0],
            signal: Signal::new(1., 2., 3., 4.).unwrap(),
            position: Position::default(),
        };
        let r = row(&p, &sample).unwrap();
        let get = |k| &r[FIELDS.iter().position(|&v| v == k).unwrap()];
        assert_eq!(get("Theta_deg"), "4");
        assert!(get("t_ps").is_empty());
    }
    #[test]
    fn scan_progress_preserves_point_order_and_reports_each_phase() {
        for workflow in [
            Workflow::Trkr,
            Workflow::Srkr,
            Workflow::Strkr,
            Workflow::Srkr2d,
        ] {
            let plan = Plan {
                workflow,
                fast: if matches!(workflow, Workflow::Trkr | Workflow::Strkr) {
                    Axis::T
                } else {
                    Axis::X
                },
                slow: Axis::Y,
                fast_range: trkr_core::Range {
                    min: 0.,
                    max: 1.,
                    step: 1.,
                },
                slow_range: trkr_core::Range {
                    min: 0.,
                    max: 1.,
                    step: 1.,
                },
                interval_s: 0.,
                ..Default::default()
            };
            let targets = plan.targets().unwrap();
            let c = controller();
            c.send(Command::Run {
                source: Source::Demo,
                config: config(),
                plan,
                output: None,
            })
            .unwrap();
            let mut phases = Vec::new();
            let mut samples = Vec::new();
            loop {
                match c.events.recv_timeout(Duration::from_secs(5)).unwrap() {
                    Event::Phase(phase) => phases.push(phase),
                    Event::Sample(sample) => samples.push(sample),
                    Event::Finished(result) => {
                        assert_eq!(result.unwrap().0, targets.len());
                        break;
                    }
                    Event::Error(error) => panic!("{error}"),
                    _ => {}
                }
            }
            assert_eq!(samples.len(), targets.len());
            assert_eq!(phases.len(), 3 * targets.len());
            for (index, phases) in phases.as_chunks::<3>().0.iter().enumerate() {
                assert!(phases[0].starts_with("Moving "));
                assert_eq!(phases[1], "Waiting");
                assert_eq!(phases[2], "Reading");
                assert_eq!(samples[index].target, targets[index]);
            }
        }
    }
    #[test]
    fn cancelling_wait_does_not_read_or_add_a_sample() {
        let c = controller();
        c.send(Command::Run {
            source: Source::Demo,
            config: config(),
            plan: Plan {
                workflow: Workflow::Trkr,
                interval_s: 5.,
                n_points: 2,
                ..Default::default()
            },
            output: None,
        })
        .unwrap();
        loop {
            if matches!(c.events.recv_timeout(Duration::from_secs(5)).unwrap(), Event::Phase(ref phase) if phase == "Waiting")
            {
                break;
            }
        }
        c.stop();
        loop {
            match c.events.recv_timeout(Duration::from_secs(5)).unwrap() {
                Event::Sample(_) => panic!("Cancelled wait must not create a sample"),
                Event::Phase(phase) => assert_ne!(phase, "Reading"),
                Event::Finished(result) => {
                    assert_eq!(result.unwrap().0, 0);
                    break;
                }
                _ => {}
            }
        }
    }
    #[test]
    fn controller_demo_saves_and_cancellation_is_terminal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.csv");
        let c = controller();
        let plan = Plan {
            interval_s: 0.,
            n_points: 5,
            ..Default::default()
        };
        c.send(Command::Run {
            source: Source::Demo,
            config: config(),
            plan,
            output: Some(path.clone()),
        })
        .unwrap();
        loop {
            if let Event::Finished(result) = c.events.recv_timeout(Duration::from_secs(5)).unwrap()
            {
                assert_eq!(result.unwrap().0, 5);
                break;
            }
        }
        let meta: Value =
            serde_json::from_slice(&std::fs::read(path.with_extension("run.json")).unwrap())
                .unwrap();
        assert_eq!(meta["status"], "complete");
        assert_eq!(csv::Reader::from_path(&path).unwrap().records().count(), 5);
        let plan = Plan {
            interval_s: 1.,
            n_points: 100,
            ..Default::default()
        };
        c.send(Command::Run {
            source: Source::Demo,
            config: config(),
            plan,
            output: None,
        })
        .unwrap();
        c.stop();
        loop {
            if let Event::Finished(result) = c.events.recv_timeout(Duration::from_secs(5)).unwrap()
            {
                assert!(result.unwrap().0 < 100);
                break;
            }
        }
    }
    #[test]
    fn read_only_controller_rejects_demo_runs_before_output_is_created() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("must-not-exist.csv");
        let controller = controller_read_only();
        let error = controller
            .send(Command::Run {
                source: Source::Demo,
                config: config(),
                plan: Plan::default(),
                output: Some(output.clone()),
            })
            .unwrap_err();
        assert!(error.contains("read-only"));
        assert!(!output.exists());
        assert!(!output.with_extension("run.json").exists());
    }

    #[test]
    fn read_only_controller_allows_only_stop_and_shutdown() {
        let controller = controller_read_only();
        assert!(
            controller
                .send(Command::Connect {
                    config: config(),
                    devices: vec![Device::Lockin],
                })
                .is_err()
        );
        assert!(controller.send(Command::Read).is_err());
        assert!(controller.send(Command::LockinSettings).is_err());
        assert!(controller.send(Command::Stop).is_ok());
        assert!(controller.send(Command::Shutdown).is_ok());
    }
    #[test]
    fn actual_readback_is_exported_with_raw_values() {
        let p = Plan {
            workflow: Workflow::Trkr,
            zero: [-122., 0., 0.],
            ..Default::default()
        };
        let sample = Sample {
            elapsed: 0.,
            target: p.targets().unwrap()[0],
            signal: Signal::default(),
            position: Position {
                physical: [Some(-171.99), None, None],
                delay_stage_mm: Some(275.),
                delay_stage_pulse: Some(1234),
                ..Default::default()
            },
        };
        let r = row(&p, &sample).unwrap();
        let get = |k| &r[FIELDS.iter().position(|&v| v == k).unwrap()];
        assert_eq!(get("t_ps"), "-171.99");
        assert_eq!(get("delay_stage_pulse"), "1234");
        assert!(get("x_um").is_empty());
    }
}
