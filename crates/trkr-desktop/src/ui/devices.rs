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
                            .truncate()
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
            self.select_device(class, selected, add);
        }
    }

    fn select_device(&mut self, class: DeviceClass, mut selected: String, add: bool) {
        if let Err(error) = self.sync_all_plans() {
            self.note(error);
            return;
        }
        let previous_settings = self.settings.clone();
        let result = (|| {
            let previous_ids = self.inventory.selected.clone();
            if add {
                selected = self.add_device(class)?;
            }
            match class {
                DeviceClass::Lockin => self.inventory.selected.lockin = selected,
                DeviceClass::Delay => self.inventory.selected.delay = selected,
                DeviceClass::Scanner => self.inventory.selected.scanner = selected,
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
        let fallback = match kind {
            "lockin" => &self.inventory.selected.lockin,
            "delay" => &self.inventory.selected.delay,
            "scanner" => &self.inventory.selected.scanner,
            _ => return false,
        };
        // Bindings are validated at load/edit time. Rendering a small badge
        // must not deserialize and validate the whole inventory every frame.
        self.settings.raw["measurements"][workflow.key()]["device_ids"][kind]
            .as_str()
            .unwrap_or(fallback)
            == id
    }

    pub(super) fn inactive_device_cards(
        &mut self,
        ui: &mut egui::Ui,
        origin: egui::Pos2,
        width: f32,
    ) -> f32 {
        let mut entries = Vec::new();
        for d in &self.inventory.lockins {
            if d.id != self.inventory.selected.lockin {
                entries.push((
                    DeviceClass::Lockin,
                    "lockin",
                    d.id.clone(),
                    d.name.clone(),
                    format!("{} · GPIB {}", d.model.name(), d.address),
                ));
            }
        }
        for d in &self.inventory.delays {
            if d.id != self.inventory.selected.delay {
                entries.push((
                    DeviceClass::Delay,
                    "delay",
                    d.id.clone(),
                    d.name.clone(),
                    d.config
                        .stage
                        .clone()
                        .unwrap_or_else(|| d.config.controller.clone()),
                ));
            }
        }
        for s in &self.inventory.scanners {
            if s.id != self.inventory.selected.scanner {
                let label =
                    s.x.as_ref()
                        .or(s.y.as_ref())
                        .map(|c| c.controller.as_str())
                        .unwrap_or("X / Y");
                entries.push((
                    DeviceClass::Scanner,
                    "scanner",
                    s.id.clone(),
                    s.name.clone(),
                    label.into(),
                ));
            }
        }
        let mut y = origin.y;
        let mut chosen = None;
        for (class, kind, id, name, label) in entries {
            let active = self.device_highlighted(kind, &id);
            let connected = self
                .snapshot
                .registered_devices
                .iter()
                .find(|d| d.kind == kind && d.id == id)
                .is_some_and(|d| d.connected.iter().any(|v| *v));
            let area = precision::rect(origin.x, y, width, 76.);
            precision::record(&mut self.geometry, &format!("inventory_{id}"), area);
            ui.painter().rect(
                area,
                10.,
                if active {
                    PANEL
                } else {
                    PANEL.gamma_multiply(0.9)
                },
                Stroke::new(
                    1_f32,
                    if active {
                        BORDER
                    } else {
                        BORDER.gamma_multiply(0.62)
                    },
                ),
                egui::StrokeKind::Inside,
            );
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(area.shrink2(Vec2::new(10., 6.)))
                    .layout(egui::Layout::top_down(egui::Align::Min)),
                |ui| {
                    let text = if active {
                        TEXT
                    } else {
                        Color32::from_rgb(153, 157, 168)
                    };
                    ui.horizontal(|ui| {
                        ui.add(egui::Label::new(egui::RichText::new(&name).color(text)).truncate())
                            .on_hover_text(&name);
                        ui.label(
                            egui::RichText::new(if active {
                                if self.alignment_tab {
                                    "ALIGNMENT"
                                } else {
                                    "IN USE"
                                }
                            } else {
                                "INACTIVE"
                            })
                            .size(9.)
                            .color(text),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_enabled(
                                    !self.snapshot.running
                                        && !self.controller.has_pending()
                                        && !self.demo_viewer,
                                    egui::Button::new(if self.alignment_tab {
                                        "Details"
                                    } else {
                                        "Use"
                                    })
                                    .small(),
                                )
                                .clicked()
                            {
                                chosen = Some((class, id.clone()));
                            }
                        });
                    });
                    ui.add(
                        egui::Label::new(egui::RichText::new(label).small().color(text)).truncate(),
                    );
                    ui.label(
                        egui::RichText::new(if connected {
                            "Connected"
                        } else {
                            "Disconnected"
                        })
                        .small()
                        .color(if connected {
                            Color32::from_rgb(93, 196, 146)
                        } else {
                            text
                        }),
                    );
                },
            );
            y += 84.;
        }
        if let Some((class, id)) = chosen {
            self.select_device(class, id, false);
        }
        y
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
