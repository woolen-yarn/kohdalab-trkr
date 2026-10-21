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
