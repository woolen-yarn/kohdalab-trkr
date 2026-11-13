use super::*;
use crate::inventory::{DelayEntry, LockinEntry, PowerMeterEntry, ScannerEntry};
use engine::MotionKind;

// Direction is a profile flag (0/1), not the sign of the sample scale. Share
// these choices with Alignment so both editors render the confirmed state.
pub(super) const DIRECTION_CHOICES: [(i32, &str); 2] = [(0, "Normal"), (1, "Reverse")];

pub(super) fn scanner_initialization_homes(controller: &str) -> bool {
    controller
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .eq_ignore_ascii_case("CONEXCC")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DeviceClass {
    Lockin,
    Delay,
    Scanner,
    PowerMeter,
}

fn template_entry<'a, T>(
    template: Option<&str>,
    entries: &'a [T],
    id: impl Fn(&T) -> &str,
) -> Result<Option<&'a T>, String> {
    template
        .map(|template| {
            entries
                .iter()
                .find(|entry| id(entry) == template)
                .ok_or_else(|| "The selected device template is no longer available".into())
        })
        .transpose()
}

impl App {
    pub(super) fn any_hardware_connected(&self) -> bool {
        self.snapshot.devices.iter().any(|v| *v)
            || self
                .snapshot
                .power_meters
                .iter()
                .any(|status| status.connected)
            || self
                .snapshot
                .registered_devices
                .iter()
                .any(|d| d.connected.iter().any(|v| *v))
    }
    /// Build a best-effort inventory for All Connect. Normal save and
    /// measurement paths keep using `sync_inventory`, which remains strict.
    pub(super) fn connection_inventory(
        &mut self,
    ) -> Result<(crate::inventory::Inventory, Vec<String>), String> {
        self.sync();
        let mut preview = self.inventory.clone();
        let mut warnings = preview.promote_legacy_for_connection(
            &self.settings.raw,
            self.stage_configured,
            self.scanner_configured.iter().any(|configured| *configured),
        );
        let (preview, mut invalid) = preview.connection_snapshot(&self.settings.raw)?;
        warnings.append(&mut invalid);
        warnings.sort();
        warnings.dedup();
        Ok((preview, warnings))
    }
    pub(super) fn sync_inventory(&mut self) -> Result<(), String> {
        // Editing the first optional device is also supported by legacy profiles.
        if self.inventory.selected.delay.is_empty() && self.stage_configured {
            let config = serde_json::from_value(
                self.settings.raw["instruments"]["delay_stage"]["t"].clone(),
            )
            .map_err(|e| format!("Delay configuration: {e}"))?;
            self.inventory.delays.push(DelayEntry::new(
                "delay-1",
                "Delay 1",
                config,
                self.plan().zero[0],
            ));
            self.inventory.selected.delay = "delay-1".into();
        }
        if self.inventory.selected.scanner.is_empty() && self.scanner_configured.iter().any(|v| *v)
        {
            let parse = |key: &str| {
                self.settings.raw["instruments"]["scanner"]
                    .get(key)
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(|e| format!("Scanner configuration: {e}"))
            };
            self.inventory.scanners.push(ScannerEntry::new(
                "scanner-1",
                "Scanner 1",
                parse("x")?,
                parse("y")?,
                [self.plan().zero[1], self.plan().zero[2]],
            ));
            self.inventory.selected.scanner = "scanner-1".into();
        }
        for (axis, value) in ["t_ps", "x_um", "y_um"]
            .into_iter()
            .zip(self.selected_zero())
        {
            Self::path_set(
                &mut self.settings.raw,
                &["measurements", "move_abs", "zero", axis],
                json!(value),
            );
        }
        self.inventory.absorb_selected(&self.settings.raw)?;
        self.inventory.project_selected(&mut self.settings.raw)
    }

    pub(super) fn select_device(
        &mut self,
        class: DeviceClass,
        mut selected: String,
        new_label: Option<&str>,
    ) {
        if let Err(error) = self.sync_all_plans() {
            self.note(error);
            return;
        }
        let previous_settings = self.settings.clone();
        let result = (|| {
            let previous_ids = self.inventory.selected.clone();
            if let Some(label) = new_label {
                let template = (!selected.is_empty()).then_some(selected.as_str());
                selected = self.add_device(class, template)?;
                self.set_device_label(class, &selected, label.to_owned());
            }
            match class {
                DeviceClass::Lockin => self.inventory.selected.lockin = selected,
                DeviceClass::Delay => self.inventory.selected.delay = selected,
                DeviceClass::Scanner => self.inventory.selected.scanner = selected,
                DeviceClass::PowerMeter => self.inventory.selected.power_meter = selected,
            }
            self.inventory.project_selected(&mut self.settings.raw)?;
            let ids = self.inventory.selected.clone();
            let mut settings = self.settings.clone();
            // Materialize other workflows against the old selection, not
            // the newly added/selected entry.
            if !self.alignment_tab {
                let mut inventory = self.inventory.clone();
                inventory.selected = previous_ids;
                inventory.project_selected(&mut settings.raw)?;
                settings.set_workflow_devices(self.workflow, ids)?;
                settings.select_workflow(self.workflow)?;
            } else {
                let mut inventory = self.inventory.clone();
                inventory.selected = previous_ids.clone();
                inventory.project_selected(&mut settings.raw)?;
                settings.set_workflow_devices(self.workflow, previous_ids)?;
                self.inventory.project_selected(&mut settings.raw)?;
            }
            self.settings = previous_settings.clone();
            self.hydrate();
            self.request_device_selection(settings, self.workflow, None)
        })();
        if let Err(error) = result {
            self.settings = previous_settings;
            self.hydrate();
            self.note(error);
        }
    }

    pub(super) fn device_highlighted(&self, kind: &str, id: &str) -> bool {
        if self.alignment_tab && !self.measurement_pending {
            return self.alignment.highlights(kind, id);
        }
        let workflow = if self.measurement_pending {
            self.active_workflow
        } else {
            self.workflow
        };
        if !measurement_settings::uses_device(workflow, kind) {
            return false;
        }
        let fallback = match kind {
            "lockin" => &self.inventory.selected.lockin,
            "delay" => &self.inventory.selected.delay,
            "scanner" => &self.inventory.selected.scanner,
            "power_meter" => &self.inventory.selected.power_meter,
            _ => return false,
        };
        // Bindings are validated at load/edit time. Rendering a small badge
        // must not deserialize and validate the whole inventory every frame.
        self.settings.raw["measurements"][workflow.key()]["device_ids"][kind]
            .as_str()
            .unwrap_or(fallback)
            == id
    }

    fn add_device(&mut self, class: DeviceClass, template: Option<&str>) -> Result<String, String> {
        let (prefix, ids): (_, Vec<&str>) = match class {
            DeviceClass::Lockin => (
                "lockin",
                self.inventory
                    .lockins
                    .iter()
                    .map(|e| e.id.as_str())
                    .collect(),
            ),
            DeviceClass::Delay => (
                "delay",
                self.inventory
                    .delays
                    .iter()
                    .map(|e| e.id.as_str())
                    .collect(),
            ),
            DeviceClass::Scanner => (
                "scanner",
                self.inventory
                    .scanners
                    .iter()
                    .map(|e| e.id.as_str())
                    .collect(),
            ),
            DeviceClass::PowerMeter => (
                "power-meter",
                self.inventory
                    .power_meters
                    .iter()
                    .map(|e| e.id.as_str())
                    .collect(),
            ),
        };
        let number = (1..)
            .find(|n| !ids.contains(&format!("{prefix}-{n}").as_str()))
            .expect("finite inventory");
        let id = format!("{prefix}-{number}");
        match class {
            DeviceClass::Lockin => {
                let address = (1..=30)
                    .find(|a| !self.inventory.lockins.iter().any(|d| d.address == *a))
                    .ok_or("All GPIB addresses are configured")?;
                let template =
                    template_entry(template, &self.inventory.lockins, |entry| &entry.id)?;
                let mut entry = LockinEntry::new(
                    &id,
                    format!("Lock-in {number}"),
                    template.map_or(self.model, |entry| entry.model),
                    address,
                );
                if let Some(template) = template {
                    entry.settings = template.settings.clone();
                    if let Some(settings) = entry.settings.as_object_mut() {
                        settings.remove("resource");
                    }
                }
                self.inventory.lockins.push(entry);
            }
            DeviceClass::Delay => {
                let template = template_entry(template, &self.inventory.delays, |entry| &entry.id)?;
                let mut config = template
                    .or(self.inventory.delay()?)
                    .map(|entry| entry.config.clone())
                    .unwrap_or_default();
                config.port.clear();
                if template.is_some() {
                    config.zero_pos_mm = None;
                }
                if template.is_none() && config.stage.is_none() {
                    config.stage = trkr_motion::catalog::delay_stages()
                        .into_iter()
                        .find(|s| s.supports_controller(&config.controller))
                        .map(|s| s.name);
                }
                self.inventory.delays.push(DelayEntry::new(
                    &id,
                    format!("Delay {number}"),
                    config,
                    0.,
                ));
            }
            DeviceClass::Scanner => {
                if let Some(template) =
                    template_entry(template, &self.inventory.scanners, |entry| &entry.id)?
                {
                    let mut x = template.x.clone();
                    let mut y = template.y.clone();
                    for axis in [&mut x, &mut y].into_iter().flatten() {
                        axis.port.clear();
                        axis.origin_pos = None;
                    }
                    self.inventory.scanners.push(ScannerEntry::new(
                        &id,
                        format!("Scanner {number}"),
                        x,
                        y,
                        [0.; 2],
                    ));
                    return Ok(id);
                }
                let mut x = self
                    .inventory
                    .scanner()?
                    .and_then(|s| s.x.clone())
                    .unwrap_or_default();
                let mut y = self
                    .inventory
                    .scanner()?
                    .and_then(|s| s.y.clone())
                    .unwrap_or_else(|| x.clone());
                x.port.clear();
                y.port.clear();
                for axis in [&mut x, &mut y] {
                    if axis.actuator.is_none() {
                        axis.actuator =
                            trkr_motion::catalog::scanner_actuators_for(&axis.controller)
                                .into_iter()
                                .next()
                                .map(|a| a.name);
                    }
                }
                self.inventory.scanners.push(ScannerEntry::new(
                    &id,
                    format!("Scanner {number}"),
                    Some(x),
                    Some(y),
                    [0.; 2],
                ));
            }
            DeviceClass::PowerMeter => {
                let mut config =
                    template_entry(template, &self.inventory.power_meters, |entry| &entry.id)?
                        .map(|entry| entry.config.clone())
                        .unwrap_or_default();
                config.resource.clear();
                self.inventory.power_meters.push(PowerMeterEntry {
                    id: id.clone(),
                    name: format!("Power Meter {number}"),
                    label: String::new(),
                    config,
                });
            }
        }
        Ok(id)
    }

    pub(super) fn reverse_device_direction(
        &mut self,
        kind: MotionKind,
        id: &str,
        axis: usize,
    ) -> Result<(), String> {
        if !self.inventory_editable() {
            return Err("Wait for motion to finish before changing Direction".into());
        }
        self.sync();
        self.sync_inventory()?;
        self.controller.send(Command::ReverseDirection {
            kind,
            id: id.to_owned(),
            axis,
        })?;
        self.device_feedback.pending_direction(kind, id, axis);
        Ok(())
    }

    pub(super) fn apply_direction(
        &mut self,
        kind: MotionKind,
        id: &str,
        config: trkr_motion::MotionConfig,
    ) {
        match kind {
            MotionKind::Delay => {
                if let Some(entry) = self.inventory.delays.iter_mut().find(|e| e.id == id)
                    && let Some(config) = config.delay
                {
                    entry.config = config;
                }
            }
            MotionKind::Scanner => {
                if let Some(entry) = self.inventory.scanners.iter_mut().find(|e| e.id == id) {
                    // The worker returns connected axes only. Keep the
                    // configuration of an unconnected sibling axis intact.
                    if let Some(x) = config.x {
                        entry.x = Some(x);
                    }
                    if let Some(y) = config.y {
                        entry.y = Some(y);
                    }
                }
            }
        }
        match self.inventory.project_selected(&mut self.settings.raw) {
            Ok(()) => {
                self.hydrate();
                self.device_feedback.confirm_direction(kind, id);
                self.note("Direction updated · current coordinates preserved");
            }
            Err(error) => {
                for axis in 0..3 {
                    self.device_feedback.fail_direction(kind, id, axis, &error);
                }
                self.note(error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn direction_change_is_rejected_during_demo_or_queued_measurement() {
        let mut app = App::with_context(&egui::Context::default(), true);
        let id = app.inventory.selected.scanner.clone();
        let original = serde_json::to_value(&app.inventory).unwrap();
        assert!(
            app.reverse_device_direction(MotionKind::Scanner, &id, 1)
                .is_err()
        );
        app.demo_viewer = false;
        app.measurement_pending = true;
        assert!(
            app.reverse_device_direction(MotionKind::Scanner, &id, 2)
                .is_err()
        );
        assert_eq!(serde_json::to_value(&app.inventory).unwrap(), original);
        assert!(!app.controller.has_pending());
    }

    #[test]
    fn templates_preserve_card_settings_with_independent_connections_and_zeroes() {
        let mut app = App::with_context(&egui::Context::default(), false);
        app.inventory.lockins[0].settings =
            json!({"test_setting": "copied", "resource": "GPIB0::5::INSTR"});
        let lockin = app.inventory.lockins[0].clone();
        let id = app
            .add_device(DeviceClass::Lockin, Some(&lockin.id))
            .unwrap();
        let copy = app
            .inventory
            .lockins
            .iter()
            .find(|entry| entry.id == id)
            .unwrap();
        assert_eq!(copy.model.name(), lockin.model.name());
        assert_eq!(copy.settings, json!({"test_setting": "copied"}));
        assert_ne!(copy.address, lockin.address);

        let delay = &mut app.inventory.delays[0];
        delay.config.port = "COM80".into();
        delay.config.direction = 1;
        delay.config.zero_pos_mm = Some(2.);
        delay.zero_ps = 5.;
        let delay = delay.clone();
        let id = app.add_device(DeviceClass::Delay, Some(&delay.id)).unwrap();
        let copy = app
            .inventory
            .delays
            .iter()
            .find(|entry| entry.id == id)
            .unwrap();
        let mut expected = delay.config.clone();
        expected.port.clear();
        expected.zero_pos_mm = None;
        assert_eq!(copy.config, expected);
        assert_eq!(copy.zero_ps, 0.);

        let scanner = &mut app.inventory.scanners[0];
        let axis = scanner.x.as_mut().unwrap();
        axis.port = "COM81".into();
        axis.origin_pos = Some(0.5);
        axis.sample_um_per_unit = 1.25;
        scanner.y = None;
        scanner.zero_um = [2., 3.];
        let scanner = scanner.clone();
        let id = app
            .add_device(DeviceClass::Scanner, Some(&scanner.id))
            .unwrap();
        let copy = app
            .inventory
            .scanners
            .iter()
            .find(|entry| entry.id == id)
            .unwrap();
        let mut expected = scanner.x.clone().unwrap();
        expected.port.clear();
        expected.origin_pos = None;
        assert_eq!(copy.x, Some(expected));
        assert!(copy.y.is_none());
        assert_eq!(copy.zero_um, [0.; 2]);

        let meter_id = app.add_device(DeviceClass::PowerMeter, None).unwrap();
        let meter = app.inventory.power_meters.last_mut().unwrap();
        meter.config.resource = "USB0::0x1313::0x8078::TEMPLATE::INSTR".into();
        meter.config.wavelength_nm = 950.;
        meter.config.correction_factor = 2.;
        let id = app
            .add_device(DeviceClass::PowerMeter, Some(&meter_id))
            .unwrap();
        let copy = app
            .inventory
            .power_meters
            .iter()
            .find(|entry| entry.id == id)
            .unwrap();
        assert!(copy.config.resource.is_empty());
        assert_eq!(copy.config.wavelength_nm, 950.);
        assert_eq!(copy.config.correction_factor, 2.);
        assert!(!app.any_hardware_connected());
    }

    #[test]
    fn missing_template_cannot_add_a_card() {
        let mut app = App::with_context(&egui::Context::default(), false);
        let before = app.inventory.ordered_panels();
        assert!(app.add_device(DeviceClass::Delay, Some("missing")).is_err());
        assert_eq!(app.inventory.ordered_panels(), before);
    }

    #[test]
    fn new_power_meter_appears_last_without_overriding_saved_panel_order() {
        let mut app = App::with_context(&egui::Context::default(), false);
        let before = app.inventory.ordered_panels();
        let id = app.add_device(DeviceClass::PowerMeter, None).unwrap();
        let key = crate::inventory::PanelKey::new("power_meter", &id);
        let mut expected = before.clone();
        expected.push(key.clone());
        assert_eq!(app.inventory.ordered_panels(), expected);
        // An explicit drag remains authoritative across profile save/reload.
        assert!(app.inventory.reorder_panel(&key, &before[0], false));
        app.inventory
            .project_selected(&mut app.settings.raw)
            .unwrap();
        let restored = crate::inventory::Inventory::from_profile(&app.settings.raw).unwrap();
        assert_eq!(restored.ordered_panels()[0], key);
    }

    #[test]
    fn reversal_of_connected_x_preserves_unconnected_y_configuration() {
        let mut app = App::with_context(&egui::Context::default(), false);
        let id = app.inventory.selected.scanner.clone();
        let original_y = app.inventory.scanner().unwrap().unwrap().y.clone();
        assert!(original_y.is_some());
        let mut x = app.inventory.scanner().unwrap().unwrap().x.clone().unwrap();
        x.direction = 1;
        x.origin_pos = Some(0.1);
        app.apply_direction(
            MotionKind::Scanner,
            &id,
            trkr_motion::MotionConfig {
                delay: None,
                x: Some(x.clone()),
                y: None,
            },
        );
        let updated = app.inventory.scanner().unwrap().unwrap();
        assert_eq!(updated.x, Some(x));
        assert_eq!(updated.y, original_y);
        assert!(app.settings.raw.pointer("/instruments/scanner/y").is_some());
    }

    #[test]
    fn connection_preview_promotes_first_unconfigured_scanner_without_editing_profile_inventory() {
        let mut app = App::with_context(&egui::Context::default(), false);
        app.settings.raw["instruments"]["scanner"] = serde_json::json!({});
        app.inventory.scanners.clear();
        app.inventory.selected.scanner.clear();
        app.hydrate();
        app.scanner_configured = [true, false];
        app.scanner_port[0] = "COM5".into();

        let (preview, warnings) = app.connection_inventory().unwrap();
        let scanner = preview.scanner().unwrap().unwrap();
        assert_eq!(preview.selected.scanner, "scanner-1");
        assert_eq!(scanner.x.as_ref().unwrap().port, "COM5");
        assert!(scanner.y.is_none());
        assert!(warnings.is_empty());
        assert!(app.inventory.scanners.is_empty());
        assert!(app.inventory.selected.scanner.is_empty());
    }
}
