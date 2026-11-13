//! Inventory editing stays transactional and never opens or moves a device.
use super::devices::DeviceClass;
use super::*;
use crate::inventory::PanelKey;

#[derive(Clone)]
struct PanelDrag {
    key: PanelKey,
    title: String,
}

pub(super) enum InventoryDialog {
    Add {
        class: DeviceClass,
        template: String,
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
            Self::PowerMeter => "power_meter",
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Lockin => "Lock-in",
            Self::Delay => "Delay",
            Self::Scanner => "Scanner",
            Self::PowerMeter => "Power meter",
        }
    }
    fn template_options(self, inventory: &crate::inventory::Inventory) -> Vec<(&str, String)> {
        match self {
            Self::Lockin => inventory
                .lockins
                .iter()
                .map(|entry| (entry.id.as_str(), entry.display_name()))
                .collect(),
            Self::Delay => inventory
                .delays
                .iter()
                .map(|entry| (entry.id.as_str(), entry.display_name()))
                .collect(),
            Self::Scanner => inventory
                .scanners
                .iter()
                .map(|entry| (entry.id.as_str(), entry.display_name()))
                .collect(),
            Self::PowerMeter => inventory
                .power_meters
                .iter()
                .map(|entry| (entry.id.as_str(), entry.display_name()))
                .collect(),
        }
    }

    fn selected(self, ids: &crate::inventory::DeviceIds) -> &str {
        match self {
            Self::Lockin => &ids.lockin,
            Self::Delay => &ids.delay,
            Self::Scanner => &ids.scanner,
            Self::PowerMeter => &ids.power_meter,
        }
    }
    fn replace(self, ids: &mut crate::inventory::DeviceIds, old: &str, new: &str) {
        let id = match self {
            Self::Lockin => &mut ids.lockin,
            Self::Delay => &mut ids.delay,
            Self::Scanner => &mut ids.scanner,
            Self::PowerMeter => &mut ids.power_meter,
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
            DeviceClass::PowerMeter => self
                .inventory
                .power_meters
                .iter_mut()
                .find(|d| d.id == id)
                .map(|d| &mut d.label),
        };
        if let Some(label) = label {
            *label = value;
        }
    }

    pub(super) fn inventory_editable(&self) -> bool {
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
                    DeviceClass::PowerMeter => self
                        .snapshot
                        .power_meters
                        .iter()
                        .find(|status| status.id == id)
                        .is_some_and(|status| status.connected),
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

    pub(super) fn device_add_footer(&mut self, ui: &mut egui::Ui, area: egui::Rect) {
        precision::record(&mut self.geometry, "device_add_footer", area);
        let enabled = self.inventory_editable();
        if ui
            .add_enabled_ui(enabled, |ui| {
                ui.put(area, egui::Button::new("+ Add device").corner_radius(6))
            })
            .inner
            .clicked()
        {
            self.inventory_dialog = Some(InventoryDialog::Add {
                class: DeviceClass::Scanner,
                template: String::new(),
                label: String::new(),
            });
        }
    }

    fn reorder_device_panels(
        &mut self,
        source: &PanelKey,
        target: &PanelKey,
        after: bool,
    ) -> Result<(), String> {
        if !self.inventory_editable() {
            return Err("Wait for the current operation before rearranging devices".into());
        }
        self.sync_all_plans()?;
        if self.inventory.reorder_panel(source, target, after) {
            self.inventory.project_selected(&mut self.settings.raw)?;
        }
        Ok(())
    }

    pub(super) fn device_inventory_cards(
        &mut self,
        ui: &mut egui::Ui,
        origin: egui::Pos2,
        width: f32,
    ) -> f32 {
        use precision::rect;
        let editable = self.inventory_editable();
        if !editable && egui::DragAndDrop::has_payload_of_type::<PanelDrag>(ui.ctx()) {
            egui::DragAndDrop::clear_payload(ui.ctx());
        }
        let order = self.inventory.ordered_panels();
        let mut entries = Vec::new();
        for d in &self.inventory.lockins {
            entries.push((
                DeviceClass::Lockin,
                d.id.clone(),
                d.label_text(),
                d.label.clone(),
                format!("{} · GPIB {}", d.model.name(), d.address),
            ));
        }
        for d in &self.inventory.delays {
            entries.push((
                DeviceClass::Delay,
                d.id.clone(),
                d.label_text(),
                d.label.clone(),
                d.config
                    .stage
                    .clone()
                    .unwrap_or_else(|| d.config.controller.clone()),
            ));
        }
        for d in &self.inventory.scanners {
            entries.push((
                DeviceClass::Scanner,
                d.id.clone(),
                d.label_text(),
                d.label.clone(),
                d.x.as_ref()
                    .or(d.y.as_ref())
                    .map(|c| c.controller.clone())
                    .unwrap_or_else(|| "X / Y".into()),
            ));
        }
        for d in &self.inventory.power_meters {
            entries.push((
                DeviceClass::PowerMeter,
                d.id.clone(),
                d.label_text(),
                d.label.clone(),
                "Thorlabs PM100USB / PM100D · USB".into(),
            ));
        }
        entries.sort_by_key(|(class, id, ..)| {
            order
                .iter()
                .position(|key| key.kind == class.key() && key.id == *id)
                .unwrap_or(usize::MAX)
        });
        let mut y = origin.y;
        let mut chosen = None;
        let mut dropped = None;
        for (class, id, fallback, mut label, model) in entries {
            let key = PanelKey::new(class.key(), &id);
            let selected = class.selected(&self.inventory.selected) == id;
            let active = self.device_highlighted(class.key(), &id);
            let body_height = if selected {
                match class {
                    DeviceClass::Lockin => self.device_height(0),
                    DeviceClass::Delay => self.device_height(1),
                    DeviceClass::Scanner => self.device_height(2) + self.device_height(3) + 28.,
                    DeviceClass::PowerMeter => self.power_meter_height(&id),
                }
            } else {
                62.
            };
            let area = rect(origin.x, y, width, 10. + body_height);
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
            let handle_rect = rect(area.min.x + 6., y + 5., 17., 22.);
            let handle = ui
                .interact(
                    handle_rect,
                    egui::Id::new(("device-drag", &key.kind, &id)),
                    if editable {
                        egui::Sense::drag()
                    } else {
                        egui::Sense::hover()
                    },
                )
                .on_hover_text("Drag to rearrange · order is saved with Config");
            for dx in [5., 10.] {
                for dy in [6., 11., 16.] {
                    ui.painter().circle_filled(
                        handle_rect.min + Vec2::new(dx, dy),
                        1.,
                        if handle.hovered() {
                            TEXT
                        } else {
                            Color32::from_rgb(108, 113, 123)
                        },
                    );
                }
            }
            if editable {
                handle.dnd_set_drag_payload(PanelDrag {
                    key: key.clone(),
                    title: format!("{}: {}", class.title(), fallback),
                });
            }
            let prefix_width = match class {
                DeviceClass::Delay => 42.,
                DeviceClass::PowerMeter => 90.,
                _ => 56.,
            };
            let title_rect = rect(area.min.x + 25., y + 5., area.width() - 95., 23.);
            let font = egui::FontId::proportional(12.);
            let response = ui
                .scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(title_rect)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 2.;
                        ui.add_sized(
                            [prefix_width, title_rect.height()],
                            egui::Label::new(
                                egui::RichText::new(format!("{}:", class.title()))
                                    .font(font.clone())
                                    .color(if active { TEXT } else { Color32::GRAY }),
                            )
                            .halign(egui::Align::Min),
                        );
                        ui.add_enabled(
                            editable,
                            egui::TextEdit::singleline(&mut label)
                                .id_salt(("inventory-label", class.key(), &id))
                                .hint_text(&fallback)
                                .char_limit(64)
                                .font(font)
                                .frame(egui::Frame::NONE)
                                .margin(Vec2::new(3., 0.))
                                .vertical_align(egui::Align::Center)
                                .desired_width(ui.available_width())
                                .min_size(Vec2::new(0., title_rect.height())),
                        )
                    },
                )
                .inner
                .on_hover_text(format!("Rename {}: {}\n{}", class.title(), fallback, id));
            if response.changed() {
                self.set_device_label(class, &id, label);
                self.sync();
                if let Err(error) = self.sync_inventory() {
                    self.note(error);
                }
            }
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
            let body = rect(origin.x, y + 10., width, body_height);
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
                        DeviceClass::PowerMeter => self.power_meter_card(ui, body, &id),
                    }
                });
            } else {
                let connected = self
                    .snapshot
                    .registered_devices
                    .iter()
                    .find(|d| d.kind == class.key() && d.id == id)
                    .is_some_and(|d| d.connected.iter().any(|v| *v));
                let connected = connected
                    || (class == DeviceClass::PowerMeter
                        && self
                            .power_meter_status(&id)
                            .is_some_and(|status| status.connected));
                let mut form = precision::Form::new(ui, &mut self.geometry, area.min);
                form.value(
                    &format!("inventory_{id}_model"),
                    rect(11., 32., width - 98., 20.),
                    &model,
                    Color32::GRAY,
                );
                form.value(
                    &format!("inventory_{id}_connection"),
                    rect(11., 51., width - 98., 16.),
                    if connected {
                        "Connected"
                    } else {
                        "Disconnected"
                    },
                    if connected {
                        Color32::from_rgb(93, 196, 146)
                    } else {
                        Color32::GRAY
                    },
                );
                let response = ui.add_enabled_ui(editable, |ui| {
                    ui.put(
                        rect(origin.x + width - 78., y + 36., 67., 24.),
                        egui::Button::new(if self.alignment_tab { "Details" } else { "Use" }),
                    )
                });
                if response.inner.clicked() {
                    chosen = Some((class, id.clone()));
                }
            }
            if editable
                && let Some(payload) = egui::DragAndDrop::payload::<PanelDrag>(ui.ctx())
                && payload.key != key
                && let Some(pointer) = ui.ctx().pointer_interact_pos()
                && area
                    .expand2(Vec2::new(0., 4.))
                    .intersect(ui.clip_rect())
                    .contains(pointer)
            {
                let after = pointer.y > area.center().y;
                let line_y = if after {
                    area.max.y + 4.
                } else {
                    area.min.y - 4.
                };
                ui.painter().line_segment(
                    [
                        egui::pos2(origin.x, line_y),
                        egui::pos2(origin.x + width, line_y),
                    ],
                    Stroke::new(2_f32, Color32::from_rgb(79, 143, 238)),
                );
                if ui.input(|input| input.pointer.any_released()) {
                    dropped = Some((payload.key.clone(), key.clone(), after));
                    egui::DragAndDrop::clear_payload(ui.ctx());
                }
            }
            y += area.height() + 8.;
        }
        if let Some(payload) = egui::DragAndDrop::payload::<PanelDrag>(ui.ctx())
            && let Some(pointer) = ui.ctx().pointer_interact_pos()
        {
            let clip = ui.clip_rect();
            if clip.contains(pointer) {
                let delta = if pointer.y < clip.min.y + 28. {
                    10.
                } else if pointer.y > clip.max.y - 28. {
                    -10.
                } else {
                    0.
                };
                if delta != 0. {
                    ui.scroll_with_delta(Vec2::new(0., delta));
                    ui.ctx().request_repaint_after(Duration::from_millis(16));
                }
            }
            let painter = ui.ctx().layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::new("device-drag-preview"),
            ));
            let galley = painter.layout_no_wrap(
                payload.title.clone(),
                egui::FontId::proportional(12.),
                TEXT,
            );
            let preview = egui::Rect::from_min_size(
                pointer + Vec2::new(14., 14.),
                galley.size() + Vec2::new(16., 10.),
            );
            painter.rect(
                preview,
                6.,
                PANEL,
                Stroke::new(1_f32, BORDER),
                egui::StrokeKind::Inside,
            );
            painter.galley(preview.min + Vec2::new(8., 5.), galley, TEXT);
        }
        if let Some((source, target, after)) = dropped
            && let Err(error) = self.reorder_device_panels(&source, &target, after)
        {
            self.note(error);
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
            .frame(egui::Frame::popup(&ctx.global_style()).fill(PANEL).inner_margin(16))
            .show(ctx, |ui| {
                ui.set_width(360.);
                match &mut dialog {
                    InventoryDialog::Add { class, template, label } => {
                        ui.strong("Add Device"); ui.add_space(12.);
                        ui.add_enabled_ui(editable, |ui| {
                            let previous_class = *class;
                            ui.horizontal(|ui| { for value in [DeviceClass::Lockin, DeviceClass::Delay, DeviceClass::Scanner, DeviceClass::PowerMeter] { ui.selectable_value(class, value, value.title()); } });
                            if *class != previous_class {
                                template.clear();
                            }
                            let options = class.template_options(&self.inventory);
                            let selected = options.iter().find(|(id, _)| *id == template.as_str())
                                .map_or("Default", |(_, name)| name);
                            ui.add_space(12.);
                            ui.label("Copy settings from");
                            egui::ComboBox::from_id_salt("device-add-template")
                                .width(ui.available_width())
                                .selected_text(selected)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(template, String::new(), "Default");
                                    for (id, name) in options {
                                        ui.selectable_value(template, id.to_owned(), name)
                                            .on_hover_text(id);
                                    }
                                });
                            ui.add_space(12.);
                            ui.label("Label");
                            ui.add(egui::TextEdit::singleline(label).hint_text("Optional name, e.g. Pump").char_limit(64).desired_width(f32::INFINITY));
                            ui.add_space(8.);
                            ui.label(egui::RichText::new("The new card has its own connection settings. Review them before connecting.").small().color(Color32::GRAY));
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
                InventoryDialog::Add {
                    class,
                    template,
                    label,
                } => self.select_device(*class, std::mem::take(template), Some(label.trim())),
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
    let mut alignment =
        crate::alignment_config::AlignmentLayout::from_profile(&settings.raw, &inventory)?;
    match class {
        DeviceClass::Delay => alignment.clear_device(engine::MotionKind::Delay, id),
        DeviceClass::Scanner => alignment.clear_device(engine::MotionKind::Scanner, id),
        DeviceClass::Lockin | DeviceClass::PowerMeter => {}
    }
    candidate.raw["alignment"] = alignment.value();
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
        DeviceClass::PowerMeter => {
            inventory.power_meters.retain(|d| d.id != id);
            inventory
                .power_meters
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
    fn power_meter_binding_is_optional_and_stays_per_workflow() {
        let mut settings = Settings::default();
        let mut inventory = crate::inventory::Inventory::from_profile(&settings.raw).unwrap();
        inventory
            .power_meters
            .push(crate::inventory::PowerMeterEntry {
                id: "power-meter-1".into(),
                name: "Power Meter 1".into(),
                label: String::new(),
                config: crate::power_meter::Config::default(),
            });
        inventory.selected.power_meter = "power-meter-1".into();
        inventory.project_selected(&mut settings.raw).unwrap();
        let mut bound = settings.workflow_device_ids(Workflow::Trkr).unwrap();
        bound.power_meter = "power-meter-1".into();
        settings
            .set_workflow_devices(Workflow::Trkr, bound)
            .unwrap();
        assert!(
            settings
                .workflow_device_ids(Workflow::SignalMonitor)
                .unwrap()
                .power_meter
                .is_empty()
        );
        let removed =
            settings_without_device(&settings, DeviceClass::PowerMeter, "power-meter-1").unwrap();
        assert!(
            removed
                .workflow_device_ids(Workflow::Trkr)
                .unwrap()
                .power_meter
                .is_empty()
        );
    }

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
        let layout =
            crate::alignment_config::AlignmentLayout::from_profile(&settings.raw, &inventory)
                .unwrap();
        settings.raw["alignment"] = layout.value();
        let original = settings.workflow_device_ids(Workflow::Trkr).unwrap();
        let original_wait = settings.plan(Workflow::Trkr).unwrap().interval_s;
        let edited = settings_without_device(&settings, DeviceClass::Scanner, "scanner-1").unwrap();
        let restored_layout = crate::alignment_config::AlignmentLayout::from_profile(
            &edited.raw,
            &crate::inventory::Inventory::from_profile(&edited.raw).unwrap(),
        )
        .unwrap();
        assert_eq!(restored_layout.delays, layout.delays);
        assert_eq!(restored_layout.scanners.len(), 2);
        assert_eq!(restored_layout.scanners[0].id, layout.scanners[0].id);
        assert_eq!(restored_layout.scanners[0].device_id, None);
        assert_eq!(restored_layout.scanners[1], layout.scanners[1]);
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
        assert_eq!(inventory.scanners[0].display_name(), "Scanner: Probe");
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
    fn adding_from_a_card_template_selects_and_persists_a_new_device() {
        let mut app = App::with_context(&egui::Context::default(), false);
        let class = DeviceClass::Scanner;
        let original = app.inventory.scanner().unwrap().unwrap().clone();
        app.select_device(class, original.id.clone(), Some("Probe copy"));
        settle(&mut app);
        let selected = app.inventory.scanner().unwrap().unwrap();
        assert_ne!(selected.id, original.id);
        assert_eq!(selected.label, "Probe copy");
        let mut expected = original.x.clone().unwrap();
        expected.port.clear();
        expected.origin_pos = None;
        assert_eq!(selected.x, Some(expected));
        let persisted = crate::inventory::Inventory::from_profile(&app.settings.raw).unwrap();
        assert_eq!(persisted.selected.scanner, selected.id);
        assert_eq!(persisted.scanners[0].x, original.x);
        assert!(!app.any_hardware_connected());
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
    fn inventory_frame(app: &mut App, ctx: &egui::Context, events: Vec<egui::Event>) {
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(precision::rect(0., 0., 400., 1000.)),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show_inside(ui, |ui| {
                        app.device_inventory_cards(ui, egui::pos2(10., 10.), 350.);
                    });
            },
        );
    }
    fn pointer_button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }
    }

    #[test]
    fn header_labels_accept_typing_and_persist_without_selecting_devices() {
        let ctx = egui::Context::default();
        let mut app = App::with_context(&ctx, false);
        let mut spare = app.inventory.lockins[0].clone();
        spare.id = "lockin-2".into();
        spare.name = "Lock-in 2".into();
        spare.address += 1;
        app.inventory.lockins.push(spare);
        app.inventory
            .project_selected(&mut app.settings.raw)
            .unwrap();
        let selected = app.inventory.selected.clone();
        inventory_frame(&mut app, &ctx, vec![]);
        for id in ["lockin-1", "lockin-2", "delay-1", "scanner-1"] {
            let area = app.geometry[&format!("inventory_{id}")];
            let point = egui::pos2(area[0] + 110., area[1] + 16.);
            inventory_frame(
                &mut app,
                &ctx,
                vec![
                    egui::Event::PointerMoved(point),
                    pointer_button(point, true),
                ],
            );
            inventory_frame(&mut app, &ctx, vec![pointer_button(point, false)]);
            inventory_frame(&mut app, &ctx, vec![egui::Event::Text("Probe".into())]);
        }
        let restored = crate::inventory::Inventory::from_profile(&app.settings.raw).unwrap();
        assert_eq!(restored.lockins[0].label, "Probe");
        assert_eq!(restored.lockins[1].label, "Probe");
        assert_eq!(restored.delays[0].label, "Probe");
        assert_eq!(restored.scanners[0].label, "Probe");
        assert_eq!(restored.selected, selected);
        assert!(!app.controller.has_pending());
    }

    #[test]
    fn dragging_card_handle_reorders_only_on_drop_and_escape_cancels() {
        let ctx = egui::Context::default();
        let mut app = App::with_context(&ctx, false);
        inventory_frame(&mut app, &ctx, vec![]);
        let before = app.inventory.ordered_panels();
        let ids = app.inventory.selected.clone();
        let scanner = app.geometry["inventory_scanner-1"];
        let start = egui::pos2(scanner[0] + 14., scanner[1] + 16.);
        let target = egui::pos2(48., 15.);
        inventory_frame(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(start),
                pointer_button(start, true),
            ],
        );
        inventory_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(target)]);
        assert_eq!(app.inventory.ordered_panels(), before);
        assert!(egui::DragAndDrop::has_payload_of_type::<PanelDrag>(&ctx));
        inventory_frame(
            &mut app,
            &ctx,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        inventory_frame(&mut app, &ctx, vec![pointer_button(target, false)]);
        assert_eq!(app.inventory.ordered_panels(), before);
        inventory_frame(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(start),
                pointer_button(start, true),
            ],
        );
        inventory_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(target)]);
        inventory_frame(&mut app, &ctx, vec![pointer_button(target, false)]);
        assert_eq!(
            app.inventory.ordered_panels()[0],
            PanelKey::new("scanner", "scanner-1")
        );
        assert_eq!(app.inventory.selected, ids);
        assert!(!app.controller.has_pending());
        for workflow in Workflow::ALL {
            assert_eq!(app.settings.workflow_device_ids(workflow).unwrap(), ids);
        }
        let restored = Settings::from_value(app.settings.raw.clone()).unwrap();
        assert_eq!(
            crate::inventory::Inventory::from_profile(&restored.raw)
                .unwrap()
                .ordered_panels(),
            app.inventory.ordered_panels()
        );
    }
}
