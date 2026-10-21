//! Typed worker states and failures. Legacy GUI/JSON strings are rendered only
//! at the boundary, so changing the internal model does not change the API.
use std::fmt;
use trkr_core::Axis;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MotionAction {
    Moving,
    Initializing,
    Returning,
}
impl MotionAction {
    fn label(self) -> &'static str {
        match self {
            Self::Moving => "Moving",
            Self::Initializing => "Initializing",
            Self::Returning => "Returning",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Preparing,
    Measuring,
    Waiting,
    Reading,
    Stopped,
    Complete,
    Error,
    Motion {
        action: MotionAction,
        axes: Vec<Axis>,
    },
}
impl Phase {
    pub fn motion(action: MotionAction, targets: [Option<f64>; 3]) -> Self {
        Self::Motion {
            action,
            axes: [Axis::T, Axis::X, Axis::Y]
                .into_iter()
                .zip(targets)
                .filter_map(|(axis, target)| target.map(|_| axis))
                .collect(),
        }
    }
    pub fn moving(axes: impl IntoIterator<Item = Axis>) -> Self {
        Self::Motion {
            action: MotionAction::Moving,
            axes: axes.into_iter().collect(),
        }
    }
}
impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Preparing => "Preparing",
            Self::Measuring => "Measuring",
            Self::Waiting => "Waiting",
            Self::Reading => "Reading",
            Self::Stopped => "Stopped",
            Self::Complete => "Complete",
            Self::Error => "Error",
            Self::Motion { action, axes } => {
                return write!(
                    f,
                    "{} {}",
                    action.label(),
                    axes.iter().map(|a| a.name()).collect::<Vec<_>>().join("/")
                );
            }
        };
        f.write_str(label)
    }
}
impl From<Phase> for String {
    fn from(phase: Phase) -> Self {
        phase.to_string()
    }
}

/// Control/preflight failures are explicit at their source. Driver/transport
/// errors retain their original messages; never classify them by string matching.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OperationError {
    ReadOnly,
    Busy,
    WorkerStopped,
    UnknownAxis,
    MonitorInterval,
    MotionDisconnected,
    LockinDisconnected,
}
impl fmt::Display for OperationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ReadOnly => "demo controller is read-only; hardware, acquisition, and file output commands are disabled",
            Self::Busy => "An operation is already running",
            Self::WorkerStopped => "Instrument worker stopped",
            Self::UnknownAxis => "Unknown motion axis",
            Self::MonitorInterval => "Hardware monitor interval must be at least 0.01 s",
            Self::MotionDisconnected => "Motion disconnected",
            Self::LockinDisconnected => "Lock-in disconnected",
        })
    }
}
impl std::error::Error for OperationError {}
impl From<OperationError> for String {
    fn from(error: OperationError) -> Self {
        error.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn operation_error_messages_preserve_the_legacy_contract() {
        for (error, text) in [
            (
                OperationError::ReadOnly,
                "demo controller is read-only; hardware, acquisition, and file output commands are disabled",
            ),
            (OperationError::Busy, "An operation is already running"),
            (OperationError::WorkerStopped, "Instrument worker stopped"),
            (OperationError::UnknownAxis, "Unknown motion axis"),
            (
                OperationError::MonitorInterval,
                "Hardware monitor interval must be at least 0.01 s",
            ),
            (OperationError::MotionDisconnected, "Motion disconnected"),
            (OperationError::LockinDisconnected, "Lock-in disconnected"),
        ] {
            assert_eq!(String::from(error), text);
        }
    }
    #[test]
    fn phase_wire_text_is_unchanged_for_all_states_and_axis_masks() {
        for (phase, text) in [
            (Phase::Preparing, "Preparing"),
            (Phase::Measuring, "Measuring"),
            (Phase::Waiting, "Waiting"),
            (Phase::Reading, "Reading"),
            (Phase::Stopped, "Stopped"),
            (Phase::Complete, "Complete"),
            (Phase::Error, "Error"),
        ] {
            assert_eq!(phase.to_string(), text);
        }
        for (action, label) in [
            (MotionAction::Moving, "Moving"),
            (MotionAction::Initializing, "Initializing"),
            (MotionAction::Returning, "Returning"),
        ] {
            for mask in 0..8 {
                let targets = std::array::from_fn(|i| ((mask >> i) & 1 == 1).then_some(0.));
                let names: Vec<_> = ["t", "x", "y"]
                    .into_iter()
                    .enumerate()
                    .filter_map(|(i, name)| ((mask >> i) & 1 == 1).then_some(name))
                    .collect();
                assert_eq!(
                    Phase::motion(action, targets).to_string(),
                    format!("{label} {}", names.join("/"))
                );
            }
        }
        assert_eq!(Phase::moving([Axis::U, Axis::V]).to_string(), "Moving u/v");
    }
}
