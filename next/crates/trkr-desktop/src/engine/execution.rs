//! Pure scan sequencing shared by preflight and acquisition.
//! No driver calls or cancellation resets belong here. The worker executes the
//! returned moves serially; each driver retains its existing stop/wait policy.
use trkr_core::{Axis, Plan, Target, Workflow};

pub(crate) fn active_axes(p: &Plan) -> [bool; 3] {
    let mut mask = [false; 3];
    if p.workflow == Workflow::SignalMonitor {
        return mask;
    }
    for axis in [Some(p.fast), p.workflow.is_2d().then_some(p.slow)]
        .into_iter()
        .flatten()
    {
        match axis {
            Axis::T => mask[0] = true,
            Axis::X => mask[1] = true,
            Axis::Y => mask[2] = true,
            Axis::U | Axis::V => {
                mask[1] = true;
                mask[2] = true;
            }
        }
    }
    mask
}
pub(super) struct ScanMove {
    pub(super) axis: Axis,
    pub(super) target: [Option<f64>; 3],
    pub(super) approach: [bool; 3],
}
pub(super) fn axis_mask(axis: Axis) -> [bool; 3] {
    match axis {
        Axis::T => [true, false, false],
        Axis::X => [false, true, false],
        Axis::Y => [false, false, true],
        Axis::U | Axis::V => [false, true, true],
    }
}
pub(super) fn masked(target: [Option<f64>; 3], mask: [bool; 3]) -> [Option<f64>; 3] {
    std::array::from_fn(|i| if mask[i] { target[i] } else { None })
}
pub(super) fn coupled_spatial(plan: &Plan) -> bool {
    plan.workflow.is_2d()
        && matches!(plan.fast, Axis::U | Axis::V)
        && matches!(plan.slow, Axis::U | Axis::V)
}
/// Python's Cartesian line sequence: slow completes before fast starts.
/// U/V maps are a Rust extension and require a coupled X/Y target.
pub(super) fn scan_moves(
    plan: &Plan,
    target: [Option<f64>; 3],
    line_start: bool,
    first_point: bool,
) -> Vec<ScanMove> {
    let axes = if plan.workflow.is_2d() && line_start && !coupled_spatial(plan) {
        vec![plan.slow, plan.fast]
    } else {
        vec![plan.fast]
    };
    axes.into_iter()
        .map(|axis| {
            let mask = axis_mask(axis);
            ScanMove {
                axis,
                target: masked(target, mask),
                approach: std::array::from_fn(|i| {
                    i > 0 && mask[i] && (first_point || (axis == plan.fast && line_start))
                }),
            }
        })
        .collect()
}
/// Legacy completion returns fast first, then slow; never re-enable a cancelled job.
pub(super) fn return_moves(plan: &Plan, target: [Option<f64>; 3]) -> Vec<[Option<f64>; 3]> {
    if coupled_spatial(plan) {
        return vec![target];
    }
    [Some(plan.fast), plan.workflow.is_2d().then_some(plan.slow)]
        .into_iter()
        .flatten()
        .map(|axis| masked(target, axis_mask(axis)))
        .filter(|target| target.iter().any(Option::is_some))
        .collect()
}
pub(super) fn axis_target(p: &Plan, fast: f64, slow: Option<f64>) -> [Option<f64>; 3] {
    let mut xyz = [None; 3];
    let mut uv = [0.; 2];
    let mut rotate = false;
    for (axis, value) in [(p.fast, Some(fast)), (p.slow, slow)] {
        if let Some(value) = value {
            match axis {
                Axis::T => xyz[0] = Some(value),
                Axis::X => xyz[1] = Some(value),
                Axis::Y => xyz[2] = Some(value),
                Axis::U => {
                    uv[0] = value;
                    rotate = true
                }
                Axis::V => {
                    uv[1] = value;
                    rotate = true
                }
            }
        }
    }
    if rotate {
        let (x, y) = trkr_core::rotated_to_xy(uv[0], uv[1], p.theta_deg);
        xyz[1] = Some(x);
        xyz[2] = Some(y);
    }
    xyz
}
pub(super) fn return_target(p: &Plan, last: &Target) -> Option<[Option<f64>; 3]> {
    if p.workflow == Workflow::SignalMonitor || !p.return_to_zero.into_iter().any(|x| x) {
        return None;
    }
    let fast = if p.return_to_zero[0] { 0. } else { last.fast };
    let slow = last.slow.map(|s| if p.return_to_zero[1] { 0. } else { s });
    let mut result = axis_target(p, fast, slow);
    if !matches!(p.fast, Axis::U | Axis::V) && !matches!(p.slow, Axis::U | Axis::V) {
        if !p.return_to_zero[0] {
            match p.fast {
                Axis::T => result[0] = None,
                Axis::X => result[1] = None,
                Axis::Y => result[2] = None,
                _ => {}
            }
        }
        if p.workflow.is_2d() && !p.return_to_zero[1] {
            match p.slow {
                Axis::T => result[0] = None,
                Axis::X => result[1] = None,
                Axis::Y => result[2] = None,
                _ => {}
            }
        }
    }
    Some(result)
}
// Spatial fast axes go straight to the new line's compensation approach after
// the slow axis moves. Do not first visit the uncompensated scan start.
pub(super) fn reset_fast_before_slow(p: &Plan) -> bool {
    p.workflow.is_2d()
        && p.return_fast_between_lines
        && p.fast == Axis::T
        && !matches!(p.slow, Axis::U | Axis::V)
}
pub(super) fn line_reset(p: &Plan, last: &Target) -> [Option<f64>; 3] {
    let mut target = axis_target(p, p.fast_range.min, None);
    if matches!(p.fast, Axis::U | Axis::V) {
        target = axis_target(p, p.fast_range.min, last.slow);
    }
    target
}
