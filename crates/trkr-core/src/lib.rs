//! Hardware-independent acquisition values and scan planning.
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

pub const MAX_AXIS_POINTS: usize = 100_000;
pub const MAX_TOTAL_POINTS: usize = 1_000_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Signal {
    pub x_v: f64,
    pub y_v: f64,
    pub r_v: f64,
    pub theta_deg: f64,
}

impl Signal {
    pub fn new(x_v: f64, y_v: f64, r_v: f64, theta_deg: f64) -> Result<Self, String> {
        if ![x_v, y_v, r_v, theta_deg].into_iter().all(f64::is_finite) || r_v < 0.0 {
            return Err("Lock-in returned a non-finite value or negative magnitude".into());
        }
        Ok(Self {
            x_v,
            y_v,
            r_v,
            theta_deg,
        })
    }

    pub fn demo(t: f64, x: f64, y: f64) -> Self {
        let envelope = (-t.max(0.0) / 150.0).exp() * (-(x * x + y * y) / 400.0).exp();
        let x_v = 1e-3 * envelope * (t * 0.07).cos();
        let y_v = 1e-3 * envelope * (t * 0.07).sin();
        Self {
            x_v,
            y_v,
            r_v: x_v.hypot(y_v),
            theta_deg: y_v.atan2(x_v).to_degrees(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DisplayMode {
    #[default]
    Xy,
    RTheta,
}

impl DisplayMode {
    pub fn labels(self) -> [&'static str; 2] {
        match self {
            Self::Xy => ["X (V)", "Y (V)"],
            Self::RTheta => ["R (V)", "θ (deg)"],
        }
    }
    pub fn values(self, signal: Signal) -> [f64; 2] {
        match self {
            Self::Xy => [signal.x_v, signal.y_v],
            Self::RTheta => [signal.r_v, signal.theta_deg],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    T,
    X,
    Y,
    U,
    V,
}

impl Axis {
    pub const ALL: [Self; 5] = [Self::T, Self::X, Self::Y, Self::U, Self::V];
    pub fn name(self) -> &'static str {
        match self {
            Self::T => "t",
            Self::X => "x",
            Self::Y => "y",
            Self::U => "u",
            Self::V => "v",
        }
    }
    pub fn unit(self) -> &'static str {
        if self == Self::T { "ps" } else { "µm" }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Workflow {
    SignalMonitor,
    Trkr,
    Srkr,
    Strkr,
    #[serde(rename = "srkr_2d")]
    Srkr2d,
}

/// Static properties and allowed axes for one measurement workflow.
///
/// This is the shared contract for frontends and execution validation.  The
/// values deliberately describe existing workflow behaviour; adding a new
/// workflow requires extending this table and its enum mapping together.
#[derive(Clone, Copy, Debug)]
pub struct WorkflowDefinition {
    pub index: usize,
    pub name: &'static str,
    pub key: &'static str,
    pub is_2d: bool,
    pub default_filename: &'static str,
    pub plot_count: usize,
    pub fast_options: &'static [Axis],
    slow_options: fn(Axis) -> &'static [Axis],
    valid_pair: fn(Axis, Axis) -> bool,
}

const AXES_ALL: [Axis; 5] = [Axis::T, Axis::X, Axis::Y, Axis::U, Axis::V];
const AXES_SPATIAL: [Axis; 4] = [Axis::X, Axis::Y, Axis::U, Axis::V];
const AXES_ONLY_T: [Axis; 1] = [Axis::T];
const AXES_ONLY_X: [Axis; 1] = [Axis::X];
const AXES_ONLY_Y: [Axis; 1] = [Axis::Y];
const AXES_ONLY_U: [Axis; 1] = [Axis::U];
const AXES_ONLY_V: [Axis; 1] = [Axis::V];

fn no_slow_options(_: Axis) -> &'static [Axis] {
    &[]
}

fn strkr_slow_options(fast: Axis) -> &'static [Axis] {
    if fast == Axis::T {
        &AXES_SPATIAL
    } else {
        &AXES_ONLY_T
    }
}

fn srkr_2d_slow_options(fast: Axis) -> &'static [Axis] {
    match fast {
        Axis::X => &AXES_ONLY_Y,
        Axis::Y => &AXES_ONLY_X,
        Axis::U => &AXES_ONLY_V,
        Axis::V => &AXES_ONLY_U,
        Axis::T => &[],
    }
}

fn monitor_valid_pair(_: Axis, _: Axis) -> bool {
    true
}

fn trkr_valid_pair(fast: Axis, _: Axis) -> bool {
    fast == Axis::T
}

fn srkr_valid_pair(fast: Axis, _: Axis) -> bool {
    fast != Axis::T
}

fn strkr_valid_pair(fast: Axis, slow: Axis) -> bool {
    (fast == Axis::T) != (slow == Axis::T)
}

fn srkr_2d_valid_pair(fast: Axis, slow: Axis) -> bool {
    srkr_2d_slow_options(fast).contains(&slow)
}

const WORKFLOW_DEFINITIONS: [WorkflowDefinition; 5] = [
    WorkflowDefinition {
        index: 0,
        name: "Signal Monitor",
        key: "signal_monitor",
        is_2d: false,
        default_filename: "signal_monitor_run",
        plot_count: 2,
        fast_options: &AXES_ONLY_T,
        slow_options: no_slow_options,
        valid_pair: monitor_valid_pair,
    },
    WorkflowDefinition {
        index: 1,
        name: "TRKR",
        key: "trkr",
        is_2d: false,
        default_filename: "trkr_run",
        plot_count: 2,
        fast_options: &AXES_ONLY_T,
        slow_options: no_slow_options,
        valid_pair: trkr_valid_pair,
    },
    WorkflowDefinition {
        index: 2,
        name: "SRKR",
        key: "srkr",
        is_2d: false,
        default_filename: "srkr_run",
        plot_count: 4,
        fast_options: &AXES_SPATIAL,
        slow_options: no_slow_options,
        valid_pair: srkr_valid_pair,
    },
    WorkflowDefinition {
        index: 3,
        name: "STRKR",
        key: "strkr",
        is_2d: true,
        default_filename: "strkr_run",
        plot_count: 4,
        fast_options: &AXES_ALL,
        slow_options: strkr_slow_options,
        valid_pair: strkr_valid_pair,
    },
    WorkflowDefinition {
        index: 4,
        name: "SRKR 2D",
        key: "srkr_2d",
        is_2d: true,
        default_filename: "srkr_2d_run",
        plot_count: 4,
        fast_options: &AXES_SPATIAL,
        slow_options: srkr_2d_slow_options,
        valid_pair: srkr_2d_valid_pair,
    },
];

impl WorkflowDefinition {
    pub fn slow_options(self, fast: Axis) -> &'static [Axis] {
        (self.slow_options)(fast)
    }
    pub fn valid_pair(self, fast: Axis, slow: Axis) -> bool {
        (self.valid_pair)(fast, slow)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Coordinate {
    #[default]
    Measurement,
    #[serde(alias = "control")]
    Interface,
    #[serde(alias = "device")]
    Instrument,
}
impl Workflow {
    pub const ALL: [Self; 5] = [
        Self::SignalMonitor,
        Self::Trkr,
        Self::Srkr,
        Self::Strkr,
        Self::Srkr2d,
    ];
    pub fn descriptor(self) -> &'static WorkflowDefinition {
        match self {
            Self::SignalMonitor => &WORKFLOW_DEFINITIONS[0],
            Self::Trkr => &WORKFLOW_DEFINITIONS[1],
            Self::Srkr => &WORKFLOW_DEFINITIONS[2],
            Self::Strkr => &WORKFLOW_DEFINITIONS[3],
            Self::Srkr2d => &WORKFLOW_DEFINITIONS[4],
        }
    }
    pub fn index(self) -> usize {
        self.descriptor().index
    }
    pub fn name(self) -> &'static str {
        self.descriptor().name
    }
    pub fn key(self) -> &'static str {
        self.descriptor().key
    }
    pub fn is_2d(self) -> bool {
        self.descriptor().is_2d
    }
    pub fn default_filename(self) -> &'static str {
        self.descriptor().default_filename
    }
    pub fn fast_options(self) -> &'static [Axis] {
        self.descriptor().fast_options
    }
    pub fn slow_options(self, fast: Axis) -> &'static [Axis] {
        self.descriptor().slow_options(fast)
    }
    pub fn valid_pair(self, fast: Axis, slow: Axis) -> bool {
        self.descriptor().valid_pair(fast, slow)
    }
    pub fn plot_count(self) -> usize {
        self.descriptor().plot_count
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Range {
    pub min: f64,
    pub max: f64,
    pub step: f64,
}
impl Range {
    /// Matches Python build_range_points: signed step, inclusive aligned endpoint,
    /// nine decimal places, and no artificial final short step.
    pub fn points(self) -> Result<Vec<f64>, String> {
        if ![self.min, self.max, self.step]
            .into_iter()
            .all(f64::is_finite)
            || self.step == 0.0
            || (self.max - self.min) * self.step < 0.0
        {
            return Err(
                "Range requires finite values and a nonzero step toward the endpoint".into(),
            );
        }
        let count = (((self.max - self.min).abs() / self.step.abs()) + 1e-9).floor() + 1.0;
        if !count.is_finite() || count > MAX_AXIS_POINTS as f64 {
            return Err("Too many points on one axis".into());
        }
        Ok((0..count as usize)
            .map(|i| ((self.min + i as f64 * self.step) * 1e9).round_ties_even() / 1e9)
            .collect())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub experiment: serde_json::Map<String, serde_json::Value>,
    pub workflow: Workflow,
    pub fast: Axis,
    pub slow: Axis,
    pub fast_range: Range,
    pub slow_range: Range,
    pub interval_s: f64,
    pub n_points: usize,
    pub theta_deg: f64,
    /// Physical origin in ps, µm, µm. Targets are measurement coordinates.
    pub zero: [f64; 3],
    #[serde(default)]
    pub coordinate: Coordinate,
    #[serde(default = "return_both")]
    pub return_to_zero: [bool; 2],
    #[serde(default = "yes")]
    pub return_fast_between_lines: bool,
}
fn return_both() -> [bool; 2] {
    [true; 2]
}
fn yes() -> bool {
    true
}
impl Default for Plan {
    fn default() -> Self {
        Self {
            experiment: Default::default(),
            workflow: Workflow::SignalMonitor,
            fast: Axis::T,
            slow: Axis::X,
            fast_range: Range {
                min: -50.0,
                max: 300.0,
                step: 5.0,
            },
            slow_range: Range {
                min: -30.0,
                max: 30.0,
                step: 2.0,
            },
            interval_s: 0.2,
            n_points: 360,
            theta_deg: 0.0,
            zero: [0.0; 3],
            coordinate: Coordinate::Measurement,
            return_to_zero: [true; 2],
            return_fast_between_lines: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub fast: f64,
    pub slow: Option<f64>,
    pub corrected: [f64; 3],
    pub physical: [f64; 3],
    pub line: usize,
}

impl Plan {
    pub fn targets(&self) -> Result<Vec<Target>, String> {
        if self.coordinate != Coordinate::Measurement
            && (self.workflow.is_2d() || matches!(self.fast, Axis::U | Axis::V))
        {
            return Err("2D and rotated scans require measurement coordinates".into());
        }
        if !self.interval_s.is_finite()
            || !(0.0..=3600.0).contains(&self.interval_s)
            || !self.theta_deg.is_finite()
            || !self.zero.into_iter().all(f64::is_finite)
        {
            return Err("Invalid interval, origin, or rotation".into());
        }
        let (fast, slow) = match self.workflow {
            Workflow::SignalMonitor => {
                if !(1..=MAX_TOTAL_POINTS).contains(&self.n_points) {
                    return Err("Invalid monitor point count".into());
                }
                (
                    (0..self.n_points)
                        .map(|i| i as f64 * self.interval_s)
                        .collect(),
                    vec![None],
                )
            }
            Workflow::Trkr => {
                if self.fast != Axis::T {
                    return Err("TRKR fast axis must be t".into());
                }
                (self.fast_range.points()?, vec![None])
            }
            Workflow::Srkr => {
                if self.fast == Axis::T {
                    return Err("SRKR requires a spatial axis".into());
                }
                (self.fast_range.points()?, vec![None])
            }
            Workflow::Strkr | Workflow::Srkr2d => {
                if self.fast == self.slow {
                    return Err("Fast and slow axes must differ".into());
                }
                if self.workflow == Workflow::Strkr {
                    if !self.workflow.valid_pair(self.fast, self.slow) {
                        return Err("STRKR combines t with a spatial axis".into());
                    }
                } else if !self.workflow.valid_pair(self.fast, self.slow) {
                    return Err("SRKR 2D requires x/y or u/v".into());
                }
                (
                    self.fast_range.points()?,
                    self.slow_range.points()?.into_iter().map(Some).collect(),
                )
            }
        };
        if fast.len().saturating_mul(slow.len()) > MAX_TOTAL_POINTS {
            return Err("Too many total scan points".into());
        }
        let mut result = Vec::with_capacity(fast.len() * slow.len());
        for (line, &s) in slow.iter().enumerate() {
            for &f in &fast {
                let mut corrected = [0.0; 3];
                let mut uv = [0.0; 2];
                let mut set = |axis, v| match axis {
                    Axis::T => corrected[0] = v,
                    Axis::X => corrected[1] = v,
                    Axis::Y => corrected[2] = v,
                    Axis::U => uv[0] = v,
                    Axis::V => uv[1] = v,
                };
                if self.workflow != Workflow::SignalMonitor {
                    set(self.fast, f);
                }
                if let Some(s) = s {
                    set(self.slow, s);
                }
                if matches!(self.fast, Axis::U | Axis::V)
                    || (s.is_some() && matches!(self.slow, Axis::U | Axis::V))
                {
                    let (x, y) = rotated_to_xy(uv[0], uv[1], self.theta_deg);
                    corrected[1] = x;
                    corrected[2] = y;
                }
                let physical = std::array::from_fn(|i| {
                    corrected[i]
                        + if self.coordinate == Coordinate::Measurement {
                            self.zero[i]
                        } else {
                            0.0
                        }
                });
                if !physical.into_iter().all(f64::is_finite) {
                    return Err("Physical target overflow".into());
                }
                result.push(Target {
                    fast: f,
                    slow: s,
                    corrected,
                    physical,
                    line,
                });
            }
        }
        Ok(result)
    }
}

pub fn rotated_to_xy(u: f64, v: f64, theta_deg: f64) -> (f64, f64) {
    let (s, c) = theta_deg.to_radians().sin_cos();
    (u * c - v * s, u * s + v * c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_definitions_preserve_existing_contract() {
        let expected = [
            (
                "Signal Monitor",
                "signal_monitor",
                false,
                "signal_monitor_run",
                2,
            ),
            ("TRKR", "trkr", false, "trkr_run", 2),
            ("SRKR", "srkr", false, "srkr_run", 4),
            ("STRKR", "strkr", true, "strkr_run", 4),
            ("SRKR 2D", "srkr_2d", true, "srkr_2d_run", 4),
        ];
        for (index, workflow) in Workflow::ALL.into_iter().enumerate() {
            let definition = workflow.descriptor();
            let (name, key, is_2d, filename, plot_count) = expected[index];
            assert_eq!(definition.index, index);
            assert_eq!(workflow.index(), index);
            assert_eq!(workflow.name(), name);
            assert_eq!(workflow.key(), key);
            assert_eq!(workflow.is_2d(), is_2d);
            assert_eq!(workflow.default_filename(), filename);
            assert_eq!(workflow.plot_count(), plot_count);
        }
    }

    #[test]
    fn workflow_axis_contract_matches_plan_validation_for_every_pair() {
        let single_point = Range {
            min: 0.,
            max: 0.,
            step: 1.,
        };
        for workflow in Workflow::ALL {
            for fast in Axis::ALL {
                for slow in Axis::ALL {
                    let plan = Plan {
                        workflow,
                        fast,
                        slow,
                        fast_range: single_point,
                        slow_range: single_point,
                        ..Plan::default()
                    };
                    assert_eq!(
                        workflow.valid_pair(fast, slow),
                        plan.targets().is_ok(),
                        "{workflow:?}: {fast:?}/{slow:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn workflow_dropdown_options_contain_only_valid_pairs() {
        for workflow in Workflow::ALL {
            for &fast in workflow.fast_options() {
                for &slow in workflow.slow_options(fast) {
                    assert!(workflow.valid_pair(fast, slow));
                }
            }
        }
        assert_eq!(Workflow::Srkr2d.slow_options(Axis::U), &[Axis::V]);
        assert_eq!(Workflow::Srkr2d.slow_options(Axis::X), &[Axis::Y]);
    }

    #[test]
    fn range_parity() {
        assert_eq!(
            Range {
                min: 0.,
                max: 0.3,
                step: 0.1
            }
            .points()
            .unwrap(),
            vec![0., 0.1, 0.2, 0.3]
        );
        assert_eq!(
            Range {
                min: 1.,
                max: 0.,
                step: -0.4
            }
            .points()
            .unwrap(),
            vec![1., 0.6, 0.2]
        );
        for r in [
            Range {
                min: 0.,
                max: 1.,
                step: -1.,
            },
            Range {
                min: 0.,
                max: 1.,
                step: 0.,
            },
            Range {
                min: 0.,
                max: f64::NAN,
                step: 1.,
            },
            Range {
                min: 0.,
                max: 1.,
                step: 1e-9,
            },
        ] {
            assert!(r.points().is_err());
        }
    }
    #[test]
    fn rotated_grid_is_slow_major_and_uses_origin() {
        let p = Plan {
            workflow: Workflow::Srkr2d,
            fast: Axis::U,
            slow: Axis::V,
            fast_range: Range {
                min: 0.,
                max: 1.,
                step: 1.,
            },
            slow_range: Range {
                min: 0.,
                max: 1.,
                step: 1.,
            },
            theta_deg: 90.,
            zero: [10., 20., 30.],
            ..Plan::default()
        };
        let t = p.targets().unwrap();
        assert_eq!(t.len(), 4);
        assert_eq!(t[2].line, 1);
        assert!((t[1].physical[2] - 31.).abs() < 1e-12);
        assert!((t[2].physical[1] - 19.).abs() < 1e-12);
    }
    #[test]
    fn rejects_invalid_combinations_and_signals() {
        let p = Plan {
            workflow: Workflow::Strkr,
            fast: Axis::X,
            slow: Axis::Y,
            ..Plan::default()
        };
        assert!(p.targets().is_err());
        assert!(Signal::new(f64::NAN, 0., 0., 0.).is_err());
        let s = Signal::new(1., 2., 3., -120.).unwrap();
        assert_eq!(DisplayMode::RTheta.values(s), [3., -120.]);
        assert_eq!(DisplayMode::Xy.values(s), [1., 2.]);
    }
}
