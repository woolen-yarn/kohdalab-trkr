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
    /// Update only the selected view axes owned by an alignment device.  The
    /// view may also contain another device whose serial connection is not
    /// involved in this command, so rebuilding or polling it here is unsafe.
    fn project_alignment_readback(&mut self, kind: MotionKind, id: &str, position: &Position) {
        let Some(config) = &self.config else {
            return;
        };
        match kind {
            MotionKind::Delay if config.device_ids.delay == id => {
                self.state.position.physical[0] = position.physical[0];
                self.state.position.delay_stage_mm = position.delay_stage_mm;
                self.state.position.delay_stage_pulse = position.delay_stage_pulse;
            }
            MotionKind::Scanner if config.device_ids.scanner == id => {
                if position.physical[1].is_some() {
                    self.state.position.physical[1] = position.physical[1];
                    self.state.position.x_scanner_mm = position.x_scanner_mm;
                    self.state.position.x_scanner_deg = position.x_scanner_deg;
                }
                if position.physical[2].is_some() {
                    self.state.position.physical[2] = position.physical[2];
                    self.state.position.y_scanner_mm = position.y_scanner_mm;
                    self.state.position.y_scanner_deg = position.y_scanner_deg;
                }
            }
            _ => {}
        }
    }

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
        let result: Result<(), String> = (|| {
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
                    self.project_alignment_readback(kind, &id, &position);
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
        // Preserve the last confirmed readback when this best-effort final
        // poll fails. Move and STOP outcomes are already established above.
        let final_readback = entry.rig.position();
        if let Ok(position) = &final_readback {
            entry.state.position = position.clone();
            self.project_alignment_readback(kind, &id, position);
        }
        // A cancellation is only clean after STOP completed.  Do not present
        // a failed STOP as a harmless ready-state cancellation.
        entry.state.phase = if matches!(&result, Err(error) if error == "Cancelled") {
            "Ready"
        } else if result.is_err() {
            "Error"
        } else {
            "Ready"
        }
        .into();
        self.connections.motions.insert(key, entry);
        self.refresh_device_states();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use trkr_motion::{ScannerConfig, protocol::Bus};

    struct AlignmentBus {
        log: Arc<Mutex<Vec<String>>>,
        position_reads: usize,
        fail_position_read: usize,
        fail_stop: bool,
        position: f64,
    }

    impl Bus for AlignmentBus {
        fn query(&mut self, command: &str) -> Result<String, String> {
            self.log.lock().unwrap().push(command.into());
            match command {
                "1TPU" => {
                    self.position_reads += 1;
                    if self.position_reads == self.fail_position_read {
                        Err("injected final readback failure".into())
                    } else {
                        Ok(format!("1TPU{:.4}", self.position))
                    }
                }
                "1TS" => Ok("1TS000032".into()),
                other => Err(format!("unexpected query {other}")),
            }
        }

        fn write(&mut self, command: &str) -> Result<(), String> {
            self.log.lock().unwrap().push(command.into());
            if let Some(target) = command.strip_prefix("1PAU") {
                self.position = target
                    .parse()
                    .map_err(|_| format!("unexpected target {target}"))?;
            }
            Ok(())
        }

        fn emergency_write(&mut self, command: &str) -> Result<(), String> {
            self.log.lock().unwrap().push(command.into());
            if self.fail_stop {
                Err("injected STOP failure".into())
            } else {
                Ok(())
            }
        }
    }

    fn alignment_motion() -> MotionConfig {
        MotionConfig {
            x: Some(ScannerConfig {
                port: "alignment-fake".into(),
                controller: "CONEXAGAP".into(),
                actuator: Some("AG-M100D".into()),
                axis: "U".into(),
                min_pos: Some(-10.),
                max_pos: Some(10.),
                resolution: Some(0.0001),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn alignment_session(rig: MotionRig, motion: MotionConfig) -> Session {
        let mut session = Session {
            config: Some(HardwareConfig {
                device_ids: crate::inventory::DeviceIds {
                    scanner: "alignment-scanner".into(),
                    ..Default::default()
                },
                model: Model::Sr830,
                address: 12,
                motion: motion.clone(),
                power_meter: None,
            }),
            state: Snapshot {
                position: Position {
                    physical: [Some(10.), Some(20.), Some(30.)],
                    y_scanner_mm: Some(3.),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        };
        session.connections.motions.insert(
            (MotionKind::Scanner, "alignment-scanner".into()),
            super::super::connections::ConnectedMotion {
                config: motion,
                rig,
                state: DeviceState {
                    id: "alignment-scanner".into(),
                    kind: "scanner".into(),
                    connected: [false, true, false],
                    position: Position::default(),
                    delay_calibration: None,
                    phase: "Ready".into(),
                },
            },
        );
        session
    }

    #[test]
    fn successful_step_survives_final_readback_failure_and_keeps_other_axes() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let motion = alignment_motion();
        let rig = MotionRig::open_with_bus_factory(&motion, {
            let log = log.clone();
            move |_, _, _, _, _, _| {
                Ok(Box::new(AlignmentBus {
                    log: log.clone(),
                    position_reads: 0,
                    fail_position_read: 5,
                    fail_stop: false,
                    position: 1.,
                }))
            }
        })
        .unwrap();
        let mut session = alignment_session(rig, motion);
        let cancel = AtomicBool::new(false);
        let (send, _) = std::sync::mpsc::sync_channel(4);

        assert_eq!(
            session.alignment(
                MotionKind::Scanner,
                "alignment-scanner".into(),
                AlignmentAction::Step {
                    axis: 1,
                    amount: 1.,
                    unit: AlignmentUnit::Native,
                },
                &cancel,
                &send,
            ),
            Ok(())
        );
        assert!(
            session
                .connections
                .motions
                .contains_key(&(MotionKind::Scanner, "alignment-scanner".into()))
        );
        assert_eq!(
            session.state.position.physical,
            [Some(10.), Some(2.), Some(30.)]
        );
        assert_eq!(session.state.position.y_scanner_mm, Some(3.));
        assert_eq!(
            *log.lock().unwrap(),
            vec![
                "1TPU",
                "1TPU",
                "1TS",
                "1PAU2.0000",
                "1TS",
                "1TPU",
                "1TPU",
                "1TPU"
            ]
        );
    }

    #[test]
    fn failed_stop_is_error_even_when_final_readback_also_fails() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let motion = alignment_motion();
        let rig = MotionRig::open_with_bus_factory(&motion, {
            let log = log.clone();
            move |_, _, _, _, _, _| {
                Ok(Box::new(AlignmentBus {
                    log: log.clone(),
                    position_reads: 0,
                    fail_position_read: 3,
                    fail_stop: true,
                    position: 1.,
                }))
            }
        })
        .unwrap();
        let mut session = alignment_session(rig, motion);
        let cancel = AtomicBool::new(true);
        let (send, _) = std::sync::mpsc::sync_channel(4);

        let error = session
            .alignment(
                MotionKind::Scanner,
                "alignment-scanner".into(),
                AlignmentAction::Step {
                    axis: 1,
                    amount: 1.,
                    unit: AlignmentUnit::Native,
                },
                &cancel,
                &send,
            )
            .unwrap_err();
        assert!(error.contains("scanner 1 STOP: injected STOP failure"));
        let entry = session
            .connections
            .motions
            .get(&(MotionKind::Scanner, "alignment-scanner".into()))
            .unwrap();
        assert_eq!(entry.state.phase, "Error");
        assert_eq!(
            session.state.position.physical,
            [Some(10.), Some(20.), Some(30.)]
        );
        assert_eq!(
            *log.lock().unwrap(),
            vec!["1TPU", "1TPU", "1STU", "1TS", "1TPU"]
        );
    }
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
    #[test]
    fn final_alignment_readback_preserves_unrelated_selected_axes_without_polling_them() {
        let mut session = Session {
            config: Some(HardwareConfig {
                device_ids: crate::inventory::DeviceIds {
                    delay: "alignment-delay".into(),
                    scanner: "measurement-scanner".into(),
                    ..Default::default()
                },
                model: Model::Sr830,
                address: 12,
                motion: MotionConfig::default(),
                power_meter: None,
            }),
            state: Snapshot {
                position: Position {
                    physical: [Some(10.), Some(20.), Some(30.)],
                    x_scanner_mm: Some(2.),
                    y_scanner_mm: Some(3.),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        };
        // A final delay readback updates its own axes only.  The configured
        // scanner is intentionally absent, modelling a failed unrelated rig.
        session.project_alignment_readback(
            MotionKind::Delay,
            "alignment-delay",
            &Position {
                physical: [Some(40.), None, None],
                delay_stage_mm: Some(4.),
                delay_stage_pulse: Some(400),
                ..Default::default()
            },
        );
        assert_eq!(
            session.state.position.physical,
            [Some(40.), Some(20.), Some(30.)]
        );
        assert_eq!(session.state.position.delay_stage_mm, Some(4.));
        assert_eq!(session.state.position.delay_stage_pulse, Some(400));
        assert_eq!(session.state.position.x_scanner_mm, Some(2.));
        assert_eq!(session.state.position.y_scanner_mm, Some(3.));
        let scanner_readback = Position {
            physical: [None, Some(50.), Some(60.)],
            x_scanner_deg: Some(0.5),
            y_scanner_deg: Some(0.6),
            ..Default::default()
        };
        session.project_alignment_readback(MotionKind::Scanner, "not-selected", &scanner_readback);
        assert_eq!(
            session.state.position.physical,
            [Some(40.), Some(20.), Some(30.)]
        );
        session.project_alignment_readback(
            MotionKind::Scanner,
            "measurement-scanner",
            &scanner_readback,
        );
        assert_eq!(
            session.state.position.physical,
            [Some(40.), Some(50.), Some(60.)]
        );
        assert_eq!(session.state.position.delay_stage_pulse, Some(400));
        assert_eq!(session.state.position.x_scanner_deg, Some(0.5));
        assert_eq!(session.state.position.y_scanner_deg, Some(0.6));
    }
}
