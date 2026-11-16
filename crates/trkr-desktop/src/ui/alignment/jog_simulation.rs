use super::*;
use trkr_motion::native_jog::{
    JogOutcome, JogPhase, NativeJogModel, NativeJogSession, SimulatedController,
};

const OWNER: u64 = 1;
const TICK: Duration = Duration::from_millis(20);
// The trial uses a synthetic scale independent of any installed stage.
const DELAY_PULSES_PER_MM: f64 = 1_000.;

pub(super) struct JogSimulation {
    pub(super) link_up: bool,
    pub(super) axis: Option<SimulatedAxis>,
    last_poll: f64,
}

pub(super) struct SimulatedAxis {
    pub(super) control: HeldControl,
    pub(super) session: NativeJogSession,
    pub(super) bus: SimulatedController,
}

impl JogSimulation {
    #[cfg(feature = "gui-interaction-tests")]
    pub(super) fn new() -> Self {
        Self {
            link_up: true,
            axis: None,
            last_poll: 0.,
        }
    }

    pub(super) fn can_start(&self) -> bool {
        self.link_up
            && self
                .axis
                .as_ref()
                .is_none_or(|axis| !matches!(axis.session.phase(), JogPhase::Unconfirmed { .. }))
    }

    pub(super) fn status(&self) -> &'static str {
        match self.axis.as_ref().map(|axis| axis.session.phase()) {
            Some(JogPhase::Unconfirmed { .. }) => "Stop unconfirmed · restart trial to reset",
            Some(JogPhase::Running { .. }) => "Jogging",
            _ if !self.link_up => "Link disconnected",
            Some(JogPhase::Stopped { .. }) => "Stopped · position confirmed",
            _ => "Simulation · no hardware",
        }
    }

    pub(super) fn toggle_link(&mut self) {
        self.link_up = !self.link_up;
        if let Some(axis) = &mut self.axis {
            if self.link_up {
                axis.bus.restore_link();
            } else {
                axis.bus.drop_link();
            }
        }
    }
}

impl App {
    #[cfg(feature = "gui-interaction-tests")]
    pub(in crate::ui) fn enable_alignment_jog_trial(&mut self) {
        self.enable_alignment_preview(false);
        self.enable_alignment_monitor_preview();
        for panel in &mut self.alignment.layout.delays {
            panel.unit = AlignmentUnit::Native;
        }
        for panel in &mut self.alignment.layout.scanners {
            panel.unit = AlignmentUnit::Native;
        }
        self.settings.raw["alignment"] = self.alignment.layout.value();
        self.alignment.jog_simulation = Some(JogSimulation::new());
    }

    pub(super) fn start_simulated_hold(&mut self, control: HeldControl) {
        let Some(state) = self.device_state(control.kind, &control.id) else {
            return;
        };
        if !self
            .alignment
            .jog_simulation
            .as_ref()
            .is_some_and(JogSimulation::can_start)
        {
            return;
        }
        let (model, native, travel, speed) = match control.kind {
            MotionKind::Delay => {
                let Some(native) = state.position.delay_stage_mm else {
                    return;
                };
                (
                    NativeJogModel::Shot302Gs {
                        axis: 1,
                        axis_count: 1,
                    },
                    native * DELAY_PULSES_PER_MM,
                    [0., 500. * DELAY_PULSES_PER_MM],
                    120.,
                )
            }
            MotionKind::Scanner => {
                let native = if control.axis == 1 {
                    state.position.x_scanner_mm
                } else {
                    state.position.y_scanner_mm
                };
                let Some(native) = native else { return };
                let travel = [0., 12.];
                (
                    NativeJogModel::ConexCc { address: 1, travel },
                    native,
                    travel,
                    0.015,
                )
            }
        };
        let mut axis = SimulatedAxis {
            control: control.clone(),
            session: NativeJogSession::new(model, OWNER),
            bus: SimulatedController::new(model, native, travel, speed),
        };
        match axis
            .session
            .start(&mut axis.bus, OWNER, control.positive, true)
        {
            Ok(()) => self.alignment.held = Some(control),
            Err(error) => self.note(error),
        }
        self.alignment
            .jog_simulation
            .as_mut()
            .expect("active simulation")
            .axis = Some(axis);
    }

    pub(super) fn tick_simulated_hold(&mut self, time: f64) {
        let Some(sim) = &mut self.alignment.jog_simulation else {
            return;
        };
        if sim.link_up && time - sim.last_poll < TICK.as_secs_f64() {
            return;
        }
        sim.last_poll = time;
        let Some(axis) = &mut sim.axis else { return };
        match axis.session.poll(&mut axis.bus, OWNER, true) {
            Ok(Some(outcome)) => self.finish_simulated_hold(outcome),
            Ok(None) => {
                if let Some(position) = axis.session.tracked_position() {
                    let control = axis.control.clone();
                    self.update_simulated_readout(&control, Some(position));
                }
            }
            Err(error) => self.note(error),
        }
    }

    pub(super) fn advance_simulated_unconfirmed_axis(&mut self, ctx: &egui::Context) {
        let Some(sim) = &mut self.alignment.jog_simulation else {
            return;
        };
        let Some(axis) = &mut sim.axis else { return };
        if !matches!(axis.session.phase(), JogPhase::Unconfirmed { .. })
            || !axis.bus.physically_moving()
        {
            return;
        }
        let time = ctx.input(|input| input.time);
        if time - sim.last_poll >= TICK.as_secs_f64() {
            axis.bus.advance(1);
            sim.last_poll = time;
        }
        ctx.request_repaint_after(TICK);
    }

    pub(super) fn release_simulated_hold(&mut self) {
        let Some(axis) = self
            .alignment
            .jog_simulation
            .as_mut()
            .and_then(|sim| sim.axis.as_mut())
        else {
            return;
        };
        match axis.session.release(&mut axis.bus, OWNER) {
            Ok(outcome) => self.finish_simulated_hold(outcome),
            Err(error) => self.note(error),
        }
    }

    fn finish_simulated_hold(&mut self, outcome: JogOutcome) {
        self.alignment.held = None;
        let axis = self
            .alignment
            .jog_simulation
            .as_ref()
            .and_then(|sim| sim.axis.as_ref())
            .expect("simulated jog");
        let control = axis.control.clone();
        match outcome {
            JogOutcome::ConfirmedStopped { position } => {
                self.update_simulated_readout(&control, Some(position));
                self.note("Simulated jog stopped · position confirmed");
            }
            JogOutcome::Unconfirmed { reason, .. } => {
                self.update_simulated_readout(&control, None);
                self.note(format!("Simulated jog stop unconfirmed: {reason}"));
            }
        }
    }

    fn update_simulated_readout(&mut self, control: &HeldControl, native: Option<f64>) {
        let Some(state) = self
            .snapshot
            .registered_devices
            .iter_mut()
            .find(|state| state.id == control.id)
        else {
            return;
        };
        match control.kind {
            MotionKind::Delay => {
                state.position.delay_stage_pulse = native.map(|value| value.round() as i64);
                let mm = native.map(|value| value / DELAY_PULSES_PER_MM);
                state.position.delay_stage_mm = mm;
                state.position.physical[0] = mm.and_then(|mm| {
                    self.inventory
                        .delays
                        .iter()
                        .find(|entry| entry.id == control.id)?
                        .config
                        .measurement_ps_from_mm(mm)
                        .ok()
                });
            }
            MotionKind::Scanner => {
                let entry = self
                    .inventory
                    .scanners
                    .iter()
                    .find(|entry| entry.id == control.id);
                let config = if control.axis == 1 {
                    state.position.x_scanner_mm = native;
                    entry.and_then(|entry| entry.x.as_ref())
                } else {
                    state.position.y_scanner_mm = native;
                    entry.and_then(|entry| entry.y.as_ref())
                };
                state.position.physical[control.axis] = config
                    .zip(native)
                    .map(|(config, value)| config.sample_um_from_raw(value));
            }
        }
    }
}

#[cfg(all(test, feature = "gui-interaction-tests"))]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Interrupt {
        Release,
        FocusLoss,
        LinkLoss,
    }

    fn pointer(harness: &Harness<'_, App>, pos: egui::Pos2, pressed: bool) {
        harness.event(egui::Event::PointerMoved(pos));
        harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }

    fn toggle_link(harness: &Harness<'_, App>) {
        harness.event(egui::Event::Key {
            key: egui::Key::F8,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
    }

    fn run_trial(kind: MotionKind, axis_index: usize, interrupt: Interrupt) {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1566., 700.))
            .with_step_dt(0.04)
            .build_eframe(|cc| App::new_alignment_jog_trial(cc).unwrap());
        h.run_steps(2);
        let (id, arrow, start, stop) = match (kind, axis_index) {
            (MotionKind::Delay, _) => ("delay-1", "Delay+", "G:", "L:1"),
            (MotionKind::Scanner, 1) => ("scanner-1", "X+", "1TK1", "1ST"),
            (MotionKind::Scanner, _) => ("scanner-1", "Y+", "1TK1", "1ST"),
        };
        if kind == MotionKind::Scanner {
            h.get_by_label("Scanners").click();
            h.run_steps(3);
        }
        let arrow_pos = h
            .query_all_by_label(arrow)
            .map(|node| node.rect())
            .filter(|rect| rect.bottom() < 690.)
            .min_by(|a, b| a.top().total_cmp(&b.top()))
            .unwrap_or_else(|| panic!("missing {arrow}"))
            .center();
        let get_position = |app: &App| {
            let state = app.device_state(kind, id).unwrap();
            match kind {
                MotionKind::Delay => state.position.delay_stage_mm,
                MotionKind::Scanner if axis_index == 1 => state.position.x_scanner_mm,
                MotionKind::Scanner => state.position.y_scanner_mm,
            }
        };
        let before = get_position(h.state()).unwrap();
        h.event(egui::Event::PointerMoved(arrow_pos));
        h.run_steps(1);
        pointer(&h, arrow_pos, true);
        h.run_steps(16);
        let sim = h.state().alignment.jog_simulation.as_ref().unwrap();
        let axis = sim.axis.as_ref().unwrap();
        assert_eq!((&*axis.control.id, axis.control.axis), (id, axis_index));
        assert!(matches!(
            axis.session.phase(),
            JogPhase::Running { positive: true }
        ));
        assert!(axis.bus.log.iter().any(|command| command == start));
        assert!(axis.bus.physically_moving());
        assert!(get_position(h.state()).unwrap() > before);
        assert!(!h.state().controller.has_pending());

        match interrupt {
            Interrupt::Release => pointer(&h, egui::pos2(100., 110.), false),
            Interrupt::FocusLoss => {
                h.input_mut().focused = false;
                h.event(egui::Event::WindowFocused(false));
            }
            Interrupt::LinkLoss => toggle_link(&h),
        }
        h.run_steps(3);
        let sim = h.state().alignment.jog_simulation.as_ref().unwrap();
        let axis = sim.axis.as_ref().unwrap();
        assert!(h.state().alignment.held.is_none());
        let commands_after_stop = axis.bus.log.len();
        if interrupt == Interrupt::LinkLoss {
            assert!(matches!(axis.session.phase(), JogPhase::Unconfirmed { .. }));
            assert_eq!(get_position(h.state()), None);
            if kind == MotionKind::Delay {
                assert!(
                    axis.bus.physically_moving(),
                    "the lost STOP must not be simulated as successful"
                );
            }
            assert!(!axis.bus.log.iter().any(|command| command == stop));
            h.run_steps(12);
            let axis = h
                .state()
                .alignment
                .jog_simulation
                .as_ref()
                .unwrap()
                .axis
                .as_ref()
                .unwrap();
            if kind == MotionKind::Scanner {
                let last_setpoint = axis
                    .bus
                    .log
                    .iter()
                    .rev()
                    .find_map(|cmd| cmd.strip_prefix("1PA"))
                    .unwrap()
                    .parse::<f64>()
                    .unwrap();
                assert!(!axis.bus.physically_moving());
                assert!((axis.bus.position - last_setpoint).abs() < 1e-6);
            }
            pointer(&h, arrow_pos, false);
            h.run_steps(1);
            h.get_by_label("Restore link").click();
            h.run_steps(3);
            assert!(h.state().alignment.jog_simulation.as_ref().unwrap().link_up);
            pointer(&h, arrow_pos, true);
            h.run_steps(14);
            pointer(&h, arrow_pos, false);
            h.run_steps(2);
            assert_eq!(get_position(h.state()), None);
            assert!(
                h.state()
                    .alignment
                    .jog_simulation
                    .as_ref()
                    .unwrap()
                    .status()
                    .starts_with("Stop unconfirmed")
            );
        } else {
            let JogPhase::Stopped { position } = axis.session.phase() else {
                panic!("expected confirmed stop, got {:?}", axis.session.phase());
            };
            let expected = if kind == MotionKind::Delay {
                position / DELAY_PULSES_PER_MM
            } else {
                *position
            };
            assert_eq!(get_position(h.state()), Some(expected));
            assert!(!axis.bus.physically_moving());
            assert!(!axis.bus.tracking());
            assert_eq!(
                axis.bus
                    .log
                    .iter()
                    .filter(|command| *command == stop)
                    .count(),
                1
            );
            if kind == MotionKind::Scanner {
                assert_eq!(
                    axis.bus
                        .log
                        .iter()
                        .filter(|command| *command == "1TK0")
                        .count(),
                    1
                );
            }
            if interrupt == Interrupt::FocusLoss {
                h.input_mut().focused = true;
                h.event(egui::Event::WindowFocused(true));
                h.run_steps(14);
                pointer(&h, arrow_pos, false);
                h.run_steps(2);
            }
        }
        let axis = h
            .state()
            .alignment
            .jog_simulation
            .as_ref()
            .unwrap()
            .axis
            .as_ref()
            .unwrap();
        assert_eq!(
            axis.bus.log.len(),
            commands_after_stop,
            "no extra Step, STOP or re-arm"
        );
        assert!(!h.state().controller.has_pending());
    }

    #[test]
    fn signal_monitor_keeps_running_through_simulated_jog_release() {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1566., 700.))
            .with_step_dt(0.04)
            .build_eframe(|cc| App::new_alignment_jog_trial(cc).unwrap());
        h.run_steps(3);
        h.get_by_label("Settings…").click();
        h.run_steps(3);
        h.get_by_label("Continuous").click();
        h.run_steps(3);
        h.get_by_label("Done").click();
        h.run_steps(3);
        h.get_by_label("Record").click();
        let deadline = Instant::now() + Duration::from_secs(4);
        while (!h.state().snapshot.running || h.state().trace.is_empty())
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
            h.run_steps(1);
        }
        assert!(h.state().snapshot.running && h.state().monitor_run_until_stop);
        let arrow = h
            .query_all_by_label("Delay+")
            .map(|node| node.rect())
            .min_by(|a, b| a.top().total_cmp(&b.top()))
            .unwrap()
            .center();
        h.event(egui::Event::PointerMoved(arrow));
        h.run_steps(1);
        pointer(&h, arrow, true);
        h.run_steps(16);
        assert!(h.state().alignment.held.is_some());
        pointer(&h, arrow, false);
        h.run_steps(2);
        assert!(h.state().alignment.held.is_none());
        assert!(h.state().snapshot.running);
        assert!(
            h.state()
                .alignment
                .jog_simulation
                .as_ref()
                .unwrap()
                .status()
                .starts_with("Stopped")
        );
        h.get_by_label("Motion Only").click();
        h.run_steps(3);
        assert!(h.state().snapshot.running);
        assert!(!h.state().geometry.contains_key("alignment_monitor_plots"));
        h.get_by_label("Stop Recording").click();
        h.run_steps(1);
        let deadline = Instant::now() + Duration::from_secs(4);
        while h.state().snapshot.running && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
            h.run_steps(1);
        }
        assert!(!h.state().snapshot.running);
    }

    #[test]
    fn delay_arrow_release_focus_loss_and_link_loss() {
        for interrupt in [
            Interrupt::Release,
            Interrupt::FocusLoss,
            Interrupt::LinkLoss,
        ] {
            run_trial(MotionKind::Delay, 0, interrupt);
        }
    }

    #[test]
    fn scanner_x_and_y_arrow_release_focus_loss_and_link_loss() {
        for axis in [1, 2] {
            for interrupt in [
                Interrupt::Release,
                Interrupt::FocusLoss,
                Interrupt::LinkLoss,
            ] {
                run_trial(MotionKind::Scanner, axis, interrupt);
            }
        }
    }
}
