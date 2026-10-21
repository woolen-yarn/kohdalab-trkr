//! UI axis editing uses the core workflow contract without mutating an
//! imported plan, so an invalid saved pair remains visible until the user
//! deliberately changes an axis.
#[cfg(test)]
use trkr_core::Range;
use trkr_core::{Axis, Plan, Workflow};

pub fn fast_options(workflow: Workflow) -> &'static [Axis] {
    workflow.fast_options()
}

pub fn slow_options(workflow: Workflow, fast: Axis) -> &'static [Axis] {
    workflow.slow_options(fast)
}

pub fn valid_pair(workflow: Workflow, fast: Axis, slow: Axis) -> bool {
    workflow.valid_pair(fast, slow)
}

pub fn pair_warning(workflow: Workflow, fast: Axis, slow: Axis) -> Option<String> {
    if valid_pair(workflow, fast, slow) {
        None
    } else {
        Some(format!(
            "Invalid axes: Fast {} / Slow {} - select an axis",
            fast.name(),
            slow.name()
        ))
    }
}

/// Applies only an explicit fast-axis choice.  Ranges, origins and calibration
/// are intentionally untouched.
pub fn apply_fast_choice(plan: &mut Plan, chosen: Axis) {
    if !fast_options(plan.workflow).contains(&chosen) {
        return;
    }
    let previous_fast = plan.fast;
    plan.fast = chosen;
    match plan.workflow {
        Workflow::Srkr2d => plan.slow = slow_options(plan.workflow, chosen)[0],
        Workflow::Strkr if chosen == Axis::T && previous_fast != Axis::T => {
            plan.slow = previous_fast
        }
        Workflow::Strkr if chosen != Axis::T => plan.slow = Axis::T,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_pair_guard_matches_core_for_every_axis_pair_with_single_point_ranges() {
        for workflow in Workflow::ALL {
            for fast in Axis::ALL {
                for slow in Axis::ALL {
                    let plan = Plan {
                        workflow,
                        fast,
                        slow,
                        fast_range: Range {
                            min: 0.,
                            max: 0.,
                            step: 1.,
                        },
                        slow_range: Range {
                            min: 0.,
                            max: 0.,
                            step: 1.,
                        },
                        ..Plan::default()
                    };
                    assert_eq!(
                        valid_pair(workflow, fast, slow),
                        plan.targets().is_ok(),
                        "{workflow:?}: {fast:?}/{slow:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn fast_axis_changes_only_repair_after_user_choice() {
        let mut plan = Plan {
            workflow: Workflow::Strkr,
            fast: Axis::U,
            slow: Axis::T,
            ..Plan::default()
        };
        apply_fast_choice(&mut plan, Axis::T);
        assert_eq!((plan.fast, plan.slow), (Axis::T, Axis::U));
        apply_fast_choice(&mut plan, Axis::V);
        assert_eq!((plan.fast, plan.slow), (Axis::V, Axis::T));

        let mut map = Plan {
            workflow: Workflow::Srkr2d,
            fast: Axis::X,
            slow: Axis::Y,
            ..Plan::default()
        };
        apply_fast_choice(&mut map, Axis::U);
        assert_eq!((map.fast, map.slow), (Axis::U, Axis::V));
    }
    #[test]
    fn dropdowns_expose_only_safe_pairs_and_invalid_imports_are_identified() {
        for workflow in [Workflow::Strkr, Workflow::Srkr2d] {
            for &fast in fast_options(workflow) {
                for &slow in slow_options(workflow, fast) {
                    assert!(valid_pair(workflow, fast, slow));
                }
            }
        }
        assert_eq!(slow_options(Workflow::Srkr2d, Axis::U), &[Axis::V]);
        assert_eq!(slow_options(Workflow::Srkr2d, Axis::X), &[Axis::Y]);
        let mut plan = Plan {
            workflow: Workflow::Srkr2d,
            fast: Axis::T,
            slow: Axis::X,
            ..Plan::default()
        };
        let message = pair_warning(plan.workflow, plan.fast, plan.slow).unwrap();
        assert!(message.contains("Fast t / Slow x"));
        apply_fast_choice(&mut plan, Axis::T);
        assert_eq!(plan.fast, Axis::T);
        apply_fast_choice(&mut plan, Axis::U);
        assert_eq!((plan.fast, plan.slow), (Axis::U, Axis::V));
        assert!(pair_warning(plan.workflow, plan.fast, plan.slow).is_none());
    }
}
