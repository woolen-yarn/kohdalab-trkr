//! Acquisition cleanup and terminal recording.
use super::{
    Event, Phase, Session, Source,
    recording::{Recording, finish_metadata},
    timing,
};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
    time::Instant,
};

pub(super) fn expected_stop_error(error: &str) -> bool {
    matches!(
        error,
        "motion cancelled"
            | "transport: GPIB read cancelled"
            | "Lock-in status: transport: GPIB read cancelled"
    )
}

pub(super) fn append_recording_error(result: &mut Result<(), String>, label: &str, error: String) {
    *result = match result {
        Ok(()) => Err(format!("{label}: {error}")),
        Err(original) => Err(format!("{original}; {label}: {error}")),
    };
}

impl Session {
    pub(super) fn stop_acquisition(
        &mut self,
        result: &mut Result<(), String>,
        recording: Option<&mut Recording>,
        cancel: &AtomicBool,
        send: &SyncSender<Event>,
        start: Instant,
    ) {
        self.state.phase = Phase::Stopping.into();
        let _ = send.try_send(Event::State(Box::new(self.state.clone())));
        let (stopped, window, _) = timing::observe(start, || {
            self.rig.as_mut().map(|rig| rig.stop()).transpose()
        });
        if let Some(recording) = recording
            && let Err(error) = recording.append_audit(&serde_json::json!({"event":"stop","device_role":"motion","timing":window,
                "verification":if self.rig.is_some() {"stationary_state_queries"} else {"no_motion_devices"},"error":stopped.as_ref().err()}))
        {
            append_recording_error(result, "record STOP outcome", error);
        }
        if let Err(error) = stopped {
            *result = Err(format!(
                "{}; STOP failed: {error}",
                result
                    .as_ref()
                    .err()
                    .map(String::as_str)
                    .unwrap_or("Cancelled")
            ));
        } else if cancel.load(Ordering::Acquire)
            && result
                .as_ref()
                .err()
                .is_none_or(|error| expected_stop_error(error))
        {
            // A Stop request only clears the expected motion cancellation;
            // it must not hide a concurrent instrument or recording failure.
            *result = Ok(());
        }
    }

    pub(super) fn finalize_acquisition(
        &mut self,
        mut result: Result<(), String>,
        mut csv: Option<Recording>,
        output: Option<&Path>,
        source: Source,
        cancel: &AtomicBool,
        send: &SyncSender<Event>,
    ) -> Result<(), String> {
        if cancel.load(Ordering::Acquire) {
            self.state.phase = Phase::Stopping.into();
            let _ = send.try_send(Event::State(Box::new(self.state.clone())));
            if let Some(gpib) = &mut self.gpib
                && let Err(error) = gpib.finish_cancelled_request()
            {
                append_recording_error(&mut result, "Stop response recovery failed", error);
            }
        }
        if cancel.load(Ordering::Acquire)
            && result
                .as_ref()
                .err()
                .is_some_and(|error| expected_stop_error(error))
        {
            result = Ok(());
        }
        let terminal_status = if result.is_err() {
            "error"
        } else if cancel.load(Ordering::Acquire) {
            "cancelled"
        } else {
            "complete"
        };
        if let Some(recording) = &mut csv {
            if let Err(error) =
                recording.set_final_position(&self.state.position, &self.state.phase)
            {
                append_recording_error(&mut result, "save final position", error);
            }
            let terminal = serde_json::json!({
                "event": "finished",
                "status": terminal_status,
                "last_observed_position":self.state.position, "last_phase":self.state.phase,
                "rows": self.state.rows,
                "simulation": matches!(source, Source::Demo),
                "error": result.as_ref().err(),
            });
            if let Err(error) = recording.append_audit(&terminal) {
                append_recording_error(&mut result, "record finished audit", error);
            }
            if let Err(error) = recording.finish() {
                append_recording_error(&mut result, "finish recording", error);
                let _ = recording.append_audit(&serde_json::json!({"event":"finalization_error", "status":"error", "error":result.as_ref().err()}));
            }
        }
        if let Some(path) = output {
            let status = if result.is_err() {
                "error"
            } else if cancel.load(Ordering::Acquire) {
                "cancelled"
            } else {
                "complete"
            };
            let metadata_result =
                finish_metadata(path, self.state.rows, status, result.as_ref().err());
            if let Err(error) = metadata_result {
                self.failure_code = Some("save_failed".into());
                append_recording_error(&mut result, "finish run metadata", error);
                if let Some(recording) = &mut csv {
                    let _ = recording.append_audit(&serde_json::json!({"event":"finalization_error", "status":"error", "error":result.as_ref().err()}));
                }
            }
        }
        if csv.as_ref().is_some_and(Recording::save_failed) {
            self.failure_code = Some("save_failed".into());
        }
        result
    }
}

pub(super) fn finalize_demo_recording(
    mut result: Result<(), String>,
    mut writer: Recording,
    output: &Path,
    rows: usize,
) -> Result<usize, String> {
    let terminal_status = if result.is_err() { "error" } else { "complete" };
    if let Err(error) = writer.append_audit(&serde_json::json!({
        "event": "finished",
        "status": terminal_status,
        "rows": rows,
        "simulation": true,
        "error": result.as_ref().err(),
    })) {
        append_recording_error(&mut result, "record finished audit", error);
    }
    if let Err(error) = writer.finish() {
        append_recording_error(&mut result, "finish recording", error);
        let _ = writer.append_audit(&serde_json::json!({"event":"finalization_error", "status":"error", "error":result.as_ref().err()}));
    }
    let status = if result.is_err() { "error" } else { "complete" };
    if let Err(error) = finish_metadata(output, rows, status, result.as_ref().err()) {
        append_recording_error(&mut result, "finish run metadata", error);
        let _ = writer.append_audit(&serde_json::json!({"event":"finalization_error", "status":"error", "error":result.as_ref().err()}));
    }
    result?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::recording::create_csv;
    use serde_json::Value;
    use trkr_core::Plan;

    #[test]
    fn expected_stop_error_accepts_only_the_exact_cancellation_strings() {
        for accepted in [
            "motion cancelled",
            "transport: GPIB read cancelled",
            "Lock-in status: transport: GPIB read cancelled",
        ] {
            assert!(expected_stop_error(accepted), "{accepted}");
        }
        for rejected in [
            "",
            "Cancelled",
            "Motion cancelled",
            "motion cancelled ",
            "motion cancelled; STOP failed: still moving",
            "transport: GPIB read cancelled; LI5640 recovery required",
            "Lock-in status: transport: GPIB read cancelled; finish recording: disk full",
        ] {
            assert!(!expected_stop_error(rejected), "{rejected}");
        }
    }

    #[test]
    fn append_recording_error_keeps_the_original_failure_first() {
        let mut result = Ok(());
        append_recording_error(&mut result, "record finished audit", "a".into());
        assert_eq!(result, Err("record finished audit: a".into()));
        append_recording_error(&mut result, "finish recording", "b".into());
        assert_eq!(
            result,
            Err("record finished audit: a; finish recording: b".into())
        );
    }

    fn demo_recording(dir: &Path) -> (std::path::PathBuf, Recording) {
        let output = dir.join("demo.csv");
        let writer = create_csv(&output, Source::Demo, &Plan::default()).unwrap();
        (output, writer)
    }

    #[test]
    fn demo_finalization_records_finished_audit_then_complete_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let (output, writer) = demo_recording(dir.path());
        assert_eq!(finalize_demo_recording(Ok(()), writer, &output, 3), Ok(3));

        let metadata: Value =
            serde_json::from_slice(&std::fs::read(output.with_extension("run.json")).unwrap())
                .unwrap();
        assert_eq!(metadata["status"], "complete");
        assert_eq!(metadata["rows"], 3);
        assert_eq!(metadata["error"], Value::Null);
        // Metadata hashes are taken after the finished event, so it must be last.
        let events = crate::raw_csv::test_events(&output.with_extension("raw.csv"));
        let finished = events.last().unwrap();
        assert_eq!(finished["event"], "finished");
        assert_eq!(finished["status"], "complete");
        assert_eq!(finished["rows"], 3);
        assert_eq!(finished["simulation"], true);
        assert_eq!(finished["error"], Value::Null);
    }

    #[test]
    fn demo_finalization_preserves_the_acquisition_error() {
        let dir = tempfile::tempdir().unwrap();
        let (output, writer) = demo_recording(dir.path());
        assert_eq!(
            finalize_demo_recording(Err("boom".into()), writer, &output, 1),
            Err("boom".into())
        );

        let metadata: Value =
            serde_json::from_slice(&std::fs::read(output.with_extension("run.json")).unwrap())
                .unwrap();
        assert_eq!(metadata["status"], "error");
        assert_eq!(metadata["rows"], 1);
        assert_eq!(metadata["error"], "boom");
        let raw = output.with_extension("raw.csv");
        let events = crate::raw_csv::test_events(&raw);
        let finished = events.last().unwrap();
        assert_eq!(finished["event"], "finished");
        assert_eq!(finished["status"], "error");
        // test_events does not restore the dedicated raw `error` column, so read it by header name.
        let mut reader = csv::Reader::from_path(&raw).unwrap();
        let error_column = reader
            .headers()
            .unwrap()
            .iter()
            .position(|name| name == "error")
            .expect("raw CSV has an error column");
        let last = reader.records().last().unwrap().unwrap();
        assert_eq!(&last[error_column], "boom");
    }
}
