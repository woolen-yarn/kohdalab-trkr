use super::*;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

struct ScriptedBus {
    log: Arc<Mutex<Vec<String>>>,
    replies: Arc<Mutex<VecDeque<Result<String, String>>>>,
    fail_write: Option<String>,
}

impl ScriptedBus {
    fn new(log: Arc<Mutex<Vec<String>>>, replies: &[&str]) -> Self {
        Self {
            log,
            replies: Arc::new(Mutex::new(
                replies.iter().map(|reply| Ok((*reply).into())).collect(),
            )),
            fail_write: None,
        }
    }

    fn failing_on(mut self, command: &str) -> Self {
        self.fail_write = Some(command.into());
        self
    }
}

impl Bus for ScriptedBus {
    fn query(&mut self, command: &str) -> Result<String, String> {
        self.log.lock().unwrap().push(command.into());
        self.replies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err(format!("unexpected query {command}")))
    }

    fn write(&mut self, command: &str) -> Result<(), String> {
        self.log.lock().unwrap().push(command.into());
        if self.fail_write.as_deref() == Some(command) {
            Err(format!("scripted write failure for {command}"))
        } else {
            Ok(())
        }
    }
}

fn agap(axis: &str) -> ScannerConfig {
    ScannerConfig {
        port: "fake".into(),
        controller: "CONEXAGAP".into(),
        actuator: Some("AG-M100D".into()),
        axis: axis.into(),
        sample_um_per_unit: 1.0,
        ..Default::default()
    }
    .normalized()
    .unwrap()
}

fn cc_with_hysteresis() -> ScannerConfig {
    ScannerConfig {
        port: "fake".into(),
        controller: "CONEXCC".into(),
        actuator: Some("TRA12CC".into()),
        sample_um_per_unit: 1.0,
        software_hysteresis: Hysteresis {
            enabled: true,
            distance_um: Some(1.0),
            direction: Some("negative".into()),
        },
        ..Default::default()
    }
    .normalized()
    .unwrap()
}

fn two_scanner_rig(bus: impl Bus + Send + 'static) -> MotionRig {
    MotionRig {
        ports: vec![Box::new(bus)],
        delay: None,
        scanners: [
            Some(Scanner {
                config: agap("U"),
                port: 0,
                prepared: false,
            }),
            Some(Scanner {
                config: agap("V"),
                port: 0,
                prepared: false,
            }),
        ],
    }
}

#[test]
fn move_starts_both_scanners_before_polling() {
    let log = Arc::new(Mutex::new(vec![]));
    let bus = ScriptedBus::new(
        log.clone(),
        &[
            "1TS000032",
            "1TS000032", // prepare X/Y
            "1TS000032",
            "1TS000032", // wait X/Y
            "1TPU0.000",
            "1TPV0.000", // readback X/Y
        ],
    );
    let mut rig = two_scanner_rig(bus);

    rig.move_to(
        [None, Some(0.0), Some(0.0)],
        [false; 3],
        &AtomicBool::new(false),
    )
    .unwrap();

    assert_eq!(
        *log.lock().unwrap(),
        vec![
            "1TS",
            "1TS",
            "1PAU0.000",
            "1PAV0.000",
            "1TS",
            "1TS",
            "1TPU",
            "1TPV",
        ]
    );
}

#[test]
fn manual_move_reports_actual_position_before_completion() {
    struct MovingBus {
        log: Arc<Mutex<Vec<String>>>,
        status_reads: usize,
        ready_delay: Duration,
    }
    impl Bus for MovingBus {
        fn query(&mut self, command: &str) -> Result<String, String> {
            self.log.lock().unwrap().push(command.into());
            match command {
                "1TS" => {
                    self.status_reads += 1;
                    if self.status_reads == 5 {
                        thread::sleep(self.ready_delay);
                    }
                    Ok(if matches!(self.status_reads, 3 | 4) {
                        "1TS000028"
                    } else {
                        "1TS000032"
                    }
                    .into())
                }
                "1TPU" => Ok(if self.status_reads <= 4 {
                    "1TPU0.001"
                } else {
                    "1TPU0.010"
                }
                .into()),
                "1TPV" => Ok(if self.status_reads <= 4 {
                    "1TPV0.002"
                } else {
                    "1TPV0.020"
                }
                .into()),
                _ => Err(format!("unexpected query {command}")),
            }
        }
        fn write(&mut self, command: &str) -> Result<(), String> {
            self.log.lock().unwrap().push(command.into());
            Ok(())
        }
    }
    // Command-based replies permit the additional readback that is correct
    // when a scheduler delay crosses the live-position update deadline.
    for ready_delay in [Duration::ZERO, Duration::from_millis(110)] {
        let log = Arc::new(Mutex::new(vec![]));
        let mut rig = two_scanner_rig(MovingBus {
            log: log.clone(),
            status_reads: 0,
            ready_delay,
        });
        let mut positions = vec![];
        let final_position = rig
            .move_to_with_progress(
                [None, Some(0.010), Some(0.020)],
                [false; 3],
                &AtomicBool::new(false),
                Some(&mut |position| {
                    positions.push(position);
                    Ok(())
                }),
            )
            .unwrap();
        assert!(!positions.is_empty());
        assert_eq!(positions[0].physical, [None, Some(0.001), Some(0.002)]);
        let expected_final = [None, Some(0.010), Some(0.020)];
        for position in positions.iter().skip(1) {
            assert_eq!(position.physical, expected_final);
        }
        if !ready_delay.is_zero() {
            assert!(
                positions.len() >= 2,
                "delayed ready polling must publish final readback"
            );
        }
        assert_eq!(final_position.physical, expected_final);
        assert!(!log.lock().unwrap().iter().any(|cmd| cmd.starts_with("1ST")));
    }
}

#[test]
fn failed_live_callback_stops_motion_before_another_poll() {
    let log = Arc::new(Mutex::new(vec![]));
    let mut rig = two_scanner_rig(ScriptedBus::new(
        log.clone(),
        &["1TS000032", "1TS000028", "1TPU0.001", "1TPV0.002"],
    ));
    let error = rig
        .move_to_with_progress(
            [None, Some(0.010), None],
            [false; 3],
            &AtomicBool::new(false),
            Some(&mut |_| Err("live lock-in read failed".into())),
        )
        .unwrap_err();
    assert_eq!(error, "live lock-in read failed");
    assert!(
        log.lock()
            .unwrap()
            .ends_with(&["1STU".into(), "1STV".into()])
    );
}

#[test]
fn cancellation_from_progress_stops_every_axis() {
    let log = Arc::new(Mutex::new(vec![]));
    let mut rig = two_scanner_rig(ScriptedBus::new(
        log.clone(),
        &["1TS000032", "1TS000028", "1TPU0.001", "1TPV0.002"],
    ));
    let cancel = AtomicBool::new(false);
    let result = rig.move_to_with_progress(
        [None, Some(0.010), None],
        [false; 3],
        &cancel,
        Some(&mut |_| {
            cancel.store(true, Ordering::Release);
            Ok(())
        }),
    );
    assert!(result.unwrap_err().contains("cancelled"));
    assert!(
        log.lock()
            .unwrap()
            .ends_with(&["1STU".into(), "1STV".into()])
    );
}

#[test]
fn initialize_delay_without_home_refreshes_calibration_without_motion() {
    let log = Arc::new(Mutex::new(vec![]));
    let mut rig = MotionRig {
        ports: vec![Box::new(ScriptedBus::new(log.clone(), &["4", "100"]))],
        delay: Some(Delay {
            config: DelayConfig {
                port: "fake".into(),
                stage: Some("SGSP46-500".into()),
                ..Default::default()
            }
            .normalized()
            .unwrap(),
            port: 0,
            micro: 1,
        }),
        scanners: [None, None],
    };
    let position = rig.initialize(0, false, &AtomicBool::new(false)).unwrap();
    assert_eq!(position.delay_stage_pulse, Some(100));
    assert_eq!(
        rig.delay_calibration().unwrap().unwrap().microstep_division,
        Some(4)
    );
    assert_eq!(*log.lock().unwrap(), vec!["?:S1", "Q:"]);
}

#[test]
fn initialize_agap_enables_closed_loop_without_home_command() {
    let log = Arc::new(Mutex::new(vec![]));
    let mut rig = two_scanner_rig(ScriptedBus::new(
        log.clone(),
        &["1TS00003C", "1TS000032", "1TPU0.0", "1TPV0.0"],
    ));
    rig.initialize(1, false, &AtomicBool::new(false)).unwrap();
    assert_eq!(
        *log.lock().unwrap(),
        vec!["1TS", "1MM1", "1TS", "1TPU", "1TPV"]
    );
}

#[test]
fn initialize_cc_homes_and_reports_intermediate_position() {
    // Progress polling is time-driven: a slow runner may read position again
    // on the ready iteration. Model device state rather than a fixed reply queue.
    struct HomingBus {
        log: Arc<Mutex<Vec<String>>>,
        status_polls: usize,
        delayed_ready: bool,
    }
    impl Bus for HomingBus {
        fn query(&mut self, command: &str) -> Result<String, String> {
            self.log.lock().unwrap().push(command.into());
            match command {
                "1TS" => {
                    self.status_polls += 1;
                    if self.status_polls == 1 {
                        Ok("1TS00001E".into())
                    } else {
                        if self.delayed_ready {
                            std::thread::sleep(Duration::from_millis(110));
                        }
                        Ok("1TS000032".into())
                    }
                }
                "1TP" if self.status_polls == 1 => Ok("1TP1.0".into()),
                "1TP" if self.status_polls >= 2 => Ok("1TP0.0".into()),
                _ => Err(format!("unexpected query {command}")),
            }
        }
        fn write(&mut self, command: &str) -> Result<(), String> {
            self.log.lock().unwrap().push(command.into());
            if command == "1OR" {
                Ok(())
            } else {
                Err(format!("unexpected write {command}"))
            }
        }
    }
    for delayed_ready in [false, true] {
        let log = Arc::new(Mutex::new(vec![]));
        let mut rig = MotionRig {
            ports: vec![Box::new(HomingBus {
                log: log.clone(),
                status_polls: 0,
                delayed_ready,
            })],
            delay: None,
            scanners: [
                Some(Scanner {
                    config: cc_with_hysteresis(),
                    port: 0,
                    prepared: true,
                }),
                None,
            ],
        };
        let mut positions = vec![];
        let final_position = rig
            .initialize_with_progress(
                1,
                true,
                &AtomicBool::new(false),
                Some(&mut |p| {
                    positions.push(p);
                    Ok(())
                }),
            )
            .unwrap();
        let commands = log.lock().unwrap();
        assert_eq!(commands[0], "1OR");
        assert_eq!(commands.iter().filter(|c| c.as_str() == "1TS").count(), 2);
        assert!(
            commands
                .iter()
                .all(|c| matches!(c.as_str(), "1OR" | "1TS" | "1TP"))
        );
        assert_eq!(positions[0].x_scanner_mm, Some(1.0));
        assert_eq!(final_position.x_scanner_mm, Some(0.0));
        if delayed_ready {
            assert_eq!(positions.len(), 2);
            assert_eq!(positions[1].x_scanner_mm, Some(0.0));
        }
        assert!(!rig.scanners[0].as_ref().unwrap().prepared);
    }
}

fn return_rig(log: Arc<Mutex<Vec<String>>>, replies: &[&str]) -> MotionRig {
    let mut shared_compensation = cc_with_hysteresis();
    shared_compensation.software_hysteresis.distance_um = Some(100.);
    let config = ScannerConfig {
        sample_um_per_unit: 100.,
        ensure_closed_loop_on_move: Some(false),
        ..shared_compensation
    }
    .normalized()
    .unwrap();
    MotionRig {
        ports: vec![Box::new(ScriptedBus::new(log, replies))],
        delay: None,
        scanners: [
            Some(Scanner {
                config,
                port: 0,
                prepared: false,
            }),
            None,
        ],
    }
}
#[test]
fn full_initialization_homes_then_moves_to_calibrated_instrument_zero() {
    let log = Arc::new(Mutex::new(vec![]));
    let mut rig = return_rig(
        log.clone(),
        &[
            "1TS000032",
            "1TP0.0",
            "1TP0.0", // home reference is raw0, not measurement0
            "1TS000032",
            "1TS000032",
            "1TP5.0", // common 100 µm approach before instrument zero
            "1TS000032",
            "1TP6.0",
            "1TP6.0",
        ],
    );
    let position = rig
        .initialize_with_origin_progress(1, true, true, &AtomicBool::new(false), None)
        .unwrap();
    assert_eq!(position.physical[1], Some(0.));
    let commands = log.lock().unwrap();
    assert_eq!(commands[0], "1OR");
    let positioning: Vec<_> = commands
        .iter()
        .filter(|c| c.starts_with("1PA"))
        .map(String::as_str)
        .collect();
    assert_eq!(positioning, ["1PA5.0000", "1PA6.0000"]);
    assert!(commands.iter().any(|c| c == "1PA6.0000"));
    assert!(!commands.iter().any(|c| c == "1PA0.0000"));
}
#[test]
fn initialization_checks_zero_approach_limits_before_homing() {
    let log = Arc::new(Mutex::new(vec![]));
    let mut rig = return_rig(log.clone(), &[]);
    rig.scanners[0]
        .as_mut()
        .unwrap()
        .config
        .software_hysteresis
        .distance_um = Some(2000.);
    assert!(
        rig.initialize_with_origin_progress(1, true, true, &AtomicBool::new(false), None)
            .is_err()
    );
    assert!(log.lock().unwrap().is_empty());
}
#[test]
fn stop_on_the_ready_callback_cancels_low_level_initialization_too() {
    let log = Arc::new(Mutex::new(vec![]));
    let mut rig = return_rig(log.clone(), &["1TS000032", "1TP0.0"]);
    let cancel = AtomicBool::new(false);
    let result = rig.initialize_with_progress(
        1,
        true,
        &cancel,
        Some(&mut |_| {
            cancel.store(true, Ordering::Release);
            Ok(())
        }),
    );
    assert!(result.unwrap_err().contains("cancelled"));
    assert_eq!(log.lock().unwrap().last().unwrap(), "1ST");
}
#[test]
fn cancelled_home_does_not_start_the_following_origin_move() {
    let log = Arc::new(Mutex::new(vec![]));
    let mut rig = return_rig(log.clone(), &["1TS000032", "1TP0.0", "1TP0.0"]);
    let cancel = AtomicBool::new(false);
    let result = rig.initialize_with_origin_progress(
        1,
        true,
        true,
        &cancel,
        Some(&mut |_| {
            cancel.store(true, Ordering::Release);
            Ok(())
        }),
    );
    assert!(result.unwrap_err().contains("cancelled"));
    let commands = log.lock().unwrap();
    assert!(commands.iter().all(|c| !c.starts_with("1PA")));
    assert_eq!(commands.last().unwrap(), "1ST");
}
#[test]
fn spatial_return_goes_100_um_below_target_before_reaching_zero() {
    let log = Arc::new(Mutex::new(vec![]));
    let mut rig = return_rig(
        log.clone(),
        &["1TS000032", "1TS000032", "1TS000032", "1TP6.0"],
    );
    rig.validate_return([None, Some(0.), None]).unwrap();
    let position = rig
        .return_to_with_progress([None, Some(0.), None], &AtomicBool::new(false), None)
        .unwrap();
    assert_eq!(position.physical[1], Some(0.));
    assert_eq!(
        *log.lock().unwrap(),
        vec!["1TS", "1PA5.0000", "1TS", "1PA6.0000", "1TS", "1TP"]
    );
    // Scan starts and completion returns use the same compensation distance.
    assert_eq!(
        rig.scanners[0]
            .as_ref()
            .unwrap()
            .config
            .approach(0.)
            .unwrap(),
        Some(-100.)
    );
}

#[test]
fn validation_and_execution_share_the_same_compensation_plan() {
    let log = Arc::new(Mutex::new(vec![]));
    let rig = return_rig(log.clone(), &[]);
    let target = [None, Some(20.), None];
    let plan = rig.plan_move(target, [false, true, false]).unwrap();

    assert_eq!(plan.target, target);
    assert_eq!(plan.approach, [None, Some(-80.), None]);
    rig.validate_moves(&[(target, [false, true, false])])
        .unwrap();
    assert!(log.lock().unwrap().is_empty());
}
#[test]
fn return_rejects_out_of_range_approach_before_any_motion() {
    let log = Arc::new(Mutex::new(vec![]));
    let rig = return_rig(log.clone(), &[]);
    assert!(rig.validate_return([None, Some(-550.), None]).is_err());
    assert!(log.lock().unwrap().is_empty());
}
#[test]
fn stop_during_return_approach_does_not_continue_to_zero() {
    let log = Arc::new(Mutex::new(vec![]));
    let mut rig = return_rig(log.clone(), &["1TS000032", "1TS000032", "1TP5.0"]);
    let cancel = AtomicBool::new(false);
    let result = rig.return_to_with_progress(
        [None, Some(0.), None],
        &cancel,
        Some(&mut |_| {
            cancel.store(true, Ordering::Release);
            Ok(())
        }),
    );
    assert!(result.unwrap_err().contains("cancelled"));
    let commands = log.lock().unwrap();
    assert!(commands.iter().any(|c| c == "1PA5.0000"));
    assert!(!commands.iter().any(|c| c == "1PA6.0000"));
    assert_eq!(commands.last().unwrap(), "1ST");
}
#[test]
fn successive_scan_lines_repeat_compensation_without_an_origin_move() {
    let log = Arc::new(Mutex::new(vec![]));
    let mut rig = return_rig(
        log.clone(),
        &[
            "1TS000032",
            "1TS000032",
            "1TS000032",
            "1TP6.2",
            "1TS000032",
            "1TS000032",
            "1TP6.4",
            "1TS000032",
            "1TS000032",
            "1TS000032",
            "1TP6.2",
        ],
    );
    let cancel = AtomicBool::new(false);
    rig.move_to([None, Some(20.), None], [false, true, false], &cancel)
        .unwrap();
    rig.move_to([None, Some(40.), None], [false; 3], &cancel)
        .unwrap();
    rig.move_to([None, Some(20.), None], [false, true, false], &cancel)
        .unwrap();
    let commands = log.lock().unwrap();
    let positions: Vec<_> = commands
        .iter()
        .filter(|command| command.starts_with("1PA"))
        .map(String::as_str)
        .collect();
    assert_eq!(
        positions,
        [
            "1PA5.2000",
            "1PA6.2000",
            "1PA6.4000",
            "1PA5.2000",
            "1PA6.2000"
        ]
    );
}
#[test]
fn shared_distance_is_configurable_validated_and_has_no_effect_on_agap() {
    let log = Arc::new(Mutex::new(vec![]));
    let mut rig = return_rig(log, &[]);
    let config = &mut rig.scanners[0].as_mut().unwrap().config;
    config.software_hysteresis.distance_um = None;
    assert_eq!(config.approach(0.).unwrap(), Some(-100.));
    config.software_hysteresis.distance_um = Some(25.);
    assert_eq!(config.approach(0.).unwrap(), Some(-25.));
    config.software_hysteresis.direction = Some("positive".into());
    assert_eq!(config.approach(0.).unwrap(), Some(25.));
    config.software_hysteresis.distance_um = Some(-1.);
    assert!(config.clone().normalized().is_err());
    let mut config = agap("U");
    config.software_hysteresis.enabled = true;
    assert_eq!(config.approach(0.).unwrap(), None);
}
#[test]
fn interface_scan_return_still_uses_saved_measurement_origin() {
    let log = Arc::new(Mutex::new(vec![]));
    let rig = return_rig(log, &[]);
    let returning = rig
        .to_physical([None, Some(0.), None], "measurement", [0., 38.237, 420.487])
        .unwrap();
    assert_eq!(returning, [None, Some(38.237), None]);
    assert_ne!(
        returning,
        rig.to_physical([None, Some(0.), None], "interface", [0., 38.237, 420.487])
            .unwrap()
    );
}
#[test]
fn failed_second_scanner_start_stops_all_scanners() {
    let log = Arc::new(Mutex::new(vec![]));
    let bus = ScriptedBus::new(log.clone(), &["1TS000032", "1TS000032"]).failing_on("1PAV0.000");
    let mut rig = two_scanner_rig(bus);

    assert!(
        rig.move_to(
            [None, Some(0.0), Some(0.0)],
            [false; 3],
            &AtomicBool::new(false),
        )
        .is_err()
    );
    assert_eq!(
        *log.lock().unwrap(),
        vec!["1TS", "1TS", "1PAU0.000", "1PAV0.000", "1STU", "1STV"]
    );
}

#[test]
fn cancelled_move_sends_stop_without_starting_motion() {
    let log = Arc::new(Mutex::new(vec![]));
    let mut rig = two_scanner_rig(ScriptedBus::new(log.clone(), &[]));

    assert!(
        rig.move_to(
            [None, Some(0.0), Some(0.0)],
            [false; 3],
            &AtomicBool::new(true),
        )
        .is_err()
    );
    assert_eq!(*log.lock().unwrap(), vec!["1STU", "1STV"]);
}

#[test]
fn delay_readback_mismatch_stops_motion() {
    let log = Arc::new(Mutex::new(vec![]));
    let stage = DelayConfig {
        port: "fake".into(),
        stage: Some("SGSP46-500".into()),
        direction: 1,
        ..Default::default()
    }
    .normalized()
    .unwrap();
    let mut rig = MotionRig {
        ports: vec![Box::new(ScriptedBus::new(
            log.clone(),
            &["OK", "OK", "12500", "R", "12501", "OK"],
        ))],
        delay: Some(Delay {
            config: stage,
            port: 0,
            micro: 1,
        }),
        scanners: [None, None],
    };

    assert!(
        rig.move_to([Some(0.0), None, None], [false; 3], &AtomicBool::new(false))
            .unwrap_err()
            .contains("readback")
    );
    let commands = log.lock().unwrap();
    assert!(commands.iter().any(|command| command == "A:1+P12500"));
    assert_eq!(commands.last().unwrap(), "L:E");
}

#[test]
fn out_of_bounds_hysteresis_approach_performs_no_io() {
    let log = Arc::new(Mutex::new(vec![]));
    let mut rig = MotionRig {
        ports: vec![Box::new(ScriptedBus::new(log.clone(), &[]))],
        delay: None,
        scanners: [
            Some(Scanner {
                config: cc_with_hysteresis(),
                port: 0,
                prepared: false,
            }),
            None,
        ],
    };

    assert!(
        rig.move_to(
            [None, Some(-6.0), None],
            [false, true, false],
            &AtomicBool::new(false),
        )
        .is_err()
    );
    assert!(log.lock().unwrap().is_empty());
}

#[test]
fn runtime_coordinates_convert_measurement_interface_and_instrument_values() {
    let log = Arc::new(Mutex::new(vec![]));
    let stage = DelayConfig {
        port: "delay".into(),
        stage: Some("SGSP46-500".into()),
        direction: 1,
        ..Default::default()
    }
    .normalized()
    .unwrap();
    let x = ScannerConfig {
        port: "x".into(),
        controller: "CONEXCC".into(),
        actuator: Some("TRA12CC".into()),
        sample_um_per_unit: 1.0,
        ..Default::default()
    }
    .normalized()
    .unwrap();
    let rig = MotionRig {
        ports: vec![
            Box::new(ScriptedBus::new(log.clone(), &[])),
            Box::new(ScriptedBus::new(log, &[])),
        ],
        delay: Some(Delay {
            config: stage.clone(),
            port: 0,
            micro: 1,
        }),
        scanners: [
            Some(Scanner {
                config: x,
                port: 1,
                prepared: false,
            }),
            None,
        ],
    };

    assert_eq!(
        rig.to_physical(
            [Some(1.0), Some(2.0), None],
            "measurement",
            [10.0, 20.0, 30.0],
        )
        .unwrap(),
        [Some(11.0), Some(22.0), None]
    );
    let interface = rig
        .to_physical([Some(250.0), Some(7.0), None], "interface", [0.0; 3])
        .unwrap();
    assert!((interface[0].unwrap() - stage.ps_from_mm(250.0, 1).unwrap()).abs() < 1e-12);
    assert_eq!(interface[1], Some(1.0));
    let instrument = rig
        .to_physical([Some(12_500.0), Some(7.0), None], "instrument", [0.0; 3])
        .unwrap();
    assert!((instrument[0].unwrap() - stage.ps_from_mm(250.0, 1).unwrap()).abs() < 1e-12);
    assert_eq!(instrument[1], Some(1.0));
    assert!(
        rig.to_physical([Some(0.5), None, None], "instrument", [0.; 3])
            .is_err()
    );
}
#[test]
fn delay_waits_for_target_readback_before_ready_completion() {
    let log = Arc::new(Mutex::new(vec![]));
    let config = DelayConfig {
        port: "fake".into(),
        stage: Some("SGSP46-500".into()),
        direction: 1,
        ..Default::default()
    }
    .normalized()
    .unwrap();
    let mut rig = MotionRig {
        ports: vec![Box::new(ScriptedBus::new(
            log.clone(),
            &["OK", "OK", "12000", "12500", "R", "12500"],
        ))],
        delay: Some(Delay {
            config,
            port: 0,
            micro: 1,
        }),
        scanners: [None, None],
    };
    let position = rig
        .move_to([Some(0.), None, None], [false; 3], &AtomicBool::new(false))
        .unwrap();
    assert_eq!(position.delay_stage_pulse, Some(12500));
    assert_eq!(
        *log.lock().unwrap(),
        ["A:1+P12500", "G:", "Q:", "Q:", "!:", "Q:"]
    );
}

#[test]
fn delay_calibration_keeps_microstep_unknown_until_connected() {
    let config = DelayConfig {
        port: "COM6".into(),
        stage: Some("SGSP46-500".into()),
        pos_um_per_pulse: Some(0.5),
        ..Default::default()
    };
    let hint = config.calibration_hint().unwrap();
    assert_eq!(hint.microstep_division, None);
    assert_eq!(hint.configured_pos_um_per_pulse, Some(0.5));
    assert!(hint.effective_ps_per_pulse.unwrap() > 0.);
    assert_eq!(hint.configuration, Some(config.normalized().unwrap()));

    let rig = MotionRig {
        ports: vec![],
        delay: Some(Delay {
            config: config.normalized().unwrap(),
            port: 0,
            micro: 8,
        }),
        scanners: [None, None],
    };
    let actual = rig.delay_calibration().unwrap().unwrap();
    assert_eq!(actual.microstep_division, Some(8));
    assert_eq!(actual.configuration, Some(config.normalized().unwrap()));
    assert_eq!(actual.effective_pos_um_per_pulse, Some(0.5));
    assert_eq!(
        actual.full_step_ps.unwrap(),
        actual.effective_ps_per_pulse.unwrap() * 8.
    );
}
