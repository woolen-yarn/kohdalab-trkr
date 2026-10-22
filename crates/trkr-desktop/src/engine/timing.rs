//! Host-observed operation windows. UTC labels and monotonic elapsed times have
//! separate meanings; neither is a timestamp supplied by an instrument.
use serde::Serialize;
use std::time::Instant;

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct AcquisitionTiming {
    pub position: Option<ReadWindow>,
    pub signal: Option<ReadWindow>,
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
