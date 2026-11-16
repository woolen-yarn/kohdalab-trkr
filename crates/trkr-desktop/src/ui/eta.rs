//! Acquisition ETA based on progress snapshots from the measurement worker.
use crate::engine::timing::ProgressTiming;
use std::time::Instant;

#[derive(Default)]
pub(super) struct Estimate {
    last: Option<ProgressTiming>,
}

impl Estimate {
    pub(super) fn start(&mut self) {
        self.last = None;
    }

    pub(super) fn observe(&mut self, timing: ProgressTiming) {
        if timing.row == 0 || timing.completed_at < timing.started_at {
            return;
        }
        if let Some(last) = self.last
            && (last.started_at != timing.started_at
                || timing.row <= last.row
                || timing.completed_at < last.completed_at)
        {
            return;
        }
        self.last = Some(timing);
    }

    pub(super) fn remaining(&self, total: usize, now: Instant) -> Option<f64> {
        let last = self.last?;
        let remaining = total.checked_sub(last.row)?;
        if remaining == 0 {
            return None;
        }
        let starts = last
            .points_per_line
            .map_or(0, |width| (total - 1) / width - (last.row - 1) / width);
        let ordinary = remaining - starts;
        // Avoid presenting a fast-axis-only estimate when unmeasured line
        // changes are still ahead. Single-point lines need no ordinary pace.
        let within = if ordinary > 0 {
            last.within_line_seconds?
        } else {
            0.
        };
        let line = if starts > 0 {
            last.line_start_seconds?
        } else {
            0.
        };
        let next_is_line_start = last
            .points_per_line
            .is_some_and(|width| last.row.is_multiple_of(width));
        let next = if next_is_line_start { line } else { within };
        let elapsed = now
            .saturating_duration_since(last.completed_at)
            .as_secs_f64();
        // Only the current point has been in flight during this elapsed time.
        let seconds = within * ordinary as f64 + line * starts as f64 - elapsed.min(next);
        (seconds.is_finite() && seconds > 0.).then_some(seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::timing::ScanClock;
    use std::time::Duration;

    #[test]
    fn excludes_startup_and_does_not_consume_future_points_when_one_is_slow() {
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let mut clock = ScanClock::new(start, None);
        let mut eta = Estimate::default();
        eta.observe(clock.complete(1, at(30)));
        assert_eq!(eta.remaining(10, at(30)), None);
        let second = clock.complete(2, at(33));
        eta.observe(second);
        eta.observe(ProgressTiming {
            completed_at: at(34),
            ..second
        });
        assert_eq!(eta.remaining(10, at(35)), Some(22.));
        assert_eq!(eta.remaining(10, at(60)), Some(21.));
        assert_eq!(eta.remaining(3, at(60)), None);
        eta.start();
        assert_eq!(eta.remaining(10, at(60)), None);
    }

    #[test]
    fn line_change_pace_survives_dropped_gui_events() {
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let mut clock = ScanClock::new(start, Some(3));
        let mut eta = Estimate::default();
        clock.complete(1, at(20));
        eta.observe(clock.complete(2, at(22)));
        assert_eq!(eta.remaining(8, at(22)), None);
        clock.complete(3, at(24));
        clock.complete(4, at(34));
        eta.observe(clock.complete(5, at(36)));
        // Two ordinary points at 2 s and one line-start point at 10 s.
        assert_eq!(eta.remaining(8, at(37)), Some(13.));
        assert_eq!(eta.remaining(8, at(44)), Some(12.));
        assert_eq!(eta.remaining(6, at(37)), Some(1.));
    }

    #[test]
    fn one_point_per_line_needs_only_line_change_pace() {
        let start = Instant::now();
        let mut clock = ScanClock::new(start, Some(1));
        clock.complete(1, start);
        let mut eta = Estimate::default();
        eta.observe(clock.complete(2, start + Duration::from_secs(4)));
        assert_eq!(eta.remaining(4, start + Duration::from_secs(5)), Some(7.));
    }
}
