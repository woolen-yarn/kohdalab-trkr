use super::*;
use crate::inventory::{DelayEntry, LockinEntry, ScannerEntry};
use engine::MotionKind;

#[derive(Clone, Copy)]
pub(super) enum DeviceClass {
    Lockin,
    Delay,
    Scanner,
}

impl App {
    pub(super) fn any_hardware_connected(&self) -> bool {
        self.snapshot.devices.iter().any(|v| *v)
            || self
                .snapshot
                .registered_devices
                .iter()
                .any(|d| d.connected.iter().any(|v| *v))
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
        for (axis, value) in ["t_ps", "x_um", "y_um"].into_iter().zip(self.plan().zero) {
            Self::path_set(
                &mut self.settings.raw,
                &["measurements", "move_abs", "zero", axis],
                json!(value),
            );
        }
        self.inventory.absorb_selected(&self.settings.raw)?;
        self.inventory.project_selected(&mut self.settings.raw)
    }

    fn inventory_config(&self) -> Result<engine::HardwareConfig, String> {
        let lockin = self.inventory.lockin()?;
        let scanner = self.inventory.scanner()?;
        Ok(engine::HardwareConfig {
            device_ids: self.inventory.selected.clone(),
            model: lockin.model,
            address: lockin.address,
            motion: trkr_motion::MotionConfig {
                delay: self.inventory.delay()?.map(|d| d.config.clone()),
                x: scanner.and_then(|s| s.x.clone()),
                y: scanner.and_then(|s| s.y.clone()),
            },
        })
    }

    pub(super) fn device_selector(
        &mut self,
        ui: &mut egui::Ui,
        class: DeviceClass,
        area: egui::Rect,
    ) {
        let (salt, mut selected, entries): (_, _, Vec<_>) = match class {
            DeviceClass::Lockin => (
                "lockin",
                self.inventory.selected.lockin.clone(),
                self.inventory
                    .lockins
                    .iter()
                    .map(|d| (d.id.clone(), d.name.clone()))
                    .collect(),
            ),
            DeviceClass::Delay => (
                "delay",
                self.inventory.selected.delay.clone(),
                self.inventory
                    .delays
                    .iter()
                    .map(|d| (d.id.clone(), d.name.clone()))
                    .collect(),
            ),
            DeviceClass::Scanner => (
                "scanner",
                self.inventory.selected.scanner.clone(),
                self.inventory
                    .scanners
                    .iter()
                    .map(|d| (d.id.clone(), d.name.clone()))
                    .collect(),
            ),
        };
        let old = selected.clone();
        let mut add = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(area), |ui| {
            ui.horizontal(|ui| {
                ui.add_enabled_ui(
                    !self.demo_viewer && !self.snapshot.running && !self.controller.has_pending(),
                    |ui| {
                        egui::ComboBox::from_id_salt(("device-instance", salt))
                            .width((area.width() - 84.).max(100.))
                            .selected_text(
                                entries
                                    .iter()
                                    .find(|d| d.0 == selected)
                                    .map(|d| d.1.as_str())
                                    .unwrap_or("Not configured"),
                            )
                            .show_ui(ui, |ui| {
                                for (id, name) in &entries {
                                    ui.selectable_value(&mut selected, id.clone(), name);
                                }
                            });
                        add = ui.small_button("+ Add").clicked();
                    },
                );
            });
        });
        if selected != old || add {
            self.sync();
            if let Err(error) = self.sync_inventory() {
                self.note(error);
                return;
            }
            let previous_settings = self.settings.clone();
            let result = (|| {
                if add {
                    selected = self.add_device(class)?;
                }
                match class {
                    DeviceClass::Lockin => self.inventory.selected.lockin = selected,
                    DeviceClass::Delay => self.inventory.selected.delay = selected,
                    DeviceClass::Scanner => self.inventory.selected.scanner = selected,
                }
                self.inventory.project_selected(&mut self.settings.raw)?;
                self.hydrate();
                self.controller.send(Command::Select {
                    config: self.inventory_config()?,
                })?;
                self.live.clear();
                self.snapshot.signal = None;
                self.snapshot.devices = [false; 4];
                self.snapshot.position = Default::default();
                Ok::<(), String>(())
            })();
            if let Err(error) = result {
                self.settings = previous_settings;
                self.hydrate();
                self.note(error);
            }
        }
    }

    fn add_device(&mut self, class: DeviceClass) -> Result<String, String> {
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
                self.inventory.lockins.push(LockinEntry::new(
                    &id,
                    format!("Lock-in {number}"),
                    self.model,
                    address,
                ));
            }
            DeviceClass::Delay => {
                let mut config = self
                    .inventory
                    .delay()?
                    .map(|d| d.config.clone())
                    .unwrap_or_default();
                config.port.clear();
                config.stage.get_or_insert_with(|| "SGSP46-500".into());
                self.inventory.delays.push(DelayEntry::new(
                    &id,
                    format!("Delay {number}"),
                    config,
                    0.,
                ));
            }
            DeviceClass::Scanner => {
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
                x.actuator.get_or_insert_with(|| "TRA12CC".into());
                y.actuator.get_or_insert_with(|| "TRA12CC".into());
                self.inventory.scanners.push(ScannerEntry::new(
                    &id,
                    format!("Scanner {number}"),
                    Some(x),
                    Some(y),
                    [0.; 2],
                ));
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
        if self.snapshot.running || self.controller.has_pending() {
            return Err("Wait for motion to finish before changing Direction".into());
        }
        self.sync();
        self.sync_inventory()?;
        self.controller.send(Command::ReverseDirection {
            kind,
            id: id.to_owned(),
            axis,
        })
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
                self.note("Direction updated · current coordinates preserved");
            }
            Err(error) => self.note(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
