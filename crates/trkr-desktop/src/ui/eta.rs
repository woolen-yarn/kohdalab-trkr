//! Display-only acquisition estimate. Never changes device timing or saved data.
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct Estimate {
    origin: Option<Instant>,
    first: Option<(usize, f64)>,
    last: Option<(usize, f64)>,
}

impl Estimate {
    pub(super) fn start(&mut self) {
        self.origin = None;
        self.first = None;
        self.last = None;
    }

    pub(super) fn observe(&mut self, completed: usize, elapsed: f64, captured_at: Instant) {
        if completed == 0
            || self.last.is_some_and(|(previous, previous_time)| {
                completed <= previous || elapsed <= previous_time
            })
        {
            return;
        }
        let Ok(duration) = Duration::try_from_secs_f64(elapsed) else {
            return;
        };
        let Some(origin) = captured_at.checked_sub(duration) else {
            return;
        };
        // Use the acquisition-side monotonic clock, not the time a backlogged
        // GUI drains its queue. CSV/JSON readers without this private field can
        // still provide a playback clock. Keep the earliest rounding anchor.
        self.origin = Some(self.origin.map_or(origin, |old| old.min(origin)));
        let observation = (completed, elapsed);
        self.first.get_or_insert(observation);
        self.last = Some(observation);
    }

    pub(super) fn remaining(&self, total: usize, now: Instant) -> Option<f64> {
        let started = self.origin?;
        let (completed, elapsed) = self.last?;
        let (first_count, first_elapsed) = self.first?;
        if completed >= total || completed <= first_count || elapsed <= first_elapsed {
            return None;
        }
        // The observed pace includes motion, compensation, reads and flushed CSV
        // writes. Count down between observations instead of freezing until the
        // next point. An overdue estimate is unknown, not a false zero seconds.
        // The first approach can start far from the scan. Do not extrapolate
        // that one-off startup cost onto every remaining point.
        let per_point = (elapsed - first_elapsed) / (completed - first_count) as f64;
        let remaining = per_point * (total - completed) as f64
            - (now.saturating_duration_since(started).as_secs_f64() - elapsed);
        (remaining.is_finite() && remaining > 0.0).then_some(remaining)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_pace_excludes_initial_approach_and_counts_down_with_zero_wait() {
        let start = Instant::now();
        let mut estimate = Estimate::default();
        estimate.start();
        assert_eq!(estimate.remaining(10, start), None);
        // A long first approach is one-off, not 30 seconds per remaining point.
        estimate.observe(1, 30.0, start + Duration::from_secs(30));
        assert_eq!(
            estimate.remaining(10, start + Duration::from_secs(30)),
            None
        );
        estimate.observe(2, 33.0, start + Duration::from_secs(33));
        assert_eq!(
            estimate.remaining(10, start + Duration::from_secs(33)),
            Some(24.0)
        );
        assert_eq!(
            estimate.remaining(10, start + Duration::from_secs(35)),
            Some(22.0)
        );
        estimate.observe(2, 36.0, start + Duration::from_secs(36));
        assert_eq!(
            estimate.remaining(10, start + Duration::from_secs(36)),
            Some(21.0)
        );
        // A slow move/line approach changes the observed acquisition pace.
        estimate.observe(3, 38.0, start + Duration::from_secs(38));
        assert_eq!(
            estimate.remaining(10, start + Duration::from_secs(38)),
            Some(28.0)
        );
        assert_eq!(
            estimate.remaining(10, start + Duration::from_secs(67)),
            None
        );
    }

    #[test]
    fn completed_runs_and_new_runs_or_replay_loops_do_not_reuse_old_timing() {
        let start = Instant::now();
        let mut estimate = Estimate::default();
        estimate.start();
        estimate.observe(1, 3.0, start + Duration::from_secs(3));
        estimate.observe(10, 30.0, start + Duration::from_secs(30));
        assert_eq!(
            estimate.remaining(10, start + Duration::from_secs(30)),
            None
        );
        estimate.start();
        assert_eq!(
            estimate.remaining(100, start + Duration::from_secs(41)),
            None
        );
        estimate.observe(16, 2.0, start + Duration::from_secs(42));
        assert_eq!(
            estimate.remaining(100, start + Duration::from_secs(42)),
            None
        );
        estimate.observe(32, 4.0, start + Duration::from_secs(44));
        assert_eq!(
            estimate.remaining(100, start + Duration::from_secs(44)),
            Some(8.5)
        );
    }
    #[test]
    fn queued_samples_keep_acquisition_pace_and_elapsed_backlog_in_countdown() {
        let start = Instant::now();
        let mut estimate = Estimate::default();
        estimate.start();
        estimate.observe(1, 3.0, start + Duration::from_secs(3));
        // All observations may be processed together after a twelve-second
        // UI stall, but retain their original acquisition-side timestamps.
        // Two acquisitions happened three seconds apart, but the UI receives
        // both at twelve seconds after a stall. They were not simultaneous.
        estimate.observe(2, 6.0, start + Duration::from_secs(6));
        estimate.observe(3, 9.0, start + Duration::from_secs(9));
        assert_eq!(
            estimate.remaining(10, start + Duration::from_secs(12)),
            Some(18.0)
        );
        estimate.observe(4, f64::NAN, start + Duration::from_secs(12));
        assert_eq!(
            estimate.remaining(10, start + Duration::from_secs(12)),
            Some(18.0)
        );
    }
}
