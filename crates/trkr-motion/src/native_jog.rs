//! Native (unbounded) jog lifecycle — **simulation only**.
//!
//! SHOT-302GS: `J:`+`G:` starts motion that only ends when `L:` arrives or
//! the travel end is reached.
//!
//! CONEX-CC has no jog command. It uses the controller's TRACKING mode
//! (Newport CONEX-CC manual EDH0246En1012): `TK1` enters tracking, where
//! `PA` updates the target on the fly; `ST` is accepted while tracking;
//! `TK0` leaves tracking. The session never sends an end-of-travel target.
//! While the hold lease is alive it rolls short, in-range setpoints at most
//! [`ROLLING_SETPOINT`] beyond the latest readback, so a lost link leaves
//! the stage at most one short setpoint from the last tracked position.
//!
//! TS reports READY T (36/37/38) when idle in position-tracking mode,
//! and TRACKING (46/47) during motion. TK can only change modes in READY
//! or READY T, so release waits for READY T before sending TK0.
//!
//! If the serial link is lost mid-jog, STOP may never arrive. Neither
//! controller documents a communication watchdog, so native jog is **not**
//! enabled on hardware: hardware holds keep using the bounded, lease-gated
//! segments of the worker.
//!
//! This module therefore only accepts [`SimulatedJogBus`] transports. The
//! trait is sealed and implemented solely by [`SimulatedController`], so a
//! real serial bus cannot be passed in. The state machine covers command
//! admission, release, STOP, stationary confirmation and tracking restore,
//! and fails closed: any lost exchange latches [`JogPhase::Unconfirmed`],
//! which is terminal for the session. Nothing is retried, and reconnecting
//! the link never re-arms a jog.
use crate::protocol::{self, Bus, DelayModel, ScannerModel, ScannerProtocol};

/// Exchanges spent waiting for a stationary state after STOP is accepted.
pub const CONFIRM_POLLS: usize = 20;

/// CONEX-CC: furthest a rolling tracking setpoint lies beyond the latest
/// position readback (controller units, mm). This bounds the travel left
/// over when the link is lost.
pub const ROLLING_SETPOINT: f64 = 0.1;

/// Tolerance for recognizing a CONEX-CC travel endpoint.
const SETTLED: f64 = 1e-6;

fn conex_tracking(state: &str) -> bool {
    matches!(state, "46" | "47")
}

fn conex_ready_tracking(state: &str) -> bool {
    matches!(state, "36" | "37" | "38")
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NativeJogModel {
    /// SHOT-302GS axis (1-based) in a controller with `axis_count` axes.
    /// Positions are in pulses.
    Shot302Gs { axis: u8, axis_count: u8 },
    /// CONEX-CC at `address`. Positions are controller units (mm); `travel`
    /// bounds every rolling tracking setpoint.
    ConexCc { address: u8, travel: [f64; 2] },
}

impl NativeJogModel {
    fn conex(address: u8) -> ScannerProtocol {
        ScannerProtocol {
            model: ScannerModel::ConexCc,
            address,
            axis: String::new(),
            ensure_closed_loop: false,
        }
    }
}

/// Whether native jog may run on real hardware for this model. It never may:
/// there is no documented device-side watchdog or physical-stop evidence
/// that ends motion when STOP is lost with the serial link.
pub fn hardware_native_jog(model: NativeJogModel) -> Result<(), &'static str> {
    Err(match model {
        NativeJogModel::Shot302Gs { .. } => {
            "SHOT-302GS jog runs until L: arrives and no communication watchdog is documented; hardware holds stay bounded"
        }
        NativeJogModel::ConexCc { .. } => {
            "CONEX-CC tracking runs until ST/TK0 arrive and no communication watchdog is documented; hardware holds stay bounded"
        }
    })
}

mod sealed {
    pub trait Sealed {}
}

/// A transport that only simulates a controller. Sealed: real buses cannot
/// implement it, so this lifecycle can never reach hardware.
pub trait SimulatedJogBus: Bus + sealed::Sealed {}

#[derive(Clone, Debug, PartialEq)]
pub enum JogPhase {
    /// Ready to admit a jog.
    Idle,
    /// Motion commanded; positions are tracked until release.
    Running { positive: bool },
    /// STOP was acknowledged and a stationary state plus a position readback
    /// confirmed it (CONEX-CC: also left tracking into READY). Tracking is
    /// restored to `position`.
    Stopped { position: f64 },
    /// The stop could not be confirmed (lost link, unacknowledged STOP,
    /// still moving, failed start). Terminal: no retries, no re-arm.
    Unconfirmed { reason: String },
}

#[derive(Clone, Debug, PartialEq)]
pub enum JogOutcome {
    ConfirmedStopped {
        position: f64,
    },
    /// Never presented as stopped. `last_tracked` is only the last readback
    /// before communication was lost, not the current position.
    Unconfirmed {
        reason: String,
        last_tracked: Option<f64>,
    },
}

impl JogOutcome {
    pub fn confirmed(&self) -> bool {
        matches!(self, Self::ConfirmedStopped { .. })
    }
}

/// One owner's native jog on one simulated axis.
#[derive(Debug)]
pub struct NativeJogSession {
    model: NativeJogModel,
    owner: u64,
    phase: JogPhase,
    tracked: Option<f64>,
}

impl NativeJogSession {
    pub fn new(model: NativeJogModel, owner: u64) -> Self {
        Self {
            model,
            owner,
            phase: JogPhase::Idle,
            tracked: None,
        }
    }

    pub fn phase(&self) -> &JogPhase {
        &self.phase
    }

    /// Last confirmed or tracked readback. After an unconfirmed stop this is
    /// stale and must not be shown as the stage position.
    pub fn tracked_position(&self) -> Option<f64> {
        self.tracked
    }

    fn owned(&self, owner: u64) -> Result<(), String> {
        if owner == self.owner {
            Ok(())
        } else {
            Err("Jog belongs to another hold; no command was sent".into())
        }
    }

    /// Admit and start one jog. Refused without sending anything unless the
    /// caller owns the session, its hold lease is alive, and no earlier stop
    /// is unconfirmed. The controller must report stationary first.
    pub fn start(
        &mut self,
        bus: &mut impl SimulatedJogBus,
        owner: u64,
        positive: bool,
        lease_alive: bool,
    ) -> Result<(), String> {
        self.owned(owner)?;
        match &self.phase {
            JogPhase::Idle | JogPhase::Stopped { .. } => {}
            JogPhase::Running { .. } => return Err("A jog is already running".into()),
            JogPhase::Unconfirmed { reason } => {
                return Err(format!(
                    "Previous jog stop is unconfirmed ({reason}); no jog is re-armed"
                ));
            }
        }
        if !lease_alive {
            return Err("Hold lease is not alive; no command was sent".into());
        }
        // Admission reads only; a failure here has started no motion.
        let stationary = match self.model {
            NativeJogModel::Shot302Gs { .. } => protocol::delay_ready(bus)?,
            // TK1 is only sent from READY; disabled or not-referenced states
            // are refused like a moving axis.
            NativeJogModel::ConexCc { address, .. } => NativeJogModel::conex(address).ready(bus)?,
        };
        if !stationary {
            return Err("Axis is not stationary; no jog was started".into());
        }
        let position = self.position(bus)?;
        self.tracked = Some(position);
        let started = match self.model {
            NativeJogModel::Shot302Gs { axis, .. } => {
                let sign = if positive { '+' } else { '-' };
                bus.query(&format!("J:{axis}{sign}"))
                    .and_then(|reply| protocol::ack(reply, "J"))
                    .and_then(|()| bus.query("G:"))
                    .and_then(|reply| protocol::ack(reply, "G"))
            }
            NativeJogModel::ConexCc { address, .. } => {
                let conex = NativeJogModel::conex(address);
                bus.write(&format!("{address}TK1"))
                    .and_then(|()| conex.state(bus))
                    .and_then(|state| {
                        if state == "36" {
                            Ok(())
                        } else {
                            Err(format!("TK1 did not enter tracking (state {state})"))
                        }
                    })
                    .and_then(|()| self.send_setpoint(bus, position, positive))
            }
        };
        match started {
            Ok(()) => {
                self.phase = JogPhase::Running { positive };
                Ok(())
            }
            Err(error) => {
                // Motion may already have begun. One STOP, never retried.
                let stop = self.send_stop(bus);
                let reason = match stop {
                    Ok(()) => format!("start failed ({error}); STOP sent but not confirmed"),
                    Err(stop) => format!("start failed ({error}); STOP failed ({stop})"),
                };
                self.phase = JogPhase::Unconfirmed {
                    reason: reason.clone(),
                };
                Err(reason)
            }
        }
    }

    /// Track a running jog. A lapsed lease releases it. A controller that
    /// stopped on its own (travel end) is confirmed by a stationary state.
    ///
    /// On a lost exchange SHOT-302GS sends one STOP (its jog is unbounded);
    /// CONEX-CC sends nothing more: its last rolling setpoint already bounds
    /// the motion. Either way the outcome latches unconfirmed.
    pub fn poll(
        &mut self,
        bus: &mut impl SimulatedJogBus,
        owner: u64,
        lease_alive: bool,
    ) -> Result<Option<JogOutcome>, String> {
        self.owned(owner)?;
        let JogPhase::Running { positive } = self.phase else {
            return Ok(None);
        };
        if !lease_alive {
            return self.release(bus, owner).map(Some);
        }
        match self.model {
            NativeJogModel::Shot302Gs { .. } => {
                let tracked = match self.position(bus) {
                    Ok(position) => protocol::delay_ready(bus).map(|ready| (position, ready)),
                    Err(error) => Err(error),
                };
                match tracked {
                    Ok((position, false)) => {
                        self.tracked = Some(position);
                        Ok(None)
                    }
                    Ok((_, true)) => Ok(Some(self.confirm_position(bus))),
                    Err(error) => {
                        let stop = self.send_stop(bus);
                        Ok(Some(self.unconfirmed(match stop {
                            Ok(()) => format!("communication lost ({error}); STOP not confirmed"),
                            Err(stop) => {
                                format!("communication lost ({error}); STOP failed ({stop})")
                            }
                        })))
                    }
                }
            }
            NativeJogModel::ConexCc { address, travel } => {
                let conex = NativeJogModel::conex(address);
                let tracked = self.position(bus).and_then(|position| {
                    let state = conex.state(bus)?;
                    if conex_tracking(&state) || conex_ready_tracking(&state) {
                        Ok(position)
                    } else {
                        Err(format!("left tracking unexpectedly (state {state})"))
                    }
                });
                let position = match tracked {
                    Ok(position) => position,
                    Err(error) => {
                        return Ok(Some(self.unconfirmed(format!(
                            "communication lost ({error}); no further command sent"
                        ))));
                    }
                };
                self.tracked = Some(position);
                let end = if positive {
                    travel[0].max(travel[1])
                } else {
                    travel[0].min(travel[1])
                };
                if (position - end).abs() <= SETTLED {
                    // At the travel end: leave tracking through the normal
                    // ST, stationary, TK0 and READY sequence.
                    return self.release(bus, owner).map(Some);
                }
                match self.send_setpoint(bus, position, positive) {
                    Ok(()) => Ok(None),
                    Err(error) => Ok(Some(self.unconfirmed(format!(
                        "communication lost ({error}); no further command sent"
                    )))),
                }
            }
        }
    }

    /// Release the hold: send STOP once, then require a stationary state and
    /// a position readback before reporting the axis as stopped. CONEX-CC
    /// additionally leaves tracking with `TK0` and must report READY.
    pub fn release(
        &mut self,
        bus: &mut impl SimulatedJogBus,
        owner: u64,
    ) -> Result<JogOutcome, String> {
        self.owned(owner)?;
        match &self.phase {
            JogPhase::Running { .. } => {}
            JogPhase::Unconfirmed { reason } => {
                return Ok(JogOutcome::Unconfirmed {
                    reason: reason.clone(),
                    last_tracked: self.tracked,
                });
            }
            _ => return Err("No jog is running".into()),
        }
        if let Err(error) = self.send_stop(bus) {
            // An unacknowledged STOP may or may not have arrived.
            return Ok(self.unconfirmed(format!("STOP not acknowledged ({error})")));
        }
        match self.model {
            NativeJogModel::Shot302Gs { .. } => {
                for _ in 0..CONFIRM_POLLS {
                    match protocol::delay_ready(bus) {
                        Ok(true) => return Ok(self.confirm_position(bus)),
                        Ok(false) => {}
                        Err(error) => {
                            return Ok(self.unconfirmed(format!(
                                "communication lost after STOP ({error}); stop not confirmed"
                            )));
                        }
                    }
                }
                Ok(self.unconfirmed("still moving after STOP; stop not confirmed".into()))
            }
            NativeJogModel::ConexCc { address, .. } => Ok(self.leave_tracking(bus, address)),
        }
    }

    /// Wait for READY T after ST before restoring ordinary positioning.
    fn leave_tracking(&mut self, bus: &mut impl SimulatedJogBus, address: u8) -> JogOutcome {
        let conex = NativeJogModel::conex(address);
        let mut settled = false;
        for _ in 0..CONFIRM_POLLS {
            match conex.state(bus) {
                Ok(state) if conex_ready_tracking(&state) => {
                    settled = true;
                    break;
                }
                Ok(state) if conex_tracking(&state) => {}
                Ok(state) => return self.unconfirmed(format!("unexpected state {state} after ST")),
                Err(error) => {
                    return self.unconfirmed(format!(
                        "communication lost after ST ({error}); stop not confirmed"
                    ));
                }
            }
        }
        if !settled {
            return self.unconfirmed("still moving after ST; TK0 not sent".into());
        }
        if let Err(error) = bus.write(&format!("{address}TK0")) {
            return self.unconfirmed(format!(
                "TK0 not acknowledged ({error}); tracking may still be active"
            ));
        }
        match conex.ready(bus) {
            Ok(true) => self.confirm_position(bus),
            Ok(false) => self.unconfirmed("TK0 did not reach ordinary READY".into()),
            Err(error) => self.unconfirmed(format!("READY after TK0 not confirmed ({error})")),
        }
    }

    /// One rolling CONEX-CC tracking setpoint: at most [`ROLLING_SETPOINT`]
    /// beyond `from`, never outside travel.
    fn send_setpoint(
        &self,
        bus: &mut impl SimulatedJogBus,
        from: f64,
        positive: bool,
    ) -> Result<(), String> {
        let NativeJogModel::ConexCc { address, travel } = self.model else {
            return Err("tracking setpoints are CONEX-CC only".into());
        };
        let step = if positive {
            ROLLING_SETPOINT
        } else {
            -ROLLING_SETPOINT
        };
        let target = (from + step).clamp(travel[0].min(travel[1]), travel[0].max(travel[1]));
        NativeJogModel::conex(address).start(bus, target)
    }

    fn confirm_position(&mut self, bus: &mut impl SimulatedJogBus) -> JogOutcome {
        match self.position(bus) {
            Ok(position) => {
                self.tracked = Some(position);
                self.phase = JogPhase::Stopped { position };
                JogOutcome::ConfirmedStopped { position }
            }
            Err(error) => self.unconfirmed(format!(
                "stationary but position readback failed ({error}); tracking not restored"
            )),
        }
    }

    fn unconfirmed(&mut self, reason: String) -> JogOutcome {
        self.phase = JogPhase::Unconfirmed {
            reason: reason.clone(),
        };
        JogOutcome::Unconfirmed {
            reason,
            last_tracked: self.tracked,
        }
    }

    fn send_stop(&self, bus: &mut impl SimulatedJogBus) -> Result<(), String> {
        match self.model {
            NativeJogModel::Shot302Gs { axis, .. } => {
                protocol::delay_stop(bus, DelayModel::Shot302Gs, axis)
            }
            NativeJogModel::ConexCc { address, .. } => NativeJogModel::conex(address).stop(bus),
        }
    }

    fn position(&self, bus: &mut impl SimulatedJogBus) -> Result<f64, String> {
        match self.model {
            NativeJogModel::Shot302Gs { axis, axis_count } => {
                protocol::delay_position(bus, axis, axis_count).map(|pulse| pulse as f64)
            }
            NativeJogModel::ConexCc { address, .. } => NativeJogModel::conex(address).position(bus),
        }
    }
}

/// A scripted controller for one axis. Every exchange is one time tick; the
/// stage keeps moving while the link is down, exactly the hazard that keeps
/// native jog off hardware.
#[derive(Debug)]
pub struct SimulatedController {
    model: NativeJogModel,
    pub position: f64,
    travel: [f64; 2],
    speed: f64,
    pending: Option<bool>,
    moving: Option<bool>,
    target: Option<f64>,
    decelerating: Option<u32>,
    decel_ticks: u32,
    link_up: bool,
    /// CONEX-CC is in TRACKING (entered with TK1, left with TK0).
    tracking: bool,
    /// CONEX-CC has started at least one tracking move since entering READY T.
    tracking_moved: bool,
    /// STOP is executed but its reply is lost.
    pub lose_stop_reply: bool,
    /// STOP is acknowledged but the stage keeps moving.
    pub ignore_stop: bool,
    /// Every command that reached the controller, in order.
    pub log: Vec<String>,
    /// CONEX-CC PA targets refused because they were outside travel.
    pub rejected: Vec<String>,
}

impl sealed::Sealed for SimulatedController {}
impl SimulatedJogBus for SimulatedController {}

impl SimulatedController {
    pub fn new(model: NativeJogModel, position: f64, travel: [f64; 2], speed: f64) -> Self {
        Self {
            model,
            position,
            travel: [travel[0].min(travel[1]), travel[0].max(travel[1])],
            speed,
            pending: None,
            moving: None,
            target: None,
            decelerating: None,
            decel_ticks: 3,
            link_up: true,
            tracking: false,
            tracking_moved: false,
            lose_stop_reply: false,
            ignore_stop: false,
            log: Vec::new(),
            rejected: Vec::new(),
        }
    }

    pub fn drop_link(&mut self) {
        self.link_up = false;
    }

    pub fn restore_link(&mut self) {
        self.link_up = true;
    }

    /// The stage is physically moving or decelerating.
    pub fn physically_moving(&self) -> bool {
        self.moving.is_some()
    }

    /// CONEX-CC is still in TRACKING.
    pub fn tracking(&self) -> bool {
        self.tracking
    }

    /// Let simulated time pass without any exchange.
    pub fn advance(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.tick();
        }
    }

    fn tick(&mut self) {
        let Some(positive) = self.moving else { return };
        let mut step = if positive { self.speed } else { -self.speed };
        if self.decelerating.is_some() {
            step *= 0.5;
        }
        let mut next = self.position + step;
        let end = self.target.unwrap_or(if positive {
            self.travel[1]
        } else {
            self.travel[0]
        });
        if (positive && next >= end) || (!positive && next <= end) {
            next = end;
            self.moving = None;
            self.decelerating = None;
            self.target = None;
        }
        self.position = next.clamp(self.travel[0], self.travel[1]);
        if let Some(left) = self.decelerating {
            if left <= 1 {
                self.moving = None;
                self.decelerating = None;
                self.target = None;
            } else {
                self.decelerating = Some(left - 1);
            }
        }
    }

    fn exchange(&mut self, command: &str) -> Result<(), String> {
        self.tick();
        if !self.link_up {
            return Err("simulated serial link lost".into());
        }
        self.log.push(command.into());
        Ok(())
    }

    fn begin_stop(&mut self) {
        if !self.ignore_stop && self.moving.is_some() && self.decelerating.is_none() {
            self.decelerating = Some(self.decel_ticks);
        }
    }

    fn conex_prefix(&self) -> Option<String> {
        match self.model {
            NativeJogModel::ConexCc { address, .. } => Some(address.to_string()),
            NativeJogModel::Shot302Gs { .. } => None,
        }
    }

    /// Reduced CONEX-CC TS model for ordinary and tracking moves.
    fn conex_state(&self) -> &'static str {
        match (self.tracking, self.moving.is_some()) {
            (true, false) if self.tracking_moved => "37",
            (true, false) => "36",
            (true, true) => "47",
            (false, true) => "28",
            (false, false) => "33",
        }
    }
}

impl Bus for SimulatedController {
    fn query(&mut self, command: &str) -> Result<String, String> {
        self.exchange(command)?;
        if let Some(prefix) = self.conex_prefix() {
            let body = command
                .strip_prefix(&prefix)
                .ok_or_else(|| format!("unexpected query {command}"))?;
            return match body {
                "TS" => Ok(format!("{prefix}TS0000{}", self.conex_state())),
                "TP" => Ok(format!("{prefix}TP{:.6}", self.position)),
                _ => Err(format!("unexpected query {command}")),
            };
        }
        let NativeJogModel::Shot302Gs { axis, axis_count } = self.model else {
            unreachable!()
        };
        let busy = self.moving.is_some();
        if let Some(rest) = command.strip_prefix("J:") {
            let positive = match rest.strip_prefix(&axis.to_string()) {
                Some("+") => true,
                Some("-") => false,
                _ => return Ok("NG".into()),
            };
            if busy {
                return Ok("NG".into());
            }
            self.pending = Some(positive);
            return Ok("OK".into());
        }
        match command {
            "G:" => match self.pending.take() {
                Some(positive) if !busy => {
                    self.moving = Some(positive);
                    Ok("OK".into())
                }
                _ => Ok("NG".into()),
            },
            "!:" => Ok(if busy { "B" } else { "R" }.into()),
            "Q:" => {
                let mut fields: Vec<String> = (1..=axis_count)
                    .map(|index| {
                        let value = if index == axis {
                            self.position.round() as i64
                        } else {
                            0
                        };
                        format!("{value:>10}")
                    })
                    .collect();
                let state = if busy { "B" } else { "R" };
                fields.extend([String::from("K"), String::from("K"), String::from(state)]);
                Ok(fields.join(","))
            }
            _ => Err(format!("unexpected query {command}")),
        }
    }

    fn write(&mut self, command: &str) -> Result<(), String> {
        self.exchange(command)?;
        let prefix = self
            .conex_prefix()
            .ok_or_else(|| format!("unexpected write {command}"))?;
        let body = command
            .strip_prefix(&prefix)
            .ok_or_else(|| format!("unexpected write {command}"))?;
        match body {
            // TK1 is accepted from a stationary READY axis only.
            "TK1" => {
                if self.moving.is_none() && !self.tracking {
                    self.tracking = true;
                    self.tracking_moved = false;
                }
                return Ok(());
            }
            // TK0 is accepted in READY T only.
            "TK0" => {
                if self.tracking && self.moving.is_none() {
                    self.tracking = false;
                }
                return Ok(());
            }
            _ => {}
        }
        let target = body
            .strip_prefix("PA")
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite())
            .ok_or_else(|| format!("unexpected write {command}"))?;
        if target < self.travel[0] || target > self.travel[1] {
            // Out-of-range targets are refused without motion.
            self.rejected.push(command.into());
            return Ok(());
        }
        if self.tracking {
            // Tracking: PA updates the target on the fly, even mid-motion.
            if self.decelerating.is_none() {
                self.tracking_moved = true;
                if (target - self.position).abs() > f64::EPSILON {
                    self.moving = Some(target > self.position);
                    self.target = Some(target);
                } else {
                    self.moving = None;
                    self.target = None;
                }
            }
        } else if self.moving.is_none() && (target - self.position).abs() > f64::EPSILON {
            self.moving = Some(target > self.position);
            self.target = Some(target);
            self.tracking_moved = false;
        }
        Ok(())
    }

    fn emergency_write(&mut self, command: &str) -> Result<(), String> {
        self.exchange(command)?;
        match self.conex_prefix() {
            // ST is accepted in and out of tracking; tracking is kept.
            Some(prefix) if command == format!("{prefix}ST") => {
                self.begin_stop();
                Ok(())
            }
            _ => Err(format!("unexpected emergency write {command}")),
        }
    }

    fn stop_query(&mut self, command: &str) -> Result<String, String> {
        self.exchange(command)?;
        let NativeJogModel::Shot302Gs { axis, .. } = self.model else {
            return Err(format!("unexpected stop {command}"));
        };
        if command != format!("L:{axis}") {
            return Err(format!("unexpected stop {command}"));
        }
        self.begin_stop();
        if self.lose_stop_reply {
            Err("simulated STOP reply lost".into())
        } else {
            Ok("OK".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: u64 = 7;

    fn shot() -> (NativeJogSession, SimulatedController) {
        let model = NativeJogModel::Shot302Gs {
            axis: 1,
            axis_count: 2,
        };
        (
            NativeJogSession::new(model, OWNER),
            SimulatedController::new(model, 1_000., [-50_000., 50_000.], 100.),
        )
    }

    fn conex() -> (NativeJogSession, SimulatedController) {
        let model = NativeJogModel::ConexCc {
            address: 1,
            travel: [-6., 6.],
        };
        (
            NativeJogSession::new(model, OWNER),
            SimulatedController::new(model, 0., [-6., 6.], 0.02),
        )
    }

    fn setpoints(bus: &SimulatedController) -> Vec<f64> {
        bus.log
            .iter()
            .filter_map(|c| c.strip_prefix("1PA"))
            .map(|v| v.parse().unwrap())
            .collect()
    }

    #[test]
    fn hardware_never_enables_native_jog() {
        for model in [shot().0.model, conex().0.model] {
            assert!(hardware_native_jog(model).unwrap_err().contains("bounded"));
        }
    }

    #[test]
    fn shot_jog_admits_tracks_stops_and_restores_tracking() {
        let (mut jog, mut bus) = shot();
        jog.start(&mut bus, OWNER, true, true).unwrap();
        assert_eq!(bus.log, ["!:", "Q:", "J:1+", "G:"]);
        assert_eq!(jog.phase(), &JogPhase::Running { positive: true });
        for _ in 0..3 {
            assert_eq!(jog.poll(&mut bus, OWNER, true).unwrap(), None);
        }
        assert!(jog.tracked_position().unwrap() > 1_000.);
        let outcome = jog.release(&mut bus, OWNER).unwrap();
        assert!(outcome.confirmed(), "{outcome:?}");
        assert!(!bus.physically_moving());
        assert!(bus.log.contains(&"L:1".to_string()));
        // Tracking is restored from a readback taken after the stationary state.
        let JogOutcome::ConfirmedStopped { position } = outcome else {
            unreachable!()
        };
        assert_eq!(position, bus.position.round());
        assert_eq!(jog.tracked_position(), Some(position));
        // A confirmed stop admits the next jog of the same owner.
        jog.start(&mut bus, OWNER, false, true).unwrap();
        assert!(jog.release(&mut bus, OWNER).unwrap().confirmed());
    }

    #[test]
    fn conex_enters_tracking_rolls_short_setpoints_and_leaves_with_st_tk0() {
        let (mut jog, mut bus) = conex();
        jog.start(&mut bus, OWNER, false, true).unwrap();
        // READY, readback, TK1, tracking verified, one short setpoint.
        assert_eq!(bus.log, ["1TS", "1TP", "1TK1", "1TS", "1PA-0.1000"]);
        assert!(bus.tracking());
        for _ in 0..8 {
            assert_eq!(jog.poll(&mut bus, OWNER, true).unwrap(), None);
        }
        let tracked = jog.tracked_position().unwrap();
        assert!(
            tracked < -0.1,
            "rolled beyond the first setpoint: {tracked}"
        );
        // Every setpoint stays in travel and within one short step of the
        // readback it was rolled from; none is an end-of-travel target.
        let targets = setpoints(&bus);
        assert!(targets.len() > 1);
        assert!(targets.iter().all(|t| (-6.0..=6.0).contains(t) && *t > -5.));
        assert!(bus.rejected.is_empty());
        let outcome = jog.release(&mut bus, OWNER).unwrap();
        assert!(outcome.confirmed(), "{outcome:?}");
        assert!(!bus.physically_moving() && !bus.tracking());
        // ST, then READY T, then TK0 and ordinary READY.
        let st = bus.log.iter().position(|c| c == "1ST").unwrap();
        let tk0 = bus.log.iter().position(|c| c == "1TK0").unwrap();
        assert!(st < tk0);
        assert_eq!(bus.log[tk0 + 1..], ["1TS", "1TP"]);
        assert_eq!(bus.conex_state(), "33");
        assert_eq!(bus.log.iter().filter(|c| *c == "1ST").count(), 1);
        assert_eq!(bus.log.iter().filter(|c| *c == "1TK0").count(), 1);
        let JogOutcome::ConfirmedStopped { position } = outcome else {
            unreachable!()
        };
        assert!((position - bus.position).abs() < 1e-6);
        // READY from tracking admits the next jog.
        jog.start(&mut bus, OWNER, true, true).unwrap();
        assert!(jog.release(&mut bus, OWNER).unwrap().confirmed());
    }

    #[test]
    fn reaching_travel_end_is_confirmed_through_the_tracking_exit() {
        let (mut jog, mut bus) = conex();
        bus.position = 5.98;
        jog.start(&mut bus, OWNER, true, true).unwrap();
        let mut outcome = None;
        for _ in 0..10 {
            outcome = jog.poll(&mut bus, OWNER, true).unwrap();
            if outcome.is_some() {
                break;
            }
        }
        assert_eq!(outcome, Some(JogOutcome::ConfirmedStopped { position: 6. }));
        assert!(!bus.tracking());
        assert!(setpoints(&bus).iter().all(|t| *t <= 6.));
        assert!(bus.rejected.is_empty());
    }

    #[test]
    fn admission_refuses_without_sending_for_wrong_owner_lapsed_lease_or_motion() {
        let (mut jog, mut bus) = shot();
        assert!(jog.start(&mut bus, OWNER + 1, true, true).is_err());
        assert!(jog.start(&mut bus, OWNER, true, false).is_err());
        assert!(bus.log.is_empty());
        // Another hold cannot release or poll this jog.
        jog.start(&mut bus, OWNER, true, true).unwrap();
        let sent = bus.log.len();
        assert!(jog.release(&mut bus, OWNER + 1).is_err());
        assert!(jog.poll(&mut bus, OWNER + 1, true).is_err());
        assert!(jog.start(&mut bus, OWNER, true, true).is_err());
        assert_eq!(bus.log.len(), sent);
        assert!(bus.physically_moving());
        // An axis already moving is never jogged.
        let (mut other, _) = shot();
        assert!(
            other
                .start(&mut bus, OWNER, true, true)
                .unwrap_err()
                .contains("not stationary")
        );
        assert!(!bus.log[sent..].iter().any(|c| c.starts_with("J:")));
        assert!(jog.release(&mut bus, OWNER).unwrap().confirmed());
    }

    #[test]
    fn conex_admission_never_sends_tk1_without_owner_lease_or_ready() {
        let (mut jog, mut bus) = conex();
        assert!(jog.start(&mut bus, OWNER + 1, true, true).is_err());
        assert!(jog.start(&mut bus, OWNER, true, false).is_err());
        assert!(bus.log.is_empty());
        jog.start(&mut bus, OWNER, true, true).unwrap();
        // A second session sees TRACKING, not READY, and sends no TK1.
        let (mut other, _) = conex();
        let sent = bus.log.len();
        assert!(other.start(&mut bus, OWNER, true, true).is_err());
        assert_eq!(bus.log[sent..], ["1TS"]);
        assert!(jog.release(&mut bus, OWNER).unwrap().confirmed());
    }

    #[test]
    fn lapsed_lease_releases_with_a_confirmed_stop() {
        for (mut jog, mut bus) in [shot(), conex()] {
            jog.start(&mut bus, OWNER, true, true).unwrap();
            let outcome = jog.poll(&mut bus, OWNER, false).unwrap().unwrap();
            assert!(outcome.confirmed(), "{outcome:?}");
            assert!(bus.log.iter().any(|c| c == "L:1" || c == "1ST"));
            assert!(!bus.tracking() && !bus.physically_moving());
        }
    }

    #[test]
    fn shot_lost_link_fails_closed_unconfirmed_and_never_rearms_on_reconnect() {
        let (mut jog, mut bus) = shot();
        jog.start(&mut bus, OWNER, true, true).unwrap();
        jog.poll(&mut bus, OWNER, true).unwrap();
        let tracked = jog.tracked_position();
        bus.drop_link();
        let outcome = jog.poll(&mut bus, OWNER, true).unwrap().unwrap();
        let JogOutcome::Unconfirmed {
            reason,
            last_tracked,
        } = &outcome
        else {
            panic!("lost link reported as {outcome:?}");
        };
        assert!(reason.contains("communication lost"), "{reason}");
        assert_eq!(*last_tracked, tracked);
        // STOP never arrived: the simulated stage is still moving.
        assert!(bus.physically_moving());
        bus.advance(5);
        bus.restore_link();
        let sent = bus.log.len();
        // Release after reconnection repeats the latched outcome only.
        assert_eq!(jog.release(&mut bus, OWNER).unwrap(), outcome);
        assert!(
            jog.start(&mut bus, OWNER, true, true)
                .unwrap_err()
                .contains("re-armed")
        );
        assert_eq!(jog.poll(&mut bus, OWNER, true).unwrap(), None);
        assert_eq!(bus.log.len(), sent, "no retry or re-arm after reconnect");
        assert!(matches!(jog.phase(), JogPhase::Unconfirmed { .. }));
    }

    #[test]
    fn conex_lost_link_sends_nothing_more_stops_within_one_setpoint_and_never_rearms() {
        let (mut jog, mut bus) = conex();
        jog.start(&mut bus, OWNER, true, true).unwrap();
        for _ in 0..4 {
            jog.poll(&mut bus, OWNER, true).unwrap();
        }
        let last_target = *setpoints(&bus).last().unwrap();
        let sent = bus.log.len();
        bus.drop_link();
        let outcome = jog.poll(&mut bus, OWNER, true).unwrap().unwrap();
        let JogOutcome::Unconfirmed { reason, .. } = &outcome else {
            panic!("lost link reported as {outcome:?}");
        };
        assert!(reason.contains("communication lost"), "{reason}");
        // No ST, PA or TK0 was attempted after the lost exchange.
        bus.advance(50);
        bus.restore_link();
        assert_eq!(bus.log.len(), sent);
        // The last rolling setpoint bounds the runaway: the stage stopped at
        // it, not at the travel end.
        assert!(!bus.physically_moving());
        assert!(
            (bus.position - last_target).abs() < 1e-9,
            "{}",
            bus.position
        );
        assert!(bus.position < 6.);
        // Reconnection repeats the latched outcome; nothing is re-armed.
        assert_eq!(jog.release(&mut bus, OWNER).unwrap(), outcome);
        assert!(
            jog.start(&mut bus, OWNER, true, true)
                .unwrap_err()
                .contains("re-armed")
        );
        assert_eq!(jog.poll(&mut bus, OWNER, true).unwrap(), None);
        assert_eq!(bus.log.len(), sent, "no write or re-arm after reconnect");
        assert!(bus.tracking(), "TK0 is never sent without a confirmed stop");
    }

    #[test]
    fn unacknowledged_stop_is_unconfirmed_even_if_the_stage_stopped() {
        let (mut jog, mut bus) = shot();
        jog.start(&mut bus, OWNER, true, true).unwrap();
        bus.lose_stop_reply = true;
        let outcome = jog.release(&mut bus, OWNER).unwrap();
        assert!(!outcome.confirmed());
        bus.advance(5);
        assert!(!bus.physically_moving());
        // Exactly one STOP: no retry.
        assert_eq!(bus.log.iter().filter(|c| *c == "L:1").count(), 1);
    }

    #[test]
    fn stop_that_does_not_settle_is_unconfirmed_and_never_sends_tk0() {
        let (mut jog, mut bus) = conex();
        jog.start(&mut bus, OWNER, false, true).unwrap();
        bus.ignore_stop = true;
        bus.speed = 0.0001;
        let outcome = jog.release(&mut bus, OWNER).unwrap();
        assert!(!outcome.confirmed(), "{outcome:?}");
        let after_st = bus.log.iter().position(|c| c == "1ST").unwrap();
        assert!(bus.log[after_st..].iter().filter(|c| *c == "1TS").count() <= CONFIRM_POLLS);
        assert_eq!(bus.log.iter().filter(|c| *c == "1ST").count(), 1);
        assert!(!bus.log.iter().any(|c| c == "1TK0"));
    }

    #[test]
    fn link_lost_after_stop_ack_is_not_reported_stopped() {
        let (mut jog, mut bus) = conex();
        jog.start(&mut bus, OWNER, true, true).unwrap();
        // ST reaches the controller; the link drops before confirmation.
        let mut bus = DropAfter {
            inner: bus,
            remaining: 1,
        };
        let outcome = jog.release(&mut bus, OWNER).unwrap();
        assert!(!outcome.confirmed(), "{outcome:?}");
        assert_eq!(bus.inner.log.last().map(String::as_str), Some("1ST"));
    }

    /// Scripted wrapper: the link drops after `remaining` exchanges.
    struct DropAfter {
        inner: SimulatedController,
        remaining: usize,
    }
    impl sealed::Sealed for DropAfter {}
    impl SimulatedJogBus for DropAfter {}
    impl DropAfter {
        fn count(&mut self) {
            if self.remaining == 0 {
                self.inner.drop_link();
            } else {
                self.remaining -= 1;
            }
        }
    }
    impl Bus for DropAfter {
        fn query(&mut self, command: &str) -> Result<String, String> {
            self.count();
            self.inner.query(command)
        }
        fn write(&mut self, command: &str) -> Result<(), String> {
            self.count();
            self.inner.write(command)
        }
        fn emergency_write(&mut self, command: &str) -> Result<(), String> {
            self.count();
            self.inner.emergency_write(command)
        }
        fn stop_query(&mut self, command: &str) -> Result<String, String> {
            self.count();
            self.inner.stop_query(command)
        }
    }

    #[test]
    fn failed_start_sends_one_stop_and_latches() {
        let (mut jog, bus) = shot();
        // The link drops after J: — G: may or may not have arrived.
        let mut bus_drop = DropAfter {
            inner: bus,
            remaining: 3,
        };
        assert!(jog.start(&mut bus_drop, OWNER, true, true).is_err());
        assert!(matches!(jog.phase(), JogPhase::Unconfirmed { .. }));
        bus_drop.inner.restore_link();
        bus_drop.remaining = usize::MAX;
        assert!(jog.start(&mut bus_drop, OWNER, true, true).is_err());
        assert_eq!(bus_drop.inner.log, ["!:", "Q:", "J:1+"]);
    }

    #[test]
    fn link_lost_after_tk1_latches_without_tk0() {
        let (mut jog, bus) = conex();
        // TS, TP and TK1 arrive; the tracking check is lost.
        let mut bus_drop = DropAfter {
            inner: bus,
            remaining: 3,
        };
        assert!(jog.start(&mut bus_drop, OWNER, true, true).is_err());
        assert!(matches!(jog.phase(), JogPhase::Unconfirmed { .. }));
        bus_drop.inner.restore_link();
        bus_drop.remaining = usize::MAX;
        assert!(jog.start(&mut bus_drop, OWNER, true, true).is_err());
        assert_eq!(bus_drop.inner.log, ["1TS", "1TP", "1TK1"]);
        // In tracking with no setpoint: stationary, but not re-armed.
        assert!(bus_drop.inner.tracking() && !bus_drop.inner.physically_moving());
    }
}
