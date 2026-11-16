//! Display-only estimates from completed acquisition rows.
use std::time::Instant;

#[derive(Default)]
pub(super) struct Estimate {
    last: Option<Observation>,
    all: Pace,
    within_line: Pace,
    line_start: Pace,
}

#[derive(Clone, Copy)]
struct Observation {
    row: usize,
    timing: crate::engine::timing::ProgressTiming,
}

#[derive(Default)]
struct Pace {
    elapsed: f64,
    steps: usize,
}

impl Pace {
    fn add(&mut self, elapsed: f64, steps: usize) {
        self.elapsed += elapsed;
        self.steps += steps;
    }

    fn seconds_per_step(&self) -> Option<f64> {
        (self.steps > 0).then(|| self.elapsed / self.steps as f64)
    }
}

fn line_starts(from: usize, through: usize, width: Option<usize>) -> usize {
    width
        .filter(|width| *width != 0)
        .map_or(0, |width| (through - 1) / width - (from - 1) / width)
}

impl Estimate {
    pub(super) fn start(&mut self) {
        *self = Self::default();
    }

    pub(super) fn observe(&mut self, row: usize, timing: crate::engine::timing::ProgressTiming) {
        if row == 0 || timing.completed_at < timing.started_at {
            return;
        }
        if let Some(last) = self.last {
            if last.timing.started_at != timing.started_at
                || last.timing.points_per_line != timing.points_per_line
                || row <= last.row
                || timing.completed_at < last.timing.completed_at
            {
                return;
            }
            let elapsed = (timing.completed_at - last.timing.completed_at).as_secs_f64();
            let steps = row - last.row;
            let new_lines = line_starts(last.row, row, timing.points_per_line);
            self.all.add(elapsed, steps);
            // A dropped event can span a mixture of row transitions and
            // ordinary moves. Count it only in the overall pace in that case.
            if new_lines == 0 {
                self.within_line.add(elapsed, steps);
            } else if new_lines == steps {
                self.line_start.add(elapsed, steps);
            }
        }
        self.last = Some(Observation { row, timing });
    }

    pub(super) fn remaining(
        &self,
        total: usize,
        monitor_interval: Option<f64>,
        now: Instant,
    ) -> Option<f64> {
        let last = self.last?;
        let remaining = total.checked_sub(last.row)?;
        if remaining == 0 {
            return None;
        }
        let elapsed = now
            .saturating_duration_since(last.timing.completed_at)
            .as_secs_f64();
        let pace = self.all.seconds_per_step();
        let seconds = if let Some(period) = monitor_interval.filter(|period| *period > 0.) {
            // The worker schedules monitor rows against a fixed start time.
            // Use measured throughput if the instrument is falling behind.
            let final_due = (total - 1) as f64 * period;
            let already_run = now
                .saturating_duration_since(last.timing.started_at)
                .as_secs_f64();
            let latency = last
                .timing
                .completed_at
                .duration_since(last.timing.started_at)
                .as_secs_f64()
                - (last.row - 1) as f64 * period;
            let scheduled = final_due + latency.clamp(0., period) - already_run;
            let throughput = pace
                .filter(|pace| *pace > period)
                .map_or(0., |pace| pace * remaining as f64 - elapsed);
            scheduled.max(throughput)
        } else {
            let fallback = pace?;
            let starts = line_starts(last.row, total, last.timing.points_per_line);
            let within = self.within_line.seconds_per_step().unwrap_or(fallback);
            let line = self.line_start.seconds_per_step().unwrap_or(fallback);
            within * (remaining - starts) as f64 + line * starts as f64 - elapsed
        };
        (seconds.is_finite() && seconds > 0.).then_some(seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::timing::ProgressTiming;
    use std::time::Duration;

    fn observation(start: Instant, seconds: u64, points_per_line: Option<usize>) -> ProgressTiming {
        ProgressTiming {
            started_at: start,
            completed_at: start + Duration::from_secs(seconds),
            points_per_line,
        }
    }

    #[test]
    fn startup_is_excluded_and_eta_counts_down_from_worker_completion() {
        let start = Instant::now();
        let mut eta = Estimate::default();
        eta.observe(1, observation(start, 30, None));
        assert_eq!(eta.remaining(10, None, start), None);
        eta.observe(2, observation(start, 33, None));
        eta.observe(2, observation(start, 34, None));
        assert_eq!(
            eta.remaining(10, None, start + Duration::from_secs(35)),
            Some(22.)
        );
        assert_eq!(
            eta.remaining(2, None, start + Duration::from_secs(35)),
            None
        );
        assert_eq!(
            eta.remaining(10, None, start + Duration::from_secs(60)),
            None
        );
        eta.start();
        assert_eq!(eta.remaining(10, None, start), None);
    }

    #[test]
    fn line_changes_only_count_where_they_are_still_ahead() {
        let start = Instant::now();
        let mut eta = Estimate::default();
        for (row, seconds) in [(1, 20), (2, 22), (3, 24), (4, 34), (5, 36)] {
            eta.observe(row, observation(start, seconds, Some(3)));
        }
        // Two ordinary moves and one line change remain.
        assert_eq!(
            eta.remaining(8, None, start + Duration::from_secs(37)),
            Some(13.)
        );
        // A mixed gap caused by a dropped GUI event does not pollute either class.
        eta.observe(8, observation(start, 50, Some(3)));
        assert_eq!(
            eta.remaining(9, None, start + Duration::from_secs(50)),
            Some(2.)
        );
    }

    #[test]
    fn monitor_uses_the_schedule_and_slower_measured_throughput() {
        let start = Instant::now();
        let mut eta = Estimate::default();
        eta.observe(1, observation(start, 1, None));
        assert_eq!(
            eta.remaining(5, Some(2.), start + Duration::from_secs(1)),
            Some(8.)
        );
        eta.observe(2, observation(start, 5, None));
        assert_eq!(
            eta.remaining(5, Some(2.), start + Duration::from_secs(6)),
            Some(11.)
        );
    }
}
