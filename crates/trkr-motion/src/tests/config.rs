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
    let cc = actuator_descriptor("TRA-12CC").unwrap();
    assert_eq!(cc.native_unit, "mm");
    assert_eq!((cc.minimum, cc.maximum, cc.resolution), (0., 12., 0.0002));
    assert!(cc.controller.capabilities().supports_home);
    assert!(cc.controller.capabilities().supports_software_hysteresis);

    let agap = actuator_descriptor("AG-M100D").unwrap();
    assert_eq!(agap.native_unit, "deg");
    assert_eq!(
        (agap.minimum, agap.maximum, agap.resolution),
        (-0.75, 0.75, 0.001)
    );
    assert!(agap.controller.capabilities().uses_named_axis);
    assert!(!agap.controller.capabilities().supports_home);
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
