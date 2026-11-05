//! Inventory editing stays transactional and never opens or moves a device.
use super::devices::DeviceClass;
use super::*;

pub(super) enum InventoryDialog {
    Add {
        class: DeviceClass,
        label: String,
    },
    Delete {
        class: DeviceClass,
        id: String,
        error: Option<String>,
    },
}

impl DeviceClass {
    fn key(self) -> &'static str {
        match self {
            Self::Lockin => "lockin",
            Self::Delay => "delay",
            Self::Scanner => "scanner",
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Lockin => "Lock-in",
            Self::Delay => "Delay",
            Self::Scanner => "Scanner",
        }
    }
    fn selected(self, ids: &crate::inventory::DeviceIds) -> &str {
        match self {
            Self::Lockin => &ids.lockin,
            Self::Delay => &ids.delay,
            Self::Scanner => &ids.scanner,
        }
    }
    fn replace(self, ids: &mut crate::inventory::DeviceIds, old: &str, new: &str) {
        let id = match self {
            Self::Lockin => &mut ids.lockin,
            Self::Delay => &mut ids.delay,
            Self::Scanner => &mut ids.scanner,
        };
        if id == old {
            *id = new.to_owned();
        }
    }
}

impl App {
    pub(super) fn set_device_label(&mut self, class: DeviceClass, id: &str, value: String) {
        let label = match class {
            DeviceClass::Lockin => self
                .inventory
                .lockins
                .iter_mut()
                .find(|d| d.id == id)
                .map(|d| &mut d.label),
            DeviceClass::Delay => self
                .inventory
                .delays
                .iter_mut()
                .find(|d| d.id == id)
                .map(|d| &mut d.label),
            DeviceClass::Scanner => self
                .inventory
                .scanners
                .iter_mut()
                .find(|d| d.id == id)
                .map(|d| &mut d.label),
        };
        if let Some(label) = label {
            *label = value;
        }
    }

    fn inventory_editable(&self) -> bool {
        !self.demo_viewer
            && !self.python_locked()
            && !self.snapshot.running
            && !self.measurement_pending
            && !self.controller.has_pending()
            && self.pending_device_selection.is_none()
    }

    fn deletion_error(&self, class: DeviceClass, id: &str) -> Option<String> {
        if self.demo_viewer {
            return Some("Device editing is unavailable in the Demo viewer.".into());
        }
        if !self.inventory_editable() {
            return Some("Wait for the current operation to finish.".into());
        }
        let connected = self
            .snapshot
            .registered_devices
            .iter()
            .any(|d| d.kind == class.key() && d.id == id && d.connected.iter().any(|v| *v))
            || (class.selected(&self.inventory.selected) == id
                && match class {
                    DeviceClass::Lockin => self.snapshot.devices[0],
                    DeviceClass::Delay => self.snapshot.devices[1],
                    DeviceClass::Scanner => self.snapshot.devices[2] || self.snapshot.devices[3],
                });
        if connected {
            return Some("Disconnect this device before deleting it.".into());
        }
        if class == DeviceClass::Lockin && self.inventory.lockins.len() == 1 {
            return Some(
                "Keep one lock-in in the measurement profile. Add a replacement first.".into(),
            );
        }
        None
    }

    fn delete_device(&mut self, class: DeviceClass, id: &str) -> Result<(), String> {
        if let Some(error) = self.deletion_error(class, id) {
            return Err(error);
        }
        self.sync_all_plans()?;
        let mut candidate = settings_without_device(&self.settings, class, id)?;
        if !self.alignment_tab {
            candidate.select_workflow(self.workflow)?;
        }
        self.request_device_selection(candidate, self.workflow, None)
    }

    pub(super) fn device_inventory_cards(
        &mut self,
        ui: &mut egui::Ui,
        origin: egui::Pos2,
        width: f32,
    ) -> f32 {
        use precision::rect;
        let editable = self.inventory_editable();
        ui.painter().text(
            origin + Vec2::new(2., 13.),
            egui::Align2::LEFT_CENTER,
            "Devices",
            egui::FontId::proportional(12.),
            Color32::GRAY,
        );
        if ui
            .add_enabled_ui(editable, |ui| {
                ui.put(
                    rect(origin.x + width - 66., origin.y, 66., 26.),
                    egui::Button::new("+ Add"),
                )
            })
            .inner
            .clicked()
        {
            self.inventory_dialog = Some(InventoryDialog::Add {
                class: DeviceClass::Scanner,
                label: String::new(),
            });
        }
        let mut entries = Vec::new();
        for (index, d) in self.inventory.lockins.iter().enumerate() {
            entries.push((
                DeviceClass::Lockin,
                d.id.clone(),
                numbered_title(DeviceClass::Lockin, &d.id, index),
                d.label.clone(),
                format!("{} · GPIB {}", d.model.name(), d.address),
            ));
        }
        for (index, d) in self.inventory.delays.iter().enumerate() {
            entries.push((
                DeviceClass::Delay,
                d.id.clone(),
                numbered_title(DeviceClass::Delay, &d.id, index),
                d.label.clone(),
                d.config
                    .stage
                    .clone()
                    .unwrap_or_else(|| d.config.controller.clone()),
            ));
        }
        for (index, d) in self.inventory.scanners.iter().enumerate() {
            entries.push((
                DeviceClass::Scanner,
                d.id.clone(),
                numbered_title(DeviceClass::Scanner, &d.id, index),
                d.label.clone(),
                d.x.as_ref()
                    .or(d.y.as_ref())
                    .map(|c| c.controller.clone())
                    .unwrap_or_else(|| "X / Y".into()),
            ));
        }
        let mut y = origin.y + 34.;
        let mut chosen = None;
        for (class, id, title, mut label, model) in entries {
            let selected = class.selected(&self.inventory.selected) == id;
            let active = self.device_highlighted(class.key(), &id);
            let body_height = if selected {
                match class {
                    DeviceClass::Lockin => self.device_height(0),
                    DeviceClass::Delay => self.device_height(1),
                    DeviceClass::Scanner => self.device_height(2) + self.device_height(3) + 28.,
                }
            } else {
                60.
            };
            let area = rect(origin.x, y, width, 36. + body_height);
            precision::record(&mut self.geometry, &format!("inventory_{id}"), area);
            ui.painter().rect(
                area,
                10.,
                PANEL,
                Stroke::new(
                    1_f32,
                    if active {
                        Color32::from_rgb(79, 106, 141)
                    } else {
                        BORDER
                    },
                ),
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                egui::pos2(area.min.x + 11., y + 17.),
                egui::Align2::LEFT_CENTER,
                &title,
                egui::FontId::proportional(12.),
                if active { TEXT } else { Color32::GRAY },
            );
            if ui
                .add_enabled_ui(editable, |ui| {
                    ui.put(
                        rect(area.max.x - 64., y + 5., 53., 23.),
                        egui::Button::new("Delete").small(),
                    )
                })
                .inner
                .clicked()
            {
                self.inventory_dialog = Some(InventoryDialog::Delete {
                    class,
                    id: id.clone(),
                    error: None,
                });
            }
            let response = ui
                .add_enabled_ui(editable, |ui| {
                    ui.put(
                        rect(area.min.x + 11., y + 31., width - 22., 23.),
                        egui::TextEdit::singleline(&mut label)
                            .id_salt(("inventory-label", &id))
                            .hint_text("Label (optional)")
                            .char_limit(64),
                    )
                })
                .inner;
            if response.changed() {
                self.set_device_label(class, &id, label);
                self.sync();
                if let Err(error) = self.sync_inventory() {
                    self.note(error);
                }
            }
            let body = rect(origin.x, y + 36., width, body_height);
            if selected {
                ui.scope(|ui| {
                    if !active {
                        ui.multiply_opacity(0.65);
                    }
                    match class {
                        DeviceClass::Lockin => self.lockin(ui, body),
                        DeviceClass::Delay => self.motion(ui, 0, "Delay Stage", Device::T, body),
                        DeviceClass::Scanner => {
                            precision::record(&mut self.geometry, "scanner_group", body);
                            // X/Y remain adjacent controls belonging to one scanner.
                            self.motion(
                                ui,
                                1,
                                "X",
                                Device::X,
                                rect(body.min.x, body.min.y + 20., width, self.device_height(2)),
                            );
                            let second_y = body.min.y + self.device_height(2) + 28.;
                            ui.painter().line_segment(
                                [
                                    egui::pos2(origin.x + 11., second_y - 4.),
                                    egui::pos2(origin.x + width - 11., second_y - 4.),
                                ],
                                Stroke::new(1_f32, BORDER),
                            );
                            self.motion(
                                ui,
                                2,
                                "Y",
                                Device::Y,
                                rect(origin.x, second_y, width, self.device_height(3)),
                            );
                        }
                    }
                });
            } else {
                let connected = self
                    .snapshot
                    .registered_devices
                    .iter()
                    .find(|d| d.kind == class.key() && d.id == id)
                    .is_some_and(|d| d.connected.iter().any(|v| *v));
                ui.put(
                    rect(origin.x + 11., y + 61., width - 93., 23.),
                    egui::Label::new(egui::RichText::new(model).small().color(Color32::GRAY))
                        .truncate(),
                );
                let response = ui.add_enabled_ui(editable, |ui| {
                    ui.put(
                        rect(origin.x + width - 78., y + 61., 67., 24.),
                        egui::Button::new(if self.alignment_tab { "Details" } else { "Use" }),
                    )
                });
                if response.inner.clicked() {
                    chosen = Some((class, id.clone()));
                }
                ui.put(
                    rect(origin.x + 11., y + 80., width - 93., 15.),
                    egui::Label::new(
                        egui::RichText::new(if connected {
                            "Connected"
                        } else {
                            "Disconnected"
                        })
                        .size(10.)
                        .color(if connected {
                            Color32::from_rgb(93, 196, 146)
                        } else {
                            Color32::GRAY
                        }),
                    ),
                );
            }
            y += area.height() + 8.;
        }
        if let Some((class, id)) = chosen {
            self.select_device(class, id, None);
        }
        y
    }

    pub(super) fn device_inventory_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.inventory_dialog.take() else {
            return;
        };
        let mut accepted = false;
        let mut cancelled = false;
        let editable = self.inventory_editable();
        let response = egui::Modal::new(egui::Id::new("device-inventory-dialog"))
            .frame(egui::Frame::popup(&ctx.style()).fill(PANEL).inner_margin(16))
            .show(ctx, |ui| {
                ui.set_width(360.);
                match &mut dialog {
                    InventoryDialog::Add { class, label } => {
                        ui.strong("Add Device"); ui.add_space(12.);
                        ui.add_enabled_ui(editable, |ui| {
                            ui.horizontal(|ui| { for value in [DeviceClass::Lockin, DeviceClass::Delay, DeviceClass::Scanner] { ui.selectable_value(class, value, value.title()); } });
                            ui.add_space(12.);
                            ui.label("Label");
                            ui.add(egui::TextEdit::singleline(label).hint_text("Optional name, e.g. Pump").char_limit(64).desired_width(f32::INFINITY));
                            ui.add_space(8.);
                            ui.label(egui::RichText::new("Numbered automatically. Configure the connection in its device card.").small().color(Color32::GRAY));
                        });
                    }
                    InventoryDialog::Delete { class, id, error } => {
                        ui.strong("Delete Device?"); ui.add_space(12.);
                        ui.label(format!("Remove {} from this configuration?", id));
                        ui.label("Measurements using it will select the next device of the same type, or remain unconfigured if none is available.");
                        ui.add_space(8.);
                        if let Some(reason) = self.deletion_error(*class, id) { ui.colored_label(Color32::LIGHT_RED, reason); }
                        if let Some(error) = error { ui.colored_label(Color32::LIGHT_RED, error.as_str()); }
                    }
                }
                ui.add_space(16.);
                let (enabled, title) = match &dialog { InventoryDialog::Add { .. } => (editable, "Add"), InventoryDialog::Delete { class, id, .. } => (self.deletion_error(*class,id).is_none(), "Delete") };
                ui.horizontal(|ui| {
                    cancelled = ui.add(egui::Button::new("Cancel").min_size(Vec2::new(80.,26.))).clicked();
                    accepted = ui.add_enabled(enabled, egui::Button::new(title).min_size(Vec2::new(80.,26.))).clicked();
                });
            });
        if accepted {
            match &mut dialog {
                InventoryDialog::Add { class, label } => {
                    self.select_device(*class, String::new(), Some(label.trim()))
                }
                InventoryDialog::Delete { class, id, error } => {
                    if let Err(message) = self.delete_device(*class, id) {
                        *error = Some(message);
                        self.inventory_dialog = Some(dialog);
                    }
                }
            }
        } else if !cancelled && !response.should_close() {
            self.inventory_dialog = Some(dialog);
        }
    }
}

fn numbered_title(class: DeviceClass, id: &str, index: usize) -> String {
    let number = id
        .strip_prefix(&format!("{}-", class.key()))
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(index + 1);
    format!("{} {number}", class.title())
}

fn settings_without_device(
    settings: &Settings,
    class: DeviceClass,
    id: &str,
) -> Result<Settings, String> {
    let mut candidate = settings.clone();
    // Materialize legacy fallbacks before changing or removing their targets.
    let bindings: Vec<_> = Workflow::ALL
        .into_iter()
        .map(|w| settings.workflow_device_ids(w).map(|ids| (w, ids)))
        .collect::<Result<_, _>>()?;
    let mut inventory = crate::inventory::Inventory::from_profile(&settings.raw)?;
    let replacement = match class {
        DeviceClass::Lockin => {
            inventory.lockins.retain(|d| d.id != id);
            inventory
                .lockins
                .first()
                .map(|d| d.id.clone())
                .ok_or("Keep at least one lock-in")?
        }
        DeviceClass::Delay => {
            inventory.delays.retain(|d| d.id != id);
            inventory
                .delays
                .first()
                .map(|d| d.id.clone())
                .unwrap_or_default()
        }
        DeviceClass::Scanner => {
            inventory.scanners.retain(|d| d.id != id);
            inventory
                .scanners
                .first()
                .map(|d| d.id.clone())
                .unwrap_or_default()
        }
    };
    class.replace(&mut inventory.selected, id, &replacement);
    for (workflow, mut ids) in bindings {
        class.replace(&mut ids, id, &replacement);
        App::path_set(
            &mut candidate.raw,
            &["measurements", workflow.key(), "device_ids"],
            json!(ids),
        );
    }
    inventory.project_selected(&mut candidate.raw)?;
    Settings::from_value(candidate.raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removing_a_scanner_repairs_every_binding_without_changing_other_devices_or_waits() {
        let mut settings = Settings::default();
        let mut inventory = crate::inventory::Inventory::from_profile(&settings.raw).unwrap();
        let mut second = inventory.scanners[0].clone();
        second.id = "scanner-2".into();
        second.name = "Scanner 2".into();
        second.label = "Probe".into();
        second.x.as_mut().unwrap().port.clear();
        second.y.as_mut().unwrap().port.clear();
        second.zero_um = [12., -8.];
        inventory.scanners.push(second);
        inventory.project_selected(&mut settings.raw).unwrap();
        let original = settings.workflow_device_ids(Workflow::Trkr).unwrap();
        let original_wait = settings.plan(Workflow::Trkr).unwrap().interval_s;
        let edited = settings_without_device(&settings, DeviceClass::Scanner, "scanner-1").unwrap();
        for workflow in Workflow::ALL {
            let ids = edited.workflow_device_ids(workflow).unwrap();
            assert_eq!(ids.scanner, "scanner-2");
            assert_eq!(ids.lockin, original.lockin);
            assert_eq!(ids.delay, original.delay);
            assert_eq!(edited.plan(workflow).unwrap().zero[1..], [12., -8.]);
        }
        assert_eq!(
            edited.plan(Workflow::Trkr).unwrap().interval_s,
            original_wait
        );
        let restored = Settings::from_value(edited.raw.clone()).unwrap();
        let inventory = crate::inventory::Inventory::from_profile(&restored.raw).unwrap();
        assert_eq!(inventory.scanners[0].label, "Probe");
        assert_eq!(inventory.scanners[0].display_name(), "Scanner 2 · Probe");
        let empty = settings_without_device(&edited, DeviceClass::Scanner, "scanner-2").unwrap();
        assert!(empty.raw.pointer("/instruments/scanner/x").is_none());
        assert!(empty.raw.pointer("/instruments/scanner/y").is_none());
        for workflow in Workflow::ALL {
            assert!(
                empty
                    .workflow_device_ids(workflow)
                    .unwrap()
                    .scanner
                    .is_empty()
            );
        }
        assert_eq!(
            settings.workflow_device_ids(Workflow::Trkr).unwrap(),
            original
        );
    }

    fn settle(app: &mut App) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while app.pending_device_selection.is_some() {
            app.receive();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn added_motion_devices_can_be_edited_and_saved_but_not_connected_without_ports() {
        let mut app = App::with_context(&egui::Context::default(), false);
        for class in [DeviceClass::Scanner, DeviceClass::Delay] {
            app.select_device(class, String::new(), Some("Probe"));
            settle(&mut app);
            assert_eq!(
                class.selected(&app.inventory.selected),
                format!("{}-2", class.key())
            );
            assert!(
                app.settings
                    .hardware()
                    .unwrap_err()
                    .contains("port is required")
            );
            app.sync_all_plans().unwrap();
            let dir = tempfile::tempdir().unwrap();
            let file = dir.path().join("draft.json");
            app.settings.save(&file).unwrap();
            let restored = Settings::load(&file).unwrap();
            assert!(
                restored
                    .hardware()
                    .unwrap_err()
                    .contains("port is required")
            );
            assert_eq!(
                restored.raw["device_inventory"][if class == DeviceClass::Scanner {
                    "scanners"
                } else {
                    "delays"
                }][1]["label"],
                "Probe"
            );
        }
        // Missing ports do not bypass any scientific calibration validation.
        app.settings.raw["instruments"]["scanner"]["x"]["sample_um_per_unit"] = json!(0.);
        assert!(app.settings.profile_hardware().is_err());
    }

    #[test]
    fn deletion_projects_the_measurement_binding_and_preserves_alignment_inspection() {
        let mut app = App::with_context(&egui::Context::default(), false);
        app.select_device(DeviceClass::Scanner, String::new(), Some("Two"));
        settle(&mut app);
        app.select_device(DeviceClass::Scanner, String::new(), Some("Three"));
        settle(&mut app);
        let mut ids = app.inventory.selected.clone();
        ids.scanner = "scanner-1".into();
        app.settings
            .set_workflow_devices(app.workflow, ids)
            .unwrap();
        // Canonical inspection and the tab binding differ, as in Alignment.
        assert_eq!(app.inventory.selected.scanner, "scanner-3");
        app.delete_device(DeviceClass::Scanner, "scanner-2")
            .unwrap();
        settle(&mut app);
        assert_eq!(app.inventory.selected.scanner, "scanner-1");
        app.alignment_tab = true;
        app.select_device(DeviceClass::Scanner, "scanner-3".into(), None);
        settle(&mut app);
        app.delete_device(DeviceClass::Scanner, "scanner-1")
            .unwrap();
        settle(&mut app);
        assert_eq!(app.inventory.selected.scanner, "scanner-3");
        assert!(app.alignment_tab);
    }

    #[test]
    fn connected_device_and_owned_gui_cannot_be_deleted() {
        let mut app = App::with_context(&egui::Context::default(), false);
        app.snapshot.devices[2] = true;
        let id = app.inventory.selected.scanner.clone();
        assert!(
            app.delete_device(DeviceClass::Scanner, &id)
                .unwrap_err()
                .contains("Disconnect")
        );
        app.snapshot.devices[2] = false;
        app.snapshot.running = true;
        assert!(
            app.delete_device(DeviceClass::Scanner, &id)
                .unwrap_err()
                .contains("Wait")
        );
        assert!(app.pending_device_selection.is_none());
    }

    #[test]
    fn each_workflow_exposes_only_required_roles() {
        for workflow in Workflow::ALL {
            assert!(measurement_settings::uses_device(workflow, "lockin"));
        }
        for (workflow, delay, scanner) in [
            (Workflow::SignalMonitor, false, false),
            (Workflow::Trkr, true, false),
            (Workflow::Srkr, false, true),
            (Workflow::Strkr, true, true),
            (Workflow::Srkr2d, false, true),
        ] {
            assert_eq!(measurement_settings::uses_device(workflow, "delay"), delay);
            assert_eq!(
                measurement_settings::uses_device(workflow, "scanner"),
                scanner
            );
        }
    }
}
