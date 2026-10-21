//! Latest result for each scan axis/pair, with the calibration used to acquire it.
use crate::engine::Sample;
use std::path::PathBuf;
use trkr_core::{Axis, Plan, Workflow};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key {
    workflow: Workflow,
    fast: Axis,
    slow: Option<Axis>,
    theta: Option<u64>,
}
impl Key {
    fn same_axes(self, other: Self) -> bool {
        self.workflow == other.workflow && self.fast == other.fast && self.slow == other.slow
    }
    fn new(plan: &Plan) -> Self {
        let spatial = matches!(
            plan.workflow,
            Workflow::Srkr | Workflow::Strkr | Workflow::Srkr2d
        );
        let slow = plan.workflow.is_2d().then_some(plan.slow);
        let rotated = spatial
            && (matches!(plan.fast, Axis::U | Axis::V)
                || slow.is_some_and(|axis| matches!(axis, Axis::U | Axis::V)));
        Self {
            workflow: plan.workflow,
            fast: if spatial { plan.fast } else { Axis::T },
            slow,
            theta: rotated.then_some(if plan.theta_deg == 0. {
                0
            } else {
                plan.theta_deg.to_bits()
            }),
        }
    }
}

pub struct Trace {
    pub plan: Plan,
    pub samples: Vec<Sample>,
    pub progress: (usize, usize),
    pub output: Option<PathBuf>,
}
impl Trace {
    fn empty(plan: &Plan) -> Self {
        Self {
            plan: plan.clone(),
            samples: vec![],
            progress: (0, 0),
            output: None,
        }
    }
}

#[derive(Default)]
pub struct History {
    entries: Vec<Trace>,
}
impl History {
    pub fn same_selection(a: &Plan, b: &Plan) -> bool {
        Key::new(a) == Key::new(b)
    }

    /// Moves buffers rather than copying potentially large measurement results.
    pub fn select(&mut self, current: Option<Trace>, desired: &Plan) -> Trace {
        let key = Key::new(desired);
        if let Some(current) = current {
            let old = Key::new(&current.plan);
            if old == key {
                return current;
            }
            // Latest result per ordered axis pair; do not grow with every theta.
            // Merely viewing an unmeasured angle must not overwrite a recorded run.
            if !current.samples.is_empty() || current.output.is_some() || current.progress != (0, 0)
            {
                if let Some(index) = self
                    .entries
                    .iter()
                    .position(|trace| Key::new(&trace.plan).same_axes(old))
                {
                    self.entries[index] = current;
                } else {
                    self.entries.push(current);
                }
            }
        }
        self.entries
            .iter()
            .position(|trace| Key::new(&trace.plan) == key)
            .map(|index| self.entries.swap_remove(index))
            .unwrap_or_else(|| Trace::empty(desired))
    }

    /// SRKR's other panel uses its own stored zero and angle, never the new run's.
    pub fn srkr_axis(&self, plan: &Plan, axis: Axis) -> Option<&Trace> {
        if plan.workflow != Workflow::Srkr {
            return None;
        }
        let mut key = Key::new(plan);
        key.fast = axis;
        self.entries
            .iter()
            .find(|trace| Key::new(&trace.plan) == key)
    }
    pub fn clear(&mut self, workflow: Workflow) {
        self.entries.retain(|trace| trace.plan.workflow != workflow);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use trkr_core::Signal;
    use trkr_motion::Position;
    fn recorded(plan: Plan, value: f64) -> Trace {
        let target = plan.targets().unwrap()[0];
        Trace {
            samples: vec![Sample {
                target,
                signal: Signal::demo(value, 0., 0.),
                elapsed: value,
                position: Position {
                    physical: target.physical.map(Some),
                    ..Default::default()
                },
            }],
            plan,
            progress: (1, 41),
            output: Some(format!("run-{value}.csv").into()),
        }
    }
    #[test]
    fn srkr_keeps_each_axis_with_its_own_zero_and_only_replaces_the_repeated_axis() {
        let mut history = History::default();
        let x = Plan {
            workflow: Workflow::Srkr,
            fast: Axis::X,
            zero: [1., 2., 3.],
            ..Default::default()
        };
        let y = Plan {
            fast: Axis::Y,
            zero: [4., 5., 6.],
            ..x.clone()
        };
        let selected = history.select(Some(recorded(x.clone(), 1.)), &y);
        assert!(selected.samples.is_empty());
        assert_eq!(history.srkr_axis(&y, Axis::X).unwrap().plan.zero, x.zero);
        let selected = history.select(Some(recorded(y.clone(), 2.)), &x);
        assert_eq!(selected.samples[0].elapsed, 1.);
        assert_eq!(selected.progress, (1, 41));
        assert_eq!(selected.output, Some("run-1.csv".into()));
        let selected = history.select(Some(recorded(x.clone(), 3.)), &y);
        assert_eq!(selected.samples[0].elapsed, 2.);
        let selected = history.select(Some(selected), &x);
        assert_eq!(selected.samples[0].elapsed, 3.);
    }
    #[test]
    fn ordered_pairs_rotation_angles_and_workflows_do_not_mix() {
        let mut history = History::default();
        let mut plans = vec![];
        for workflow in [Workflow::Strkr, Workflow::Srkr2d] {
            let pairs = if workflow == Workflow::Strkr {
                vec![(Axis::T, Axis::X), (Axis::X, Axis::T), (Axis::T, Axis::U)]
            } else {
                vec![(Axis::X, Axis::Y), (Axis::Y, Axis::X), (Axis::U, Axis::V)]
            };
            for (fast, slow) in pairs {
                plans.push(Plan {
                    workflow,
                    fast,
                    slow,
                    theta_deg: 45.,
                    ..Default::default()
                });
            }
        }
        let mut current = None;
        for (i, plan) in plans.iter().enumerate() {
            assert!(history.select(current, plan).samples.is_empty());
            current = Some(recorded(plan.clone(), i as f64));
        }
        for (i, plan) in plans.iter().enumerate() {
            let selected = history.select(current, plan);
            assert_eq!(selected.samples[0].elapsed, i as f64);
            current = Some(selected);
        }
    }
    #[test]
    fn rotated_history_keeps_only_the_latest_result_per_axis_pair() {
        let mut history = History::default();
        let first = Plan {
            workflow: Workflow::Strkr,
            fast: Axis::U,
            slow: Axis::T,
            theta_deg: 0.,
            ..Default::default()
        };
        let mut current = Some(recorded(first.clone(), 0.));
        for angle in 1..360 {
            let plan = Plan {
                theta_deg: angle as f64,
                ..first.clone()
            };
            let empty = history.select(current, &plan);
            assert!(empty.samples.is_empty());
            assert!(history.entries.len() <= 1);
            current = Some(recorded(plan, angle as f64));
        }
        let empty = history.select(current, &first);
        assert!(empty.samples.is_empty());
        let latest = Plan {
            theta_deg: 359.,
            ..first
        };
        let restored = history.select(Some(empty), &latest);
        assert_eq!(restored.samples[0].elapsed, 359.);
        assert!(history.entries.is_empty());
    }
    #[test]
    fn clear_only_removes_the_current_workflow_and_signed_zero_theta_is_shared() {
        let mut history = History::default();
        let u = Plan {
            workflow: Workflow::Srkr,
            fast: Axis::U,
            theta_deg: -0.,
            ..Default::default()
        };
        let v = Plan {
            fast: Axis::V,
            theta_deg: 0.,
            ..u.clone()
        };
        let map = Plan {
            workflow: Workflow::Srkr2d,
            fast: Axis::X,
            slow: Axis::Y,
            ..Default::default()
        };
        let current = history.select(Some(recorded(u.clone(), 1.)), &v);
        assert!(history.srkr_axis(&v, Axis::U).is_some());
        let current = history.select(Some(recorded(current.plan, 3.)), &map);
        let current = history.select(Some(recorded(current.plan, 2.)), &u);
        history.clear(Workflow::Srkr2d);
        assert!(history.srkr_axis(&current.plan, Axis::V).is_some());
        assert!(history.select(Some(current), &map).samples.is_empty());
    }
}
