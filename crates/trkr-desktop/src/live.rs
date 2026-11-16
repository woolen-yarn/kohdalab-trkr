//! Display-only live history, isolated by acquisition source and connection epoch.
use crate::engine::ReadingOrigin;
use std::{collections::VecDeque, time::Instant};
use trkr_core::{DisplayMode, Signal};

pub struct History {
    samples: VecDeque<(f64, Signal)>,
    origin: Option<ReadingOrigin>,
    unit: String,
    started: Instant,
}
impl Default for History {
    fn default() -> Self {
        Self {
            samples: VecDeque::new(),
            origin: None,
            unit: String::new(),
            started: Instant::now(),
        }
    }
}
impl History {
    pub fn clear(&mut self) {
        self.samples.clear();
        self.origin = None;
        self.unit.clear();
        self.started = Instant::now();
    }
    pub fn connection_changed(&mut self, previous: bool, current: bool) {
        if previous != current {
            self.clear();
        }
    }
    pub fn push(&mut self, signal: Signal, origin: ReadingOrigin, unit: &str) {
        let unit = if unit.is_empty() { "unknown" } else { unit };
        if self.origin != Some(origin) || self.unit != unit {
            self.clear();
            self.origin = Some(origin);
            self.unit.push_str(unit);
        }
        let now = self.started.elapsed().as_secs_f64();
        self.samples.push_back((now, signal));
        while self.samples.len() > 1024 || self.samples.front().is_some_and(|(t, _)| now - t > 30.)
        {
            self.samples.pop_front();
        }
    }
    pub fn newest(&self, active: bool) -> f64 {
        if active {
            self.started.elapsed().as_secs_f64()
        } else {
            self.samples
                .back()
                .map(|(time, _)| *time)
                .unwrap_or_else(|| self.started.elapsed().as_secs_f64())
        }
    }
    pub fn age(&self) -> Option<f64> {
        self.samples
            .back()
            .map(|(time, _)| (self.started.elapsed().as_secs_f64() - time).max(0.))
    }
    pub fn origin(&self) -> Option<ReadingOrigin> {
        self.origin
    }
    pub fn unit(&self) -> Option<&str> {
        self.origin.map(|_| self.unit.as_str())
    }

    pub fn signal(&self) -> Option<Signal> {
        self.samples.back().map(|(_, s)| *s)
    }
    pub fn recent(&self) -> bool {
        self.samples
            .back()
            .is_some_and(|(t, _)| self.started.elapsed().as_secs_f64() - t < 2.)
    }
    pub fn points_at(
        &self,
        mode: DisplayMode,
        channel: usize,
        voltage_scale: f64,
        newest: f64,
    ) -> Vec<[f64; 2]> {
        let scale = if mode == DisplayMode::RTheta && channel == 1 {
            1.
        } else {
            voltage_scale
        };
        self.samples
            .iter()
            .filter(|(time, _)| newest - time <= 30.)
            .map(|(time, signal)| [time - newest, mode.values(*signal)[channel] * scale])
            .filter(|point| point[1].is_finite())
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn signal() -> Signal {
        Signal::new(3e-6, 4e-6, 5e-6, 53.130102).unwrap()
    }
    #[test]
    fn a_unit_change_clears_the_previous_live_values() {
        let mut history = History::default();
        history.push(signal(), ReadingOrigin::Hardware, "V");
        history.push(
            Signal::new(1e-12, 0., 1e-12, 0.).unwrap(),
            ReadingOrigin::Hardware,
            "A",
        );
        assert_eq!(history.unit(), Some("A"));
        let points = history.points_at(DisplayMode::Xy, 0, 1e12, history.newest(false));
        assert_eq!(points, [[0., 1.]]);
        history.push(signal(), ReadingOrigin::Hardware, "");
        assert_eq!(history.unit(), Some("unknown"));
        assert_eq!(history.samples.len(), 1);
    }

    #[test]
    fn source_changes_and_connection_epochs_cannot_mix_live_history() {
        let mut h = History::default();
        h.push(signal(), ReadingOrigin::Demo, "V");
        h.push(signal(), ReadingOrigin::Demo, "V");
        assert_eq!(h.samples.len(), 2);
        h.push(signal(), ReadingOrigin::Hardware, "V");
        assert_eq!(h.samples.len(), 1);
        h.connection_changed(true, true);
        assert_eq!(h.samples.len(), 1);
        h.connection_changed(true, false);
        assert!(h.signal().is_none());
        h.push(signal(), ReadingOrigin::Replay, "V");
        h.connection_changed(false, true);
        assert!(h.signal().is_none());
        h.push(signal(), ReadingOrigin::Hardware, "V");
        assert_eq!(h.origin(), Some(ReadingOrigin::Hardware));
    }
    #[test]
    fn display_switch_preserves_voltage_scaling_and_theta_degrees() {
        let mut h = History::default();
        h.push(signal(), ReadingOrigin::Hardware, "V");
        assert!((h.points_at(DisplayMode::Xy, 0, 1e6, h.newest(false))[0][1] - 3.).abs() < 1e-12);
        assert!((h.points_at(DisplayMode::Xy, 1, 1e6, h.newest(false))[0][1] - 4.).abs() < 1e-12);
        assert!(
            (h.points_at(DisplayMode::RTheta, 0, 1e6, h.newest(false))[0][1] - 5.).abs() < 1e-12
        );
        assert!(
            (h.points_at(DisplayMode::RTheta, 1, 1e6, h.newest(false))[0][1] - 53.130102).abs()
                < 1e-12
        );
        assert_eq!(h.signal().unwrap().x_v, 3e-6);
    }
    #[test]
    fn held_history_uses_its_final_timestamp_and_reports_age() {
        let mut h = History::default();
        h.push(signal(), ReadingOrigin::Hardware, "V");
        let held = h.newest(false);
        assert!(held >= 0.);
        assert!(h.age().is_some());
        assert_eq!(h.points_at(DisplayMode::Xy, 0, 1e6, held).len(), 1);
        h.started -= std::time::Duration::from_secs(60);
        assert!(h.age().unwrap() >= 60.);
        assert_eq!(
            h.points_at(DisplayMode::Xy, 0, 1e6, h.newest(false)).len(),
            1
        );
        assert!(
            h.points_at(DisplayMode::Xy, 0, 1e6, h.newest(true))
                .is_empty()
        );
    }
    #[test]
    fn unacquired_pair_has_no_live_plot_points() {
        let mut history = History::default();
        history.push(
            Signal::from_pair(DisplayMode::Xy, 1., 2.).unwrap(),
            ReadingOrigin::Hardware,
            "V",
        );
        assert_eq!(
            history
                .points_at(DisplayMode::Xy, 0, 1., history.newest(false))
                .len(),
            1
        );
        assert!(
            history
                .points_at(DisplayMode::RTheta, 0, 1., history.newest(false))
                .is_empty()
        );
        assert!(
            history
                .points_at(DisplayMode::RTheta, 1, 1., history.newest(false))
                .is_empty()
        );
    }
}
