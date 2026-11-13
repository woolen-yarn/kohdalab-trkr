//! Brief, device-scoped feedback. A local edit is never reported as hardware acknowledgement.
use super::*;
use std::collections::HashMap;

#[derive(Clone)]
enum State {
    Edited(Vec<usize>),
    DirectionPending,
    DirectionUpdated,
    DirectionFailed(String),
}
#[derive(Default)]
pub(super) struct Feedback {
    entries: HashMap<String, (State, Instant)>,
}
impl Feedback {
    fn set(&mut self, key: String, state: State) {
        self.entries.retain(|_, (state, time)| {
            matches!(state, State::DirectionPending) || time.elapsed() < Duration::from_secs(5)
        });
        self.entries.insert(key, (state, Instant::now()));
    }
    fn active(&self, key: &str) -> Option<&State> {
        self.entries
            .get(key)
            .filter(|(state, time)| {
                matches!(state, State::DirectionPending) || time.elapsed() < Duration::from_secs(5)
            })
            .map(|(state, _)| state)
    }
    pub(super) fn clear(&mut self) {
        self.entries.clear();
    }
    pub(super) fn pending_direction(&mut self, kind: engine::MotionKind, id: &str, axis: usize) {
        self.set(key(kind, id, axis), State::DirectionPending);
    }
    pub(super) fn confirm_direction(&mut self, kind: engine::MotionKind, id: &str) {
        for axis in if kind == engine::MotionKind::Delay {
            vec![0]
        } else {
            vec![1, 2]
        } {
            let key = key(kind, id, axis);
            if matches!(self.active(&key), Some(State::DirectionPending)) {
                self.set(key, State::DirectionUpdated);
            }
        }
    }
    pub(super) fn fail_direction(
        &mut self,
        kind: engine::MotionKind,
        id: &str,
        axis: usize,
        error: &str,
    ) {
        if let Some((state, time)) = self.entries.get_mut(&key(kind, id, axis))
            && matches!(state, State::DirectionPending)
        {
            *state = State::DirectionFailed(error.into());
            *time = Instant::now();
        }
    }
    pub(super) fn caption(&self, key: &str, axis: usize) -> Option<(String, Color32, String)> {
        let (text,color,help)=match self.active(key)? {
            State::Edited(fields) => (
                if fields.len()==1 { format!("{} changed · next Connect", field_names(axis)[fields[0]]) } else { "Settings changed · next Connect".into() },
                Color32::from_rgb(114,170,247),
                "Local settings changed. The next Connect uses these settings. Config → Save keeps them in the JSON file.".into(),
            ),
            State::DirectionPending => ("Updating direction…".into(),Color32::from_rgb(237,174,74),"Waiting for the controller. The displayed direction changes only after acknowledgement.".into()),
            State::DirectionUpdated => ("Direction updated".into(),Color32::from_rgb(109,219,210),"Controller update confirmed; current coordinates preserved. Config → Save keeps the new direction and origin.".into()),
            State::DirectionFailed(error) => ("Direction failed".into(),Color32::from_rgb(255,115,115),error.clone()),
        };
        Some((text, color, help))
    }
    pub(super) fn direction_caption(
        &self,
        kind: engine::MotionKind,
        id: &str,
        axis: usize,
    ) -> Option<(String, Color32, String)> {
        self.caption(&key(kind, id, axis), axis)
    }
    pub(super) fn paint(
        &self,
        ui: &egui::Ui,
        geometry: &precision::Geometry,
        key: &str,
        axis: usize,
    ) {
        let fields = match self.active(key) {
            Some(State::Edited(fields)) => fields.as_slice(),
            Some(State::DirectionPending | State::DirectionUpdated | State::DirectionFailed(_)) => {
                &[4]
            }
            None => return,
        };
        let prefix = ["t", "x", "y"][axis];
        let ids = if axis == 0 {
            [
                "t_controller_combo".into(),
                "t_stage_combo".into(),
                "t_port_combo".into(),
                "t_axis_selector".into(),
                "t_direction_spin".into(),
                "t_round_trips".into(),
            ]
        } else {
            [
                format!("{prefix}_controller_combo"),
                format!("{prefix}_actuator_combo"),
                format!("{prefix}_port_combo"),
                format!("{prefix}_axis_spin"),
                format!("{prefix}_direction"),
                format!("{prefix}_scale_spin"),
            ]
        };
        let color = self.caption(key, axis).unwrap().1;
        for &field in fields {
            if let Some([x, y, w, h]) = geometry.get(&ids[field]) {
                ui.painter().rect_stroke(
                    precision::rect(*x, *y, *w, *h).expand(1.),
                    6.,
                    Stroke::new(1.5_f32, color),
                    egui::StrokeKind::Outside,
                );
            }
        }
    }
}
fn key(kind: engine::MotionKind, id: &str, axis: usize) -> String {
    format!("{kind:?}:{id}:{axis}")
}
fn field_names(axis: usize) -> [&'static str; 6] {
    [
        "Controller",
        if axis == 0 { "Stage" } else { "Actuator" },
        "Port",
        "Axis",
        "Direction",
        if axis == 0 { "Round trips" } else { "Scale" },
    ]
}
impl App {
    pub(super) fn motion_feedback_key(&self, axis: usize) -> String {
        if axis == 0 {
            key(
                engine::MotionKind::Delay,
                &self.inventory.selected.delay,
                axis,
            )
        } else {
            key(
                engine::MotionKind::Scanner,
                &self.inventory.selected.scanner,
                axis,
            )
        }
    }
    pub(super) fn motion_editor_values(&self, axis: usize) -> [String; 6] {
        if axis == 0 {
            [
                self.stage_model.clone(),
                self.stage_preset.clone(),
                self.stage_port.clone(),
                self.stage_axis.to_string(),
                self.stage_direction.to_string(),
                self.stage_round_trips.to_string(),
            ]
        } else {
            let i = axis - 1;
            [
                self.scanner_model[i].clone(),
                self.scanner_actuator[i].clone(),
                self.scanner_port[i].clone(),
                self.scanner_axis[i].clone(),
                self.scanner_direction[i].to_string(),
                self.scanner_scale[i].to_string(),
            ]
        }
    }
    pub(super) fn record_motion_edits(&mut self, axis: usize, before: [String; 6]) {
        let fields: Vec<_> = before
            .iter()
            .zip(self.motion_editor_values(axis))
            .enumerate()
            .filter_map(|(i, (before, after))| (*before != after).then_some(i))
            .collect();
        if !fields.is_empty() {
            self.device_feedback
                .set(self.motion_feedback_key(axis), State::Edited(fields));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unrelated_background_error_does_not_fail_direction_update() {
        let mut app = App::with_context(&egui::Context::default(), false);
        let id = app.inventory.selected.scanner.clone();
        app.device_feedback
            .pending_direction(engine::MotionKind::Scanner, &id, 1);
        let (send, receive) = std::sync::mpsc::sync_channel(8);
        app.controller.events = receive;
        send.send(Event::Error("Lock-in read failed".into()))
            .unwrap();
        app.receive();
        assert!(matches!(
            app.device_feedback.active(&app.motion_feedback_key(1)),
            Some(State::DirectionPending)
        ));
        send.send(Event::DirectionFailed {
            kind: engine::MotionKind::Scanner,
            id,
            axis: 1,
            error: "Position unavailable".into(),
        })
        .unwrap();
        app.receive();
        assert_eq!(
            app.device_feedback
                .caption(&app.motion_feedback_key(1), 1)
                .unwrap()
                .2,
            "Position unavailable"
        );
    }
    #[test]
    fn local_edits_do_not_claim_hardware_acknowledgement() {
        let mut app = App::with_context(&egui::Context::default(), false);
        let before = app.motion_editor_values(0);
        app.stage_port = "COM8".into();
        app.record_motion_edits(0, before);
        assert!(
            app.device_feedback
                .caption(&app.motion_feedback_key(0), 0)
                .unwrap()
                .0
                .contains("next Connect")
        );
        assert!(!app.controller.has_pending());
        assert!(
            app.device_feedback
                .caption(&app.motion_feedback_key(1), 1)
                .is_none()
        );
    }
    #[test]
    fn direction_waits_for_matching_confirmation_and_reports_failure() {
        let mut feedback = Feedback::default();
        feedback.pending_direction(engine::MotionKind::Scanner, "scanner-1", 1);
        feedback.confirm_direction(engine::MotionKind::Scanner, "scanner-2");
        let key = key(engine::MotionKind::Scanner, "scanner-1", 1);
        assert!(matches!(
            feedback.active(&key),
            Some(State::DirectionPending)
        ));
        feedback.confirm_direction(engine::MotionKind::Scanner, "scanner-1");
        assert!(matches!(
            feedback.active(&key),
            Some(State::DirectionUpdated)
        ));
        feedback.pending_direction(engine::MotionKind::Scanner, "scanner-1", 1);
        feedback.fail_direction(
            engine::MotionKind::Scanner,
            "scanner-2",
            1,
            "unrelated read failed",
        );
        assert!(matches!(
            feedback.active(&key),
            Some(State::DirectionPending)
        ));
        feedback.fail_direction(engine::MotionKind::Scanner, "scanner-1", 1, "read failed");
        assert_eq!(feedback.caption(&key, 1).unwrap().2, "read failed");
        feedback.clear();
        assert!(feedback.active(&key).is_none());
    }
}
