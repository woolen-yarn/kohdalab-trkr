//! One persistent worker owns instruments, motion and acquisition.
mod execution;
mod recording;
mod status;
use crate::lockin::{self, Model};
pub(crate) use execution::active_axes;
#[cfg(test)]
use execution::masked;
use execution::{
    axis_mask, line_reset, reset_fast_before_slow, return_moves, return_target, scan_moves,
};
pub use recording::FIELDS;
pub(crate) use recording::row;
use recording::{create_csv, finish_metadata};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use status::{MotionAction, OperationError, Phase};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};
#[cfg(test)]
use trkr_core::Axis;
use trkr_core::{Coordinate, Plan, Signal, Target, Workflow};
use trkr_motion::{DelayCalibration, MotionConfig, MotionRig, Position};
use trkr_transport::Gpib;

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
                let gpib = gpib.ok_or(OperationError::LockinDisconnected)?;
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
            return Err(OperationError::ReadOnly.into());
        }
        if matches!(command, Command::Stop) {
            self.stop();
            return Ok(());
        }
        let job = command.is_job();
        if job {
            if self.busy.swap(true, Ordering::AcqRel) {
                return Err(OperationError::Busy.into());
            }
            self.cancel.store(false, Ordering::Release);
        }
        if self.commands.send(command).is_err() {
            if job {
                self.busy.store(false, Ordering::Release);
            }
            return Err(OperationError::WorkerStopped.into());
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
                session.state.phase = Phase::Preparing.into();
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
                    Phase::Stopped
                } else if result.is_ok() {
                    Phase::Complete
                } else {
                    Phase::Error
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
                self.state.phase = Phase::motion(MotionAction::Moving, targets).into();
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
                    return Err(OperationError::UnknownAxis.into());
                }
                self.connect(config, vec![[Device::T, Device::X, Device::Y][axis]])?;
                self.state.phase = Phase::motion(
                    MotionAction::Initializing,
                    std::array::from_fn(|i| (i == axis).then_some(0.)),
                )
                .into();
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
        let rig = self
            .rig
            .as_mut()
            .ok_or(OperationError::MotionDisconnected)?;
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
    fn set_phase(&mut self, phase: Phase, send: &mpsc::SyncSender<Event>) {
        self.state.phase = phase.into();
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
            return Err(OperationError::MonitorInterval.into());
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
        self.state.phase = Phase::Measuring.into();
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
                        Phase::moving([plan.fast, plan.slow])
                    } else {
                        Phase::moving([plan.fast])
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
                        let rig = self
                            .rig
                            .as_ref()
                            .ok_or(OperationError::MotionDisconnected)?;
                        let reset =
                            rig.to_physical(reset, coordinate_name(plan.coordinate), plan.zero)?;
                        rig.validate_targets(&[reset])?;
                        self.move_live(reset, [false; 3], cancel, send)?;
                    }
                    let first = index == 0 || target.line != targets[index - 1].line;
                    for movement in scan_moves(plan, physical[index], first, index == 0) {
                        self.set_phase(Phase::moving([movement.axis]), send);
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
                self.set_phase(Phase::Waiting, send);
                if !self.wait_live(due, source, cancel, send)? {
                    return Ok(());
                }
                self.set_phase(Phase::Reading, send);
                let signal = match source {
                    Source::Gpib { model, .. } => {
                        if let Some(rig) = &mut self.rig {
                            self.state.position = rig.position()?;
                        }
                        lockin::read(
                            model,
                            self.gpib
                                .as_mut()
                                .ok_or(OperationError::LockinDisconnected)?,
                        )?
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
                    self.state.phase = Phase::motion(MotionAction::Returning, target).into();
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
#[path = "engine/tests.rs"]
mod tests;
