use super::*;
impl App {
    pub fn new_gui_interaction(cc: &eframe::CreationContext<'_>) -> Result<Self, std::io::Error> {
        let temp = tempfile::tempdir()?;
        let paths = UserConfigPaths::from_kohdalab_dir(temp.path().join(".kohdalab"));
        let mut app = Self::with_context_paths(&cc.egui_ctx, false, paths, false);
        app.controller.restrict_to_demo();
        app.source_demo = true;
        app.workflow = Workflow::Trkr;
        app.active_workflow = Workflow::Trkr;
        app.config_path = temp.path().join("trial-config.json").display().to_string();
        for plan in &mut app.plans {
            plan.fast_range = Range {
                min: -5.,
                max: 5.,
                step: 1.,
            };
            plan.slow_range = Range {
                min: -2.,
                max: 2.,
                step: 1.,
            };
            plan.interval_s = 0.05;
            plan.n_points = 100;
        }
        for output in &mut app.outputs {
            output.dir = temp.path().join("results").display().to_string();
        }
        std::fs::create_dir_all(temp.path().join("results"))?;
        app.gui_interaction.sandbox = Some(temp);
        app.note("GUI TRIAL — SIMULATED · hardware disabled · temporary default paths");
        Ok(app)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};
    #[test]
    fn sandbox_preserves_actual_app_and_rejects_hardware_commands() {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1566., 900.))
            .build_eframe(|cc| App::new_gui_interaction(cc).unwrap());
        h.run_steps(3);
        assert!(!h.state().demo_viewer);
        assert!(h.state().source_demo);
        let config = h.state_mut().config().unwrap();
        assert!(
            h.state()
                .controller
                .send(Command::Move {
                    config,
                    targets: [Some(0.), None, None],
                    coordinate: Coordinate::Measurement,
                    zero: [0.; 3],
                    approach: [false; 3]
                })
                .is_err()
        );
        assert!(h.state().controller.send(Command::Read).is_err());
        h.get_by_label("Start");
        assert!(h.state().remote.is_none());
    }
    #[test]
    fn actual_start_uses_existing_demo_engine_and_finishes() {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1566., 900.))
            .build_eframe(|cc| App::new_gui_interaction(cc).unwrap());
        h.run_steps(3);
        h.get_by_label("Start").click();
        h.run_steps(3);
        let deadline = Instant::now() + Duration::from_secs(4);
        while (h.state().measurement_pending
            || h.state().snapshot.running
            || h.state().trace.is_empty())
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
            h.run_steps(1);
        }
        assert_eq!(h.state().trace.len(), 11);
        assert!(!h.state().snapshot.running);
        assert!(h.state().log.iter().any(|s| s.starts_with("Finished ·")));
    }
    #[test]
    fn full_app_device_drag_works_in_scrolled_inventory() {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1566., 900.))
            .build_eframe(|cc| App::new_gui_interaction(cc).unwrap());
        h.run_steps(3);
        let before = h.state().inventory.ordered_panels();
        let original_y = h.state().geometry["inventory_delay-1"][1];
        h.event(egui::Event::PointerMoved(egui::pos2(170., 300.)));
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0., -40.),
            modifiers: Default::default(),
            phase: egui::TouchPhase::Move,
        });
        h.run_steps(3);
        let delay = h.state().geometry["inventory_delay-1"];
        assert!(delay[1] < original_y, "inventory must actually scroll");
        let lockin = h.state().geometry["inventory_lockin-1"];
        let start = egui::pos2(delay[0] + 14., delay[1] + 16.);
        let target = egui::pos2(lockin[0] + 44., lockin[1] + 80.);
        h.event(egui::Event::PointerMoved(start));
        h.event(egui::Event::PointerButton {
            pos: start,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        h.run_steps(1);
        h.event(egui::Event::PointerMoved(target));
        h.run_steps(2);
        assert_eq!(h.state().inventory.ordered_panels(), before);
        h.event(egui::Event::PointerButton {
            pos: target,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        h.run_steps(2);
        assert_eq!(
            h.state().inventory.ordered_panels()[0],
            crate::inventory::PanelKey::new("delay", "delay-1")
        );
    }
    #[test]
    fn full_app_drag_release_outside_inventory_cannot_reorder_later() {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1566., 900.))
            .build_eframe(|cc| App::new_gui_interaction(cc).unwrap());
        h.run_steps(3);
        let before = h.state().inventory.ordered_panels();
        let delay = h.state().geometry["inventory_delay-1"];
        let start = egui::pos2(delay[0] + 14., delay[1] + 16.);
        let outside = egui::pos2(900., 800.);
        h.event(egui::Event::PointerMoved(start));
        h.event(egui::Event::PointerButton {
            pos: start,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        h.run_steps(1);
        h.event(egui::Event::PointerMoved(outside));
        h.run_steps(2);
        h.event(egui::Event::PointerButton {
            pos: outside,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        h.run_steps(2);
        assert!(!egui::DragAndDrop::has_any_payload(&h.ctx));
        let lockin = h.state().geometry["inventory_lockin-1"];
        let target = egui::pos2(lockin[0] + 44., lockin[1] + 8.);
        h.event(egui::Event::PointerMoved(target));
        h.event(egui::Event::PointerButton {
            pos: target,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        h.run_steps(2);
        assert_eq!(h.state().inventory.ordered_panels(), before);
    }
    #[test]
    fn invalid_numeric_draft_keeps_value_and_geometry() {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1566., 900.))
            .build_eframe(|cc| App::new_gui_interaction(cc).unwrap());
        h.run_steps(3);
        let before = h.state().geometry["trkr_wait_spin"];
        let value = h.state().plan().interval_s;
        let id = egui::Id::new("trkr_wait_spin");
        h.ctx.memory_mut(|m| m.request_focus(id));
        h.ctx
            .data_mut(|d| d.insert_temp(id, "not-a-number".to_owned()));
        h.run_steps(2);
        assert_eq!(h.state().geometry["trkr_wait_spin"], before);
        assert_eq!(h.state().plan().interval_s, value);
    }
}
