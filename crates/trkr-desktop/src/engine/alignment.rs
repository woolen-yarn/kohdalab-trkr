//! Manual motion uses the acquisition worker and its cancellation path.
use super::*;
use std::sync::Mutex;

pub const HOLD_LEASE: Duration = Duration::from_millis(300);
#[derive(Clone, Default)]
pub struct HoldLease(Arc<Mutex<Option<Instant>>>);
impl HoldLease {
    pub fn renew(&self) {
        if let Ok(mut last) = self.0.lock() {
            *last = Some(Instant::now());
        }
    }
    pub fn release(&self) {
        if let Ok(mut last) = self.0.lock() {
            *last = None;
        }
    }
    fn alive(&self) -> bool {
        self.0
            .lock()
            .ok()
            .and_then(|v| *v)
            .is_some_and(|v| v.elapsed() < HOLD_LEASE)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AlignmentUnit {
    #[default]
    Physical,
    Corrected,
    Native,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum AlignmentAction {
    Step {
        axis: usize,
        amount: f64,
        unit: AlignmentUnit,
    },
    Jog {
        axis: usize,
        amount: f64,
        unit: AlignmentUnit,
    },
    End {
        axis: usize,
        positive: bool,
    },
    Initialize {
        axis: usize,
        home: bool,
        move_to_origin: bool,
    },
}
impl AlignmentAction {
    fn axis(&self) -> usize {
        match *self {
            Self::Step { axis, .. }
            | Self::Jog { axis, .. }
            | Self::End { axis, .. }
            | Self::Initialize { axis, .. } => axis,
        }
    }
}

fn jog_increment(requested: f64, span: f64, resolution: f64) -> Result<f64, String> {
    if !requested.is_finite() || requested.abs() < resolution {
        return Err("Hold increment is below the device resolution; increase Step".into());
    }
    Ok(requested.signum() * requested.abs().min((span / 1000.).max(resolution)))
}
fn native_position(position: &Position, axis: usize) -> Option<f64> {
    match axis {
        0 => position.delay_stage_mm,
        1 => position.x_scanner_mm.or(position.x_scanner_deg),
        2 => position.y_scanner_mm.or(position.y_scanner_deg),
        _ => None,
    }
}
fn bounds(config: &MotionConfig, axis: usize, micro: u32) -> Result<(f64, f64), String> {
    match axis {
        0 => config
            .delay
            .as_ref()
            .ok_or("No delay stage")?
            .native_limits_mm(micro),
        1 => config.x.as_ref().ok_or("No X axis")?.native_limits(),
        2 => config.y.as_ref().ok_or("No Y axis")?.native_limits(),
        _ => Err("Unknown motion axis".into()),
    }
}
fn physical_target(rig: &MotionRig, axis: usize, raw: f64) -> Result<[Option<f64>; 3], String> {
    let mut target = [None; 3];
    target[axis] = Some(raw);
    rig.to_physical(target, "interface", [0.; 3])
}

impl Session {
    pub(super) fn alignment(
        &mut self,
        kind: MotionKind,
        id: String,
        action: AlignmentAction,
        cancel: &AtomicBool,
        send: &SyncSender<Event>,
    ) -> Result<(), String> {
        let axis = action.axis();
        if !matches!(
            (kind, axis),
            (MotionKind::Delay, 0) | (MotionKind::Scanner, 1..=2)
        ) {
            return Err("Axis does not belong to this device".into());
        }
        let key = (kind, id.clone());
        let mut entry = self
            .connections
            .motions
            .remove(&key)
            .ok_or("Connect the alignment device first")?;
        let result = (|| {
            if !entry.state.connected[axis] {
                return Err("Connect this axis first".into());
            }
            let micro = entry
                .rig
                .delay_calibration()?
                .and_then(|c| c.microstep_division)
                .unwrap_or(1);
            let effective = entry.rig.recording_configuration();
            let (minimum, maximum) = bounds(&effective, axis, micro)?;
            let native_resolution = if axis == 0 {
                entry
                    .rig
                    .delay_calibration()?
                    .and_then(|c| c.effective_pos_um_per_pulse)
                    .map(|um| um / 1000.)
                    .ok_or("No stage resolution")?
            } else {
                let scanner = if axis == 1 {
                    &effective.x
                } else {
                    &effective.y
                };
                scanner
                    .as_ref()
                    .and_then(|c| c.resolution.or(c.min_step))
                    .unwrap_or(0.0001)
            };
            let jog = matches!(action, AlignmentAction::Jog { .. });
            let hold = self.hold.clone();
            let source = self
                .gpib
                .as_ref()
                .and(self.config.as_ref())
                .map(|c| Source::Gpib {
                    model: c.model,
                    address: c.address,
                });
            let mut position = entry.rig.position()?;
            loop {
                if cancel.load(Ordering::Acquire) || (jog && !hold.alive()) {
                    cancel.store(true, Ordering::Release);
                    entry.rig.stop()?;
                    return Err("Cancelled".into());
                }
                entry.state.phase = if matches!(action, AlignmentAction::Initialize { .. }) {
                    "Initializing"
                } else {
                    "Moving"
                }
                .into();
                let mut progress = |position: Position| -> Result<(), String> {
                    if jog && !hold.alive() {
                        cancel.store(true, Ordering::Release);
                    }
                    entry.state.position = position.clone();
                    self.refresh_device_states();
                    self.state.registered_devices.push(entry.state.clone());
                    if let Some(config) = &self.config {
                        if kind == MotionKind::Delay && config.device_ids.delay == id {
                            self.state.position.physical[0] = position.physical[0];
                            self.state.position.delay_stage_mm = position.delay_stage_mm;
                            self.state.position.delay_stage_pulse = position.delay_stage_pulse;
                        } else if kind == MotionKind::Scanner && config.device_ids.scanner == id {
                            self.state.position.physical[1..]
                                .copy_from_slice(&position.physical[1..]);
                            self.state.position.x_scanner_mm = position.x_scanner_mm;
                            self.state.position.x_scanner_deg = position.x_scanner_deg;
                            self.state.position.y_scanner_mm = position.y_scanner_mm;
                            self.state.position.y_scanner_deg = position.y_scanner_deg;
                        }
                    }
                    let _ = send.try_send(Event::State(Box::new(self.state.clone())));
                    if let Some(source) = source {
                        poll_live(
                            &mut self.live,
                            source,
                            self.gpib.as_mut(),
                            &mut self.state,
                            send,
                            Some(cancel),
                        )?;
                    }
                    Ok(())
                };
                progress(position.clone())?;
                if let AlignmentAction::Initialize {
                    home,
                    move_to_origin,
                    ..
                } = action
                {
                    position = entry.rig.initialize_with_origin_progress(
                        axis,
                        home,
                        move_to_origin,
                        cancel,
                        Some(&mut progress),
                    )?;
                } else {
                    let target = match action {
                        AlignmentAction::End { positive, .. } => physical_target(
                            &entry.rig,
                            axis,
                            if positive { maximum } else { minimum },
                        )?,
                        AlignmentAction::Step { amount, unit, .. }
                        | AlignmentAction::Jog { amount, unit, .. } => {
                            if !amount.is_finite() || amount == 0. {
                                return Err("Motion increment must be finite and nonzero".into());
                            }
                            let raw =
                                native_position(&position, axis).ok_or("No current position")?;
                            let requested = if unit == AlignmentUnit::Native {
                                amount
                            } else {
                                // Derive a relative scale without assuming a particular direction or optical path.
                                let p0 = physical_target(&entry.rig, axis, raw)?[axis]
                                    .ok_or("No coordinate scale")?;
                                let p1 = physical_target(&entry.rig, axis, raw + 1.)?[axis]
                                    .ok_or("No coordinate scale")?;
                                amount / (p1 - p0)
                            };
                            let delta = if jog {
                                // Bound each outstanding move. A missing GUI lease cannot enqueue further travel.
                                jog_increment(requested, maximum - minimum, native_resolution)?
                            } else {
                                requested
                            };
                            let raw_target = if jog {
                                (raw + delta).clamp(minimum, maximum)
                            } else {
                                raw + delta
                            };
                            if jog && (raw_target - raw).abs() < f64::EPSILON {
                                return Ok(());
                            }
                            physical_target(&entry.rig, axis, raw_target)?
                        }
                        AlignmentAction::Initialize { .. } => unreachable!(),
                    };
                    let mut approach = [false; 3];
                    approach[axis] = !jog && axis != 0;
                    position = entry.rig.move_to_with_progress(
                        target,
                        approach,
                        cancel,
                        Some(&mut progress),
                    )?;
                }
                entry.state.position = position.clone();
                if !jog {
                    return Ok(());
                }
            }
        })();
        // Publish the final readback when possible, retaining the last known readback on failure.
        if let Ok(position) = entry.rig.position() {
            entry.state.position = position;
        }
        entry.state.phase = if result.is_err() && !cancel.load(Ordering::Acquire) {
            "Error"
        } else {
            "Ready"
        }
        .into();
        self.connections.motions.insert(key, entry);
        self.refresh_device_states();
        self.read_position()?;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn jog_segments_are_bounded_and_respect_resolution() {
        assert_eq!(jog_increment(100., 12., 0.0002).unwrap(), 0.012);
        assert_eq!(jog_increment(-100., 12., 0.0002).unwrap(), -0.012);
        assert!(jog_increment(0.00001, 12., 0.0002).is_err());
        assert!(jog_increment(f64::NAN, 12., 0.0002).is_err());
    }
    #[test]
    fn hold_requires_a_recent_live_pointer_lease() {
        let lease = HoldLease::default();
        assert!(!lease.alive());
        lease.renew();
        assert!(lease.alive());
        *lease.0.lock().unwrap() = Some(Instant::now() - HOLD_LEASE - Duration::from_millis(1));
        assert!(!lease.alive());
        lease.renew();
        lease.release();
        assert!(!lease.alive());
    }
}
