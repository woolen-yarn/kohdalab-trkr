//! Start has one availability rule for the button and keyboard.
use super::*;

pub(super) struct Availability {
    blocked: bool,
    replay_ready: bool,
    source_demo: bool,
    devices: [bool; 4],
}
impl Availability {
    pub(super) fn allows(&self, plan: &Plan) -> bool {
        !self.blocked
            && self.replay_ready
            && (!plan.workflow.is_2d()
                || axis_choices::valid_pair(plan.workflow, plan.fast, plan.slow))
            && (self.source_demo
                || (self.devices[0]
                    && engine::active_axes(plan)
                        .iter()
                        .enumerate()
                        .all(|(i, needed)| !needed || self.devices[i + 1])))
    }
}

impl App {
    pub(super) fn start_availability(&self) -> Availability {
        Availability {
            blocked: self.alignment_tab
                || self.snapshot.running
                || self.measurement_pending
                || self.controller.has_pending()
                || self.pending_device_selection.is_some()
                || self.python_locked(),
            replay_ready: !self.demo_viewer
                || (self.snapshot.devices[0]
                    && self.demo.datasets[Self::ix(self.workflow)].is_some()
                    && self.demo.pending.is_none()),
            source_demo: self.source_demo,
            devices: self.snapshot.devices,
        }
    }
    pub(super) fn start_overlay_open(&self, ctx: &egui::Context) -> bool {
        self.measurement_settings.is_some()
            || self.inventory_dialog.is_some()
            || self.dialog.is_some()
            || self.initialize_axis.is_some()
            || ctx.memory(|m| m.top_modal_layer().is_some())
            || egui::Popup::is_any_open(ctx)
    }
    pub(super) fn finish_start_shortcut(
        &mut self,
        ctx: &egui::Context,
        requested: bool,
        was_blocked: bool,
    ) {
        if requested
            && !was_blocked
            && !self.start_overlay_open(ctx)
            && self.start_availability().allows(self.plan())
            && ctx.input(|i| i.focused)
        {
            self.start();
        }
    }
}

pub(super) fn consume(ctx: &egui::Context) -> bool {
    ctx.input_mut(|input| {
        let mut requested = false;
        input.events.retain(|event| {
            if let egui::Event::Key {
                key: egui::Key::Enter,
                pressed: true,
                repeat,
                modifiers,
                ..
            } = event
                && modifiers.ctrl
                && !modifiers.alt
                && !modifiers.shift
                && !modifiers.mac_cmd
            {
                requested |= !repeat && input.focused;
                false
            } else {
                true
            }
        });
        requested
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn press(ctx: &egui::Context, modifiers: egui::Modifiers, repeat: bool) -> bool {
        // A fresh press follows a release; egui otherwise marks it as autorepeat.
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: false,
                    repeat: false,
                    modifiers,
                }],
                ..Default::default()
            },
            |_| {},
        );
        if repeat {
            let _ = ctx.run(
                egui::RawInput {
                    events: vec![egui::Event::Key {
                        key: egui::Key::Enter,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers,
                    }],
                    ..Default::default()
                },
                |_| {},
            );
        }
        let mut result = false;
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat,
                    modifiers,
                }],
                ..Default::default()
            },
            |ctx| {
                result = consume(ctx);
                assert!(!consume(ctx));
            },
        );
        result
    }
    #[test]
    fn shortcut_requires_control_and_ignores_key_repeat() {
        let ctx = egui::Context::default();
        assert!(!press(&ctx, egui::Modifiers::NONE, false));
        assert!(!press(&ctx, egui::Modifiers::CTRL, true));
        assert!(press(&ctx, egui::Modifiers::CTRL, false));
        assert!(!press(
            &ctx,
            egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
            false
        ));
        assert!(!press(&ctx, egui::Modifiers::MAC_CMD, false));
    }
    #[test]
    fn readiness_checks_devices_pending_and_alignment_without_io() {
        let ctx = egui::Context::default();
        let mut app = App::with_context(&ctx, false);
        app.workflow = Workflow::Trkr;
        app.source_demo = false;
        let plan = app.plan().clone();
        assert!(!app.start_availability().allows(&plan));
        app.snapshot.devices = [true, true, false, false];
        assert!(app.start_availability().allows(&plan));
        app.measurement_pending = true;
        assert!(!app.start_availability().allows(&plan));
        app.measurement_pending = false;
        app.alignment_tab = true;
        assert!(!app.start_availability().allows(&plan));
        app.alignment_tab = false;
        app.source_demo = true;
        app.snapshot.devices = [false; 4];
        assert!(app.start_availability().allows(&plan));
        app.open_measurement_settings();
        assert!(app.start_overlay_open(&ctx));
        app.finish_start_shortcut(&ctx, true, false);
        assert!(!app.measurement_pending);
        app.measurement_settings = None;
        app.finish_start_shortcut(&ctx, true, true);
        assert!(!app.measurement_pending);
        app.demo_viewer = true;
        assert!(!app.start_availability().allows(&plan));
    }

    #[test]
    fn shortcut_starts_replay_after_same_frame_numeric_edit_once() {
        let ctx = egui::Context::default();
        let mut app = App::with_context(&ctx, true);
        app.workflow = Workflow::Trkr;
        let index = App::ix(app.workflow);
        app.snapshot.devices[0] = true;
        app.demo.datasets[index] = Some(Arc::new(crate::replay_data::Dataset {
            plan: app.plan().clone(),
            samples: Vec::new(),
        }));
        let area = precision::rect(0., 0., 850., 250.);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.measure(ui, area));
        });
        let id = egui::Id::new("trkr_wait_spin");
        ctx.memory_mut(|m| m.request_focus(id));
        ctx.data_mut(|d| d.insert_temp(id, String::new()));
        let _ = ctx.run(
            egui::RawInput {
                events: vec![
                    egui::Event::Text("0.37".into()),
                    egui::Event::Key {
                        key: egui::Key::Enter,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::CTRL,
                    },
                ],
                ..Default::default()
            },
            |ctx| {
                let requested = consume(ctx);
                let blocked =
                    app.start_overlay_open(ctx) || !app.start_availability().allows(app.plan());
                egui::CentralPanel::default().show(ctx, |ui| app.measure(ui, area));
                app.finish_start_shortcut(ctx, requested, blocked);
            },
        );
        assert!(app.snapshot.running);
        assert_eq!(app.trace_plans[index].as_ref().unwrap().interval_s, 0.37);
        let epoch = app.plot_epoch;
        app.finish_start_shortcut(&ctx, true, false);
        assert_eq!(app.plot_epoch, epoch);
        assert!(!app.controller.has_pending()); // Replay cannot open hardware.
    }
}
