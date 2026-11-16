//! Host-observed operation windows. UTC labels and monotonic elapsed times have
//! separate meanings; neither is a timestamp supplied by an instrument.
use serde::Serialize;
use std::time::Instant;

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct AcquisitionTiming {
    pub position: Option<ReadWindow>,
    pub signal: Option<ReadWindow>,
    #[serde(skip)]
    pub(crate) progress: Option<ProgressTiming>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ProgressTiming {
    pub started_at: Instant,
    pub completed_at: Instant,
    pub row: usize,
    pub points_per_line: Option<usize>,
    pub within_line_seconds: Option<f64>,
    pub line_start_seconds: Option<f64>,
}

pub(crate) struct ScanClock {
    started_at: Instant,
    previous: Option<Instant>,
    points_per_line: Option<usize>,
    within_line: Pace,
    line_start: Pace,
}

#[derive(Default)]
struct Pace {
    elapsed: f64,
    points: usize,
}

impl Pace {
    fn average(&self) -> Option<f64> {
        (self.points > 0).then(|| self.elapsed / self.points as f64)
    }
}

impl ScanClock {
    pub(crate) fn new(started_at: Instant, points_per_line: Option<usize>) -> Self {
        Self {
            started_at,
            previous: None,
            points_per_line: points_per_line.filter(|&width| width > 0),
            within_line: Pace::default(),
            line_start: Pace::default(),
        }
    }

    pub(crate) fn complete(&mut self, row: usize, completed_at: Instant) -> ProgressTiming {
        // The first point includes an arbitrary initial move, so its pace
        // is unrepresentative. Every later completion is counted on the worker.
        if let Some(previous) = self.previous {
            let line_start = self
                .points_per_line
                .is_some_and(|width| (row - 1).is_multiple_of(width));
            let pace = if line_start {
                &mut self.line_start
            } else {
                &mut self.within_line
            };
            pace.elapsed += completed_at.duration_since(previous).as_secs_f64();
            pace.points += 1;
        }
        self.previous = Some(completed_at);
        ProgressTiming {
            started_at: self.started_at,
            completed_at,
            row,
            points_per_line: self.points_per_line,
            within_line_seconds: self.within_line.average(),
            line_start_seconds: self.line_start.average(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ReadWindow {
    #[serde(rename = "host_started_at_utc")]
    pub started_at: Option<String>,
    #[serde(rename = "host_completed_at_utc")]
    pub finished_at: Option<String>,
    #[serde(rename = "host_started_elapsed_s")]
    pub started_elapsed_s: f64,
    #[serde(rename = "host_completed_elapsed_s")]
    pub finished_elapsed_s: f64,
    pub device_timestamp_utc: Option<String>,
}

pub(super) fn observe<T>(
    origin: Instant,
    operation: impl FnOnce() -> T,
) -> (T, ReadWindow, Instant) {
    // Read the two host clocks adjacently; format only after completion.
    let start_utc = time::OffsetDateTime::now_utc();
    let begin = Instant::now();
    let result = operation();
    let end_utc = time::OffsetDateTime::now_utc();
    let end = Instant::now();
    let started_at = start_utc
        .format(&time::format_description::well_known::Rfc3339)
        .ok();
    let finished_at = end_utc
        .format(&time::format_description::well_known::Rfc3339)
        .ok();
    (
        result,
        ReadWindow {
            started_at,
            finished_at,
            device_timestamp_utc: None,
            started_elapsed_s: begin.duration_since(origin).as_secs_f64(),
            finished_elapsed_s: end.duration_since(origin).as_secs_f64(),
        },
        end,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn later_storage_work_does_not_change_a_completed_read_window() {
        let origin = Instant::now();
        let (value, window, completed) = observe(origin, || 42);
        let original = serde_json::to_value(&window).unwrap();
        let (_, saved, _) = observe(origin, || {
            std::thread::sleep(std::time::Duration::from_millis(2))
        });
        assert_eq!(value, 42);
        assert!(window.finished_elapsed_s >= window.started_elapsed_s);
        assert!(saved.started_elapsed_s >= window.finished_elapsed_s);
        assert!(completed >= origin);
        assert_eq!(serde_json::to_value(window).unwrap(), original);
    }
}
