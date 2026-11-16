//! Hardware Signal Monitor only: a separate executor that exclusively owns
//! every selected connected axis (delay stage T, Scanner X/Y) for the run, so
//! the GUI motion cards can move them while the monitor reads the lock-in.
//! The monitor itself never reads or records a T/X/Y position.
//!
//! Ownership is mechanical rather than advisory. The executor's rig holds
//! exclusive claims on its serial connections
//! (`trkr_motion::MotionRig::split_exclusive`), so the measurement worker can
//! no longer reach those controllers. Requests never pass through the
//! measurement loop. The loop only drains finished audit records after a
//! point is written, so a move never delays a scheduled read.
//!
//! Every move is validated without I/O, preceded by a stationary-state check,
//! executed with the driver's closed-loop preparation, readback verification
//! and STOP-on-error policy, and any failure latches a fault that refuses all
//! later moves of the run. If the stationary state cannot be confirmed after a
//! failure, the monitor ends with an error and normal cleanup STOPs every axis.
use super::timing;
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread,
    time::{Duration, Instant},
};
use trkr_motion::{MotionRig, Position};

const AXES: [&str; 3] = ["t", "x", "y"];
pub(super) const UNCONFIRMED: &str =
    "Parallel motion stationary state could not be confirmed after a failure";

/// One standalone move of executor-owned axes in physical sample µm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParallelMove {
    pub targets: [Option<f64>; 3],
    pub approach: [bool; 3],
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ParallelMotionStatus {
    /// T/X/Y axes owned by the executor for this run.
    pub axes: [bool; 3],
    /// New requests are accepted only while the monitor is measuring.
    pub accepting: bool,
    pub moving: bool,
    /// Last readback of the owned axes.
    pub position: Position,
    /// Latched failure; every later request of this run is refused.
    pub fault: Option<String>,
    /// The stationary state could not be confirmed after a failure.
    pub unconfirmed: bool,
    /// Most recent request outcome that did not latch a fault (for example a
    /// target outside the configured limits, rejected before any I/O).
    pub last_error: Option<String>,
    /// Number of finished requests, so observers can report each outcome once.
    pub finished: u64,
}

struct Shared {
    status: Mutex<ParallelMotionStatus>,
    requests: Sender<ParallelMove>,
    /// Immediate abort for the in-flight move: global Stop or failure cleanup.
    abort: Arc<AtomicBool>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Publication point shared by the controller and its worker. It holds an
/// executor only while that executor owns its axes.
#[derive(Clone, Default)]
pub(crate) struct Slot(Arc<Mutex<Option<Arc<Shared>>>>);

impl Slot {
    fn current(&self) -> Option<Arc<Shared>> {
        lock(&self.0).clone()
    }
    pub(crate) fn status(&self) -> Option<ParallelMotionStatus> {
        self.current().map(|shared| lock(&shared.status).clone())
    }
    /// Global Stop: abort an in-flight parallel move without waiting for the
    /// measurement worker to notice cancellation.
    pub(crate) fn abort(&self) {
        if let Some(shared) = self.current() {
            lock(&shared.status).accepting = false;
            shared.abort.store(true, Ordering::Release);
        }
    }
    /// Hand one move to the executor. At most one move is accepted at a time;
    /// nothing is queued behind a running move.
    pub(crate) fn request(&self, request: ParallelMove) -> Result<(), String> {
        let shared = self.current().ok_or(
            "Moves during a run are available only for axes handed off by a running hardware Signal Monitor",
        )?;
        let mut status = lock(&shared.status);
        validate_request(&status, &request)?;
        if shared.abort.load(Ordering::Acquire) {
            return Err("Stop requested; no parallel move was sent".into());
        }
        status.moving = true;
        if shared.requests.send(request).is_err() {
            status.moving = false;
            return Err("Parallel motion executor stopped".into());
        }
        Ok(())
    }
}

fn validate_request(status: &ParallelMotionStatus, request: &ParallelMove) -> Result<(), String> {
    if !status.accepting {
        return Err("Parallel motion is not accepting moves; no move was sent".into());
    }
    if let Some(fault) = &status.fault {
        return Err(format!(
            "Parallel motion is refused for the rest of this monitor after a failure: {fault}"
        ));
    }
    if status.moving {
        return Err("A parallel move is already in progress".into());
    }
    if request.targets.iter().all(Option::is_none) {
        return Err("No parallel move target".into());
    }
    for (axis, target) in request.targets.iter().enumerate() {
        if let Some(value) = target {
            if !status.axes[axis] {
                return Err(format!(
                    "Axis {} is not owned by parallel motion",
                    AXES[axis]
                ));
            }
            if !value.is_finite() {
                return Err("Motion target must be finite".into());
            }
        }
    }
    Ok(())
}

/// Result of joining an executor.
pub(super) struct Finished {
    pub audits: Vec<Value>,
    /// Makes the run fail: unconfirmed stationary state or a failed executor.
    pub error: Option<String>,
    /// The executor's last readback of its axes, for display only.
    pub position: Position,
}

pub(super) struct Executor {
    slot: Slot,
    shared: Arc<Shared>,
    audits: Receiver<Value>,
    thread: Option<thread::JoinHandle<MotionRig>>,
}

impl Executor {
    /// Starts the executor and publishes it. `rig` must already hold the
    /// exclusive claims; it is returned to the worker (and the claims
    /// released) only by `finish` or `Drop`, after the thread has exited.
    pub(super) fn spawn(
        slot: &Slot,
        rig: MotionRig,
        global: Arc<AtomicBool>,
        origin: Instant,
    ) -> Result<Self, String> {
        let axes = rig.connected_axes();
        if !axes.iter().any(|axis| *axis) {
            return Err("Parallel motion needs at least one connected axis".into());
        }
        let (requests, receiver) = mpsc::channel();
        let (audit_sender, audits) = mpsc::channel();
        let shared = Arc::new(Shared {
            status: Mutex::new(ParallelMotionStatus {
                axes,
                accepting: true,
                ..Default::default()
            }),
            requests,
            abort: Arc::new(AtomicBool::new(global.load(Ordering::Acquire))),
        });
        let worker = shared.clone();
        let thread = thread::Builder::new()
            .name("trkr-parallel-motion".into())
            .spawn(move || run(rig, receiver, &worker, &global, origin, &audit_sender))
            .map_err(|error| format!("Start parallel motion: {error}"))?;
        *lock(&slot.0) = Some(shared.clone());
        Ok(Self {
            slot: slot.clone(),
            shared,
            audits,
            thread: Some(thread),
        })
    }
    pub(super) fn take_audits(&self) -> Vec<Value> {
        self.audits.try_iter().collect()
    }
    pub(super) fn unconfirmed(&self) -> bool {
        lock(&self.shared.status).unconfirmed
    }
    /// Stops accepting requests and joins the executor. Without `abort`, a
    /// move that was already accepted finishes (global Stop can still abort
    /// it, because the executor stays published until it has exited).
    pub(super) fn finish(mut self, abort: bool) -> Finished {
        let joined = self.join(abort);
        let mut audits: Vec<Value> = self.audits.try_iter().collect();
        let status = lock(&self.shared.status).clone();
        let error = match joined {
            Some(true) if status.unconfirmed => Some(match status.fault {
                Some(fault) => format!("{UNCONFIRMED}: {fault}"),
                None => UNCONFIRMED.to_owned(),
            }),
            Some(true) => None,
            _ => {
                let error = "Parallel motion executor failed; scanner state is unconfirmed";
                audits.push(json!({"event":"parallel_motion_failed","device_role":"parallel_motion","error":error}));
                Some(error.to_owned())
            }
        };
        Finished {
            audits,
            error,
            position: status.position,
        }
    }
    /// `Some(true)` after a clean exit, `Some(false)` after a panic.
    fn join(&mut self, abort: bool) -> Option<bool> {
        let thread = self.thread.take()?;
        lock(&self.shared.status).accepting = false;
        if abort {
            self.shared.abort.store(true, Ordering::Release);
        }
        // Dropping the returned rig releases the exclusive claims.
        let clean = thread.join().map(drop).is_ok();
        let mut slot = lock(&self.slot.0);
        if slot
            .as_ref()
            .is_some_and(|published| Arc::ptr_eq(published, &self.shared))
        {
            *slot = None;
        }
        Some(clean)
    }
}

impl Drop for Executor {
    fn drop(&mut self) {
        // Unwinding or an early return must never leave a running owner.
        self.join(true);
    }
}

fn run(
    mut rig: MotionRig,
    requests: Receiver<ParallelMove>,
    shared: &Shared,
    global: &AtomicBool,
    origin: Instant,
    audits: &Sender<Value>,
) -> MotionRig {
    let cancelled = || global.load(Ordering::Acquire) || shared.abort.load(Ordering::Acquire);
    match rig.position() {
        Ok(position) => lock(&shared.status).position = position,
        Err(error) => lock(&shared.status).fault = Some(format!("Initial readback: {error}")),
    }
    loop {
        match requests.recv_timeout(Duration::from_millis(20)) {
            Ok(request) => {
                if cancelled() {
                    let _ = audits.send(
                        json!({"event":"parallel_move","device_role":"parallel_motion",
                        "status":"not_started","target":request.targets,"approach":request.approach,
                        "error":"motion cancelled"}),
                    );
                    let mut status = lock(&shared.status);
                    status.moving = false;
                    status.finished += 1;
                    status.last_error = Some("motion cancelled".into());
                    continue;
                }
                execute(&mut rig, request, shared, global, origin, audits);
            }
            Err(RecvTimeoutError::Timeout) => {
                if cancelled() || !lock(&shared.status).accepting {
                    break;
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    rig
}

fn execute(
    rig: &mut MotionRig,
    request: ParallelMove,
    shared: &Shared,
    global: &AtomicBool,
    origin: Instant,
    audits: &Sender<Value>,
) {
    let base = json!({"event":"parallel_move","device_role":"parallel_motion",
        "target":request.targets,"approach":request.approach});
    // Bounds and compensation approach are checked before any I/O. A rejected
    // target never reaches a controller and does not latch a fault.
    if let Err(error) = rig.validate_moves(&[(request.targets, request.approach)]) {
        let mut event = base;
        event["status"] = json!("rejected");
        event["error"] = json!(error);
        let _ = audits.send(event);
        let mut status = lock(&shared.status);
        status.moving = false;
        status.finished += 1;
        status.last_error = Some(error);
        return;
    }
    let abort = &*shared.abort;
    let mut before = None;
    let (result, window, _) = timing::observe(origin, || {
        if global.load(Ordering::Acquire) || abort.load(Ordering::Acquire) {
            return Err("motion cancelled".to_owned());
        }
        before = Some(rig.confirm_stationary()?);
        let mut progress = |position: Position| -> Result<(), String> {
            if global.load(Ordering::Acquire) {
                abort.store(true, Ordering::Release);
            }
            lock(&shared.status).position = position;
            Ok(())
        };
        rig.move_to_with_progress(
            request.targets,
            request.approach,
            abort,
            Some(&mut progress),
        )
    });
    let mut event = base;
    event["timing"] = json!(window);
    event["pre_move_position"] = json!(before);
    match result {
        Ok(position) => {
            event["status"] = json!("complete");
            event["final_position"] = json!(position);
            let _ = audits.send(event);
            let mut status = lock(&shared.status);
            status.position = position;
            status.moving = false;
            status.finished += 1;
            status.last_error = None;
        }
        Err(error) => {
            // The driver has already sent STOP for a started move. Confirm the
            // stationary state independently; never infer it from the text.
            let (stationary, check, _) = timing::observe(origin, || rig.confirm_stationary());
            event["status"] = json!("failed");
            event["error"] = json!(error);
            event["stationary_check"] = json!({"timing":check,"position":stationary.as_ref().ok(),
                "error":stationary.as_ref().err()});
            let _ = audits.send(event);
            let mut status = lock(&shared.status);
            match stationary {
                Ok(position) => status.position = position,
                Err(check) => {
                    status.unconfirmed = true;
                    status.fault = Some(format!("{error}; stationary check: {check}"));
                }
            }
            status.fault.get_or_insert(error.clone());
            status.last_error = Some(error);
            status.moving = false;
            status.finished += 1;
        }
    }
}

#[cfg(test)]
#[path = "parallel_motion_tests.rs"]
pub(super) mod tests;
