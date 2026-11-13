//! Consistent, no-clobber snapshots of measurement CSVs and provenance.
//! No GUI or instrument state is mutated while staging a snapshot.
use crate::raw_csv::SnapshotState;
use serde_json::{Value, json};
use std::path::PathBuf;

fn classify_snapshot(metadata: Option<&Value>, run_active: bool) -> SnapshotState {
    if run_active {
        return SnapshotState::Active;
    }
    match metadata
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
    {
        Some("error") => SnapshotState::Failed,
        Some("complete" | "cancelled") => SnapshotState::Finalized,
        _ => SnapshotState::Active,
    }
}

pub(super) fn copy_complete_csv(
    source: &std::path::Path,
    destination: &std::path::Path,
    run_active: bool,
) -> Result<(), String> {
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let source_metadata_path = source.with_extension("run.json");
    let mut metadata = if source_metadata_path.exists() {
        Some(
            serde_json::from_slice::<Value>(
                &std::fs::read(&source_metadata_path).map_err(|e| e.to_string())?,
            )
            .map_err(|e| format!("Save snapshot metadata: {e}"))?,
        )
    } else {
        None
    };
    let source_audit_path = source_audit_path(source, metadata.as_ref())?;
    if let Some(path) = &source_audit_path
        && !path.exists()
    {
        return Err(format!(
            "Save snapshot: declared readings audit is missing: {}",
            path.display()
        ));
    }

    let source_log = source.with_extension("log");
    let has_log = metadata.as_ref().and_then(|m| m.get("log")).is_some();
    if has_log && !source_log.is_file() {
        return Err("Save snapshot: declared measurement log is missing".into());
    }
    let destination_log = destination.with_extension("log");
    let destination_metadata = destination.with_extension("run.json");
    let raw_format = source_audit_path
        .as_ref()
        .is_some_and(|path| path.extension().is_some_and(|ext| ext == "csv"));
    let destination_audit = destination.with_extension(if raw_format {
        "raw.csv"
    } else {
        "readings.jsonl"
    });
    // Check every related destination before staging.  This also avoids
    // combining a legacy CSV snapshot with sidecars from another run.
    for path in [
        destination,
        &destination_metadata,
        &destination_log,
        &destination_audit,
        &destination.with_extension(if raw_format {
            "readings.jsonl"
        } else {
            "raw.csv"
        }),
    ] {
        if path.exists() {
            return Err(format!("Save snapshot: {} already exists", path.display()));
        }
    }

    // Freeze classification with the source metadata before copying any rows.
    // A stale GUI flag must not turn an unfinished source into a finalized copy.
    let snapshot_state = classify_snapshot(metadata.as_ref(), run_active);
    let (csv, rows) = stage_complete_csv(source, parent)?;
    let audit = source_audit_path
        .as_deref()
        .map(|path| stage_complete_audit(path, parent, rows, snapshot_state))
        .transpose()?;
    let log = if has_log {
        use std::io::Write;
        let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        crate::run_log::snapshot(&source_log, &mut staged, rows, snapshot_state)?;
        staged
            .flush()
            .and_then(|_| staged.as_file().sync_all())
            .map_err(|e| e.to_string())?;
        Some(staged)
    } else {
        None
    };
    if let Some(metadata) = &mut metadata
        && raw_format
    {
        metadata["file_hashes"] = json!({"csv_sha256":crate::raw_csv::file_sha256(csv.path())?,
            "raw_csv_sha256":audit.as_ref().map(|file|crate::raw_csv::file_sha256(file.path())).transpose()?});
    }
    if let Some(metadata) = &mut metadata {
        if let Some(log) = &log {
            metadata["file_hashes"]["log_sha256"] = json!(crate::raw_csv::file_sha256(log.path())?);
            metadata["log"]["filename"] =
                json!(destination_log.file_name().and_then(|s| s.to_str()));
        }
        if let Some(bundle) = metadata.get_mut("run_bundle") {
            bundle["filenames"] = json!({
                "analysis_csv":destination.file_name().and_then(|s|s.to_str()),
                "raw_csv":destination_audit.file_name().and_then(|s|s.to_str()),
                "log":destination_log.file_name().and_then(|s|s.to_str()),
                "conditions_json":destination_metadata.file_name().and_then(|s|s.to_str()),
            });
        }
    }
    let metadata = metadata
        .as_mut()
        .map(|metadata| {
            stage_snapshot_metadata(
                metadata,
                parent,
                rows,
                snapshot_state,
                audit.is_some().then_some(&destination_audit),
            )
        })
        .transpose()?;

    let mut staged = vec![(destination.to_path_buf(), csv)];
    if let Some(metadata) = metadata {
        staged.push((destination_metadata, metadata));
    }
    if let Some(audit) = audit {
        staged.push((destination_audit, audit));
    }
    if let Some(log) = log {
        staged.push((destination_log, log));
    }
    persist_snapshot_noclobber(staged)
}

fn source_audit_path(
    source: &std::path::Path,
    metadata: Option<&Value>,
) -> Result<Option<PathBuf>, String> {
    let default_path = if source.with_extension("raw.csv").exists() {
        source.with_extension("raw.csv")
    } else {
        source.with_extension("readings.jsonl")
    };
    let Some(metadata) = metadata else {
        return Ok(default_path.exists().then_some(default_path));
    };
    let declared = metadata
        .pointer("/raw_csv/filename")
        .or_else(|| metadata.get("readings_file"))
        .and_then(Value::as_str);
    let Some(declared) = declared else {
        return Ok(default_path.exists().then_some(default_path));
    };
    let declared_path = std::path::Path::new(declared);
    if declared_path.is_absolute()
        || declared_path.file_name().is_none()
        || declared_path != declared_path.file_name().unwrap()
    {
        return Err("Save snapshot: readings_file must be a basename".into());
    }
    let path = source
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(declared_path);
    // Any explicit declaration must remain valid in the saved copy.
    Ok(Some(path))
}

fn stage_complete_csv(
    source: &std::path::Path,
    parent: &std::path::Path,
) -> Result<(tempfile::NamedTempFile, usize), String> {
    use std::io::{BufRead, Read, Write};
    let input = std::fs::File::open(source).map_err(|e| e.to_string())?;
    let length = input.metadata().map_err(|e| e.to_string())?.len();
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|e| format!("Save snapshot: {e}"))?;
    let mut reader = std::io::BufReader::new(input.take(length));
    let mut line = Vec::new();
    let mut complete_lines = 0usize;
    // Native fields are single-line. Capture the starting file length and omit
    // an in-progress final row while preserving original acquisition timestamps.
    loop {
        line.clear();
        if reader
            .read_until(b'\n', &mut line)
            .map_err(|e| e.to_string())?
            == 0
        {
            break;
        }
        if !line.ends_with(b"\n") {
            break;
        }
        temporary.write_all(&line).map_err(|e| e.to_string())?;
        complete_lines += 1;
    }
    temporary
        .flush()
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    Ok((temporary, complete_lines.saturating_sub(1)))
}

fn stage_complete_audit(
    source: &std::path::Path,
    parent: &std::path::Path,
    csv_rows: usize,
    state: crate::raw_csv::SnapshotState,
) -> Result<tempfile::NamedTempFile, String> {
    use std::io::{BufRead, Read, Write};
    if source
        .extension()
        .is_some_and(|extension| extension == "csv")
    {
        let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        crate::raw_csv::snapshot(source, &mut temporary, csv_rows, state)?;
        temporary
            .flush()
            .and_then(|()| temporary.as_file().sync_all())
            .map_err(|e| e.to_string())?;
        return Ok(temporary);
    }
    let input = std::fs::File::open(source).map_err(|e| e.to_string())?;
    let length = input.metadata().map_err(|e| e.to_string())?.len();
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|e| format!("Save snapshot: {e}"))?;
    let mut reader = std::io::BufReader::new(input.take(length));
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader
            .read_until(b'\n', &mut line)
            .map_err(|e| e.to_string())?
            == 0
        {
            break;
        }
        if !line.ends_with(b"\n") {
            break;
        }
        let record: Value = serde_json::from_slice(&line)
            .map_err(|error| format!("Save snapshot audit: {error}"))?;
        // A finalized failed run may have a valid raw reading whose CSV write
        // failed. Preserve that evidence even though there is no matching row.
        if state == crate::raw_csv::SnapshotState::Failed
            || audit_record_is_in_snapshot(
                &record,
                csv_rows,
                state == crate::raw_csv::SnapshotState::Active,
            )
        {
            temporary.write_all(&line).map_err(|e| e.to_string())?;
        }
    }
    temporary
        .flush()
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    Ok(temporary)
}

pub(super) fn audit_record_is_in_snapshot(
    record: &Value,
    csv_rows: usize,
    run_active: bool,
) -> bool {
    // The source can finish after the CSV length was captured. Do not attach
    // a later terminal count to an earlier snapshot of complete CSV rows.
    if matches!(
        record.get("event").and_then(Value::as_str),
        Some("finished" | "run_finished")
    ) {
        if run_active {
            return false;
        }
        if record
            .get("rows")
            .and_then(Value::as_u64)
            .is_some_and(|rows| rows > csv_rows as u64)
        {
            return false;
        }
    }
    let Some(point_index) = record.get("point_index").and_then(Value::as_u64) else {
        // Context, schema/header, and other non-point provenance describe the
        // complete snapshot regardless of the current acquisition row.
        return true;
    };
    // Preserve the established N+1 error evidence even when that attempted
    // point could not produce a main-CSV row. Do not include later successes.
    let failed = record.get("status").and_then(Value::as_str) == Some("error");
    point_index <= csv_rows.saturating_add(usize::from(failed)) as u64
}

fn stage_snapshot_metadata(
    metadata: &mut Value,
    parent: &std::path::Path,
    rows: usize,
    state: SnapshotState,
    destination_audit: Option<&PathBuf>,
) -> Result<tempfile::NamedTempFile, String> {
    use std::io::Write;
    let object = metadata
        .as_object_mut()
        .ok_or_else(|| "Save snapshot metadata must be a JSON object".to_string())?;
    let source_status = object
        .get("status")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let status = match state {
        SnapshotState::Active => "snapshot".to_owned(),
        SnapshotState::Failed => "error".to_owned(),
        SnapshotState::Finalized => source_status.expect("classified terminal metadata"),
    };
    object.insert("rows".into(), json!(rows));
    object.insert("status".into(), json!(status));
    if let Some(path) = destination_audit {
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "Save snapshot readings filename must be UTF-8".to_string())?;
        if let Some(raw) = object.get_mut("raw_csv") {
            raw["filename"] = json!(filename);
            raw["path"] = json!(path.display().to_string());
        } else {
            object.insert("readings_file".into(), json!(filename));
        }
    }
    let bytes = serde_json::to_vec_pretty(metadata).map_err(|e| e.to_string())?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|e| format!("Save snapshot: {e}"))?;
    temporary.write_all(&bytes).map_err(|e| e.to_string())?;
    temporary
        .flush()
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    Ok(temporary)
}

pub(super) fn persist_snapshot_noclobber(
    staged: Vec<(PathBuf, tempfile::NamedTempFile)>,
) -> Result<(), String> {
    let mut created = Vec::new();
    for (destination, temporary) in staged {
        match temporary.persist_noclobber(&destination) {
            Ok(_) => created.push(destination),
            Err(error) => {
                // Do not delete paths by name: another process may have
                // replaced them after activation. Preserve partial evidence.
                return Err(format!(
                    "Save snapshot: {error}; partial snapshot files retained: {created:?}"
                ));
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_gui_idle_flag_does_not_finalize_unfinished_source_in_either_audit_format() {
        for source_status in [None, Some("running"), Some("snapshot")] {
            for raw_csv in [false, true] {
                let directory = tempfile::tempdir().unwrap();
                let source = directory.path().join("source.csv");
                let destination = directory.path().join("snapshot.csv");
                std::fs::write(&source, "run_id,point_index\nrun,1\n").unwrap();
                let mut metadata = json!({"run_id":"run"});
                if let Some(status) = source_status {
                    metadata["status"] = json!(status);
                }
                let records = [
                    json!({"event":"reading", "point_index":1, "status":"ok"}),
                    // Acquisition completed after its metadata was observed,
                    // even though the GUI is already showing idle.
                    json!({"event":"finished", "rows":1, "status":"complete"}),
                ];
                if raw_csv {
                    metadata["raw_csv"] = json!({"filename":"source.raw.csv"});
                    let mut writer =
                        crate::raw_csv::RawWriter::create(&source.with_extension("raw.csv"), "run")
                            .unwrap();
                    for record in &records {
                        writer.append(record).unwrap();
                    }
                } else {
                    metadata["readings_file"] = json!("source.readings.jsonl");
                    let lines = records
                        .iter()
                        .map(|record| format!("{record}\n"))
                        .collect::<String>();
                    std::fs::write(source.with_extension("readings.jsonl"), lines).unwrap();
                }
                std::fs::write(
                    source.with_extension("run.json"),
                    serde_json::to_vec(&metadata).unwrap(),
                )
                .unwrap();
                copy_complete_csv(&source, &destination, false).unwrap();
                let copied: Value = serde_json::from_slice(
                    &std::fs::read(destination.with_extension("run.json")).unwrap(),
                )
                .unwrap();
                assert_eq!(copied["status"], "snapshot");
                assert_eq!(copied["rows"], 1);
                let audit = std::fs::read_to_string(destination.with_extension(if raw_csv {
                    "raw.csv"
                } else {
                    "readings.jsonl"
                }))
                .unwrap();
                assert!(audit.contains("reading"));
                assert!(
                    !audit.contains("finished"),
                    "status={source_status:?} raw={raw_csv}"
                );
            }
        }
    }

    #[test]
    fn terminal_source_status_and_failed_raw_evidence_survive_snapshot() {
        for status in ["complete", "cancelled", "error"] {
            let directory = tempfile::tempdir().unwrap();
            let source = directory.path().join("source.csv");
            let destination = directory.path().join("snapshot.csv");
            std::fs::write(&source, "run_id,point_index\nrun,1\n").unwrap();
            std::fs::write(
                source.with_extension("run.json"),
                serde_json::to_vec(&json!({
                    "status":status, "readings_file":"source.readings.jsonl"
                }))
                .unwrap(),
            )
            .unwrap();
            std::fs::write(source.with_extension("readings.jsonl"), format!(
                "{{\"event\":\"reading\",\"point_index\":2,\"status\":\"ok\"}}\n{{\"event\":\"finished\",\"rows\":1,\"status\":\"{status}\"}}\n"
            )).unwrap();
            copy_complete_csv(&source, &destination, false).unwrap();
            let copied: Value = serde_json::from_slice(
                &std::fs::read(destination.with_extension("run.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(copied["status"], status);
            let audit =
                std::fs::read_to_string(destination.with_extension("readings.jsonl")).unwrap();
            assert!(audit.contains("finished"));
            // Only a failed run retains a successful instrument reading whose
            // corresponding main CSV write failed.
            assert_eq!(audit.contains("reading"), status == "error");
        }
    }
}

#[cfg(test)]
mod bundle_tests {
    use super::*;
    #[test]
    fn completed_bundle_copy_relocates_and_hashes_log_and_refuses_missing_log() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.csv");
        crate::engine::demo_csv(
            trkr_core::Plan {
                n_points: 2,
                ..Default::default()
            },
            &source,
        )
        .unwrap();
        let destination = dir.path().join("copy.csv");
        copy_complete_csv(&source, &destination, false).unwrap();
        let meta: Value =
            serde_json::from_slice(&std::fs::read(destination.with_extension("run.json")).unwrap())
                .unwrap();
        assert_eq!(meta["run_bundle"]["filenames"]["analysis_csv"], "copy.csv");
        assert_eq!(meta["log"]["filename"], "copy.log");
        assert_eq!(
            meta["file_hashes"]["log_sha256"],
            crate::raw_csv::file_sha256(&destination.with_extension("log")).unwrap()
        );
        std::fs::remove_file(source.with_extension("log")).unwrap();
        let missing = dir.path().join("missing.csv");
        assert!(
            copy_complete_csv(&source, &missing, false)
                .unwrap_err()
                .contains("log is missing")
        );
        assert!(!missing.exists());
    }
}
