//! One persistent worker owns instruments, motion and acquisition.
mod execution;
mod recording;
mod status;
pub(crate) mod timing;
use crate::lockin::{self, Model};
pub(crate) use execution::active_axes;
#[cfg(test)]
use execution::{axis_mask, masked};
use execution::{line_reset, reset_fast_before_slow, return_moves, return_target, scan_moves};
pub use recording::FIELDS;
#[cfg(test)]
pub(crate) use recording::csv_float;
pub(crate) use recording::row;
use recording::{Recording, create_csv, finish_metadata};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use status::{MotionAction, OperationError, Phase};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
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
#[derive(Clone, Debug, Default)]
pub(crate) struct SampleRecording {
    pub run_id: String,
    pub point_index: usize,
    pub signal_unit: String,
    pub quality_flags: String,
    pub canonical_target: [Option<f64>; 3],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sample {
    #[serde(skip)]
    pub(crate) recording: Box<SampleRecording>,
    /// Host read intervals are recorded separately from protocol samples.
    #[serde(skip)]
    pub(crate) timing: Box<timing::AcquisitionTiming>,
    /// In-process GUI timing only; never part of JSON or CSV contracts.
    #[serde(skip)]
    pub(crate) captured_at: Option<Instant>,
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
    next_status: Instant,
}
impl Default for LiveMonitor {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            next: now,
            started: now,
            read_budget: LIVE_INTERVAL,
            demo_coordinates: [None; 3],
            next_status: now,
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
    mut gpib: Option<&mut Gpib>,
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
                let gpib = gpib
                    .as_deref_mut()
                    .ok_or(OperationError::LockinDisconnected)?;
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
        if let Source::Gpib { model, .. } = source
            && Instant::now() >= monitor.next_status
        {
            let started = Instant::now();
            monitor.next_status = started + Duration::from_millis(500);
            let gpib = gpib.ok_or(OperationError::LockinDisconnected)?;
            let status = match cancel {
                Some(token) => lockin::status_recorded_cancellable(model, gpib, token),
                None => lockin::status_recorded(model, gpib),
            };
            update_lockin_status(state, &status.result);
            let _ = send.try_send(Event::State(Box::new(state.clone())));
            monitor.read_budget += started.elapsed();
            status
                .result
                .map_err(|error| format!("Lock-in status: {error}"))?;
        }
    }
    Ok(())
}
fn update_lockin_status(state: &mut Snapshot, status: &Result<Value, String>) {
    if !state.lockin.is_object() {
        state.lockin = serde_json::json!({});
    }
    state.lockin["Overload"] = status.as_ref().ok().cloned().unwrap_or(Value::Null);
    state.lockin["Status Error"] = status
        .as_ref()
        .err()
        .map(|error| serde_json::json!(error))
        .unwrap_or(Value::Null);
}
fn expected_stop_error(error: &str) -> bool {
    matches!(
        error,
        "motion cancelled"
            | "transport: GPIB read cancelled"
            | "Lock-in status: transport: GPIB read cancelled"
    )
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
    /// Correlated completion for GUI-owned external operations only. Untagged
    /// GUI/stdio commands retain their existing event stream.
    RemoteFinished {
        id: String,
        phase: String,
        result: Result<(usize, Option<PathBuf>), String>,
        /// Stable category selected from the originating command. Error
        /// strings are intentionally never parsed for classification.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error_code: Option<String>,
    },
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

/// Classify only what the worker knows structurally at the command boundary.
/// Lower layers override this fallback when a recording or validation branch
/// identifies the origin. Never infer a category from the displayed text.
fn remote_failure_code(command: &Command) -> Option<&'static str> {
    Some(match command {
        Command::Initialize { .. } => "initialization_failed",
        Command::Connect { .. }
        | Command::Disconnect { .. }
        | Command::Read
        | Command::Move { .. }
        | Command::LockinSettings
        | Command::SetLockin { .. }
        | Command::AutoLockin { .. } => "device_error",
        Command::Run { .. } | Command::Replay { .. } => "job_failed",
        Command::Stop | Command::Shutdown => return None,
    })
}

pub struct Controller {
    pub events: Receiver<Event>,
    commands: Sender<(Command, Option<String>)>,
    cancel: Arc<AtomicBool>,
    busy: Arc<AtomicBool>,
    pending: Arc<AtomicUsize>,
    read_only: bool,
    thread: Option<thread::JoinHandle<()>>,
}
impl Controller {
    pub fn send(&self, command: Command) -> Result<(), String> {
        self.enqueue(command, None)
    }
    pub(crate) fn send_remote(&self, id: String, command: Command) -> Result<(), String> {
        if matches!(command, Command::Stop | Command::Shutdown) {
            return Err("Stop/shutdown cannot be queued as an external job".into());
        }
        self.enqueue(command, Some(id))
    }
    pub(crate) fn has_pending(&self) -> bool {
        self.pending.load(Ordering::Acquire) > 0
    }
    fn enqueue(&self, command: Command, remote_id: Option<String>) -> Result<(), String> {
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
        self.pending.fetch_add(1, Ordering::AcqRel);
        if self.commands.send((command, remote_id)).is_err() {
            self.pending.fetch_sub(1, Ordering::AcqRel);
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
        let _ = self.commands.send((Command::Shutdown, None));
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
    let (tx, rx) = mpsc::channel::<(Command, Option<String>)>();
    let (send, events) = mpsc::sync_channel(2048);
    let cancel = Arc::new(AtomicBool::new(false));
    let busy = Arc::new(AtomicBool::new(false));
    let token = cancel.clone();
    let working = busy.clone();
    let pending = Arc::new(AtomicUsize::new(0));
    let pending_worker = pending.clone();
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
            let (cmd, remote_id) = match rx.recv_timeout(timeout) {
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
            let remote_failure_code = remote_failure_code(&cmd);
            session.failure_code = None;
            if job {
                session.state.running = true;
                session.state.phase = Phase::Preparing.into();
                session.state.rows = 0;
                session.state.total = 0;
                let _ = send.send(Event::State(Box::new(session.state.clone())));
            }
            let mut result = if remote_id.is_some() {
                session.execute_remote(cmd, &token, &send)
            } else {
                session.execute(cmd, &token, &send)
            };
            let remote_failure_code = session
                .failure_code
                .clone()
                .or_else(|| remote_failure_code.map(str::to_owned));
            // Jobs own motion cleanup and its audit. Do not issue another
            // unrecorded STOP after their finalized result/metadata.
            if !job
                && result.is_err()
                && let Some(rig) = &mut session.rig
                && let Err(stop) = rig.stop()
            {
                result = Err(format!(
                    "{}; STOP failed: {stop}",
                    result.as_ref().unwrap_err()
                ));
            }
            if job && token.load(Ordering::Acquire) {
                session.state.phase = Phase::Stopping.into();
                let _ = send.try_send(Event::State(Box::new(session.state.clone())));
                let recovery = session
                    .gpib
                    .as_mut()
                    .map(Gpib::finish_cancelled_request)
                    .transpose();
                if let Err(error) = recovery {
                    result = Err(format!(
                        "{}; Stop response recovery failed: {error}",
                        result
                            .as_ref()
                            .err()
                            .map(String::as_str)
                            .unwrap_or("Cancelled")
                    ));
                } else if result
                    .as_ref()
                    .err()
                    .is_some_and(|error| expected_stop_error(error))
                {
                    result = Ok(None);
                }
            }
            if let Err(error) = &result {
                let _ = send.send(Event::Error(error.clone()));
            }
            session.clear_failed_lockin();
            if job {
                session.state.running = false;
                session.state.phase = if result.is_err() {
                    Phase::Error
                } else if token.load(Ordering::Acquire) {
                    Phase::Stopped
                } else if result.is_ok() {
                    Phase::Complete
                } else {
                    Phase::Error
                }
                .into();
                working.store(false, Ordering::Release);
            }
            pending_worker.fetch_sub(1, Ordering::AcqRel);
            let _ = send.send(Event::State(Box::new(session.state.clone())));
            if acquisition {
                let _ = send.send(Event::Finished(
                    result.clone().map(|out| (session.state.rows, out)),
                ));
            }
            if let Some(id) = remote_id {
                let error_code = result
                    .as_ref()
                    .err()
                    .and(remote_failure_code.as_deref())
                    .map(str::to_owned);
                let _ = send.send(Event::RemoteFinished {
                    id,
                    phase: if job {
                        session.state.phase.clone()
                    } else if result.is_ok() {
                        "Complete".into()
                    } else {
                        "Error".into()
                    },
                    result: result
                        .map(|out| (if acquisition { session.state.rows } else { 0 }, out)),
                    error_code,
                });
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
        pending,
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
    lockin_context: Option<Value>,
    state: Snapshot,
    /// Origin of the current command's failure, if a lower layer exposed one.
    /// This is per-worker state; it is never shared globally.
    failure_code: Option<String>,
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
            self.lockin_context = None;
            return true;
        }
        false
    }
    fn connect(&mut self, config: HardwareConfig, devices: Vec<Device>) -> Result<(), String> {
        // Reopening serial handles never initiates motion or homing.
        if self.config.as_ref().is_some_and(|previous| {
            previous.model != config.model || previous.address != config.address
        }) {
            self.lockin_context = None;
        }
        if devices.contains(&Device::Lockin) {
            let changed = self
                .config
                .as_ref()
                .is_none_or(|c| c.model != config.model || c.address != config.address);
            if changed {
                self.lockin_context = None;
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
    fn execute_remote(
        &mut self,
        command: Command,
        cancel: &AtomicBool,
        send: &SyncSender<Event>,
    ) -> Result<Option<PathBuf>, String> {
        let refresh = matches!(
            command,
            Command::SetLockin { .. } | Command::AutoLockin { .. }
        );
        let result = self.execute(command, cancel, send)?;
        if refresh {
            self.execute(Command::LockinSettings, cancel, send)
                .map_err(|error| {
                    format!("Lock-in setting was sent, but readback failed: {error}")
                })?;
        }
        Ok(result)
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
                    self.lockin_context = None;
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
                rig.validate_moves(&[(physical, approach)])
                    .inspect_err(|_| self.failure_code = Some("protection".into()))?;
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
                self.lockin_context = None;
                let model = self.config.as_ref().ok_or("Connect a lock-in first")?.model;
                self.state.lockin =
                    lockin::settings(self.gpib.as_mut().ok_or("Connect a lock-in first")?, model)?;
                self.lockin_context = Some(serde_json::json!({
                    "value": self.state.lockin,
                    "observed_at": recording::utc_now().ok(),
                    "qualification": "previous_settings_query_not_requeried_at_acquisition",
                }));
            }
            Command::SetLockin { setting, value } => {
                self.lockin_context = None;
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
                self.lockin_context = None;
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

    fn recording_context(&self, source: Source, configured: &HardwareConfig) -> Value {
        let mut context = serde_json::Map::new();
        context.insert("source".into(), serde_json::json!(source));
        context.insert("configured_hardware".into(), serde_json::json!(configured));
        context.insert(
            "transport".into(),
            if self.state.transport.is_empty() {
                Value::Null
            } else {
                serde_json::json!(self.state.transport)
            },
        );
        context.insert(
            "effective_delay_calibration".into(),
            serde_json::json!(self.state.delay_calibration),
        );
        context.insert(
            "effective_motion".into(),
            self.rig
                .as_ref()
                .map(|rig| serde_json::json!(rig.recording_configuration()))
                .unwrap_or(Value::Null),
        );
        context.insert(
            "connected_devices".into(),
            serde_json::json!(self.state.devices),
        );
        context.insert("resolution".into(), Value::Null);
        context.insert("uncertainty".into(), Value::Null);
        if let Source::Gpib { model, address } = source {
            context.insert(
                "lockin".into(),
                serde_json::json!({
                    "configured_model": model.name(),
                    "configured_address": address,
                    "model_qualification": "configured_not_verified_by_idn",
                }),
            );
        }
        if matches!(source, Source::Gpib { .. })
            && let Some(settings) = &self.lockin_context
        {
            context.insert("lockin_settings".into(), settings.clone());
        }
        Value::Object(context)
    }

    fn append_recording_error(result: &mut Result<(), String>, label: &str, error: String) {
        *result = match result {
            Ok(()) => Err(format!("{label}: {error}")),
            Err(original) => Err(format!("{original}; {label}: {error}")),
        };
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
        if matches!(source, Source::Demo) && plan.coordinate != Coordinate::Measurement {
            return Err("Demo scans require measurement coordinates; raw inputs need actual controller calibration".into());
        }
        let requested_config = config.clone();
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
                config
                    .motion
                    .validate_targets(&physical)
                    .inspect_err(|_| self.failure_code = Some("protection".into()))?;
                if let Some(r) = returns {
                    config
                        .motion
                        .validate_targets(&[std::array::from_fn(|i| {
                            r[i].map(|v| v + plan.zero[i])
                        })])
                        .inspect_err(|_| self.failure_code = Some("protection".into()))?;
                }
            }
            self.connect(config, devices)
                .inspect_err(|_| self.failure_code = Some("device_error".into()))?;
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
                rig.validate_targets(&physical)
                    .inspect_err(|_| self.failure_code = Some("protection".into()))?;
                if let Some(r) = returns {
                    rig.validate_targets(&[r])
                        .inspect_err(|_| self.failure_code = Some("protection".into()))?;
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
                        rig.validate_return(target)
                            .inspect_err(|_| self.failure_code = Some("protection".into()))?;
                    }
                }
                rig.validate_moves(&moves)
                    .inspect_err(|_| self.failure_code = Some("protection".into()))?;
            }
        }
        let mut csv = match output.map(|p| create_csv(p, source, plan)).transpose() {
            Ok(csv) => csv,
            Err(error) => {
                self.failure_code = Some("save_failed".into());
                return Err(error);
            }
        };
        let mut preparation_error = None;
        // Snapshot read-only settings before acquisition. Failure stays explicit;
        // do not guess units/status or change the instrument configuration.
        if let Source::Gpib { model, .. } = source {
            let gpib = self
                .gpib
                .as_mut()
                .ok_or(OperationError::LockinDisconnected)?;
            let mut snapshot = lockin::settings_recorded_cancellable(model, gpib, cancel);
            if let Ok(value) = &mut snapshot.result
                && let Some(object) = value.as_object_mut()
            {
                object.remove("available");
            }
            let observed_at = recording::utc_now().ok();
            if let Some(recording) = &mut csv {
                recording.append_audit(&serde_json::json!({"event":"settings_snapshot", "device_role":"lockin", "observed_at":observed_at,
                    "result":if snapshot.result.is_ok() { "ok" } else { "error" }, "exchanges":snapshot.exchanges,
                    "settings":snapshot.result.as_ref().ok(), "error":snapshot.result.as_ref().err()})).inspect_err(|_| self.failure_code = Some("save_failed".into()))?;
            }
            let fatal_snapshot =
                self.gpib.as_ref().is_some_and(Gpib::is_poisoned) || cancel.load(Ordering::Acquire);
            if fatal_snapshot {
                preparation_error = Some(
                    snapshot
                        .result
                        .as_ref()
                        .err()
                        .cloned()
                        .unwrap_or_else(|| "motion cancelled".into()),
                );
            }
            self.lockin_context = Some(match snapshot.result {
                Ok(mut value) => {
                    if let Some(object) = value.as_object_mut() {
                        object.remove("available");
                    }
                    serde_json::json!({"value":value,"observed_at":observed_at,"qualification":"queried_at_run_start","unqueried_controls":"unknown"})
                }
                Err(error) => {
                    serde_json::json!({"value":null,"observed_at":observed_at,"qualification":"query_failed","error":error})
                }
            });
        }
        let context_config = self.config.as_ref().unwrap_or(&requested_config);
        let context_result = preparation_error.map_or(Ok(()), Err).and_then(|()| {
            csv.as_mut()
                .map(|recording| {
                    recording.set_context(&self.recording_context(source, context_config))
                })
                .transpose()
                .map(|_| ())
        });
        self.state.total = targets.len();
        self.state.rows = 0;
        self.state.phase = Phase::Measuring.into();
        let _ = send.send(Event::State(Box::new(self.state.clone())));
        let start = Instant::now();
        if matches!(source, Source::Demo) {
            self.state.position = Position::default();
            self.live.started = start;
        }
        let acquisition_started = context_result.is_ok();
        let mut result = context_result.and_then(|()| {
            (|| {
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
                    if matches!(source, Source::Gpib { .. })
                        && plan.workflow != Workflow::SignalMonitor
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
                            let reset = rig.to_physical(
                                reset,
                                coordinate_name(plan.coordinate),
                                plan.zero,
                            )?;
                            rig.validate_targets(&[reset]).inspect_err(|_| self.failure_code = Some("protection".into()))?;
                            let (result,window,_) = timing::observe(start, || self.move_live(reset,[false;3],cancel,send));
                            if let Some(recording)=&mut csv {
                                let event=serde_json::json!({"event":"line_reset","point_index":index+1,"device_role":"motion","target":reset,"timing":window,"last_observed_position":self.state.position,"error":result.as_ref().err()});
                                if let Err(error)=recording.append_audit(&event) { return Err(match result {Err(original)=>format!("{original}; raw line reset: {error}"),Ok(())=>error}); }
                            }
                            result.inspect_err(|_| self.failure_code = Some("device_error".into()))?;
                        }
                        let first = index == 0 || target.line != targets[index - 1].line;
                        for movement in scan_moves(plan, physical[index], first, index == 0) {
                            self.set_phase(Phase::moving([movement.axis]), send);
                            let (movement_result,window,_) = timing::observe(start, || self.move_live(movement.target, movement.approach, cancel, send));
                            if let Some(recording)=&mut csv {
                                let event=serde_json::json!({"event":"move","point_index":index+1,"device_role":"motion", "target":movement.target,
                                    "approach":movement.approach,"timing":window,"final_position":self.state.position,"error":movement_result.as_ref().err()});
                                if let Err(error)=recording.append_audit(&event) { return Err(match movement_result { Err(original)=>format!("{original}; raw movement audit: {error}"),Ok(())=>error }); }
                            }
                            movement_result.inspect_err(|_| self.failure_code = Some("device_error".into()))?;
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
                    if !self.wait_live(due, source, cancel, send).inspect_err(|_| self.failure_code = Some("device_error".into()))? {
                        return Ok(());
                    }
                    self.set_phase(Phase::Reading, send);
                    let mut point_timing = timing::AcquisitionTiming::default();
                    let (signal, exchanges, captured) = match source {
                        Source::Gpib { model, .. } => {
                            if let Some(rig) = &mut self.rig {
                                let ((position, position_exchanges), window, _) = timing::observe(start, || rig.position_recorded());
                                point_timing.position = Some(window);
                                if let Some(recording) = &mut csv {
                                    recording.append_audit(&serde_json::json!({"event":"position_read", "point_index":index+1,
                                        "device_role":"motion", "position":position.as_ref().ok(), "exchanges":position_exchanges,
                                        "timing":point_timing, "error":position.as_ref().err()}))?;
                                }
                                match position {
                                    Ok(position) => self.state.position = position,
                                    Err(error) => {
                                        if let Some(recording) = &mut csv
                                            && let Err(audit_error) = recording.append_audit(&serde_json::json!({
                                                "event":"reading", "point_index":index+1, "status":"error",
                                                "failed_phase":"position_read", "error":error, "timing":point_timing,
                                            })) { return Err(format!("{error}; record position timing: {audit_error}")); }
                                        self.failure_code = Some("device_error".into());
                                        return Err(error);
                                    }
                                }
                            }
                            let gpib = self.gpib.as_mut().ok_or(OperationError::LockinDisconnected)?;
                            let (recorded, window, captured) = timing::observe(start, || lockin::read_recorded_cancellable(model, gpib, cancel));
                            point_timing.signal = Some(window);
                            let exchanges = serde_json::to_value(&recorded.exchanges)
                                .map_err(|error| error.to_string())?;
                            match recorded.result {
                                Ok(signal) => (signal, Some(exchanges), captured),
                                Err(error) => {
                                    if let Some(recording) = &mut csv {
                                        let audit = serde_json::json!({
                                            "event": "reading",
                                            "point_index": index + 1,
                                            "status": "error",
                                            "simulation": false,
                                            "elapsed_s": captured.duration_since(start).as_secs_f64(),
                                            "target": target,
                                            "position": self.state.position.clone(),
                                            "exchanges": exchanges,
                                            "error": error,
                                            "timing": point_timing,
                                        });
                                        if let Err(audit_error) = recording.append_audit(&audit) {
                                            return Err(format!(
                                                "{error}; record reading audit: {audit_error}"
                                            ));
                                        }
                                    }
                                    self.failure_code = Some("device_error".into());
                                    return Err(error);
                                }
                            }
                        }
                        Source::Demo => {
                            let (signal, window, captured) = timing::observe(start, || Signal::demo(
                                if plan.workflow == Workflow::SignalMonitor { start.elapsed().as_secs_f64() * 10. }
                                else { target.corrected[0] }, target.corrected[1], target.corrected[2]));
                            point_timing.signal = Some(window);
                            (signal, None, captured)
                        }
                    };
                    let mut signal_unit = "V".to_owned();
                    let mut quality_flags = "simulated".to_owned();
                    let mut status_event = None;
                    let mut status_failure = None;
                    if let Source::Gpib { model, .. } = source {
                        let gpib=self.gpib.as_mut().ok_or(OperationError::LockinDisconnected)?;
                        let (status,window,_) = timing::observe(start, || lockin::status_recorded_cancellable(model,gpib,cancel));
                        signal_unit = if model==Model::Sr7265 && status.result.as_ref().ok().and_then(|v|v["input_mode"].as_f64())==Some(0.) { "V" } else { "unknown" }.into();
                        update_lockin_status(&mut self.state, &status.result);
                        let _ = send.try_send(Event::State(Box::new(self.state.clone())));
                        status_failure = status.result.as_ref().err().cloned();
                        let mut flags=Vec::new();
                        match &status.result {
                            Ok(value) => { if value["overload"]==true { flags.push("overload"); }
                                if value["reference_unlock"]==true { flags.push("reference_unlock"); }
                                if value.get("reference_unlock").is_none() { flags.push("reference_status_unknown"); } },
                            Err(_) => flags.push("status_unknown"),
                        }
                        if signal_unit=="unknown" { flags.push("unit_unknown"); }
                        quality_flags=if flags.is_empty() { "ok".into() } else { flags.join("|") };
                        status_event=Some(serde_json::json!({"event":"status_check","point_index":index+1,"device_role":"lockin",
                            "timing":window, "settings":status.result.as_ref().ok(), "error":status.result.as_ref().err(),"exchanges":status.exchanges}));
                    }
                    let sample = Sample {
                        recording: Box::new(SampleRecording {
                            run_id: csv.as_ref().map(|r| r.run_id().to_owned()).unwrap_or_default(), point_index:index+1,
                            signal_unit, quality_flags,
                            canonical_target:std::array::from_fn(|i| physical[index][i].map(|v| v-plan.zero[i])),
                        }),
                        timing: Box::new(point_timing),
                        captured_at: Some(captured),
                        elapsed: captured.duration_since(start).as_secs_f64(),
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
                        let mut audit = serde_json::json!({
                            "event": "reading",
                            "point_index": index + 1,
                            "status": "ok",
                            "simulation": matches!(source, Source::Demo),
                            "elapsed_s": sample.elapsed,
                            "signal": sample.signal, "signal_unit":sample.recording.signal_unit,
                            "input_coordinate":coordinate_name(plan.coordinate),
                            "target": sample.target,
                            "position": sample.position.clone(),
                            "timing": sample.timing,
                        });
                        if let Some(exchanges) = exchanges {
                            audit["exchanges"] = exchanges;
                        }
                        csv.append_audit(&audit)?;
                        if let Some(status_event) = status_event { csv.append_audit(&status_event)?; }
                        csv.write_sample(plan, &sample, start, index + 1, &mut self.state.rows)?;
                    } else { self.state.rows += 1; }
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
                    let _ = send.try_send(Event::Sample(sample));
                    if let Some(error) = status_failure { self.failure_code = Some("device_error".into()); return Err(format!("Lock-in status: {error}")); }
                }
                if !cancel.load(Ordering::Acquire)
                    && matches!(source, Source::Gpib { .. })
                    && let Some(r) = returns
                {
                    for target in return_moves(plan, r) {
                        self.state.phase = Phase::motion(MotionAction::Returning, target).into();
                        let _ = send.send(Event::State(Box::new(self.state.clone())));
                        let (result,window,_) = timing::observe(start, || self.motion_live(target,[false;3],true,cancel,send));
                        if let Some(recording)=&mut csv {
                            let event=serde_json::json!({"event":"return_move","device_role":"motion","target":target,"timing":window,"last_observed_position":self.state.position,"error":result.as_ref().err()});
                            if let Err(error)=recording.append_audit(&event) { return Err(match result {Err(original)=>format!("{original}; raw return: {error}"),Ok(())=>error}); }
                        }
                        result?;
                    }
                }
                Ok(())
            })()
        });
        if acquisition_started && (result.is_err() || cancel.load(Ordering::Acquire)) {
            self.state.phase = Phase::Stopping.into();
            let _ = send.try_send(Event::State(Box::new(self.state.clone())));
            let (stopped, window, _) = timing::observe(start, || {
                self.rig.as_mut().map(|rig| rig.stop()).transpose()
            });
            if let Some(recording) = &mut csv
                && let Err(error) = recording.append_audit(&serde_json::json!({"event":"stop","device_role":"motion","timing":window,
                    "verification":if self.rig.is_some() {"stationary_state_queries"} else {"no_motion_devices"},"error":stopped.as_ref().err()}))
            {
                Self::append_recording_error(&mut result, "record STOP outcome", error);
            }
            if let Err(error) = stopped {
                result = Err(format!(
                    "{}; STOP failed: {error}",
                    result
                        .as_ref()
                        .err()
                        .map(String::as_str)
                        .unwrap_or("Cancelled")
                ));
            } else if cancel.load(Ordering::Acquire)
                && result
                    .as_ref()
                    .err()
                    .is_none_or(|error| expected_stop_error(error))
            {
                // A Stop request only clears the expected motion cancellation;
                // it must not hide a concurrent instrument or recording failure.
                result = Ok(());
            }
        }
        if cancel.load(Ordering::Acquire) {
            self.state.phase = Phase::Stopping.into();
            let _ = send.try_send(Event::State(Box::new(self.state.clone())));
            if let Some(gpib) = &mut self.gpib
                && let Err(error) = gpib.finish_cancelled_request()
            {
                Self::append_recording_error(&mut result, "Stop response recovery failed", error);
            }
        }
        if cancel.load(Ordering::Acquire)
            && result
                .as_ref()
                .err()
                .is_some_and(|error| expected_stop_error(error))
        {
            result = Ok(());
        }
        let terminal_status = if result.is_err() {
            "error"
        } else if cancel.load(Ordering::Acquire) {
            "cancelled"
        } else {
            "complete"
        };
        if let Some(recording) = &mut csv {
            if let Err(error) =
                recording.set_final_position(&self.state.position, &self.state.phase)
            {
                Self::append_recording_error(&mut result, "save final position", error);
            }
            let terminal = serde_json::json!({
                "event": "finished",
                "status": terminal_status,
                "last_observed_position":self.state.position, "last_phase":self.state.phase,
                "rows": self.state.rows,
                "simulation": matches!(source, Source::Demo),
                "error": result.as_ref().err(),
            });
            if let Err(error) = recording.append_audit(&terminal) {
                Self::append_recording_error(&mut result, "record finished audit", error);
            }
            if let Err(error) = recording.finish() {
                Self::append_recording_error(&mut result, "finish recording", error);
                let _ = recording.append_audit(&serde_json::json!({"event":"finalization_error", "status":"error", "error":result.as_ref().err()}));
            }
        }
        if let Some(path) = output {
            let status = if result.is_err() {
                "error"
            } else if cancel.load(Ordering::Acquire) {
                "cancelled"
            } else {
                "complete"
            };
            let metadata_result =
                finish_metadata(path, self.state.rows, status, result.as_ref().err());
            if let Err(error) = metadata_result {
                self.failure_code = Some("save_failed".into());
                Self::append_recording_error(&mut result, "finish run metadata", error);
                if let Some(recording) = &mut csv {
                    let _ = recording.append_audit(&serde_json::json!({"event":"finalization_error", "status":"error", "error":result.as_ref().err()}));
                }
            }
        }
        if csv.as_ref().is_some_and(Recording::save_failed) {
            self.failure_code = Some("save_failed".into());
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
        let modern = headers.iter().any(|value| value == "X");
        for key in if modern {
            ["X", "Y", "R", "Theta_deg"]
        } else {
            ["X_V", "Y_V", "R_V", "Theta_deg"]
        } {
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
                required(if modern { "X" } else { "X_V" })?,
                required(if modern { "Y" } else { "Y_V" })?,
                required(if modern { "R" } else { "R_V" })?,
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
                recording: Default::default(),
                timing: Default::default(),
                captured_at: None,
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
pub(crate) fn coordinate_name(c: Coordinate) -> &'static str {
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
    if plan.coordinate != Coordinate::Measurement {
        return Err("Demo scans require measurement coordinates; raw inputs need actual controller calibration".into());
    }
    let mut plan = plan;
    plan.interval_s = 0.0;
    let targets = plan.targets()?;
    let mut writer = create_csv(output, Source::Demo, &plan)?;
    let mut rows = 0usize;
    let start = Instant::now();
    let mut result = writer
        .set_context(&serde_json::json!({
            "source": Source::Demo,
            "transport": Value::Null,
            "resolution": Value::Null,
            "uncertainty": Value::Null,
        }))
        .and_then(|()| {
            for (index, target) in targets.iter().enumerate() {
                let p = target.corrected;
                let (signal, window, captured) =
                    timing::observe(start, || Signal::demo(p[0] + target.fast, p[1], p[2]));
                let sample = Sample {
                    recording: Box::new(SampleRecording {
                        run_id: writer.run_id().to_owned(),
                        point_index: index + 1,
                        signal_unit: "V".into(),
                        quality_flags: "simulated".into(),
                        canonical_target: target.corrected.map(Some),
                    }),
                    timing: Box::new(timing::AcquisitionTiming {
                        position: None,
                        signal: Some(window),
                    }),
                    captured_at: None,
                    elapsed: captured.duration_since(start).as_secs_f64(),
                    target: *target,
                    signal,
                    position: Position {
                        physical: target.physical.map(Some),
                        ..Default::default()
                    },
                };
                writer.append_audit(&serde_json::json!({
                    "event": "reading",
                    "point_index": index + 1,
                    "status": "ok",
                    "simulation": true,
                    "elapsed_s": sample.elapsed,
                    "signal": sample.signal, "signal_unit":sample.recording.signal_unit,
                            "input_coordinate":coordinate_name(plan.coordinate),
                    "target": sample.target,
                    "position": sample.position.clone(),
                    "timing": sample.timing,
                }))?;
                writer.write_sample(&plan, &sample, start, index + 1, &mut rows)?;
            }
            Ok(())
        });
    let terminal_status = if result.is_err() { "error" } else { "complete" };
    if let Err(error) = writer.append_audit(&serde_json::json!({
        "event": "finished",
        "status": terminal_status,
        "rows": rows,
        "simulation": true,
        "error": result.as_ref().err(),
    })) {
        Session::append_recording_error(&mut result, "record finished audit", error);
    }
    if let Err(error) = writer.finish() {
        Session::append_recording_error(&mut result, "finish recording", error);
        let _ = writer.append_audit(&serde_json::json!({"event":"finalization_error", "status":"error", "error":result.as_ref().err()}));
    }
    let status = if result.is_err() { "error" } else { "complete" };
    if let Err(error) = finish_metadata(output, rows, status, result.as_ref().err()) {
        Session::append_recording_error(&mut result, "finish run metadata", error);
        let _ = writer.append_audit(&serde_json::json!({"event":"finalization_error", "status":"error", "error":result.as_ref().err()}));
    }
    result?;
    Ok(rows)
}

#[cfg(test)]
#[path = "engine/tests.rs"]
mod tests;

#[cfg(test)]
mod recording_integration_tests {
    use super::*;

    #[test]
    fn demo_csv_records_context_readings_and_a_terminal_event_without_raw_exchanges() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("demo.csv");
        let plan = Plan {
            workflow: Workflow::Trkr,
            fast_range: trkr_core::Range {
                min: 0.0,
                max: 0.0,
                step: 1.0,
            },
            ..Default::default()
        };
        assert_eq!(demo_csv(plan, &output).unwrap(), 1);

        let metadata: Value =
            serde_json::from_slice(&std::fs::read(output.with_extension("run.json")).unwrap())
                .unwrap();
        assert_eq!(metadata["status"], "complete");
        assert_eq!(metadata["rows"], 1);
        assert_eq!(metadata["context"]["source"]["type"], "demo");
        assert_eq!(metadata["context"]["resolution"], Value::Null);
        assert_eq!(metadata["context"]["uncertainty"], Value::Null);

        let events = crate::raw_csv::test_events(&output.with_extension("raw.csv"));
        assert_eq!(events[0]["event"], "context");
        assert_eq!(events[1]["event"], "reading");
        assert_eq!(events[1]["point_index"], 1);
        assert_eq!(events[1]["status"], "ok");
        assert_eq!(events[1]["simulation"], true);
        assert!(events[1].get("exchanges").is_none());
        assert_eq!(events[2]["event"], "csv_write");
        let read = &events[1]["timing"]["signal"];
        let write = &events[2]["timing"];
        assert!(
            write["host_started_elapsed_s"].as_f64().unwrap()
                >= read["host_completed_elapsed_s"].as_f64().unwrap()
        );
        assert_eq!(read["device_timestamp_utc"], Value::Null);
        assert_eq!(events[1]["elapsed_s"], read["host_completed_elapsed_s"]);
        let mut csv = csv::Reader::from_path(&output).unwrap();
        let row = csv.records().next().unwrap().unwrap();
        assert_eq!(&row[2], read["host_completed_at_utc"].as_str().unwrap());
        assert_eq!(
            row[3].parse::<f64>().unwrap(),
            read["host_completed_elapsed_s"].as_f64().unwrap()
        );
        assert_eq!(events[3]["event"], "finished");
        assert_eq!(events[3]["status"], "complete");
    }
}

#[cfg(test)]
mod remote_failure_tests {
    use super::*;

    #[test]
    fn remote_failure_categories_follow_command_origin() {
        let config = HardwareConfig {
            model: Model::Sr7265,
            address: 0,
            motion: MotionConfig::default(),
        };
        let initialize = Command::Initialize {
            config: config.clone(),
            axis: 0,
            home: false,
            move_to_origin: false,
        };
        assert_eq!(
            remote_failure_code(&initialize),
            Some("initialization_failed")
        );
        assert_eq!(remote_failure_code(&Command::Read), Some("device_error"));
        assert_eq!(
            remote_failure_code(&Command::Run {
                source: Source::Demo,
                config,
                plan: Default::default(),
                output: None,
            }),
            Some("job_failed")
        );
        assert_eq!(remote_failure_code(&Command::Stop), None);
    }
}
