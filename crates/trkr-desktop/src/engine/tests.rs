use super::*;

fn paused_controller() -> (Controller, Receiver<WorkerCommand>) {
    let (commands, requests) = mpsc::channel();
    let (_, events) = mpsc::sync_channel(8);
    (
        Controller {
            #[cfg(feature = "gui-interaction-tests")]
            demo_only: false,
            hold: alignment::HoldLease::default(),
            events,
            commands,
            cancel: Arc::new(AtomicBool::new(false)),
            busy: Arc::new(AtomicBool::new(false)),
            pending: Arc::new(AtomicUsize::new(0)),
            settings_refresh_pending: Arc::new(AtomicBool::new(false)),
            read_only: false,
            parallel: Default::default(),
            thread: None,
        },
        requests,
    )
}

#[test]
fn lockin_setting_writes_are_rejected_not_queued_while_a_job_runs() {
    let (controller, requests) = paused_controller();
    controller
        .send(Command::Run {
            source: Source::Demo,
            config: config(),
            plan: Plan::default(),
            output: None,
        })
        .unwrap();
    let _run = requests.try_recv().unwrap();
    for command in [
        Command::SetLockin {
            setting: "sensitivity".into(),
            value: serde_json::json!(1e-6),
        },
        Command::SetLockin {
            setting: "ac_gain".into(),
            value: serde_json::json!(10.0),
        },
        Command::AutoLockin {
            action: "phase".into(),
        },
    ] {
        assert_eq!(
            controller.send(command).unwrap_err(),
            OperationError::LockinBusy.to_string()
        );
    }
    assert_eq!(
        controller
            .send_remote(
                "set-1".into(),
                Command::SetLockin {
                    setting: "time_constant".into(),
                    value: serde_json::json!(1.0),
                },
            )
            .unwrap_err(),
        OperationError::LockinBusy.to_string()
    );
    assert!(
        requests.try_recv().is_err(),
        "nothing queued behind the run"
    );
    // Signal-pair selection keeps its documented serialized behavior.
    controller
        .send(Command::SetLockin {
            setting: "display_mode".into(),
            value: "xy".into(),
        })
        .unwrap();
    // Idle: settings writes are accepted and serialized on the worker.
    controller.busy.store(false, Ordering::Release);
    controller
        .send(Command::SetLockin {
            setting: "slope".into(),
            value: serde_json::json!(12),
        })
        .unwrap();
}

#[test]
fn display_refresh_is_coalesced_and_yields_to_start() {
    let (controller, requests) = paused_controller();
    controller.refresh_lockin_settings().unwrap();
    controller.refresh_lockin_settings().unwrap();
    assert!(!controller.has_pending());
    assert!(controller.settings_refresh_pending.load(Ordering::Acquire));
    controller
        .send(Command::Run {
            source: Source::Demo,
            config: config(),
            plan: Plan::default(),
            output: None,
        })
        .unwrap();
    assert!(controller.has_pending());
    assert!(controller.busy.load(Ordering::Acquire));
    controller.refresh_lockin_settings().unwrap();
    assert!(matches!(
        receive_worker_command(
            &requests,
            Duration::ZERO,
            &controller.pending,
            &controller.settings_refresh_pending,
        )
        .unwrap(),
        WorkerCommand {
            command: Command::Run { .. },
            remote_id: None,
            background_refresh: false
        }
    ));
    assert!(requests.try_recv().is_err());
    assert!(!controller.settings_refresh_pending.load(Ordering::Acquire));
    assert_eq!(controller.pending.load(Ordering::Acquire), 1);
}

#[test]
fn explicit_settings_queries_keep_their_pending_and_remote_receipts() {
    let (controller, requests) = paused_controller();
    controller.send(Command::LockinSettings).unwrap();
    controller
        .send_remote("settings-1".into(), Command::LockinSettings)
        .unwrap();
    assert_eq!(controller.pending.load(Ordering::Acquire), 2);
    controller.refresh_lockin_settings().unwrap();
    assert!(matches!(
        requests.try_recv().unwrap(),
        WorkerCommand {
            command: Command::LockinSettings,
            remote_id: None,
            background_refresh: false
        }
    ));
    assert!(
        matches!(requests.try_recv().unwrap(), WorkerCommand { command: Command::LockinSettings, remote_id: Some(id), background_refresh: false } if id == "settings-1")
    );
    assert!(requests.try_recv().is_err());
    assert!(!controller.settings_refresh_pending.load(Ordering::Acquire));
}

#[test]
fn display_refresh_failure_retires_without_underflowing_pending() {
    let controller = controller();
    for _ in 0..2 {
        controller.refresh_lockin_settings().unwrap();
        // An unconnected worker fails without opening any device. Completion
        // must release the refresh slot so a later scheduled refresh can run.
        let Event::Error(error) = controller
            .events
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
        else {
            panic!("expected unconnected settings error");
        };
        assert_eq!(error, "Connect a lock-in first");
        assert!(matches!(
            controller
                .events
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            Event::State(_)
        ));
        assert!(!controller.has_pending());
        assert!(!controller.settings_refresh_pending.load(Ordering::Acquire));
    }
}

#[test]
fn display_refresh_respects_read_only_mode_and_closed_worker() {
    let (mut controller, requests) = paused_controller();
    controller.read_only = true;
    assert!(controller.refresh_lockin_settings().is_err());
    assert!(requests.try_recv().is_err());
    controller.read_only = false;
    drop(requests);
    assert!(controller.refresh_lockin_settings().is_err());
    assert!(!controller.has_pending());
    assert!(!controller.settings_refresh_pending.load(Ordering::Acquire));
}

#[test]
fn direction_failure_identifies_its_device_and_axis_without_hardware() {
    let mut session = Session::default();
    let (send, events) = mpsc::sync_channel(8);
    let result = session.execute(
        Command::ReverseDirection {
            kind: MotionKind::Scanner,
            id: "missing-scanner".into(),
            axis: 2,
        },
        &AtomicBool::new(false),
        &send,
    );
    let Event::DirectionFailed {
        kind,
        id,
        axis,
        error,
    } = events.try_recv().unwrap()
    else {
        panic!("missing correlated direction failure");
    };
    assert_eq!(kind, MotionKind::Scanner);
    assert_eq!(id, "missing-scanner");
    assert_eq!(axis, 2);
    assert_eq!(result.unwrap_err(), error);
}

fn config() -> HardwareConfig {
    HardwareConfig {
        power_meter: None,
        device_ids: Default::default(),
        model: Model::Sr830,
        address: 12,
        motion: MotionConfig::default(),
    }
}

fn invalid_connection_inventory() -> crate::inventory::Inventory {
    use crate::inventory::{DelayEntry, Inventory, LockinEntry, PowerMeterEntry, ScannerEntry};
    Inventory {
        lockins: vec![LockinEntry::new(
            "bad-lockin",
            "bad lock-in",
            Model::Sr830,
            0,
        )],
        delays: vec![DelayEntry::new(
            "bad-delay",
            "bad delay",
            serde_json::from_value(serde_json::json!({"controller":"INVALID"})).unwrap(),
            0.,
        )],
        scanners: vec![ScannerEntry::new(
            "bad-scanner",
            "bad scanner",
            Some(serde_json::from_value(serde_json::json!({"controller":"INVALID"})).unwrap()),
            None,
            [0.; 2],
        )],
        power_meters: vec![PowerMeterEntry {
            id: "bad-power".into(),
            name: "bad power".into(),
            label: String::new(),
            config: crate::power_meter::Config {
                correction_factor: 0.,
                ..Default::default()
            },
        }],
        ..Default::default()
    }
}

#[test]
fn all_connect_collects_each_device_error_including_power_after_motion_failure() {
    let mut session = Session::default();
    let (send, _events) = mpsc::sync_channel(16);
    let error = session
        .execute(
            Command::ConnectInventory {
                inventory: invalid_connection_inventory(),
                errors: vec!["edited device could not be parsed".into()],
            },
            &AtomicBool::new(false),
            &send,
        )
        .unwrap_err();
    for name in [
        "bad-lockin",
        "bad-delay",
        "bad-scanner",
        "bad-power",
        "edited device could not be parsed",
    ] {
        assert!(error.contains(name), "{error}");
    }
    assert!(error.contains("Connections retained: none"), "{error}");
    assert!(session.state.registered_devices.is_empty());
    assert!(session.state.power_meters.is_empty());
}

#[test]
fn all_connect_after_stop_attempts_devices_and_releases_the_worker() {
    let controller = controller();
    for _ in 0..2 {
        controller.stop();
        controller
            .send(Command::ConnectInventory {
                inventory: invalid_connection_inventory(),
                errors: vec![],
            })
            .unwrap();
        let Event::Error(error) = controller
            .events
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
        else {
            panic!("expected a per-device connection error");
        };
        assert!(
            error.contains("bad-lockin") && error.contains("bad-power"),
            "{error}"
        );
        assert!(!error.contains("Connection attempts stopped"), "{error}");
        assert!(matches!(
            controller
                .events
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            Event::State(_)
        ));
        assert!(!controller.has_pending());
        assert!(!controller.busy.load(Ordering::Acquire));
    }
}

#[test]
fn all_connect_stop_does_not_attempt_the_remaining_devices() {
    let mut session = Session::default();
    let (send, _events) = mpsc::sync_channel(16);
    let error = session
        .connect_inventory(
            invalid_connection_inventory(),
            vec![],
            &AtomicBool::new(true),
            &send,
        )
        .unwrap_err();
    assert!(error.contains("Connection attempts stopped"), "{error}");
    assert!(!error.contains("[bad-"), "{error}");
    assert!(session.state.registered_devices.is_empty());
}

#[cfg(unix)]
#[test]
fn all_connect_reaches_a_good_lockin_after_bad_config_and_preserves_it() {
    let _lease = PROTECTION_TEST_LEASE.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut session = scripted_lockin_session(
        dir.path(),
        r#"#!/bin/sh
printf 'KIV\tOK\t-\n'
while read -r operation address timeout payload; do
  if [ "$operation" = EXIT ]; then break; fi
  case "$payload" in
    534e41503f312c322c332c340a) printf 'KIV\tOK\t312c322c332c34\n' ;;
    *) printf 'KIV\tOK\t30\n' ;;
  esac
done
"#,
    );
    let mut inventory = invalid_connection_inventory();
    inventory.lockins.push(crate::inventory::LockinEntry::new(
        "lockin-1",
        "good lock-in",
        Model::Sr830,
        12,
    ));
    inventory.selected.delay = "bad-delay".into();
    inventory.selected.scanner = "bad-scanner".into();
    let selected = inventory.selected.clone();
    let (send, _events) = mpsc::sync_channel(16);
    let error = session
        .connect_inventory(inventory, vec![], &AtomicBool::new(false), &send)
        .unwrap_err();
    assert!(
        error.contains("bad-delay") && error.contains("bad-power"),
        "{error}"
    );
    assert!(
        error.contains("Connections retained: lockin lockin-1"),
        "{error}"
    );
    assert_eq!(session.state.signal.unwrap().x_v, 1.);
    assert_eq!(session.config.as_ref().unwrap().device_ids, selected);
    assert!(session.state.devices[0]);
    assert_eq!(session.state.registered_devices.len(), 1);
    session.read().unwrap();
    assert_eq!(session.state.signal.unwrap().y_v, 2.);

    // A later malformed edit is omitted from the snapshot. Restoring the
    // selection must use this same connection's confirmed identity, not 0.
    let error = session
        .connect_inventory(
            invalid_connection_inventory(),
            vec!["Selected lock-in edit skipped".into()],
            &AtomicBool::new(false),
            &send,
        )
        .unwrap_err();
    assert!(!error.contains("Restore measurement selection"), "{error}");
    assert_eq!(session.config.as_ref().unwrap().address, 12);
    assert!(session.state.devices[0]);
}

#[test]
fn demo_run_metadata_uses_requested_devices_instead_of_previous_selection() {
    let mut previous = config();
    previous.device_ids.scanner = "scanner-2".into();
    let mut session = Session {
        config: Some(previous),
        ..Default::default()
    };
    let requested = config();
    let plan = Plan {
        workflow: Workflow::SignalMonitor,
        n_points: 2,
        interval_s: 0.,
        ..Default::default()
    };
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("demo.csv");
    let (send, _events) = mpsc::sync_channel(64);
    session
        .acquire(
            Source::Demo,
            requested.clone(),
            &plan,
            Some(&output),
            &AtomicBool::new(false),
            &send,
        )
        .unwrap();
    let metadata: Value =
        serde_json::from_slice(&std::fs::read(directory.path().join("demo.run.json")).unwrap())
            .unwrap();
    assert_eq!(
        metadata["context"]["configured_hardware"]["device_ids"],
        serde_json::json!(requested.device_ids)
    );
    assert_eq!(metadata["rows"], 2);
    // A finite monitor never claims the Until Stop mode.
    assert!(metadata["context"].get("signal_monitor_length").is_none());
}
#[test]
fn until_stop_monitor_reads_past_points_until_stop_and_records_every_row() {
    let mut session = Session::default();
    // Points (2) is ignored: the monitor reads until Stop.
    let plan = Plan {
        workflow: Workflow::SignalMonitor,
        n_points: 2,
        interval_s: 0.,
        ..Default::default()
    };
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("until-stop.csv");
    let (send, events) = mpsc::sync_channel(4096);
    let cancel = Arc::new(AtomicBool::new(false));
    let stopper = {
        let cancel = cancel.clone();
        thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(20);
            let mut samples = 0;
            while samples < 25 && Instant::now() < deadline {
                match events.recv_timeout(Duration::from_millis(50)) {
                    Ok(Event::Sample(_)) => samples += 1,
                    Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return samples,
                }
            }
            // The operator's Stop. Keep draining so no send can block it.
            cancel.store(true, Ordering::Release);
            while events.recv().is_ok() {}
            samples
        })
    };
    let result = session.acquire_run(
        Source::Demo,
        config(),
        &plan,
        Some(&output),
        true,
        &cancel,
        &send,
    );
    drop(send);
    let samples = stopper.join().unwrap();
    result.unwrap();
    assert!(samples >= 25, "{samples} samples");
    assert_eq!(session.state.total, 0);
    assert!(session.state.rows >= 25, "{} rows", session.state.rows);
    let metadata: Value = serde_json::from_slice(
        &std::fs::read(directory.path().join("until-stop.run.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        metadata["context"]["signal_monitor_length"]["mode"],
        "until_stop"
    );
    // Stop is the normal end: every row read is in the finished recording.
    assert_eq!(metadata["status"], "cancelled");
    assert_eq!(metadata["rows"], serde_json::json!(session.state.rows));
    let csv = std::fs::read_to_string(&output).unwrap();
    assert!(csv.lines().count() > session.state.rows);
}
#[test]
fn until_stop_is_never_accepted_from_json_and_only_for_signal_monitor() {
    assert!(
        serde_json::from_value::<Command>(serde_json::json!({
            "command": "monitor_until_stop",
            "source": "demo",
            "config": {},
            "plan": {},
            "output": null,
        }))
        .is_err()
    );
    let mut session = Session::default();
    let (send, _events) = mpsc::sync_channel(64);
    let plan = Plan {
        workflow: Workflow::Trkr,
        ..Default::default()
    };
    let error = session
        .acquire_run(
            Source::Demo,
            config(),
            &plan,
            None,
            true,
            &AtomicBool::new(false),
            &send,
        )
        .unwrap_err();
    assert!(error.contains("only for Signal Monitor"));
    assert_eq!(session.state.rows, 0);
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
    assert!(matches!(command, Command::Move { approach, .. } if approach == [false, true, true]));
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
fn manual_motion_phase_names_only_the_commanded_axes() {
    assert_eq!(
        Phase::motion(MotionAction::Moving, [None, None, Some(1.)]).to_string(),
        "Moving y"
    );
    assert_eq!(
        Phase::motion(MotionAction::Moving, [None, Some(1.), Some(2.)]).to_string(),
        "Moving x/y"
    );
    assert_eq!(
        Phase::motion(MotionAction::Initializing, [Some(0.), None, None]).to_string(),
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
        if let Event::Finished(result) = c.events.recv_timeout(Duration::from_secs(5)).unwrap() {
            assert_eq!(result.unwrap().0, 5);
            break;
        }
    }
    let meta: Value =
        serde_json::from_slice(&std::fs::read(path.with_extension("run.json")).unwrap()).unwrap();
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
        if let Event::Finished(result) = c.events.recv_timeout(Duration::from_secs(5)).unwrap() {
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
fn recording_context_invalidates_settings_when_identity_or_settings_change() {
    let mut session = Session {
        config: Some(config()),
        lockin_context: Some(serde_json::json!({"value":{"Sensitivity":"old"}})),
        ..Default::default()
    };
    let mut changed = config();
    changed.address = 13;
    session.connect(changed, vec![]).unwrap();
    assert!(session.lockin_context.is_none());
    session.lockin_context = Some(serde_json::json!({"value":{"Sensitivity":"old"}}));
    let (send, _events) = mpsc::sync_channel(8);
    assert!(
        session
            .execute(
                Command::SetLockin {
                    setting: "sensitivity".into(),
                    value: "unknown".into()
                },
                &AtomicBool::new(false),
                &send
            )
            .is_err()
    );
    assert!(session.lockin_context.is_none());
    session.lockin_context = Some(serde_json::json!({"value":{}}));
    assert!(
        session
            .recording_context(Source::Demo, &config())
            .get("cached_lockin_settings")
            .is_none()
    );
}

#[test]
fn lockin_status_display_tracks_new_faults_and_does_not_keep_stale_success() {
    let mut state = Snapshot {
        lockin: serde_json::json!({"Time Constant":2.,"Overload":{"overload":false}}),
        ..Default::default()
    };
    update_lockin_status(
        &mut state,
        &Ok(serde_json::json!({"overload":true,"reference_unlock":true})),
    );
    assert_eq!(state.lockin["Overload"]["overload"], true);
    assert_eq!(state.lockin["Time Constant"], 2.);
    update_lockin_status(&mut state, &Err("status transport failed".into()));
    assert!(state.lockin["Overload"].is_null());
    assert_eq!(state.lockin["Status Error"], "status transport failed");
}
#[test]
fn stop_normalizes_cancellation_but_never_a_failed_stop_or_restoration() {
    assert!(expected_stop_error("transport: GPIB read cancelled"));
    assert!(expected_stop_error(
        "Lock-in status: transport: GPIB read cancelled"
    ));
    assert!(!expected_stop_error(
        "motion cancelled; STOP failed: still moving"
    ));
    assert!(!expected_stop_error(
        "transport: GPIB read cancelled; LI5640 recovery required"
    ));
}

/// Serializes the scripted lock-in tests. A failing test panics while holding
/// the lease; that must report its own failure, not poison the lease and fail
/// every later test that waits for it.
#[cfg(unix)]
struct ProtectionLease(std::sync::Mutex<()>);
#[cfg(unix)]
impl ProtectionLease {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, ()>, std::convert::Infallible> {
        Ok(self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()))
    }
}
#[cfg(unix)]
static PROTECTION_TEST_LEASE: ProtectionLease = ProtectionLease(std::sync::Mutex::new(()));

#[cfg(unix)]
fn scripted_lockin_session(dir: &Path, source: &str) -> Session {
    use std::os::unix::fs::PermissionsExt;
    let helper = dir.join("helper.sh");
    std::fs::write(&helper, source).unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
    Session {
        gpib: Some(Gpib::open(&helper, 12).unwrap()),
        config: Some(config()),
        state: Snapshot {
            devices: [true, false, false, false],
            ..Default::default()
        },
        ..Default::default()
    }
}

#[cfg(unix)]
#[test]
fn status_transport_failure_marks_even_the_last_point_run_as_error() {
    let _lease = PROTECTION_TEST_LEASE.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let script = r#"#!/bin/sh
printf 'KIV\tOK\t-\n'
status_count=0
while read -r operation address timeout payload; do
  if [ "$operation" = EXIT ]; then break; fi
  case "$payload" in
    534e41503f312c322c332c340a) printf 'KIV\tOK\t312c322c332c34\n' ;;
    4c4941533f0a)
      status_count=$((status_count+1))
      if [ "$status_count" -eq 2 ]; then printf 'KIV\tERR\t617564697420737461747573206661696c757265\n'; else printf 'KIV\tOK\t30\n'; fi ;;
    *) printf 'KIV\tOK\t30\n' ;;
  esac
done
"#;
    let mut session = scripted_lockin_session(dir.path(), script);
    let path = dir.path().join("fault.csv");
    let plan = Plan {
        n_points: 1,
        interval_s: 0.02,
        ..Default::default()
    };
    let (send, _events) = mpsc::sync_channel(64);
    let error = session
        .acquire(
            Source::Gpib {
                model: Model::Sr830,
                address: 12,
            },
            config(),
            &plan,
            Some(&path),
            &AtomicBool::new(false),
            &send,
        )
        .unwrap_err();
    assert!(error.contains("audit status failure"), "{error}");
    let metadata: Value =
        serde_json::from_slice(&std::fs::read(path.with_extension("run.json")).unwrap()).unwrap();
    assert_eq!(metadata["status"], "error");
    assert_eq!(metadata["rows"], 1);
    assert!(session.state.lockin["Overload"].is_null());
    let mut csv = csv::Reader::from_path(&path).unwrap();
    let headers = csv.headers().unwrap().clone();
    let quality = headers.iter().position(|v| v == "quality_flags").unwrap();
    assert!(csv.records().next().unwrap().unwrap()[quality].contains("status_unknown"));
}

#[cfg(unix)]
#[test]
fn stop_during_a_recorded_read_keeps_connection_and_finalizes_cancelled_metadata() {
    let _lease = PROTECTION_TEST_LEASE.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("formal-read");
    let script = format!(
        r#"#!/bin/sh
printf 'KIV\tOK\t-\n'
reads=0
while read -r operation address timeout payload; do
  if [ "$operation" = EXIT ]; then break; fi
  if [ "$payload" = 534e41503f312c322c332c340a ]; then
    reads=$((reads+1))
    if [ "$reads" -eq 2 ]; then printf started > '{}'; sleep 0.2; fi
    printf 'KIV\tOK\t312c322c332c34\n'
  else printf 'KIV\tOK\t30\n'; fi
done
"#,
        marker.display()
    );
    let mut session = scripted_lockin_session(dir.path(), &script);
    let token = Arc::new(AtomicBool::new(false));
    let cancel = token.clone();
    let stopper = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !marker.exists() {
            assert!(Instant::now() < deadline, "formal read never started");
            thread::sleep(Duration::from_millis(2));
        }
        cancel.store(true, Ordering::Release);
    });
    let path = dir.path().join("stopped.csv");
    let plan = Plan {
        n_points: 2,
        interval_s: 0.02,
        ..Default::default()
    };
    let (send, _events) = mpsc::sync_channel(64);
    session
        .acquire(
            Source::Gpib {
                model: Model::Sr830,
                address: 12,
            },
            config(),
            &plan,
            Some(&path),
            &token,
            &send,
        )
        .unwrap();
    stopper.join().unwrap();
    let gpib = session.gpib.as_mut().unwrap();
    assert!(!gpib.is_poisoned());
    assert_eq!(gpib.query("SNAP?1,2,3,4\n").unwrap(), "1,2,3,4");
    let metadata: Value =
        serde_json::from_slice(&std::fs::read(path.with_extension("run.json")).unwrap()).unwrap();
    assert_eq!(metadata["status"], "cancelled");
    assert_eq!(metadata["rows"], 0);
}

#[cfg(unix)]
#[test]
fn external_lockin_write_reads_back_the_settings_before_acknowledging() {
    let _lease = PROTECTION_TEST_LEASE.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let script = r#"#!/bin/sh
printf 'KIV\tOK\t-\n'
changed=0
while read -r operation address timeout payload; do
  if [ "$operation" = EXIT ]; then break; fi
  case "$payload" in
    53454e532031300a) changed=1; printf 'KIV\tOK\t-\n' ;;
    53454e533f0a) if [ "$changed" -eq 1 ]; then printf 'KIV\tOK\t3130\n'; else printf 'KIV\tOK\t30\n'; fi ;;
    *) printf 'KIV\tOK\t30\n' ;;
  esac
done
"#;
    let mut session = scripted_lockin_session(dir.path(), script);
    let (send, _events) = mpsc::sync_channel(10);
    session
        .execute_remote(
            Command::SetLockin {
                setting: "Sensitivity".into(),
                value: serde_json::json!(5e-6),
            },
            &AtomicBool::new(false),
            &send,
        )
        .unwrap();
    assert_eq!(session.state.lockin["Sensitivity"], serde_json::json!(5e-6));
    assert!(session.lockin_context.is_some());
}

#[test]
fn disconnected_signal_mode_selection_is_acknowledged_without_connecting_devices() {
    let mut session = Session::default();
    let (send, _events) = mpsc::sync_channel(8);
    session
        .execute_remote(
            Command::SetLockin {
                setting: "display_mode".into(),
                value: "r_theta".into(),
            },
            &AtomicBool::new(false),
            &send,
        )
        .unwrap();
    assert_eq!(session.state.signal_mode, DisplayMode::RTheta);
    assert!(session.gpib.is_none() && session.rig.is_none());
    assert!(session.state.devices.iter().all(|connected| !*connected));
    assert!(
        session
            .execute(
                Command::SetLockin {
                    setting: "display_mode".into(),
                    value: "bad".into()
                },
                &AtomicBool::new(false),
                &send
            )
            .is_err()
    );
    assert_eq!(session.state.signal_mode, DisplayMode::RTheta);
}

#[cfg(unix)]
#[test]
fn li5640_acquisition_records_only_selected_pair_and_never_switches_displays_per_point() {
    let _lease = PROTECTION_TEST_LEASE.lock().unwrap();
    for (mode, unit, input) in [
        (DisplayMode::Xy, "V", "0"),
        (DisplayMode::RTheta, "V", "0"),
        (DisplayMode::Xy, "A", "2"),
        (DisplayMode::RTheta, "A", "3"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let hex = |text: &str| {
            text.as_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        let display = if mode == DisplayMode::Xy { "0" } else { "1" };
        let mut script = "#!/bin/sh\nprintf 'KIV\\tOK\\t-\\n'\nwhile read -r operation address timeout payload; do\n if [ \"$operation\" = EXIT ]; then break; fi\n if [ \"$operation\" = WRITE ]; then printf 'KIV\\tERR\\t756e6578706563746564207772697465\\n'; continue; fi\n case \"$payload\" in\n".to_owned();
        for (command, reply) in [
            ("OTYP?\n", "1,2,3"),
            ("DDEF? 1\n", display),
            ("DDEF? 2\n", display),
            ("DOUT?\n", "1.234567890123456e-9,-20,464.2"),
            ("ISRC?\n", input),
            ("ISEN?\n", if input == "2" { "4" } else { "1" }),
        ] {
            script.push_str(&format!(
                "  {}) printf 'KIV\\tOK\\t{}\\n' ;;\n",
                hex(command),
                hex(reply)
            ));
        }
        script.push_str("  *) printf 'KIV\\tOK\\t30\\n' ;;\n esac\ndone\n");
        let mut session = scripted_lockin_session(dir.path(), &script);
        let mut hardware = config();
        hardware.model = Model::Li5640;
        session.config = Some(hardware.clone());
        session.state.signal_mode = mode;
        let plan = Plan {
            n_points: 2,
            interval_s: 0.01,
            ..Default::default()
        };
        let path = dir.path().join("li.csv");
        let (send, _events) = mpsc::sync_channel(128);
        session
            .acquire(
                Source::Gpib {
                    model: Model::Li5640,
                    address: 12,
                },
                hardware,
                &plan,
                Some(&path),
                &AtomicBool::new(false),
                &send,
            )
            .unwrap();
        assert_eq!(session.state.rows, 2);
        assert_eq!(session.state.signal.unwrap().acquired_pair(), Some(mode));
        let mut csv = csv::Reader::from_path(&path).unwrap();
        let headers = csv.headers().unwrap().clone();
        for record in csv.records() {
            let record = record.unwrap();
            let get = |field| &record[headers.iter().position(|h| h == field).unwrap()];
            assert_eq!(get("signal_unit"), unit);
            if unit != "V" {
                assert!(get("X_V").is_empty() && get("Y_V").is_empty() && get("R_V").is_empty());
                assert!(get("quality_flags").contains("voltage_unavailable"));
            }
            if mode == DisplayMode::Xy {
                assert!(get("R_V").is_empty() && get("Theta_deg").is_empty());
                assert!(get("quality_flags").contains("signal_xy_only"));
            } else {
                assert!(get("X_V").is_empty() && get("Y_V").is_empty());
                assert!(get("quality_flags").contains("signal_rtheta_only"));
            }
        }
        let metadata: Value =
            serde_json::from_slice(&std::fs::read(path.with_extension("run.json")).unwrap())
                .unwrap();
        assert_eq!(
            metadata["context"]["signal_mode"],
            serde_json::to_value(mode).unwrap()
        );
        let mut raw = csv::Reader::from_path(path.with_extension("raw.csv")).unwrap();
        let headers = raw.headers().unwrap().clone();
        let command = headers.iter().position(|h| h == "command").unwrap();
        let response = headers.iter().position(|h| h == "raw_response").unwrap();
        let mut signal_responses = 0;
        for record in raw.records() {
            let record = record.unwrap();
            assert!(record[command].is_empty() || record[command].contains('?'));
            if &record[command] == "DOUT?\n" {
                assert_eq!(&record[response], "1.234567890123456e-9,-20,464.2");
                signal_responses += 1;
            }
        }
        assert_eq!(signal_responses, 3);
    }
}

#[cfg(unix)]
#[test]
fn li5640_changed_unit_during_signal_read_is_preserved_as_unknown_and_stops_before_next_point() {
    let _lease = PROTECTION_TEST_LEASE.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let script = r#"#!/bin/sh
printf 'KIV\tOK\t-\n'
reads=0
while read -r operation address timeout payload; do
  if [ "$operation" = EXIT ]; then break; fi
  case "$payload" in
    4f5459503f0a) printf 'KIV\tOK\t312c322c33\n' ;;
    444f55543f0a) reads=$((reads+1)); printf 'KIV\tOK\t312c322c3330\n' ;;
    495352433f0a) if [ "$reads" -ge 3 ]; then printf 'KIV\tOK\t32\n'; else printf 'KIV\tOK\t30\n'; fi ;;
    *) printf 'KIV\tOK\t30\n' ;;
  esac
done
"#;
    let mut session = scripted_lockin_session(dir.path(), script);
    let mut hardware = config();
    hardware.model = Model::Li5640;
    session.config = Some(hardware.clone());
    let plan = Plan {
        n_points: 2,
        interval_s: 0.01,
        ..Default::default()
    };
    let path = dir.path().join("unit-changed.csv");
    let (send, _events) = mpsc::sync_channel(128);
    let error = session
        .acquire(
            Source::Gpib {
                model: Model::Li5640,
                address: 12,
            },
            hardware,
            &plan,
            Some(&path),
            &AtomicBool::new(false),
            &send,
        )
        .unwrap_err();
    assert!(error.contains("unit context changed"), "{error}");
    assert_eq!(session.state.rows, 1);
    let mut csv = csv::Reader::from_path(&path).unwrap();
    let headers = csv.headers().unwrap().clone();
    let records = csv.records().map(Result::unwrap).collect::<Vec<_>>();
    assert_eq!(records.len(), 1);
    let cell = |field| &records[0][headers.iter().position(|h| h == field).unwrap()];
    assert_eq!(cell("signal_unit"), "unknown");
    assert!(cell("quality_flags").contains("unit_context_changed"));
    let metadata: Value =
        serde_json::from_slice(&std::fs::read(path.with_extension("run.json")).unwrap()).unwrap();
    assert_eq!(metadata["status"], "error");
    assert_eq!(metadata["rows"], 1);
    let mut raw = csv::Reader::from_path(path.with_extension("raw.csv")).unwrap();
    let headers = raw.headers().unwrap().clone();
    let event = headers.iter().position(|h| h == "event_type").unwrap();
    let details = headers.iter().position(|h| h == "details_json").unwrap();
    let mut before = None;
    let mut after = None;
    for row in raw.records() {
        let row = row.unwrap();
        if &row[event] == "unit_context_before_signal" {
            before = Some(
                serde_json::from_str::<Value>(&row[details]).unwrap()["settings"]["input_mode"]
                    .clone(),
            );
        }
        if &row[event] == "status_check" {
            after = Some(
                serde_json::from_str::<Value>(&row[details]).unwrap()["settings"]["input_mode"]
                    .clone(),
            );
        }
    }
    assert_eq!(before, Some(serde_json::json!(0)));
    assert_eq!(after, Some(serde_json::json!(2)));
}

#[test]
fn sample_json_exposes_qualified_units_and_flags_without_private_recording_bookkeeping() {
    let plan = Plan::default();
    let sample = Sample {
        recording: Box::new(SampleRecording {
            signal_unit: "A".into(),
            quality_flags: "signal_xy_only".into(),
            run_id: "private-csv-id".into(),
            point_index: 7,
            ..Default::default()
        }),
        timing: Default::default(),
        captured_at: None,
        elapsed: 0.,
        target: plan.targets().unwrap()[0],
        signal: Signal::from_pair(DisplayMode::Xy, 1e-9, 2e-9).unwrap(),
        position: Default::default(),
    };
    let value = serde_json::to_value(&sample).unwrap();
    assert_eq!(value["signal_unit"], "A");
    assert_eq!(value["quality_flags"], "signal_xy_only");
    assert!(value["signal"]["r_v"].is_null());
    assert!(value.get("recording").is_none());
    assert!(value.get("run_id").is_none());
    let decoded: Sample = serde_json::from_value(value).unwrap();
    assert_eq!(decoded.recording.signal_unit, "A");
    assert_eq!(decoded.signal.acquired_pair(), Some(DisplayMode::Xy));
}
#[test]
fn native_replay_preserves_single_pair_units_and_unknown_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("single.csv");
    std::fs::write(
        &path,
        "X,Y,R,Theta_deg,quality_flags,signal_unit,elapsed_s\n,,1e-9,40,signal_rtheta_only,A,0\n",
    )
    .unwrap();
    let mut session = Session::default();
    let (send, events) = mpsc::sync_channel(8);
    session
        .replay(&path, 0., &AtomicBool::new(false), &send)
        .unwrap();
    let Event::Sample(sample) = events.try_recv().unwrap() else {
        panic!("missing sample")
    };
    assert_eq!(sample.recording.signal_unit, "A");
    assert_eq!(sample.signal.acquired_pair(), Some(DisplayMode::RTheta));
    assert!(sample.signal.x_v.is_nan());
}
#[test]
fn live_json_qualifies_its_cached_unit_as_last_observed_not_atomic() {
    let (send, events) = mpsc::sync_channel(8);
    let mut monitor = LiveMonitor {
        signal_unit: "A".into(),
        ..LiveMonitor::default()
    };
    monitor.publish(
        Signal::from_pair(DisplayMode::Xy, 1e-9, 2e-9).unwrap(),
        ReadingOrigin::Hardware,
        &Position::default(),
        &send,
    );
    let Event::Live(reading) = events.try_recv().unwrap() else {
        panic!("missing live event")
    };
    let mut value = serde_json::to_value(reading).unwrap();
    assert_eq!(value["signal_unit"], "A");
    assert_eq!(value["unit_qualification"], "last_observed_status");
    value.as_object_mut().unwrap().remove("signal_unit");
    let legacy: LiveReading = serde_json::from_value(value).unwrap();
    assert_eq!(legacy.signal_unit, "unknown");
}

#[test]
fn existing_output_is_rejected_before_selecting_or_opening_hardware() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("occupied.csv");
    std::fs::write(&output, b"existing measurement").unwrap();
    let mut session = Session::default();
    let mut requested = config();
    requested.address = u8::MAX;
    let (send, _events) = mpsc::sync_channel(64);
    let error = session
        .acquire(
            Source::Gpib {
                model: requested.model,
                address: requested.address,
            },
            requested,
            &Plan::default(),
            Some(&output),
            &AtomicBool::new(false),
            &send,
        )
        .unwrap_err();
    assert!(
        error.contains("Recording destination already exists"),
        "{error}"
    );
    assert_eq!(session.failure_code.as_deref(), Some("save_failed"));
    assert!(session.config.is_none());
    assert_eq!(std::fs::read(&output).unwrap(), b"existing measurement");
}

#[test]
fn optional_power_does_not_block_recording_and_missing_values_are_explicit() {
    for selected in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("power.csv");
        let mut requested = config();
        if selected {
            requested.device_ids.power_meter = "power-1".into();
            requested.power_meter = Some(crate::power_meter::Config {
                resource: "USB0::0x1313::0x8078::FAKE::INSTR".into(),
                ..Default::default()
            });
        }
        let session = Session::default();
        assert_eq!(
            session
                .observe_power(
                    &requested,
                    Source::Gpib {
                        model: requested.model,
                        address: 12
                    },
                    &AtomicBool::new(false)
                )
                .is_some(),
            selected
        );
        let mut session = session;
        let plan = Plan {
            n_points: 2,
            interval_s: 0.,
            ..Default::default()
        };
        let (send, _events) = mpsc::sync_channel(64);
        session
            .acquire(
                Source::Demo,
                requested,
                &plan,
                Some(&output),
                &AtomicBool::new(false),
                &send,
            )
            .unwrap();
        let mut csv = csv::Reader::from_path(&output).unwrap();
        let headers = csv.headers().unwrap().clone();
        let power = headers.iter().position(|v| v == "sample_power_W").unwrap();
        let quality = headers.iter().position(|v| v == "quality_flags").unwrap();
        let rows: Vec<_> = csv.records().map(Result::unwrap).collect();
        assert_eq!(rows.len(), 2);
        for row in rows {
            assert_eq!(&row[power], "");
            assert_eq!(row[quality].contains("power_unavailable"), selected);
        }
        let mut raw = csv::Reader::from_path(output.with_extension("raw.csv")).unwrap();
        let headers = raw.headers().unwrap().clone();
        let event = headers.iter().position(|v| v == "event_type").unwrap();
        let error = headers.iter().position(|v| v == "error").unwrap();
        let power_rows: Vec<_> = raw
            .records()
            .map(Result::unwrap)
            .filter(|r| &r[event] == "power_read")
            .collect();
        assert_eq!(power_rows.len(), if selected { 2 } else { 0 });
        for row in power_rows {
            assert!(row[error].contains("simulated"));
        }
    }
}

/// A Signal Monitor recording never contains a T/X/Y position: blank main CSV
/// position columns, no position_read event, null reading positions and no
/// final position. Handed-off moves are audited only as parallel_move events.
fn assert_no_recorded_position(path: &Path) -> usize {
    let mut csv = csv::Reader::from_path(path).unwrap();
    let headers = csv.headers().unwrap().clone();
    let columns: Vec<usize> = ["t_ps", "t_cor_ps", "x_um", "x_cor_um", "y_um", "y_cor_um"]
        .iter()
        .map(|name| {
            headers
                .iter()
                .position(|header| header == *name)
                .unwrap_or_else(|| panic!("missing {name}"))
        })
        .collect();
    let rows: Vec<_> = csv.records().map(Result::unwrap).collect();
    for (index, row) in rows.iter().enumerate() {
        for column in &columns {
            assert_eq!(&row[*column], "", "row {index} column {column}");
        }
    }
    let events = crate::raw_csv::test_events(&path.with_extension("raw.csv"));
    assert!(events.iter().all(|e| e["event"] != "position_read"));
    for reading in events.iter().filter(|e| e["event"] == "reading") {
        assert!(
            reading["position"]["physical"]
                .as_array()
                .unwrap()
                .iter()
                .all(Value::is_null),
            "{reading}"
        );
    }
    let finished = events.iter().find(|e| e["event"] == "finished").unwrap();
    assert!(finished["last_observed_position"].is_null(), "{finished}");
    let metadata: Value =
        serde_json::from_slice(&std::fs::read(path.with_extension("run.json")).unwrap()).unwrap();
    assert!(metadata["last_observed_position"].is_null());
    assert_eq!(
        metadata["position_qualification"],
        "not_recorded_signal_monitor"
    );
    rows.len()
}

#[test]
fn demo_signal_monitor_records_no_position_and_hands_off_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("demo.csv");
    let (send, _events) = mpsc::sync_channel(256);
    let mut session = Session::default();
    // A stale pre-run observation must not leak into any row.
    session.state.position.physical = [Some(1.0), Some(2.0), Some(3.0)];
    let plan = Plan {
        n_points: 3,
        interval_s: 0.0,
        ..Default::default()
    };
    assert_eq!(plan.workflow, Workflow::SignalMonitor);
    session
        .acquire(
            Source::Demo,
            config(),
            &plan,
            Some(&output),
            &AtomicBool::new(false),
            &send,
        )
        .unwrap();
    assert!(session.parallel.is_none());
    assert!(session.parallel_slot.status().is_none());
    assert_eq!(assert_no_recorded_position(&output), 3);
}

#[test]
fn controller_refuses_parallel_moves_without_a_hardware_monitor() {
    let controller = controller();
    assert!(controller.parallel_motion().is_none());
    assert!(
        controller
            .move_parallel([None, Some(1.0), None], spatial_approach())
            .is_err()
    );
    let read_only = controller_read_only();
    assert!(
        read_only
            .move_parallel([None, Some(1.0), None], spatial_approach())
            .is_err()
    );
}

#[cfg(unix)]
const MONITOR_LOCKIN: &str = r#"#!/bin/sh
printf 'KIV\tOK\t-\n'
while read -r operation address timeout payload; do
  if [ "$operation" = EXIT ]; then break; fi
  if [ "$payload" = 534e41503f312c322c332c340a ]; then printf 'KIV\tOK\t312c322c332c34\n'; else printf 'KIV\tOK\t30\n'; fi
done
"#;

#[cfg(unix)]
fn register_motion(
    session: &mut Session,
    kind: MotionKind,
    mut rig: MotionRig,
    motion: MotionConfig,
) {
    let ids = &session.config.as_ref().unwrap().device_ids;
    let id = match kind {
        MotionKind::Delay => ids.delay.clone(),
        MotionKind::Scanner => ids.scanner.clone(),
    };
    let position = rig.position().unwrap();
    let connected = rig.connected_axes();
    let delay_calibration = rig.delay_calibration().unwrap();
    session.connections.motions.insert(
        (kind, id.clone()),
        connections::ConnectedMotion {
            config: motion,
            rig,
            state: DeviceState {
                id,
                kind: match kind {
                    MotionKind::Delay => "delay",
                    MotionKind::Scanner => "scanner",
                }
                .into(),
                connected,
                position,
                delay_calibration,
                phase: "Ready".into(),
            },
        },
    );
}

#[cfg(unix)]
#[test]
fn idle_selected_position_is_shared_with_its_card_and_parked_positions_refresh() {
    use parallel_motion::tests::{Sim, cc, scanner_rig};

    let config = config();
    let selected_id = config.device_ids.scanner.clone();
    let mut session = Session {
        config: Some(config),
        ..Session::default()
    };
    let active = Sim::with_addresses(&[1]);
    let active_config = MotionConfig {
        x: Some(cc("COM5", 1)),
        ..Default::default()
    };
    register_motion(
        &mut session,
        MotionKind::Scanner,
        scanner_rig(&active_config, &[("COM5", &active)]),
        active_config,
    );
    for (id, port) in [("parked-1", "COM4"), ("parked-2", "COM8")] {
        let configuration = MotionConfig {
            x: Some(cc(port, 1)),
            ..Default::default()
        };
        let sim = Sim::with_addresses(&[1]);
        session.config.as_mut().unwrap().device_ids.scanner = id.into();
        register_motion(
            &mut session,
            MotionKind::Scanner,
            scanner_rig(&configuration, &[(port, &sim)]),
            configuration,
        );
        sim.axis(1, |axis| axis.position = 6.1);
    }
    session.config.as_mut().unwrap().device_ids.scanner = selected_id.clone();
    session.rebuild_selected_motion().unwrap();
    active.axis(1, |axis| axis.position = 5.9);
    let before = active.commands().len();
    for _ in 0..2 {
        session.read_position().unwrap();
        session.poll_registered_positions().unwrap();
    }
    assert_eq!(&active.commands()[before..], ["1TP", "1TP"]);
    let cards = &session.state.registered_devices;
    assert_eq!(session.state.position.x_scanner_mm, Some(5.9));
    assert_eq!(
        cards
            .iter()
            .find(|card| card.id == selected_id)
            .unwrap()
            .position
            .x_scanner_mm,
        Some(5.9)
    );
    for id in ["parked-1", "parked-2"] {
        assert_eq!(
            cards
                .iter()
                .find(|card| card.id == id)
                .unwrap()
                .position
                .x_scanner_mm,
            Some(6.1)
        );
    }
}

#[cfg(unix)]
fn monitor_plan(points: usize, interval_s: f64) -> Plan {
    Plan {
        n_points: points,
        interval_s,
        ..Default::default()
    }
}

#[cfg(unix)]
fn gpib() -> Source {
    Source::Gpib {
        model: Model::Sr830,
        address: 12,
    }
}

#[cfg(unix)]
const EXECUTOR: Option<&str> = Some("trkr-parallel-motion");

/// After the executor's first exchange, nobody else reached this controller
/// until the run returned.
#[cfg(unix)]
fn owned_only_by_executor(log: &[(String, Option<String>)]) {
    let first = log
        .iter()
        .position(|(_, thread)| thread.as_deref() == EXECUTOR)
        .expect("executor exchange");
    for (command, thread) in &log[first..] {
        assert_eq!(thread.as_deref(), EXECUTOR, "{command}");
    }
}

#[cfg(unix)]
fn wait_until_true(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(2));
    }
}

/// Cancels the acquisition if the helper thread that drives parallel moves
/// fails, so the monitor ends (aborting the in-flight move) and the test
/// reports that failure promptly instead of waiting on a driver timeout while
/// it holds the protection lease.
#[cfg(unix)]
struct CancelOnPanic(Arc<AtomicBool>);
#[cfg(unix)]
impl Drop for CancelOnPanic {
    fn drop(&mut self) {
        if thread::panicking() {
            self.0.store(true, Ordering::Release);
        }
    }
}

/// Signal Monitor keeps its read schedule while parallel moves run.
///
/// Run in isolation on macOS CI; see the note in `.github/workflows/test.yml`
/// and keep its `--skip`/`--exact` filters in step if this test is renamed.
#[cfg(unix)]
#[test]
fn monitor_hands_off_every_connected_axis_and_records_no_position() {
    use parallel_motion::{
        ParallelMove,
        tests::{DelaySim, Sim, cc, delay_config, delay_rig, scanner_rig},
    };
    let _lease = PROTECTION_TEST_LEASE.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut session = scripted_lockin_session(dir.path(), MONITOR_LOCKIN);
    let delay = DelaySim::default();
    let delay_motion = MotionConfig {
        delay: Some(delay_config("COM6")),
        ..Default::default()
    };
    register_motion(
        &mut session,
        MotionKind::Delay,
        delay_rig(&delay_motion, &delay),
        delay_motion,
    );
    let (x, y) = (Sim::with_addresses(&[1]), Sim::with_addresses(&[1]));
    x.axis(1, |a| a.move_polls = 6);
    let motion = MotionConfig {
        x: Some(cc("COM5", 1)),
        y: Some(cc("COM4", 1)),
        ..Default::default()
    };
    register_motion(
        &mut session,
        MotionKind::Scanner,
        scanner_rig(&motion, &[("COM5", &x), ("COM4", &y)]),
        motion,
    );
    let slot = session.parallel_slot.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let guard = CancelOnPanic(cancel.clone());
    let requester = thread::spawn(move || {
        let _guard = guard;
        wait_until_true("published executor", || {
            slot.status()
                .is_some_and(|s| s.position.physical.iter().all(Option::is_some))
        });
        let status = slot.status().unwrap();
        // Every selected connected axis, as one unit; no selection toggle.
        assert_eq!(status.axes, [true, true, true]);
        let t = status.position.physical[0].unwrap() + 10.0;
        slot.request(ParallelMove {
            targets: [Some(t), None, None],
            approach: spatial_approach(),
        })
        .unwrap();
        wait_until_true("delay move", || {
            slot.status().is_some_and(|s| s.finished == 1)
        });
        slot.request(ParallelMove {
            targets: [None, Some(2.0), None],
            approach: spatial_approach(),
        })
        .unwrap();
        wait_until_true("scanner move", || {
            slot.status().is_some_and(|s| s.finished == 2)
        });
        (t, slot.status().unwrap())
    });
    let path = dir.path().join("monitor.csv");
    let interval = 0.1;
    let (send, _events) = mpsc::sync_channel(4096);
    let started = Instant::now();
    let acquired = session.acquire(
        gpib(),
        config(),
        &monitor_plan(20, interval),
        Some(&path),
        &cancel,
        &send,
    );
    // Join first so a requester failure is reported as the root cause.
    let (t, moved) = requester.join().unwrap();
    acquired.unwrap();
    // 20 reads at 0.1 s; nothing may wait on a driver timeout.
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "monitor took {:?}",
        started.elapsed()
    );
    assert!(
        moved.fault.is_none() && moved.last_error.is_none(),
        "{moved:?}"
    );
    assert_eq!(moved.position.physical[1], Some(2.0));
    assert!((moved.position.physical[0].unwrap() - t).abs() < 0.5);

    // No position anywhere in the recording, and every read stays on the
    // monitor schedule while T and X move.
    assert_eq!(assert_no_recorded_position(&path), 20);
    let mut csv = csv::Reader::from_path(&path).unwrap();
    let headers = csv.headers().unwrap().clone();
    let elapsed = headers.iter().position(|h| h == "elapsed_s").unwrap();
    let rows: Vec<_> = csv.records().map(Result::unwrap).collect();
    for (index, row) in rows.iter().enumerate() {
        let late = row[elapsed].parse::<f64>().unwrap() - index as f64 * interval;
        assert!(
            (0.0..0.05).contains(&late),
            "row {index} read {late} s late"
        );
    }

    let events = crate::raw_csv::test_events(&path.with_extension("raw.csv"));
    let moves: Vec<_> = events
        .iter()
        .filter(|e| e["event"] == "parallel_move")
        .collect();
    assert_eq!(moves.len(), 2);
    assert!(moves.iter().all(|m| m["status"] == "complete"));
    // The separate operation audit is not aligned to any measurement point.
    assert!(moves.iter().all(|m| m["point_index"].is_null()));
    // The moves genuinely overlapped measurement reads.
    assert!(moves.iter().any(|movement| {
        let window = (
            movement["timing"]["host_started_elapsed_s"]
                .as_f64()
                .unwrap(),
            movement["timing"]["host_completed_elapsed_s"]
                .as_f64()
                .unwrap(),
        );
        events.iter().any(|e| {
            e["event"] == "reading"
                && e["timing"]["signal"]["host_started_elapsed_s"]
                    .as_f64()
                    .is_some_and(|t| window.0 < t && t < window.1)
        })
    }));
    let metadata: Value =
        serde_json::from_slice(&std::fs::read(path.with_extension("run.json")).unwrap()).unwrap();
    assert_eq!(metadata["status"], "complete");
    let axes = &metadata["context"]["signal_monitor_motion"]["parallel_axes"];
    assert_eq!(
        (&axes["t"], &axes["x"], &axes["y"]),
        (&Value::Bool(true), &Value::Bool(true), &Value::Bool(true))
    );

    // Once the executor touched each controller, nobody else did.
    owned_only_by_executor(&delay.log());
    owned_only_by_executor(&lock_log(&x));
    owned_only_by_executor(&lock_log(&y));
    assert!(y.commands().iter().all(|c| !c.contains("PA")));

    // Ownership returned: one normal selected view of T, X and Y again; the
    // display shows the executor's readback, not the pre-run state.
    assert!(session.parallel.is_none());
    assert!(session.parallel_slot.status().is_none());
    assert_eq!(session.state.position.physical[1], Some(2.0));
    let rig = session.rig.as_mut().unwrap();
    assert_eq!(rig.connected_axes(), [true, true, true]);
    assert_eq!(rig.position().unwrap().physical[1], Some(2.0));
}

#[cfg(unix)]
#[test]
fn monitor_without_motion_axes_runs_without_an_executor() {
    let _lease = PROTECTION_TEST_LEASE.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut session = scripted_lockin_session(dir.path(), MONITOR_LOCKIN);
    let path = dir.path().join("lockin-only.csv");
    let (send, _events) = mpsc::sync_channel(1024);
    session
        .acquire(
            gpib(),
            config(),
            &monitor_plan(3, 0.02),
            Some(&path),
            &AtomicBool::new(false),
            &send,
        )
        .unwrap();
    assert!(session.parallel.is_none());
    assert!(session.parallel_slot.status().is_none());
    assert!(session.rig.is_none());
    assert_eq!(assert_no_recorded_position(&path), 3);
    let metadata: Value =
        serde_json::from_slice(&std::fs::read(path.with_extension("run.json")).unwrap()).unwrap();
    assert_eq!(metadata["status"], "complete");
    let axes = &metadata["context"]["signal_monitor_motion"]["parallel_axes"];
    assert_eq!(axes["t"], false);
    assert_eq!(axes["x"], false);
    assert_eq!(axes["y"], false);
}

#[cfg(unix)]
#[test]
fn stop_during_a_parallel_move_aborts_it_and_cleanup_stops_every_axis() {
    use parallel_motion::tests::{DelaySim, Sim, cc, delay_config, delay_rig, scanner_rig};
    let _lease = PROTECTION_TEST_LEASE.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut session = scripted_lockin_session(dir.path(), MONITOR_LOCKIN);
    let delay = DelaySim::default();
    let delay_motion = MotionConfig {
        delay: Some(delay_config("COM6")),
        ..Default::default()
    };
    register_motion(
        &mut session,
        MotionKind::Delay,
        delay_rig(&delay_motion, &delay),
        delay_motion,
    );
    // X and Y share one serial session: they are handed off as one unit.
    let x = Sim::with_addresses(&[1, 2]);
    x.axis(1, |a| a.move_polls = u32::MAX);
    let motion = MotionConfig {
        x: Some(cc("COM4", 1)),
        y: Some(cc("COM4", 2)),
        ..Default::default()
    };
    register_motion(
        &mut session,
        MotionKind::Scanner,
        scanner_rig(&motion, &[("COM4", &x)]),
        motion,
    );
    let token = Arc::new(AtomicBool::new(false));
    session.global_cancel = token.clone();
    let slot = session.parallel_slot.clone();
    let stopper = {
        let token = token.clone();
        let x = x.clone();
        let guard = CancelOnPanic(token.clone());
        thread::spawn(move || {
            let _guard = guard;
            wait_until_true("published executor", || {
                slot.status().is_some_and(|s| s.axes == [true; 3])
            });
            slot.request(parallel_motion::ParallelMove {
                targets: [None, Some(2.0), None],
                approach: spatial_approach(),
            })
            .unwrap();
            wait_until_true("motion start", || x.axis(1, |a| a.state == "28"));
            // Controller::stop
            token.store(true, Ordering::Release);
            slot.abort();
        })
    };
    let path = dir.path().join("stopped.csv");
    let (send, _events) = mpsc::sync_channel(4096);
    session
        .acquire(
            gpib(),
            config(),
            &monitor_plan(100, 0.05),
            Some(&path),
            &token,
            &send,
        )
        .unwrap();
    stopper.join().unwrap();
    let metadata: Value =
        serde_json::from_slice(&std::fs::read(path.with_extension("run.json")).unwrap()).unwrap();
    assert_eq!(metadata["status"], "cancelled");
    assert_no_recorded_position(&path);
    let events = crate::raw_csv::test_events(&path.with_extension("raw.csv"));
    let movement = events
        .iter()
        .find(|e| e["event"] == "parallel_move")
        .expect("aborted move recorded");
    assert_eq!(movement["status"], "failed");
    let mut raw = csv::Reader::from_path(path.with_extension("raw.csv")).unwrap();
    let headers = raw.headers().unwrap();
    let event_column = headers
        .iter()
        .position(|name| name == "event_type")
        .unwrap();
    let error_column = headers.iter().position(|name| name == "error").unwrap();
    let movement_row = raw
        .records()
        .map(Result::unwrap)
        .find(|row| &row[event_column] == "parallel_move")
        .expect("aborted move row");
    assert_eq!(&movement_row[error_column], "motion cancelled");
    // The executor stopped its move; after ownership returned, the normal
    // Stop cleanup reached T, X and Y from the worker as well.
    let x_log = lock_log(&x);
    let pa = x_log
        .iter()
        .position(|(c, _)| c.starts_with("1PA"))
        .unwrap();
    for (command, by_executor) in [("1ST", true), ("1ST", false), ("2ST", false)] {
        assert!(
            x_log[pa..]
                .iter()
                .any(|(c, t)| c == command && (t.as_deref() == EXECUTOR) == by_executor),
            "{command} by executor: {by_executor}"
        );
    }
    let delay_log = delay.log();
    for by_executor in [true, false] {
        assert!(
            delay_log
                .iter()
                .any(|(c, t)| c == "L:1" && (t.as_deref() == EXECUTOR) == by_executor),
            "delay STOP by executor: {by_executor}"
        );
    }
    // Worker cleanup verified every axis stationary, including the delay.
    let stop = events
        .iter()
        .find(|e| e["event"] == "stop")
        .expect("STOP outcome recorded");
    assert_eq!(stop["verification"], "stationary_state_queries");
    assert!(stop["error"].is_null(), "{stop}");
    let worker_stop = delay_log
        .iter()
        .rposition(|(c, t)| c == "L:1" && t.as_deref() != EXECUTOR)
        .unwrap();
    assert!(delay_log[worker_stop..].iter().any(|(c, _)| c == "!:"));
    assert!(session.parallel_slot.status().is_none());
    assert_eq!(session.rig.as_ref().unwrap().connected_axes(), [true; 3]);
}

#[cfg(unix)]
fn lock_log(sim: &parallel_motion::tests::Sim) -> Vec<(String, Option<String>)> {
    sim.commands().into_iter().zip(sim.threads()).collect()
}
