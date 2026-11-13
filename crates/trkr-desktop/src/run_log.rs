//! Compact, human-readable lifecycle log for a measurement output bundle.
//!
//! This is deliberately not a transport trace: raw commands and replies belong
//! only in the raw CSV evidence stream.
use serde_json::Value;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
};

pub struct RunLog {
    file: File,
    run_id: String,
}

impl RunLog {
    pub fn create(path: &Path, run_id: &str) -> Result<Self, String> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            file,
            run_id: run_id.into(),
        })
    }

    /// Append a durable lifecycle/progress/error entry. Context is intentionally
    /// small and raw transport payloads are removed defensively.
    pub fn append(&mut self, level: &str, event: &str, mut context: Value) -> Result<(), String> {
        if let Some(object) = context.as_object_mut() {
            for key in [
                "raw_response",
                "wire_response",
                "command",
                "wire_command",
                "exchanges",
            ] {
                object.remove(key);
            }
        }
        writeln!(
            self.file,
            "{}\t{}\t{}\t{}\t{}",
            timestamp(),
            level,
            self.run_id,
            event,
            serde_json::to_string(&context).map_err(|error| error.to_string())?
        )
        .map_err(|error| error.to_string())?;
        self.file.flush().map_err(|error| error.to_string())
    }

    pub fn finish(&mut self) -> Result<(), String> {
        self.file.flush().map_err(|error| error.to_string())?;
        self.file.sync_all().map_err(|error| error.to_string())
    }
}

fn timestamp() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}Z",
        now.year(),
        now.month() as u8,
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
        now.nanosecond() / 1_000
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn log_is_create_new_and_excludes_transport_payloads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.log");
        let mut log = RunLog::create(&path, "run-test").unwrap();
        log.append(
            "ERROR",
            "run_finished",
            serde_json::json!({"rows": 1,
            "raw_response": "secret", "command": "READ?"}),
        )
        .unwrap();
        log.finish().unwrap();
        let line = std::fs::read_to_string(&path).unwrap();
        assert!(line.contains("\tERROR\trun-test\trun_finished\t"));
        assert!(line.contains("\"rows\":1"));
        assert!(!line.contains("secret") && !line.contains("READ?"));
        assert!(RunLog::create(&path, "other").is_err());
    }
}

/// Freeze complete log lines at the same saved-point boundary as main/raw CSV.
/// Never copy a later terminal claim into an active Save Now snapshot.
pub fn snapshot(
    source: &Path,
    output: &mut impl Write,
    max_point: usize,
    state: crate::raw_csv::SnapshotState,
) -> Result<(), String> {
    use std::io::{BufRead, Read};
    let file = File::open(source).map_err(|e| e.to_string())?;
    let length = file.metadata().map_err(|e| e.to_string())?.len();
    let mut input = std::io::BufReader::new(file.take(length));
    let mut line = String::new();
    loop {
        line.clear();
        if input.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            break;
        }
        if !line.ends_with('\n') {
            break;
        }
        let parts: Vec<_> = line.trim_end_matches('\n').splitn(5, '\t').collect();
        if parts.len() != 5 {
            return Err("Invalid measurement log entry".into());
        }
        let context: Value = serde_json::from_str(parts[4]).map_err(|e| e.to_string())?;
        let terminal = matches!(parts[3], "finished" | "run_finished");
        if state == crate::raw_csv::SnapshotState::Active && terminal {
            continue;
        }
        if state != crate::raw_csv::SnapshotState::Failed {
            let allowed = max_point.saturating_add(usize::from(parts[1] == "ERROR"));
            if context
                .get("point_index")
                .and_then(Value::as_u64)
                .is_some_and(|p| p > allowed as u64)
            {
                continue;
            }
            if terminal
                && context
                    .get("rows")
                    .and_then(Value::as_u64)
                    .is_some_and(|p| p > max_point as u64)
            {
                continue;
            }
        }
        output
            .write_all(line.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    output.flush().map_err(|e| e.to_string())
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    use crate::raw_csv::SnapshotState;
    #[test]
    fn active_snapshot_omits_later_points_terminal_and_torn_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.log");
        let mut log = RunLog::create(&path, "id").unwrap();
        log.append("INFO", "run_created", serde_json::json!({}))
            .unwrap();
        log.append("INFO", "csv_write", serde_json::json!({"point_index":1}))
            .unwrap();
        log.append("INFO", "csv_write", serde_json::json!({"point_index":2}))
            .unwrap();
        log.append("INFO", "finished", serde_json::json!({"rows":2}))
            .unwrap();
        log.file.write_all(b"torn line").unwrap();
        let mut bytes = Vec::new();
        snapshot(&path, &mut bytes, 1, SnapshotState::Active).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(
            !text.contains("finished")
                && !text.contains("torn")
                && !text.contains("\"point_index\":2")
        );
    }
}
