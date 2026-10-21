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
    "acquisition_elapsed_s",
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
    #[serde(default = "hardware_origin")]
    pub origin: ReadingOrigin,
}
fn hardware_origin() -> ReadingOrigin {
    ReadingOrigin::Hardware
}

const LIVE_INTERVAL: Duration = Duration::from_millis(100);

/// Shares one cadence between idle, motion, settling and acquisition reads.
/// Display-only samples never become measurement rows.
struct LiveMonitor {
    next: Instant,
    started: Instant,
    read_budget: Duration,
    demo_coordinates: [Option<f64>; 3],
}
impl Default for LiveMonitor {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            next: now,
            started: now,
            read_budget: LIVE_INTERVAL,
            demo_coordinates: [None; 3],
        }
    }
}
impl LiveMonitor {
    fn publish(
        &mut self,
        signal: Signal,
        origin: ReadingOrigin,
        position: &Position,
        send: &SyncSender<Event>,
    ) {
        self.next = Instant::now() + LIVE_INTERVAL;
        let _ = send.try_send(Event::Live(LiveReading {
            signal,
            position: position.clone(),
            origin,
        }));
    }
    fn publish_if_due(
        &mut self,
        signal: Signal,
        origin: ReadingOrigin,
        position: &Position,
        send: &SyncSender<Event>,
    ) {
        if Instant::now() >= self.next {
            self.publish(signal, origin, position, send);
        }
    }
    fn poll(
        &mut self,
        read: impl FnOnce() -> Result<Signal, String>,
        origin: ReadingOrigin,
        position: &Position,
        send: &SyncSender<Event>,
    ) -> Result<Option<Signal>, String> {
        let now = Instant::now();
        if now < self.next {
            return Ok(None);
        }
        // Schedule from the start of the read, with no catch-up burst after slow I/O.
        self.next = now + LIVE_INTERVAL;
        let signal = read()?;
        self.read_budget =
            (now.elapsed() + Duration::from_millis(10)).max(Duration::from_millis(20));
        let _ = send.try_send(Event::Live(LiveReading {
            signal,
            position: position.clone(),
            origin,
        }));
        Ok(Some(signal))
    }
}

fn poll_live(
    monitor: &mut LiveMonitor,
    source: Source,
    gpib: Option<&mut Gpib>,
    state: &mut Snapshot,
    send: &SyncSender<Event>,
    cancel: Option<&AtomicBool>,
) -> Result<(), String> {
    let origin = match source {
        Source::Demo => ReadingOrigin::Demo,
        _ => ReadingOrigin::Hardware,
    };
    let position = state.position.clone();
    let elapsed = monitor.started.elapsed().as_secs_f64();
    let demo_coordinates = monitor.demo_coordinates;
    if let Some(signal) = monitor.poll(
        || match source {
            Source::Gpib { model, .. } => {
                let gpib = gpib.ok_or("Lock-in disconnected")?;
                match cancel {
                    Some(token) => lockin::read_cancellable(model, gpib, token),
                    None => lockin::read(model, gpib),
                }
            }
            Source::Demo => Ok(Signal::demo(
                demo_coordinates[0].unwrap_or(elapsed * 10.),
                demo_coordinates[1].unwrap_or(0.),
                demo_coordinates[2].unwrap_or(0.),
            )),
        },
        origin,
        &position,
        send,
    )? {
        state.signal = Some(signal);
        state.signal_origin = Some(origin);
    }
    Ok(())
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
        #[serde(default = "spatial_approach")]
        approach: [bool; 3],
    },
    Initialize {
        config: HardwareConfig,
        axis: usize,
        home: bool,
        /// GUI initialization also moves to the instrument's calibrated zero.
        #[serde(default)]
        move_to_origin: bool,
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
        let mut last_position = Instant::now() - Duration::from_millis(500);
        loop {
            let now = Instant::now();
            let lockin_due = session.live.next;
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
                        let config = session.config.as_ref().expect("connected lock-in config");
                        let source = Source::Gpib {
                            model: config.model,
                            address: config.address,
                        };
                        if let Err(e) = poll_live(
                            &mut session.live,
                            source,
                            session.gpib.as_mut(),
                            &mut session.state,
                            &send,
                            None,
                        ) {
                            let _ = send.send(Event::Error(e));
                        } else {
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
                    if session.clear_failed_lockin() {
                        changed = true;
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
            session.clear_failed_lockin();
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
    live: LiveMonitor,
    gpib: Option<Gpib>,
    rig: Option<MotionRig>,
    config: Option<HardwareConfig>,
    state: Snapshot,
}
impl Session {
    fn clear_failed_lockin(&mut self) -> bool {
        if self.gpib.as_ref().is_some_and(Gpib::is_poisoned) {
            self.gpib = None;
            self.state.devices[0] = false;
            self.state.signal = None;
            self.state.signal_origin = None;
            self.state.transport.clear();
            self.state.lockin = Value::Null;
            return true;
        }
        false
    }
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
                rig.validate_moves(&[(physical, approach)])?;
                self.state.phase = motion_phase("Moving", targets);
                let _ = send.send(Event::State(Box::new(self.state.clone())));
                self.move_live(physical, approach, cancel, send)?;
            }
            Command::Initialize {
                config,
                axis,
                home,
                move_to_origin,
            } => {
                if axis > 2 {
                    return Err("Unknown motion axis".into());
                }
                self.connect(config, vec![[Device::T, Device::X, Device::Y][axis]])?;
                self.state.phase = motion_phase(
                    "Initializing",
                    std::array::from_fn(|i| (i == axis).then_some(0.)),
                );
                let _ = send.send(Event::State(Box::new(self.state.clone())));
                let state = &mut self.state;
                let source = self
                    .gpib
                    .as_ref()
                    .and(self.config.as_ref())
                    .map(|c| Source::Gpib {
                        model: c.model,
                        address: c.address,
                    });
                let gpib = &mut self.gpib;
                let live = &mut self.live;
                let rig = self.rig.as_mut().ok_or("No motion device")?;
                state.position = rig.initialize_with_origin_progress(
                    axis,
                    home,
                    move_to_origin,
                    cancel,
                    Some(&mut |position| {
                        state.position = position;
                        let _ = send.try_send(Event::State(Box::new(state.clone())));
                        if let Some(source) = source {
                            poll_live(live, source, gpib.as_mut(), state, send, Some(cancel))?;
                        }
                        Ok(())
                    }),
                )?;
                state.delay_calibration = rig.delay_calibration()?;
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
    fn move_live(
        &mut self,
        targets: [Option<f64>; 3],
        approach: [bool; 3],
        cancel: &AtomicBool,
        send: &SyncSender<Event>,
    ) -> Result<(), String> {
        self.motion_live(targets, approach, false, cancel, send)
    }
    fn motion_live(
        &mut self,
        targets: [Option<f64>; 3],
        approach: [bool; 3],
        returning: bool,
        cancel: &AtomicBool,
        send: &SyncSender<Event>,
    ) -> Result<(), String> {
        let source = self
            .gpib
            .as_ref()
            .and(self.config.as_ref())
            .map(|c| Source::Gpib {
                model: c.model,
                address: c.address,
            });
        let state = &mut self.state;
        let gpib = &mut self.gpib;
        let live = &mut self.live;
        let rig = self.rig.as_mut().ok_or("Motion disconnected")?;
        let mut progress = |position| {
            state.position = position;
            let _ = send.try_send(Event::State(Box::new(state.clone())));
            if let Some(source) = source {
                poll_live(live, source, gpib.as_mut(), state, send, Some(cancel))?;
            }
            Ok(())
        };
        state.position = if returning {
            rig.return_to_with_progress(targets, cancel, Some(&mut progress))?
        } else {
            rig.move_to_with_progress(targets, approach, cancel, Some(&mut progress))?
        };
        Ok(())
    }
    fn wait_live(
        &mut self,
        deadline: Instant,
        source: Source,
        cancel: &AtomicBool,
        send: &SyncSender<Event>,
    ) -> Result<bool, String> {
        while !cancel.load(Ordering::Acquire) {
            let now = Instant::now();
            if now >= deadline {
                return Ok(true);
            }
            // Leave the end of the settling window free for the measurement read.
            if deadline - now >= self.live.read_budget {
                poll_live(
                    &mut self.live,
                    source,
                    self.gpib.as_mut(),
                    &mut self.state,
                    send,
                    Some(cancel),
                )?;
            }
            thread::sleep(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(10)),
            );
        }
        Ok(false)
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
                    // Return zeros are always measurement coordinates, including
                    // scans whose input range uses mm, degrees or pulses.
                    *r = rig.to_physical(*r, "measurement", plan.zero)?;
                }
                rig.validate_targets(&physical)?;
                if let Some(r) = returns {
                    rig.validate_targets(&[r])?;
                }
                let mut moves = Vec::with_capacity(targets.len());
                for (index, target) in targets.iter().enumerate() {
                    let first = index == 0 || target.line != targets[index - 1].line;
                    if index > 0 && first && reset_fast_before_slow(plan) {
                        let reset = rig.to_physical(
                            line_reset(plan, &targets[index - 1]),
                            coordinate_name(plan.coordinate),
                            plan.zero,
                        )?;
                        moves.push((reset, [false; 3]));
                    }
                    for movement in scan_moves(plan, physical[index], first, index == 0) {
                        moves.push((movement.target, movement.approach));
                    }
                }
                if let Some(r) = returns {
                    for target in return_moves(plan, r) {
                        rig.validate_return(target)?;
                    }
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
        if matches!(source, Source::Demo) {
            self.state.position = Position::default();
            self.live.started = start;
        }
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
                    if index > 0
                        && target.line != targets[index - 1].line
                        && reset_fast_before_slow(plan)
                    {
                        let reset = line_reset(plan, &targets[index - 1]);
                        let rig = self.rig.as_ref().ok_or("Motion disconnected")?;
                        let reset =
                            rig.to_physical(reset, coordinate_name(plan.coordinate), plan.zero)?;
                        rig.validate_targets(&[reset])?;
                        self.move_live(reset, [false; 3], cancel, send)?;
                    }
                    let first = index == 0 || target.line != targets[index - 1].line;
                    for movement in scan_moves(plan, physical[index], first, index == 0) {
                        self.set_phase(format!("Moving {}", movement.axis.name()), send);
                        self.move_live(movement.target, movement.approach, cancel, send)?;
                    }
                } else if plan.workflow != Workflow::SignalMonitor {
                    self.state.position.physical = physical[index];
                }
                if matches!(source, Source::Demo) {
                    self.live.demo_coordinates = if plan.workflow == Workflow::SignalMonitor {
                        [None; 3]
                    } else {
                        target.corrected.map(Some)
                    };
                }
                let due = if plan.workflow == Workflow::SignalMonitor {
                    start + Duration::from_secs_f64(index as f64 * plan.interval_s)
                } else {
                    Instant::now() + Duration::from_secs_f64(plan.interval_s)
                };
                self.set_phase("Waiting".into(), send);
                if !self.wait_live(due, source, cancel, send)? {
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
                self.live.publish_if_due(
                    signal,
                    match source {
                        Source::Demo => ReadingOrigin::Demo,
                        _ => ReadingOrigin::Hardware,
                    },
                    &self.state.position,
                    send,
                );
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
                && let Some(r) = returns
            {
                for target in return_moves(plan, r) {
                    self.state.phase = motion_phase("Returning", target);
                    let _ = send.send(Event::State(Box::new(self.state.clone())));
                    self.motion_live(target, [false; 3], true, cancel, send)?;
                }
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
                elapsed: get("elapsed_s")
                    .or_else(|| get("acquisition_elapsed_s"))
                    .unwrap_or(0.),
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
fn motion_phase(action: &str, targets: [Option<f64>; 3]) -> String {
    let axes: Vec<_> = ["t", "x", "y"]
        .into_iter()
        .zip(targets)
        .filter_map(|(axis, target)| target.map(|_| axis))
        .collect();
    format!("{action} {}", axes.join("/"))
}
fn filtered(config: &MotionConfig, mask: &[bool; 4]) -> MotionConfig {
    MotionConfig {
        delay: mask[1].then(|| config.delay.clone()).flatten(),
        x: mask[2].then(|| config.x.clone()).flatten(),
        y: mask[3].then(|| config.y.clone()).flatten(),
    }
}
/// Normal standalone positioning compensates spatial axes; the delay stage has
/// no software scanner hysteresis. Unselected/disabled scanners remain untouched.
pub(crate) fn spatial_approach() -> [bool; 3] {
    [false, true, true]
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
struct ScanMove {
    axis: Axis,
    target: [Option<f64>; 3],
    approach: [bool; 3],
}
fn axis_mask(axis: Axis) -> [bool; 3] {
    match axis {
        Axis::T => [true, false, false],
        Axis::X => [false, true, false],
        Axis::Y => [false, false, true],
        Axis::U | Axis::V => [false, true, true],
    }
}
fn masked(target: [Option<f64>; 3], mask: [bool; 3]) -> [Option<f64>; 3] {
    std::array::from_fn(|i| if mask[i] { target[i] } else { None })
}
fn coupled_spatial(plan: &Plan) -> bool {
    plan.workflow.is_2d()
        && matches!(plan.fast, Axis::U | Axis::V)
        && matches!(plan.slow, Axis::U | Axis::V)
}
/// Python's Cartesian line sequence: slow completes before fast starts.
/// U/V maps are a Rust extension and require a coupled X/Y target.
fn scan_moves(
    plan: &Plan,
    target: [Option<f64>; 3],
    line_start: bool,
    first_point: bool,
) -> Vec<ScanMove> {
    let axes = if plan.workflow.is_2d() && line_start && !coupled_spatial(plan) {
        vec![plan.slow, plan.fast]
    } else {
        vec![plan.fast]
    };
    axes.into_iter()
        .map(|axis| {
            let mask = axis_mask(axis);
            ScanMove {
                axis,
                target: masked(target, mask),
                approach: std::array::from_fn(|i| {
                    i > 0 && mask[i] && (first_point || (axis == plan.fast && line_start))
                }),
            }
        })
        .collect()
}
/// Legacy completion returns fast first, then slow; never re-enable a cancelled job.
fn return_moves(plan: &Plan, target: [Option<f64>; 3]) -> Vec<[Option<f64>; 3]> {
    if coupled_spatial(plan) {
        return vec![target];
    }
    [Some(plan.fast), plan.workflow.is_2d().then_some(plan.slow)]
        .into_iter()
        .flatten()
        .map(|axis| masked(target, axis_mask(axis)))
        .filter(|target| target.iter().any(Option::is_some))
        .collect()
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
// Spatial fast axes go straight to the new line's compensation approach after
// the slow axis moves. Do not first visit the uncompensated scan start.
fn reset_fast_before_slow(p: &Plan) -> bool {
    p.workflow.is_2d()
        && p.return_fast_between_lines
        && p.fast == Axis::T
        && !matches!(p.slow, Axis::U | Axis::V)
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

/// Python str(float): shortest round-trip digits, .0 for integral fixed values,
/// fixed notation for 1e-4 <= abs < 1e16, and signed two-digit exponents.
fn csv_float(value: f64) -> String {
    let shortest = format!("{value:?}");
    if !value.is_finite() {
        return shortest;
    }
    let sign = if value.is_sign_negative() { "-" } else { "" };
    let text = shortest.trim_start_matches('-');
    let (mantissa, extra) = text
        .split_once('e')
        .map(|(m, e)| (m, e.parse::<i32>().expect("Rust exponent")))
        .unwrap_or((text, 0));
    let point = mantissa.find('.').unwrap_or(mantissa.len()) as i32;
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let Some(first) = digits.find(|c: char| c != '0') else {
        return format!("{sign}0.0");
    };
    let exponent = point - first as i32 - 1 + extra;
    let digits = digits[first..].trim_end_matches('0');
    if !(-4..16).contains(&exponent) {
        let fraction = if digits.len() > 1 {
            format!(".{}", &digits[1..])
        } else {
            String::new()
        };
        format!("{sign}{}{fraction}e{exponent:+03}", &digits[..1])
    } else if exponent < 0 {
        format!("{sign}0.{}{}", "0".repeat((-exponent - 1) as usize), digits)
    } else {
        let point = exponent as usize + 1;
        if digits.len() <= point {
            format!("{sign}{digits}{}.0", "0".repeat(point - digits.len()))
        } else {
            format!("{sign}{}.{}", &digits[..point], &digits[point..])
        }
    }
}

fn csv_voltage(value: f64) -> String {
    let text = format!("{value:.6e}");
    let Some((mantissa, exponent)) = text.split_once('e') else {
        return text;
    };
    let exponent: i32 = exponent.parse().expect("Rust scientific exponent");
    format!("{mantissa}e{exponent:+03}")
}

pub(crate) fn row(plan: &Plan, sample: &Sample) -> Result<Vec<String>, String> {
    let mut result = vec![String::new(); FIELDS.len()];
    let mut put =
        |key: &str, value: String| result[FIELDS.iter().position(|&k| k == key).unwrap()] = value;
    put("timestamp", utc_now()?);
    put("measurement", plan.workflow.key().into());
    put("acquisition_elapsed_s", csv_float(sample.elapsed));
    let monitor = plan.workflow == Workflow::SignalMonitor;
    let exported_axes = match plan.workflow {
        Workflow::SignalMonitor => [false; 3],
        Workflow::Trkr => [true, false, false],
        Workflow::Srkr => axis_mask(plan.fast),
        Workflow::Strkr | Workflow::Srkr2d => [true; 3],
    };
    if monitor {
        put("elapsed_s", csv_float(sample.elapsed));
        put("fast_axis", "elapsed_s".into());
        put("target_elapsed_s", csv_float(sample.target.fast));
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
                    csv_float(value),
                );
            }
        }
        for (i, axis) in ["t", "x", "y"].iter().enumerate() {
            let unit = if i == 0 { "ps" } else { "um" };
            if exported_axes[i]
                && let Some(value) = sample.position.physical[i]
            {
                put(&format!("{axis}_{unit}"), csv_float(value));
                put(
                    &format!("{axis}_cor_{unit}"),
                    csv_float(value - plan.zero[i]),
                );
            }
        }
        let (s, c) = plan.theta_deg.to_radians().sin_cos();
        let [_, x, y] = sample.target.corrected;
        put("u_cor_um", csv_float(x * c + y * s));
        put("v_cor_um", csv_float(-x * s + y * c));
        put("spatial_theta_deg", csv_float(plan.theta_deg));
        put("coordinate", coordinate_name(plan.coordinate).into());
    }
    for (axis, key, value) in [
        (0, "delay_stage_mm", sample.position.delay_stage_mm),
        (1, "x_scanner_mm", sample.position.x_scanner_mm),
        (1, "x_scanner_deg", sample.position.x_scanner_deg),
        (2, "y_scanner_mm", sample.position.y_scanner_mm),
        (2, "y_scanner_deg", sample.position.y_scanner_deg),
    ] {
        if exported_axes[axis]
            && let Some(value) = value
        {
            put(key, csv_float(value));
        }
    }
    if exported_axes[0]
        && let Some(value) = sample.position.delay_stage_pulse
    {
        put("delay_stage_pulse", value.to_string());
    }
    put("X_V", csv_voltage(sample.signal.x_v));
    put("Y_V", csv_voltage(sample.signal.y_v));
    put("R_V", csv_voltage(sample.signal.r_v));
    put("Theta_deg", csv_float(sample.signal.theta_deg));
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
    fn live_monitor_reads_fresh_signals_on_its_own_cadence_without_catchup() {
        let (send, events) = mpsc::sync_channel(10);
        let mut monitor = LiveMonitor::default();
        let position = Position::default();
        let first = Signal::new(1., 2., 3., 4.).unwrap();
        let second = Signal::new(5., 6., 7., 8.).unwrap();
        assert_eq!(
            monitor
                .poll(|| Ok(first), ReadingOrigin::Hardware, &position, &send)
                .unwrap(),
            Some(first)
        );
        // Measurement reads may happen arbitrarily often; they must not add live points.
        monitor.publish_if_due(second, ReadingOrigin::Hardware, &position, &send);
        assert!(
            monitor
                .poll(
                    || panic!("not yet due"),
                    ReadingOrigin::Hardware,
                    &position,
                    &send
                )
                .unwrap()
                .is_none()
        );
        monitor.next = Instant::now() - Duration::from_secs(5);
        assert_eq!(
            monitor
                .poll(|| Ok(second), ReadingOrigin::Hardware, &position, &send)
                .unwrap(),
            Some(second)
        );
        assert!(monitor.next > Instant::now());
        let values: Vec<_> = events
            .try_iter()
            .map(|event| match event {
                Event::Live(live) => live.signal,
                _ => panic!("not a display-only event"),
            })
            .collect();
        assert_eq!(values, vec![first, second]);
    }

    #[test]
    fn live_read_failure_is_reported_without_publishing_stale_data() {
        let (send, events) = mpsc::sync_channel(10);
        let mut monitor = LiveMonitor::default();
        assert_eq!(
            monitor
                .poll(
                    || Err("transport failed".into()),
                    ReadingOrigin::Hardware,
                    &Position::default(),
                    &send
                )
                .unwrap_err(),
            "transport failed"
        );
        assert!(events.try_recv().is_err());
        assert!(monitor.next > Instant::now());
    }

    #[test]
    fn live_updates_continue_between_measurement_points_without_adding_csv_rows() {
        for workflow in [
            Workflow::Trkr,
            Workflow::Srkr,
            Workflow::Strkr,
            Workflow::Srkr2d,
            Workflow::SignalMonitor,
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("run.csv");
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
                    max: 0.,
                    step: 1.,
                },
                slow_range: trkr_core::Range {
                    min: 0.,
                    max: 0.,
                    step: 1.,
                },
                interval_s: 0.35,
                n_points: 2,
                zero: [10., 20., 30.],
                ..Default::default()
            };
            let expected = plan.targets().unwrap().len();
            let controller = controller();
            controller
                .send(Command::Run {
                    source: Source::Demo,
                    config: config(),
                    plan,
                    output: Some(path.clone()),
                })
                .unwrap();
            let mut samples = 0;
            let mut lives = 0;
            let mut between = 0;
            let mut phase = String::new();
            loop {
                match controller
                    .events
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
                {
                    Event::Phase(next) => phase = next,
                    Event::Live(live) => {
                        assert_eq!(live.origin, ReadingOrigin::Demo);
                        if workflow == Workflow::SignalMonitor {
                            assert_eq!(live.position.physical, [None; 3]);
                        } else {
                            assert_eq!(live.signal, Signal::demo(0., 0., 0.));
                        }
                        lives += 1;
                        if phase == "Waiting" {
                            between += 1;
                        }
                    }
                    Event::Sample(sample) => {
                        samples += 1;
                        if workflow != Workflow::SignalMonitor {
                            assert!(sample.elapsed >= 0.35);
                        }
                    }
                    Event::Finished(result) => {
                        assert_eq!(result.unwrap().0, expected);
                        break;
                    }
                    Event::Error(error) => panic!("{error}"),
                    _ => {}
                }
            }
            assert_eq!(samples, expected);
            assert!(
                between >= 2,
                "{workflow:?}: only {between} live reads while settling"
            );
            assert!(lives > samples, "{workflow:?}");
            assert_eq!(
                csv::Reader::from_path(&path).unwrap().records().count(),
                expected
            );
        }
    }

    #[test]
    fn cancel_after_live_read_does_not_commit_a_measurement() {
        let controller = controller();
        controller
            .send(Command::Run {
                source: Source::Demo,
                config: config(),
                plan: Plan {
                    workflow: Workflow::Trkr,
                    interval_s: 2.,
                    ..Default::default()
                },
                output: None,
            })
            .unwrap();
        loop {
            match controller
                .events
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
            {
                Event::Live(_) => {
                    controller.stop();
                    break;
                }
                Event::Sample(_) => panic!("premature measurement"),
                _ => {}
            }
        }
        loop {
            match controller
                .events
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
            {
                Event::Finished(result) => {
                    assert_eq!(result.unwrap().0, 0);
                    break;
                }
                Event::Sample(_) => panic!("cancelled measurement was committed"),
                _ => {}
            }
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
    fn scan_sequence_waits_for_slow_before_fast_and_only_approaches_once() {
        let physical = [Some(1.), Some(2.), Some(3.)];
        for (fast, slow) in [
            (Axis::T, Axis::X),
            (Axis::X, Axis::T),
            (Axis::X, Axis::Y),
            (Axis::Y, Axis::X),
            (Axis::U, Axis::T),
        ] {
            let p = Plan {
                workflow: if fast == Axis::T || slow == Axis::T {
                    Workflow::Strkr
                } else {
                    Workflow::Srkr2d
                },
                fast,
                slow,
                ..Default::default()
            };
            let moves = scan_moves(&p, physical, true, true);
            assert_eq!(
                moves.iter().map(|m| m.axis).collect::<Vec<_>>(),
                vec![slow, fast]
            );
            for movement in moves {
                assert_eq!(movement.target, masked(physical, axis_mask(movement.axis)));
            }
            let line = scan_moves(&p, physical, true, false);
            assert_eq!(line[0].approach, [false; 3]);
            assert_eq!(
                line[1].approach,
                std::array::from_fn(|i| i > 0 && axis_mask(fast)[i])
            );
            let inner = scan_moves(&p, physical, false, false);
            assert_eq!(inner.len(), 1);
            assert_eq!(inner[0].axis, fast);
            assert_eq!(inner[0].target, masked(physical, axis_mask(fast)));
            assert_eq!(
                return_moves(&p, physical),
                vec![
                    masked(physical, axis_mask(fast)),
                    masked(physical, axis_mask(slow))
                ]
            );
        }
    }
    #[test]
    fn rotated_maps_keep_coupled_motor_targets_and_returns_do_not_add_disabled_axes() {
        let p = Plan {
            workflow: Workflow::Srkr2d,
            fast: Axis::U,
            slow: Axis::V,
            ..Default::default()
        };
        let target = [None, Some(10.), Some(20.)];
        let moves = scan_moves(&p, target, true, true);
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].target, target);
        assert_eq!(return_moves(&p, target), vec![target]);
        let p = Plan {
            fast: Axis::X,
            slow: Axis::Y,
            ..p
        };
        assert_eq!(
            return_moves(&p, [None, None, Some(20.)]),
            vec![[None, None, Some(20.)]]
        );
    }
    #[test]
    fn standalone_move_defaults_to_spatial_compensation_with_explicit_opt_out() {
        let mut value = serde_json::json!({
            "command": "move", "config": config(),
            "targets": [null, 20., null], "coordinate": "measurement", "zero": [0., 0., 0.]
        });
        let command: Command = serde_json::from_value(value.clone()).unwrap();
        assert!(
            matches!(command, Command::Move { approach, .. } if approach == [false, true, true])
        );
        value["approach"] = serde_json::json!([false, false, false]);
        let command: Command = serde_json::from_value(value).unwrap();
        assert!(matches!(command, Command::Move { approach, .. } if approach == [false; 3]));
    }
    #[test]
    fn all_scan_entries_compensate_their_active_spatial_axes() {
        let mut cases = vec![(Workflow::Trkr, Axis::T, Axis::X)];
        for spatial in [Axis::X, Axis::Y, Axis::U, Axis::V] {
            cases.push((Workflow::Srkr, spatial, Axis::T));
            cases.push((Workflow::Strkr, spatial, Axis::T));
            cases.push((Workflow::Strkr, Axis::T, spatial));
        }
        for (fast, slow) in [
            (Axis::X, Axis::Y),
            (Axis::Y, Axis::X),
            (Axis::U, Axis::V),
            (Axis::V, Axis::U),
        ] {
            cases.push((Workflow::Srkr2d, fast, slow));
        }
        for (workflow, fast, slow) in cases {
            let plan = Plan {
                workflow,
                fast,
                slow,
                ..Default::default()
            };
            let mut compensated = [false; 3];
            for movement in scan_moves(&plan, [Some(1.), Some(2.), Some(3.)], true, true) {
                for (i, applied) in movement.approach.into_iter().enumerate() {
                    assert!(!applied || movement.target[i].is_some());
                    compensated[i] |= applied;
                }
            }
            let expected = std::array::from_fn(|i| i > 0 && active_axes(&plan)[i]);
            assert_eq!(compensated, expected, "{workflow:?}: {fast:?}/{slow:?}");
            for movement in scan_moves(&plan, [Some(1.), Some(2.), Some(3.)], false, false) {
                assert_eq!(movement.approach, [false; 3], "interior point {workflow:?}");
            }
        }
    }
    #[test]
    fn old_initialize_commands_stay_nonmoving_unless_origin_move_is_explicit() {
        let command: Command = serde_json::from_value(
            serde_json::json!({"command":"initialize","config":config(),"axis":1,"home":false}),
        )
        .unwrap();
        assert!(matches!(
            command,
            Command::Initialize {
                home: false,
                move_to_origin: false,
                ..
            }
        ));
    }
    #[test]
    fn spatial_lines_approach_each_scan_start_without_visiting_zero() {
        for (fast, slow, workflow) in [
            (Axis::X, Axis::Y, Workflow::Srkr2d),
            (Axis::Y, Axis::X, Workflow::Srkr2d),
            (Axis::U, Axis::V, Workflow::Srkr2d),
            (Axis::V, Axis::U, Workflow::Srkr2d),
            (Axis::X, Axis::T, Workflow::Strkr),
            (Axis::U, Axis::T, Workflow::Strkr),
        ] {
            let plan = Plan {
                workflow,
                fast,
                slow,
                fast_range: trkr_core::Range {
                    min: 10.,
                    max: 30.,
                    step: 10.,
                },
                slow_range: trkr_core::Range {
                    min: 5.,
                    max: 7.,
                    step: 2.,
                },
                return_to_zero: [false; 2],
                ..Default::default()
            };
            assert!(!reset_fast_before_slow(&plan));
            let targets = plan.targets().unwrap();
            let mut fast_approaches = 0;
            for (index, target) in targets.iter().enumerate() {
                let line_start = index % 3 == 0;
                let moves = scan_moves(&plan, target.physical.map(Some), line_start, index == 0);
                for movement in moves {
                    if movement.axis == fast {
                        assert_eq!(movement.approach.iter().any(|b| *b), line_start);
                        fast_approaches += usize::from(line_start);
                        assert_eq!(
                            movement.target,
                            masked(target.physical.map(Some), axis_mask(fast))
                        );
                    } else {
                        assert_eq!(movement.axis, slow);
                        assert!(line_start);
                        assert_eq!(
                            movement.approach.iter().any(|b| *b),
                            index == 0 && slow != Axis::T
                        );
                    }
                }
            }
            assert_eq!(fast_approaches, 2);
            assert!(return_target(&plan, targets.last().unwrap()).is_none());
        }
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
        assert!(reset_fast_before_slow(&p));
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
    fn csv_float_preserves_python_repr_thresholds_sign_and_precision() {
        for (value, expected) in [
            (0., "0.0"),
            (-0., "-0.0"),
            (4., "4.0"),
            (1e-4, "0.0001"),
            (1e-5, "1e-05"),
            (1e15, "1000000000000000.0"),
            (1e16, "1e+16"),
            (-1e20, "-1e+20"),
            (1e-100, "1e-100"),
            (1.0000000000000002, "1.0000000000000002"),
        ] {
            assert_eq!(csv_float(value), expected);
        }
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
        assert_eq!(get("Theta_deg"), "4.0"); // Python CSV float representation.
        assert!(get("t_ps").is_empty());
    }
    #[test]
    fn manual_motion_phase_names_only_the_commanded_axes() {
        assert_eq!(motion_phase("Moving", [None, None, Some(1.)]), "Moving y");
        assert_eq!(
            motion_phase("Moving", [None, Some(1.), Some(2.)]),
            "Moving x/y"
        );
        assert_eq!(
            motion_phase("Initializing", [Some(0.), None, None]),
            "Initializing t"
        );
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
