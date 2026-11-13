use super::*;

fn legacy() -> Value {
    json!({"instruments":{"lockin":{"main":{"model":"SR7265","resource":"GPIB0::12::INSTR"}},"delay_stage":{"t":{"controller":"SHOT302GS","stage":"SGSP46-500","port":"COM6","direction":1}},"scanner":{"x":{"controller":"CONEXCC","actuator":"TRA12CC","port":"COM5","axis":1,"sample_um_per_unit":582.},"y":{"controller":"CONEXCC","actuator":"TRA12CC","port":"COM4","axis":1,"sample_um_per_unit":412.}}},"measurements":{"move_abs":{"zero":{"t_ps":2.,"x_um":3.,"y_um":4.}}}})
}

#[test]
fn old_device_ids_need_no_optional_power_meter() {
    let ids: DeviceIds =
        serde_json::from_value(json!({"lockin":"l", "delay":"d", "scanner":"s"})).unwrap();
    assert!(ids.power_meter.is_empty());
    assert_eq!(ids.lockin, "l");
}

#[test]
fn legacy_projection_preserves_unknown_device_fields() {
    let mut profile = legacy();
    profile["instruments"]["scanner"]["x"]["operator_note"] = json!("keep");
    let inventory = Inventory::from_profile(&profile).unwrap();
    inventory.project_selected(&mut profile).unwrap();
    let reloaded = Inventory::from_profile(&profile).unwrap();
    reloaded.project_selected(&mut profile).unwrap();
    assert_eq!(
        profile["instruments"]["scanner"]["x"]["operator_note"],
        "keep"
    );
    assert_eq!(
        profile[INVENTORY_KEY]["scanners"][0]["x"]["operator_note"],
        "keep"
    );
    assert_eq!(profile[INVENTORY_KEY]["selected"]["scanner"], "scanner-1");
}

#[test]
fn selected_projection_uses_one_device_of_each_kind() {
    let mut inventory = Inventory::from_profile(&legacy()).unwrap();
    let mut other = inventory.delays[0].clone();
    other.id = "delay-2".into();
    other.name = "Delay 2".into();
    other.config.port = "COM7".into();
    other.zero_ps = 22.;
    inventory.delays.push(other);
    inventory.selected.delay = "delay-2".into();
    let mut profile = legacy();
    inventory.project_selected(&mut profile).unwrap();
    assert_eq!(profile["instruments"]["delay_stage"]["t"]["port"], "COM7");
    assert_eq!(profile["measurements"]["move_abs"]["zero"]["t_ps"], 22.);
}

#[test]
fn rejects_duplicate_hardware_and_unknown_selection() {
    let mut inventory = Inventory::from_profile(&legacy()).unwrap();
    let mut duplicate = inventory.lockins[0].clone();
    duplicate.id = "lockin-2".into();
    inventory.lockins.push(duplicate);
    assert!(inventory.validate().unwrap_err().contains("Duplicate GPIB"));
    inventory.lockins.pop();
    inventory.selected.scanner = "missing".into();
    assert!(inventory.validate().unwrap_err().contains("does not exist"));
}

#[test]
fn connection_snapshot_omits_invalid_unselected_meter_without_blocking_motion() {
    let raw = legacy();
    let mut inventory = Inventory::from_profile(&raw).unwrap();
    inventory.power_meters.push(PowerMeterEntry {
        id: "power-bad".into(),
        name: "Power Meter bad".into(),
        label: String::new(),
        config: crate::power_meter::Config {
            wavelength_nm: 0.,
            ..Default::default()
        },
    });
    let (snapshot, warnings) = inventory.connection_snapshot(&raw).unwrap();
    assert!(snapshot.power_meters.is_empty());
    assert_eq!(snapshot.delays.len(), 1);
    assert_eq!(snapshot.scanners.len(), 1);
    assert!(warnings.iter().any(|warning| warning.contains("power-bad")));
    // The preview only filters the request; it never edits the profile.
    assert_eq!(inventory.power_meters.len(), 1);
}

#[test]
fn connection_snapshot_drops_bad_selected_axis_instead_of_reusing_stale_value() {
    let mut raw = legacy();
    let inventory = Inventory::from_profile(&raw).unwrap();
    raw["instruments"]["scanner"]["x"]["port"] = json!("");
    let (snapshot, warnings) = inventory.connection_snapshot(&raw).unwrap();
    let scanner = snapshot.scanner().unwrap().unwrap();
    assert!(scanner.x.is_none());
    assert!(scanner.y.is_some());
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("scanner-1 X skipped"))
    );
    assert!(inventory.scanner().unwrap().unwrap().x.is_some());
}

#[test]
fn connection_snapshot_keeps_structural_id_validation_strict() {
    let raw = legacy();
    let mut inventory = Inventory::from_profile(&raw).unwrap();
    let mut duplicate = inventory.delays[0].clone();
    duplicate.id = inventory.delays[0].id.clone();
    inventory.delays.push(duplicate);
    assert!(
        inventory
            .connection_snapshot(&raw)
            .unwrap_err()
            .contains("Duplicate delay ID")
    );
}
#[test]
fn presentation_order_and_names_do_not_reassign_hardware() {
    let mut raw = legacy();
    let mut inventory = Inventory::from_profile(&raw).unwrap();
    inventory.project_selected(&mut raw).unwrap();
    let instruments = raw["instruments"].clone();
    let zero = raw["measurements"]["move_abs"]["zero"].clone();
    let selected = inventory.selected.clone();
    let scanner = PanelKey::new("scanner", "scanner-1");
    let lockin = PanelKey::new("lockin", "lockin-1");
    assert!(inventory.reorder_panel(&scanner, &lockin, false));
    inventory.scanners[0].label = "Pump".into();
    assert_eq!(inventory.scanners[0].display_name(), "Scanner: Pump");
    inventory.project_selected(&mut raw).unwrap();
    let restored = Inventory::from_profile(&raw).unwrap();
    assert_eq!(restored.ordered_panels()[0], scanner);
    assert_eq!(restored.selected, selected);
    assert_eq!(raw["instruments"], instruments);
    assert_eq!(raw["measurements"]["move_abs"]["zero"], zero);
    assert_eq!(restored.scanners[0].display_name(), "Scanner: Pump");
    assert_eq!(restored.delays[0].display_name(), "Delay: 1");
    assert!(!inventory.reorder_panel(&scanner, &scanner, true));
    assert!(!inventory.reorder_panel(&scanner, &PanelKey::new("scanner", "missing"), true));
}

#[test]
fn stale_or_duplicate_panel_order_is_repaired_without_losing_new_entries() {
    let mut inventory = Inventory::from_profile(&legacy()).unwrap();
    let scanner = PanelKey::new("scanner", "scanner-1");
    inventory.panel_order = vec![
        PanelKey::new("scanner", "deleted"),
        scanner.clone(),
        scanner.clone(),
    ];
    let order = inventory.ordered_panels();
    assert_eq!(order.len(), 3);
    assert_eq!(order[0], scanner);
    let mut raw = legacy();
    inventory.project_selected(&mut raw).unwrap();
    assert_eq!(
        raw["device_inventory"]["panel_order"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    inventory.scanners.clear();
    inventory.selected.scanner.clear();
    inventory.project_selected(&mut raw).unwrap();
    assert!(
        !raw["device_inventory"]["panel_order"]
            .to_string()
            .contains("scanner")
    );
}

#[test]
fn legacy_connection_promotion_skips_malformed_axis_without_stale_config() {
    let mut raw = legacy();
    raw["instruments"]["scanner"]["x"]["port"] = json!(5);
    let mut inventory = Inventory::default();
    inventory.scanners.clear();
    inventory.selected.scanner.clear();
    let warnings = inventory.promote_legacy_for_connection(&raw, false, true);
    assert_eq!(inventory.selected.scanner, "scanner-1");
    assert!(inventory.scanners[0].x.is_none());
    assert!(inventory.scanners[0].y.is_some());
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("scanner-1 X skipped"))
    );
}
