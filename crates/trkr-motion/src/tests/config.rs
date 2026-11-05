use super::*;
fn stage() -> DelayConfig {
    DelayConfig {
        port: "test".into(),
        stage: Some("SGSP46-500".into()),
        direction: 1,
        ..Default::default()
    }
    .normalized()
    .unwrap()
}
fn scanner() -> ScannerConfig {
    ScannerConfig {
        port: "test".into(),
        controller: "CONEXAGAP".into(),
        actuator: Some("AG-M100D".into()),
        sample_um_per_unit: 412.,
        axis: "V".into(),
        ..Default::default()
    }
    .normalized()
    .unwrap()
}
#[test]
fn delay_optical_path_and_runtime_microstep() {
    let c = stage();
    assert!((c.ps_from_mm(200., 10).unwrap() - 333.564095198152).abs() < 1e-9);
    assert!((c.to_mm(-122., 10).unwrap() - 268.287339938).abs() < 1e-9);
    assert_eq!(c.pulse(0., 10).unwrap(), 125000);
    assert_eq!(c.um_per_pulse(10).unwrap(), 2.);
    assert!(c.pulse(2000., 10).is_err());
}
#[test]
fn delay_midpoint_uses_runtime_pulse_limits() {
    let c = DelayConfig {
        port: "test".into(),
        max_pulse: Some(10000),
        pos_um_per_pulse: Some(0.5),
        ..Default::default()
    }
    .normalized()
    .unwrap();
    assert_eq!(c.origin(8).unwrap(), 2.5);
    assert_eq!(c.pulse(0., 8).unwrap(), 5000);
}
#[test]
fn scanner_scale_has_no_extra_thousand() {
    let c = scanner();
    assert_eq!(c.sample_um_from_raw(0.5), 206.);
    assert_eq!(c.raw_from_sample_um(-103.), -0.25);
    assert!(c.raw_target(310.).is_err());
    assert_eq!(c.raw_target(10.).unwrap(), 0.024);
    assert_eq!(c.digits(), 3);
    assert_eq!(c.position_tolerance, Some(0.01));
}
#[test]
fn scanner_position_tolerance_defaults_and_validates_overrides() {
    assert_eq!(scanner().effective_position_tolerance().unwrap(), 0.01);

    let cc = ScannerConfig {
        port: "test".into(),
        actuator: Some("TRA12CC".into()),
        ..Default::default()
    }
    .normalized()
    .unwrap();
    assert_eq!(cc.effective_position_tolerance().unwrap(), 0.0002);

    let mut tighter = scanner();
    tighter.position_tolerance = Some(0.002);
    assert_eq!(
        tighter.normalized().unwrap().position_tolerance,
        Some(0.002)
    );
    let mut looser = scanner();
    looser.position_tolerance = Some(0.02);
    assert_eq!(looser.normalized().unwrap().position_tolerance, Some(0.02));

    for tolerance in [0., -0.01, f64::NAN, f64::INFINITY, 1.500_001] {
        let mut invalid = scanner();
        invalid.position_tolerance = Some(tolerance);
        assert!(
            invalid.normalized().is_err(),
            "{tolerance:?} must be rejected"
        );
    }
}
#[test]
fn scanner_negative_scale_and_preset_alias() {
    let mut c = scanner();
    c.sample_um_per_unit = -412.;
    assert_eq!(c.sample_um_from_raw(-0.5), 206.);
    assert_eq!(c.raw_from_sample_um(206.), -0.5);
    let c:ScannerConfig=serde_json::from_str(r#"{"scanner_controller":"CONEX-AGAP","port":"COM2","actuator":"AG-M100D","axis":2,"sample_um_per_unit":412}"#).unwrap();
    assert_eq!(c.normalized().unwrap().axis, "V");
}
#[test]
fn hysteresis_applies_only_to_cc_and_is_bounded() {
    let mut c = scanner();
    c.software_hysteresis = Hysteresis {
        enabled: true,
        distance_um: Some(5.),
        direction: None,
    };
    assert_eq!(c.approach(-309.).unwrap(), None);
    let mut c = ScannerConfig {
        port: "test".into(),
        actuator: Some("TRA12CC".into()),
        sample_um_per_unit: 10.,
        ..Default::default()
    }
    .normalized()
    .unwrap();
    c.software_hysteresis = Hysteresis {
        enabled: true,
        distance_um: Some(10.),
        direction: None,
    };
    assert!(c.approach(-60.).is_err());
    assert_eq!(c.approach(0.).unwrap(), Some(-10.));
}

#[test]
fn actuator_descriptors_keep_native_defaults_and_capabilities_together() {
    let cc = catalog::scanner_actuator("TRA-12CC").unwrap();
    assert_eq!(cc.native_unit, "mm");
    assert_eq!((cc.minimum, cc.maximum, cc.resolution), (0., 12., 0.0002));
    assert!(cc.supports_controller("CONEXCC"));

    let agap = catalog::scanner_actuator("AG-M100D").unwrap();
    assert_eq!(agap.native_unit, "deg");
    assert_eq!(
        (agap.minimum, agap.maximum, agap.resolution),
        (-0.75, 0.75, 0.001)
    );
    assert!(agap.supports_controller("CONEXAGAP"));
}

#[test]
fn delay_round_trips_apply_to_all_coordinate_scales() {
    let mut one = stage();
    one.direction = 0;
    let mut three = one.clone();
    three.round_trips = 3;
    let mm = 300.;
    assert!(
        (three.ps_from_mm(mm, 10).unwrap() - 3. * one.ps_from_mm(mm, 10).unwrap()).abs() < 1e-12
    );
    let ps = three.ps_from_mm(mm, 10).unwrap();
    assert!((three.to_mm(ps, 10).unwrap() - mm).abs() < 1e-12);
    assert!(
        DelayConfig {
            round_trips: 0,
            ..one
        }
        .normalized()
        .is_err()
    );
}

#[test]
fn reversing_direction_preserves_displayed_position_without_motion() {
    let c = DelayConfig {
        port: "test".into(),
        stage: Some("SGSP46-500".into()),
        ..Default::default()
    }
    .normalized()
    .unwrap();
    let current_mm = 100.;
    let displayed = c.ps_from_mm(current_mm, 1).unwrap();
    let reversed = c.reversed_preserving_display_at_mm(current_mm).unwrap();
    assert_eq!(reversed.direction, 1);
    assert!((reversed.ps_from_mm(current_mm, 1).unwrap() - displayed).abs() < 1e-12);
    assert!(reversed.zero_pos_mm.unwrap() < 0.);

    let scanner = ScannerConfig {
        port: "test".into(),
        actuator: Some("TRA12CC".into()),
        sample_um_per_unit: 10.,
        ..Default::default()
    }
    .normalized()
    .unwrap();
    let current_raw = 1.;
    let displayed = scanner.sample_um_from_raw(current_raw);
    let reversed = scanner
        .reversed_preserving_display_at_raw(current_raw)
        .unwrap();
    assert_eq!(reversed.direction, 1);
    assert!((reversed.sample_um_from_raw(current_raw) - displayed).abs() < 1e-12);
    assert!(reversed.origin_pos.unwrap() < 0.);
}

#[test]
fn scanner_direction_combines_once_with_legacy_signed_scale() {
    let legacy = ScannerConfig {
        port: "test".into(),
        actuator: Some("TRA12CC".into()),
        sample_um_per_unit: -10.,
        ..Default::default()
    }
    .normalized()
    .unwrap();
    let mut reversed = legacy.clone();
    reversed.direction = 1;
    let reversed = reversed.normalized().unwrap();
    assert_eq!(legacy.sample_um_from_raw(7.), -10.);
    assert_eq!(reversed.sample_um_from_raw(7.), 10.);
}

#[test]
fn direction_reversal_preserves_native_compensation_approach_side() {
    let mut config = ScannerConfig {
        port: "fake".into(),
        actuator: Some("TRA12CC".into()),
        sample_um_per_unit: 500.,
        software_hysteresis: Hysteresis {
            enabled: true,
            distance_um: Some(100.),
            direction: Some("negative".into()),
        },
        ..Default::default()
    }
    .normalized()
    .unwrap();
    for scale in [500., -500.] {
        config.sample_um_per_unit = scale;
        let current = 5.;
        let displayed = config.sample_um_from_raw(current);
        let native_before = config.raw_from_sample_um(config.approach(displayed).unwrap().unwrap());
        let reversed = config.reversed_preserving_display_at_raw(current).unwrap();
        let native_after =
            reversed.raw_from_sample_um(reversed.approach(displayed).unwrap().unwrap());
        assert!((native_before - native_after).abs() < 1e-12);
        let restored = reversed
            .reversed_preserving_display_at_raw(current)
            .unwrap();
        assert_eq!(restored.direction, config.direction);
        assert_eq!(restored.origin_pos, config.origin_pos);
    }
}

#[test]
fn pulse_defined_origin_uses_the_observed_microstep_for_display() {
    let config = DelayConfig {
        port: "fake".into(),
        stage: Some("SGSP46-500".into()),
        max_pulse: Some(10_000),
        round_trips: 2,
        ..Default::default()
    }
    .normalized()
    .unwrap();
    let (min, max) = config.native_limits_mm(10).unwrap();
    assert_eq!(
        config
            .measurement_ps_from_mm_with_micro((min + max) / 2., 10)
            .unwrap(),
        0.
    );
    let reversed = config
        .reversed_preserving_display_at_mm_with_micro(max * 0.75, 10)
        .unwrap();
    assert_eq!(
        config.ps_from_mm(max * 0.75, 10).unwrap(),
        reversed.ps_from_mm(max * 0.75, 10).unwrap()
    );
    assert!(config.measurement_ps_from_mm_with_micro(1., 0).is_err());
}
#[test]
fn rejects_invalid_calibrations_before_serial_io() {
    let mut c = scanner();
    c.sample_um_per_unit = 0.;
    assert!(c.normalized().is_err());
    let mut d = stage();
    d.step_angle_deg = Some(f64::NAN);
    assert!(d.normalized().is_err());
    assert!(
        MotionConfig {
            delay: None,
            x: Some(scanner()),
            y: None
        }
        .validate_targets(&[[None, Some(400.), None]])
        .is_err()
    );
}
