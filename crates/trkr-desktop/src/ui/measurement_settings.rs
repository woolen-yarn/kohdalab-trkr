//! Per-workflow device roles. Configuration is staged until OK; the worker
//! remains the sole owner of all connections, including inactive instruments.
use super::*;
use crate::inventory::DeviceIds;

pub(super) struct Draft {
    workflow: Workflow,
    ids: DeviceIds,
    error: Option<String>,
}

pub(super) struct PendingSelection {
    id: String,
    previous: Settings,
    previous_workflow: Workflow,
    requested_workflow: Workflow,
    requested_ids: DeviceIds,
}

/// Roles actively required by a measurement; unshown position-context bindings
/// remain intact so CSV readbacks retain their configured physical coordinates.
pub(super) fn uses_device(workflow: Workflow, kind: &str) -> bool {
    match kind {
        "lockin" => true,
        "delay" => matches!(workflow, Workflow::Trkr | Workflow::Strkr),
        "scanner" => matches!(
            workflow,
            Workflow::Srkr | Workflow::Strkr | Workflow::Srkr2d
        ),
        "power_meter" => true,
        _ => false,
    }
}

impl App {
    pub(super) fn selected_zero(&self) -> [f64; 3] {
        let delay = self.inventory.delay().ok().flatten();
        let scanner = self.inventory.scanner().ok().flatten();
        [
            delay.map_or(0., |d| d.zero_ps),
            scanner.map_or(0., |s| s.zero_um[0]),
            scanner.map_or(0., |s| s.zero_um[1]),
        ]
    }

    pub(super) fn set_selected_zero(&mut self, axis: usize, value: f64) {
        self.sync();
        if let Err(error) = self.sync_inventory() {
            self.note(error);
            return;
        }
        if axis == 0 {
            if let Some(d) = self
                .inventory
                .delays
                .iter_mut()
                .find(|d| d.id == self.inventory.selected.delay)
            {
                d.zero_ps = value;
            }
        } else if let Some(s) = self
            .inventory
            .scanners
            .iter_mut()
            .find(|s| s.id == self.inventory.selected.scanner)
        {
            s.zero_um[axis - 1] = value;
        }
        // Keep edits local to the physical device, including when Alignment
        // or a history tab is visible rather than its measurement workflow.
        if let Err(error) = self
            .inventory
            .project_selected(&mut self.settings.raw)
            .and_then(|_| self.refresh_plan_coordinates())
        {
            self.note(error);
        }
    }

    pub(super) fn refresh_plan_coordinates(&mut self) -> Result<(), String> {
        for workflow in Workflow::ALL {
            let saved = self.settings.plan(workflow)?;
            let plan = &mut self.plans[Self::ix(workflow)];
            plan.zero = saved.zero;
            plan.theta_deg = saved.theta_deg;
        }
        Ok(())
    }

    pub(super) fn workflow_connected(&self, plan: &Plan) -> bool {
        let devices = self.workflow_device_state(plan);
        devices[0]
            && engine::active_axes(plan)
                .iter()
                .enumerate()
                .all(|(axis, needed)| !needed || devices[axis + 1])
    }

    /// Connection state for the plan's bound device group. `Snapshot::devices`
    /// is only the inspected worker selection, so it is a valid fallback only
    /// when the queried ID still equals that selection.
    pub(super) fn workflow_device_state(&self, plan: &Plan) -> [bool; 4] {
        let Ok(ids) = self.settings.workflow_device_ids(plan.workflow) else {
            return [false; 4];
        };
        let connected = |kind: &str, id: &str, axis: usize, selected: &str, index: usize| {
            self.snapshot
                .registered_devices
                .iter()
                .find(|d| d.kind == kind && d.id == id)
                .map_or(id == selected && self.snapshot.devices[index], |d| {
                    d.connected[axis]
                })
        };
        let selected = &self.inventory.selected;
        [
            connected("lockin", &ids.lockin, 0, &selected.lockin, 0),
            connected("delay", &ids.delay, 0, &selected.delay, 1),
            connected("scanner", &ids.scanner, 1, &selected.scanner, 2),
            connected("scanner", &ids.scanner, 2, &selected.scanner, 3),
        ]
    }

    /// All selection callers use the same correlated worker completion. An
    /// enqueue succeeds before a selection is confirmed; failures restore the
    /// previous profile and tab rather than leaving a misleading active card.
    pub(super) fn request_device_selection(
        &mut self,
        settings: Settings,
        workflow: Workflow,
        remote_id: Option<String>,
    ) -> Result<(), String> {
        if self.snapshot.running
            || self.measurement_pending
            || self.pending_device_selection.is_some()
        {
            return Err("Wait for the current operation before selecting devices".into());
        }
        let config = settings.profile_hardware()?;
        let requested_ids = config.device_ids.clone();
        let id = remote_id.unwrap_or_else(|| format!("gui-selection-{}", uuid::Uuid::new_v4()));
        self.controller
            .send_remote(id.clone(), Command::Select { config })?;
        self.pending_device_selection = Some(PendingSelection {
            id,
            previous: self.settings.clone(),
            previous_workflow: self.workflow,
            requested_workflow: workflow,
            requested_ids,
        });
        self.selection_retry_blocked = None;
        self.settings = settings;
        self.switch_workflow_view(workflow);
        self.hydrate();
        self.clear_selected_readings();
        self.note("Selecting devices…");
        Ok(())
    }

    pub(super) fn finish_device_selection(
        &mut self,
        id: &str,
        result: &Result<(usize, Option<PathBuf>), String>,
    ) {
        if !self
            .pending_device_selection
            .as_ref()
            .is_some_and(|p| p.id == id)
        {
            return;
        }
        let pending = self
            .pending_device_selection
            .take()
            .expect("matching selection");
        if let Err(error) = result {
            self.selection_retry_blocked =
                Some((pending.requested_workflow, pending.requested_ids));
            self.settings = pending.previous;
            self.switch_workflow_view(pending.previous_workflow);
            self.hydrate();
            self.live.clear();
            self.hydrate_lockin();
            self.note(format!("Device selection unchanged: {error}"));
        } else {
            self.selection_retry_blocked = None;
            self.note("Devices selected");
        }
    }

    pub(super) fn clear_selected_readings(&mut self) {
        self.live.clear();
        self.snapshot.signal = None;
        self.snapshot.signal_origin = None;
        self.snapshot.lockin = Value::Null;
        self.snapshot.devices = [false; 4];
        self.snapshot.position = Default::default();
        self.snapshot.delay_calibration = None;
        self.snapshot.transport.clear();
    }

    pub(super) fn open_measurement_settings(&mut self) {
        let result = self
            .sync_all_plans()
            .and_then(|_| self.settings.workflow_device_ids(self.workflow));
        match result {
            Ok(ids) => {
                self.measurement_settings = Some(Draft {
                    workflow: self.workflow,
                    ids,
                    error: None,
                })
            }
            Err(error) => self.note(error),
        }
    }

    fn apply_measurement_settings(&mut self, draft: &Draft) -> Result<(), String> {
        if self.snapshot.running || self.controller.has_pending() || self.python_locked() {
            return Err("Wait for the current operation before applying settings".into());
        }
        self.sync_all_plans()?;
        let mut settings = self.settings.clone();
        settings.set_workflow_devices(draft.workflow, draft.ids.clone())?;
        settings.select_workflow(draft.workflow)?;
        if draft.ids == self.inventory.selected {
            self.settings = settings;
            self.hydrate();
            Ok(())
        } else {
            self.request_device_selection(settings, draft.workflow, None)
        }
    }

    pub(super) fn measurement_settings_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.measurement_settings.take() else {
            return;
        };
        let mut accepted = false;
        let mut cancelled = false;
        let locked = self.python_locked() || self.snapshot.running || self.controller.has_pending();
        let response = egui::Modal::new(egui::Id::new("measurement_settings"))
            .frame(
                egui::Frame::popup(&ctx.global_style())
                    .fill(PANEL)
                    .inner_margin(16),
            )
            .show(ctx, |ui| {
                ui.set_width(380.);
                ui.strong("Measurement Settings");
                ui.label(
                    egui::RichText::new(draft.workflow.name())
                        .small()
                        .color(Color32::GRAY),
                );
                ui.add_space(10.);
                egui::ScrollArea::vertical()
                    .max_height((ctx.content_rect().height() - 190.).max(160.))
                    .show(ui, |ui| {
                        ui.add_enabled_ui(!locked, |ui| {
                            settings_section(ui, "Devices", |ui| {
                                ui.label(
                                    egui::RichText::new("Remembered for this measurement tab.")
                                        .small()
                                        .color(Color32::GRAY),
                                );
                                ui.add_space(6.);
                                egui::Grid::new("measurement_device_roles")
                                    .num_columns(2)
                                    .spacing([10., 8.])
                                    .show(ui, |ui| {
                                        binding_choice(
                                            ui,
                                            "Lock-in",
                                            &mut draft.ids.lockin,
                                            self.inventory
                                                .lockins
                                                .iter()
                                                .map(|e| {
                                                    (
                                                        e.id.clone(),
                                                        format!(
                                                            "{} · {}",
                                                            e.display_name(),
                                                            e.model.name()
                                                        ),
                                                    )
                                                })
                                                .collect(),
                                        );
                                        if uses_device(draft.workflow, "delay") {
                                            binding_choice(
                                                ui,
                                                "Delay Stage",
                                                &mut draft.ids.delay,
                                                self.inventory
                                                    .delays
                                                    .iter()
                                                    .map(|e| {
                                                        (
                                                            e.id.clone(),
                                                            format!(
                                                                "{} · {}",
                                                                e.display_name(),
                                                                e.config
                                                                    .stage
                                                                    .as_deref()
                                                                    .unwrap_or(
                                                                        &e.config.controller
                                                                    )
                                                            ),
                                                        )
                                                    })
                                                    .collect(),
                                            );
                                        }
                                        if uses_device(draft.workflow, "scanner") {
                                            binding_choice(
                                                ui,
                                                "Scanner",
                                                &mut draft.ids.scanner,
                                                self.inventory
                                                    .scanners
                                                    .iter()
                                                    .map(|e| (e.id.clone(), e.display_name()))
                                                    .collect(),
                                            );
                                        }
                                        optional_binding_choice(
                                            ui,
                                            "Power meter",
                                            &mut draft.ids.power_meter,
                                            self.inventory
                                                .power_meters
                                                .iter()
                                                .map(|e| {
                                                    (
                                                        e.id.clone(),
                                                        format!(
                                                            "{} · Thorlabs PM100USB/PM100D",
                                                            e.display_name()
                                                        ),
                                                    )
                                                })
                                                .collect(),
                                        );
                                    });
                                ui.add_space(6.);
                                ui.label(
                                    egui::RichText::new(
                                        "Other connected devices remain available.",
                                    )
                                    .small()
                                    .color(Color32::GRAY),
                                );
                            });
                        });
                        if let Some(error) = &draft.error {
                            ui.colored_label(Color32::LIGHT_RED, error);
                        }
                    });
                ui.add_space(10.);
                ui.label(
                    egui::RichText::new("Use Config > Save to save these settings to the file.")
                        .small()
                        .color(Color32::GRAY),
                );
                ui.add_space(10.);
                ui.horizontal(|ui| {
                    accepted = ui
                        .add_enabled(
                            !locked,
                            egui::Button::new("OK").min_size(Vec2::new(80., 26.)),
                        )
                        .clicked();
                    cancelled = ui
                        .add(egui::Button::new("Cancel").min_size(Vec2::new(80., 26.)))
                        .clicked();
                });
            });
        if accepted {
            if let Err(error) = self.apply_measurement_settings(&draft) {
                draft.error = Some(error);
                self.measurement_settings = Some(draft);
            }
        } else if !cancelled && !response.should_close() {
            self.measurement_settings = Some(draft);
        }
    }
}

fn settings_section(ui: &mut egui::Ui, title: &str, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .stroke(Stroke::new(1_f32, BORDER))
        .corner_radius(8)
        .inner_margin(10)
        .show(ui, |ui| {
            ui.set_min_width(358.);
            ui.strong(title);
            ui.add_space(6.);
            contents(ui);
        });
}

fn binding_choice(
    ui: &mut egui::Ui,
    label: &str,
    selected: &mut String,
    entries: Vec<(String, String)>,
) {
    ui.label(label);
    let current = entries
        .iter()
        .find(|(id, _)| id == selected)
        .map(|(_, name)| name.clone())
        .unwrap_or_else(|| "Not configured".into());
    egui::ComboBox::from_id_salt(("measurement-binding", label))
        .width(235.)
        .truncate()
        .selected_text(&current)
        .show_ui(ui, |ui| {
            for (id, name) in &entries {
                ui.selectable_value(selected, id.clone(), name);
            }
        })
        .response
        .on_hover_text(current);
    ui.end_row();
}

fn optional_binding_choice(
    ui: &mut egui::Ui,
    label: &str,
    selected: &mut String,
    mut entries: Vec<(String, String)>,
) {
    entries.insert(0, (String::new(), "None".into()));
    binding_choice(ui, label, selected, entries);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_groups() -> App {
        let mut app = App::with_context(&egui::Context::default(), false);
        app.sync_all_plans().unwrap();
        let mut second = app.inventory.scanners[0].clone();
        second.id = "scanner-2".into();
        second.name = "Scanner 2".into();
        second.zero_um = [21., -3.];
        for axis in [&mut second.x, &mut second.y].into_iter().flatten() {
            axis.port = "COM20".into();
        }
        app.inventory.scanners.push(second);
        app.inventory
            .project_selected(&mut app.settings.raw)
            .unwrap();
        let mut ids = app.inventory.selected.clone();
        ids.scanner = "scanner-2".into();
        app.settings
            .set_workflow_devices(Workflow::Srkr, ids)
            .unwrap();
        app.hydrate();
        app
    }

    fn settle_selection(app: &mut App) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while app.pending_device_selection.is_some() {
            app.receive();
            assert!(Instant::now() < deadline, "selection completion timed out");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn tab_navigation_preserves_device_card_geometry_and_disclosures() {
        let ctx = egui::Context::default();
        let mut app = two_groups();
        app.device_expanded = [true, false, true, false];
        let draw = |app: &mut App| {
            app.geometry.clear();
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(precision::rect(0., 0., 400., 2400.)),
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show_inside(ui, |ui| {
                        app.device_inventory_cards(ui, egui::pos2(10., 10.), 328.);
                    });
                },
            );
            app.geometry.clone()
        };
        let initial = draw(&mut app);
        for workflow in [
            Workflow::Srkr,
            Workflow::Strkr,
            Workflow::Trkr,
            Workflow::Srkr2d,
            Workflow::SignalMonitor,
        ] {
            app.switch_workflow(workflow);
            assert_eq!(draw(&mut app), initial);
            assert_eq!(app.device_expanded, [true, false, true, false]);
            assert!(app.pending_device_selection.is_none());
        }
    }

    #[test]
    fn tabs_keep_the_inspected_group_while_measurement_roles_change() {
        let mut app = two_groups();
        let initial = app.inventory.selected.clone();
        app.switch_workflow(Workflow::Srkr);
        assert_eq!(app.inventory.selected, initial);
        assert_eq!(
            app.settings
                .workflow_device_ids(Workflow::Srkr)
                .unwrap()
                .scanner,
            "scanner-2"
        );
        assert!(app.pending_device_selection.is_none());
        let mut run_settings = app.settings.clone();
        run_settings.select_workflow(Workflow::Srkr).unwrap();
        assert_eq!(
            run_settings.hardware().unwrap().device_ids.scanner,
            "scanner-2"
        );
        // Editing the open left card stays with the inspected device, rather
        // than silently applying this tab's scanner-2 calibration.
        app.set_selected_zero(1, 42.);
        assert_eq!(app.inventory.scanners[0].zero_um[0], 42.);
        assert_eq!(app.inventory.scanners[1].zero_um[0], 21.);
        app.switch_workflow(Workflow::Trkr);
        assert_eq!(app.inventory.selected, initial);
        assert_eq!(
            app.settings
                .workflow_device_ids(Workflow::Srkr)
                .unwrap()
                .scanner,
            "scanner-2"
        );
        assert!(!app.any_hardware_connected());
        let reloaded = Settings::from_value(app.settings.raw.clone()).unwrap();
        assert_eq!(
            reloaded
                .workflow_device_ids(Workflow::Srkr)
                .unwrap()
                .scanner,
            "scanner-2"
        );
    }

    #[test]
    fn history_tab_during_run_keeps_inspected_devices_and_readback_zero() {
        let mut app = two_groups();
        let zero = app.selected_zero();
        let selected = app.inventory.selected.clone();
        app.active_workflow = Workflow::Trkr;
        app.snapshot.running = true;
        app.measurement_pending = true;
        app.switch_workflow(Workflow::Srkr);
        assert_eq!(app.workflow, Workflow::Srkr);
        assert_eq!(app.inventory.selected, selected);
        assert_eq!(app.selected_zero(), zero);
        assert!(app.pending_device_selection.is_none());
        assert!(app.device_highlighted("delay", &selected.delay));
        assert!(!app.device_highlighted("scanner", &selected.scanner));
        assert!(!app.device_highlighted("scanner", "scanner-2"));
        app.snapshot.running = false;
        app.measurement_pending = false;
        assert_eq!(app.inventory.selected, selected);
    }

    #[test]
    fn settings_are_staged_until_ok_and_leave_wait_times_unchanged() {
        let mut app = two_groups();
        let before = app.settings.raw.clone();
        let srkr_wait = app.settings.plan(Workflow::Srkr).unwrap().interval_s;
        app.open_measurement_settings();
        let draft = app.measurement_settings.as_mut().unwrap();
        draft.ids.scanner = "scanner-2".into();
        app.measurement_settings = None; // Cancel/Escape drops only the draft.
        assert_eq!(app.settings.raw, before);
        app.open_measurement_settings();
        let draft = app.measurement_settings.take().unwrap();
        let trkr_wait = app.plan().interval_s;
        app.apply_measurement_settings(&draft).unwrap();
        assert_eq!(app.plan().interval_s, trkr_wait);
        assert_eq!(
            app.settings.plan(Workflow::Srkr).unwrap().interval_s,
            srkr_wait
        );
        assert!(app.pending_device_selection.is_none());
    }

    #[test]
    fn failed_selection_restores_previous_tab_binding_and_zero() {
        let mut app = two_groups();
        let old = app.settings.clone();
        let old_workflow = app.workflow;
        let old_ids = app.inventory.selected.clone();
        app.settings.select_workflow(Workflow::Srkr).unwrap();
        app.switch_workflow_view(Workflow::Srkr);
        app.hydrate();
        app.pending_device_selection = Some(PendingSelection {
            id: "selection-test".into(),
            previous: old,
            previous_workflow: old_workflow,
            requested_workflow: Workflow::Srkr,
            requested_ids: app.inventory.selected.clone(),
        });
        app.finish_device_selection("unrelated", &Ok((0, None)));
        assert!(app.pending_device_selection.is_some());
        app.finish_device_selection("selection-test", &Err("test rejection".into()));
        assert_eq!(app.workflow, old_workflow);
        assert_eq!(app.inventory.selected, old_ids);
        assert!(app.pending_device_selection.is_none());
        assert!(app.status.contains("unchanged"));
    }

    #[test]
    fn zero_edits_update_same_device_workflows_without_losing_other_plan_edits() {
        let mut app = two_groups();
        app.plans[Workflow::Trkr.index()].interval_s = 7.;
        app.set_selected_zero(1, 123.);
        app.sync_all_plans().unwrap();
        assert_eq!(app.settings.plan(Workflow::Trkr).unwrap().zero[1], 123.);
        assert_eq!(app.settings.plan(Workflow::Srkr2d).unwrap().zero[1], 123.);
        assert_eq!(app.settings.plan(Workflow::Srkr).unwrap().zero[1], 21.);
        assert_eq!(app.settings.plan(Workflow::Trkr).unwrap().interval_s, 7.);
    }

    #[test]
    fn failed_idle_restore_waits_for_explicit_retry() {
        let mut app = two_groups();
        app.switch_workflow_view(Workflow::Srkr);
        let previous = app.settings.clone();
        let desired = app.settings.workflow_device_ids(Workflow::Srkr).unwrap();
        app.pending_device_selection = Some(PendingSelection {
            id: "restore-test".into(),
            previous,
            previous_workflow: Workflow::Srkr,
            requested_workflow: Workflow::Srkr,
            requested_ids: desired,
        });
        app.finish_device_selection("restore-test", &Err("test rejection".into()));
        assert!(app.pending_device_selection.is_none());
        assert!(!app.controller.has_pending());
        let mut settings = app.settings.clone();
        settings.select_workflow(Workflow::Srkr).unwrap();
        app.request_device_selection(settings, Workflow::Srkr, None)
            .unwrap();
        settle_selection(&mut app);
        assert!(app.selection_retry_blocked.is_none());
        assert_eq!(app.inventory.selected.scanner, "scanner-2");
    }

    #[test]
    fn readiness_resolves_requested_workflow_devices_not_the_visible_group() {
        let mut app = two_groups();
        app.snapshot.devices = [true; 4];
        let plan = app.settings.plan(Workflow::Srkr).unwrap();
        assert!(!app.workflow_connected(&plan));
        app.snapshot.registered_devices.push(engine::DeviceState {
            id: "scanner-2".into(),
            kind: "scanner".into(),
            connected: [false, true, true],
            position: Default::default(),
            delay_calibration: None,
            phase: "Ready".into(),
        });
        assert!(app.workflow_connected(&plan));
        app.snapshot.registered_devices[0].connected = [false; 3];
        assert!(!app.workflow_connected(&plan));
    }

    #[test]
    fn manual_move_highlights_the_current_group_not_the_last_measurement() {
        let mut app = two_groups();
        app.switch_workflow(Workflow::Srkr);
        settle_selection(&mut app);
        app.active_workflow = Workflow::Trkr;
        app.snapshot.running = true;
        assert!(app.device_highlighted("scanner", "scanner-2"));
        assert!(!app.device_highlighted("scanner", "scanner-1"));
    }
}
