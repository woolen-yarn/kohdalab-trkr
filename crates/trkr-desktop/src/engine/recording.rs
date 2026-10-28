//! CSV schema, Python-compatible number formatting and run metadata.
//! Instrument ownership and acquisition scheduling remain in the parent engine.
use super::{Sample, Source};
use crate::raw_csv::{RawWriter, file_sha256, new_run_id};
use serde_json::Value;
use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use trkr_core::{Axis, Coordinate, Plan, Workflow};

pub const FIELDS: &[&str] = &[
    "run_id",
    "point_index",
    "timestamp",
    "elapsed_s",
    "quality_flags",
    "measurement",
    "fast_axis",
    "slow_axis",
    "line_index",
    "target_elapsed_s",
    "target_t_cor_ps",
    "target_x_cor_um",
    "target_y_cor_um",
    "target_u_cor_um",
    "target_v_cor_um",
    "t_ps",
    "x_um",
    "y_um",
    "t_cor_ps",
    "x_cor_um",
    "y_cor_um",
    "u_cor_um",
    "v_cor_um",
    "spatial_theta_deg",
    "X",
    "Y",
    "R",
    "Theta_deg",
    "signal_unit",
];

pub(super) struct Recording {
    csv: csv::Writer<File>,
    audit: RawWriter,
    metadata_path: PathBuf,
    save_failed: bool,
}

impl Recording {
    fn mark_save<T>(&mut self, result: Result<T, String>) -> Result<T, String> {
        if result.is_err() {
            self.save_failed = true;
        }
        result
    }

    pub(super) fn save_failed(&self) -> bool {
        self.save_failed
    }
}

impl Recording {
    pub(super) fn run_id(&self) -> &str {
        self.audit.run_id()
    }
    pub(super) fn write_record<I, T>(&mut self, record: I) -> csv::Result<()>
    where
        I: IntoIterator<Item = T>,
        T: AsRef<[u8]>,
    {
        self.csv.write_record(record)
    }

    pub(super) fn flush(&mut self) -> io::Result<()> {
        self.csv.flush()
    }

    /// Append one flushed raw CSV provenance event.  Each row is flushed so a
    /// completed acquisition point never relies on a later buffer flush.
    pub(super) fn append_audit(&mut self, value: &Value) -> Result<(), String> {
        let result = self.audit.append(value);
        self.mark_save(result)
    }

    /// Count a flushed CSV row before persisting its later completion event.
    /// The completion time cannot truthfully be embedded in the row just written.
    pub(super) fn write_sample(
        &mut self,
        plan: &Plan,
        sample: &Sample,
        origin: std::time::Instant,
        point_index: usize,
        rows: &mut usize,
    ) -> Result<(), String> {
        let (result, window, _) = super::timing::observe(origin, || {
            self.write_record(row(plan, sample)?)
                .map_err(|error| error.to_string())?;
            self.flush().map_err(|error| error.to_string())
        });
        let result = self.mark_save(result);
        if result.is_ok() {
            *rows += 1;
        }
        let audit = self.append_audit(&serde_json::json!({
            "event":"csv_write", "point_index":point_index,
            "status":if result.is_ok() { "ok" } else { "error" },
            "timing":window, "error":result.as_ref().err(),
        }));
        match (result, audit) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Err(error), Err(audit)) => Err(format!("{error}; record CSV write timing: {audit}")),
        }
    }

    /// Attach immutable run context without rewriting the audit history.
    pub(super) fn set_context(&mut self, context: &Value) -> Result<(), String> {
        let result = (|| {
            let mut metadata: Value = serde_json::from_slice(
                &std::fs::read(&self.metadata_path).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            let object = metadata
                .as_object_mut()
                .ok_or_else(|| "Run metadata must be a JSON object".to_string())?;
            object.insert("context".into(), context.clone());
            replace_json_atomically(&self.metadata_path, &metadata)?;
            Ok(())
        })();
        self.mark_save(result)?;
        self.append_audit(&serde_json::json!({"event": "context", "context": context}))
    }

    pub(super) fn set_final_position(
        &mut self,
        position: &trkr_motion::Position,
        phase: &str,
    ) -> Result<(), String> {
        let result = (|| {
            let mut metadata: Value = serde_json::from_slice(
                &std::fs::read(&self.metadata_path).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            metadata["last_observed_position"] = serde_json::json!(position);
            metadata["last_phase"] = serde_json::json!(phase);
            metadata["position_qualification"] =
                serde_json::json!("last_reported_position_no_extra_final_query");
            replace_json_atomically(&self.metadata_path, &metadata)
        })();
        self.mark_save(result)
    }

    /// Durably finish both output streams once an acquisition is complete.
    /// Per-point audit writes intentionally flush but do not issue `sync_all`.
    pub(super) fn finish(&mut self) -> Result<(), String> {
        let result = (|| {
            self.csv.flush().map_err(|e| e.to_string())?;
            self.csv.get_ref().sync_all().map_err(|e| e.to_string())?;
            self.audit.finish()
        })();
        self.mark_save(result)
    }
}

pub(super) fn finish_metadata(
    path: &Path,
    rows: usize,
    status: &str,
    error: Option<&String>,
) -> Result<(), String> {
    let csv_path = path;
    let path = path.with_extension("run.json");
    let mut meta: Value = serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    meta["rows"] = serde_json::json!(rows);
    meta["status"] = serde_json::json!(status);
    meta["error"] = serde_json::json!(error);
    meta["finished_at"] = serde_json::json!(utc_now()?);
    meta["file_hashes"] = serde_json::json!({
        "csv_sha256": file_sha256(csv_path)?,
        "raw_csv_sha256": file_sha256(&csv_path.with_extension("raw.csv"))?,
    });
    replace_json_atomically(&path, &meta)
}
pub(super) fn create_csv(path: &Path, source: Source, plan: &Plan) -> Result<Recording, String> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if path.with_extension("readings.jsonl").exists() {
        return Err("Legacy raw sidecar already exists; choose another filename".into());
    }
    let metadata_path = path.with_extension("run.json");
    if metadata_path == path {
        return Err("CSV path must not have the .run.json extension".into());
    }
    let readings_path = path.with_extension("raw.csv");
    let run_id = new_run_id();
    if readings_path == path || readings_path == metadata_path {
        return Err("CSV path must permit distinct metadata and readings files".into());
    }
    let metadata = serde_json::json!({
        "application": "kohdalab-trkr", "version": env!("CARGO_PKG_VERSION"),
        "simulation": matches!(source, Source::Demo), "source": format!("{source:?}"),
        "plan": plan, "experiment":plan.experiment, "timestamp": utc_now()?,
        "schema_version": 3,
        "recording_revision": 5,
        "csv_schema": "trkr_analysis_v1", "raw_schema": "raw_csv_v1",
        "run_id":run_id, "status":"running", "rows":0,
        "software": {"version":env!("CARGO_PKG_VERSION"), "git_commit":option_env!("KOHDALAB_BUILD_COMMIT")},
        "coordinate_policy": {
            "readback_fields": "all_available_axes; missing readings stay empty",
            "rotated_origin": "measurement_zero_xy",
            "rotated_readback": "u=x_cor*cos(theta)+y_cor*sin(theta); v=-x_cor*sin(theta)+y_cor*cos(theta)",
            "rotated_targets": "commanded u/v; orthogonal axis explicitly zero",
        },
        "csv_numeric_format": "shortest_roundtrip_scientific",
        "csv_scientific_fields": ["X", "Y", "R"],
        "other_numeric_fields": "shortest_roundtrip_python_compatible",
        "precision_policy": "no_accuracy_claim",
        "timing_policy": {
            "clock": "host UTC labels and monotonic elapsed seconds; device timestamp unknown",
            "csv_timestamp": "signal read completion; empty when unavailable",
            "position": "aggregate position read window before signal read",
            "csv_write": "separate audit event; completion after CSV flush, not per-row fsync",
        },
        "raw_csv": {"schema":"raw_csv_v1", "filename":readings_path.file_name().and_then(|name|name.to_str()).ok_or("Raw filename must be UTF-8")?},
    });
    let bytes = serde_json::to_vec_pretty(&metadata).map_err(|e| e.to_string())?;
    let mut csv_created = false;
    let mut metadata_created = false;
    let mut readings_created = false;
    let result = (|| {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| format!("Create {}: {e}", path.display()))?;
        csv_created = true;
        let mut meta = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&metadata_path)
            .map_err(|e| format!("Create run metadata: {e}"))?;
        metadata_created = true;
        meta.write_all(&bytes).map_err(|e| e.to_string())?;
        meta.sync_all().map_err(|e| e.to_string())?;
        let audit = RawWriter::create(&readings_path, &run_id)?;
        readings_created = true;
        let mut writer = csv::Writer::from_writer(file);
        writer.write_record(FIELDS).map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())?;
        Ok(Recording {
            csv: writer,
            audit,
            metadata_path: metadata_path.clone(),
            save_failed: false,
        })
    })();
    if result.is_err() {
        if csv_created {
            let _ = std::fs::remove_file(path);
        }
        if metadata_created {
            let _ = std::fs::remove_file(metadata_path);
        }
        if readings_created {
            let _ = std::fs::remove_file(readings_path);
        }
    }
    result
}

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Create a private new temporary file, sync it, and only then atomically
/// replace the destination.  A failed replacement leaves the old metadata in
/// place and never reuses a foreign temporary filename.
fn replace_json_atomically(path: &Path, value: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Metadata filename must be valid UTF-8".to_string())?;
    let mut temporary = None;
    for _ in 0..32 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), sequence));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut file) => {
                if let Err(error) = (|| -> io::Result<()> {
                    file.write_all(&bytes)?;
                    file.sync_all()
                })() {
                    let _ = std::fs::remove_file(&candidate);
                    return Err(error.to_string());
                }
                temporary = Some(candidate);
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
    let temporary =
        temporary.ok_or_else(|| "Could not reserve metadata temporary file".to_string())?;
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(temporary);
        return Err(error.to_string());
    }
    Ok(())
}

pub(super) fn utc_now() -> Result<String, String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|e| e.to_string())
}

/// Python str(float): shortest round-trip digits, .0 for integral fixed values,
/// fixed notation for 1e-4 <= abs < 1e16, and signed two-digit exponents.
pub(crate) fn csv_float(value: f64) -> String {
    let shortest = format!("{value:?}");
    if !value.is_finite() {
        return shortest;
    }
    let sign = if value.is_sign_negative() { "-" } else { "" };
    let text = shortest.trim_start_matches('-');
    let (mantissa, extra) = text
        .split_once('e')
        .map(|(m, e)| (m, e.parse::<i32>().expect("Rust exponent")))
        .unwrap_or((text, 0));
    let point = mantissa.find('.').unwrap_or(mantissa.len()) as i32;
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let Some(first) = digits.find(|c: char| c != '0') else {
        return format!("{sign}0.0");
    };
    let exponent = point - first as i32 - 1 + extra;
    let digits = digits[first..].trim_end_matches('0');
    if !(-4..16).contains(&exponent) {
        let fraction = if digits.len() > 1 {
            format!(".{}", &digits[1..])
        } else {
            String::new()
        };
        format!("{sign}{}{fraction}e{exponent:+03}", &digits[..1])
    } else if exponent < 0 {
        format!("{sign}0.{}{}", "0".repeat((-exponent - 1) as usize), digits)
    } else {
        let point = exponent as usize + 1;
        if digits.len() <= point {
            format!("{sign}{digits}{}.0", "0".repeat(point - digits.len()))
        } else {
            format!("{sign}{}.{}", &digits[..point], &digits[point..])
        }
    }
}

/// Shortest f64 round-trip representation, always in scientific notation.
/// The output describes the acquired binary value; it does not imply a
/// physical accuracy or an instrument resolution.
fn csv_voltage(value: f64) -> Result<String, String> {
    if !value.is_finite() {
        return Err("Non-finite raw lock-in voltage cannot be saved".into());
    }
    let shortest = format!("{value:?}");
    let sign = if value.is_sign_negative() { "-" } else { "" };
    let text = shortest.trim_start_matches('-');
    let (mantissa, extra) = text
        .split_once('e')
        .map(|(mantissa, exponent)| {
            (
                mantissa,
                exponent
                    .parse::<i32>()
                    .expect("Rust exponent from Debug f64"),
            )
        })
        .unwrap_or((text, 0));
    let point = mantissa.find('.').unwrap_or(mantissa.len()) as i32;
    let digits: String = mantissa
        .chars()
        .filter(|character| *character != '.')
        .collect();
    let Some(first) = digits.find(|character: char| character != '0') else {
        return Ok(format!("{sign}0e+00"));
    };
    let exponent = point - first as i32 - 1 + extra;
    let digits = digits[first..].trim_end_matches('0');
    let fraction = if digits.len() > 1 {
        format!(".{}", &digits[1..])
    } else {
        String::new()
    };
    Ok(format!("{sign}{}{fraction}e{exponent:+03}", &digits[..1]))
}

pub(crate) fn row(plan: &Plan, sample: &Sample) -> Result<Vec<String>, String> {
    let mut result = vec![String::new(); FIELDS.len()];
    let mut put =
        |key: &str, value: String| result[FIELDS.iter().position(|&k| k == key).unwrap()] = value;
    put(
        "timestamp",
        sample
            .timing
            .signal
            .as_ref()
            .and_then(|window| window.finished_at.clone())
            .unwrap_or_default(),
    );
    put("run_id", sample.recording.run_id.clone());
    put("point_index", sample.recording.point_index.to_string());
    put("elapsed_s", csv_float(sample.elapsed));
    put("quality_flags", sample.recording.quality_flags.clone());
    put(
        "line_index",
        if plan.workflow.is_2d() {
            sample.target.line.to_string()
        } else {
            String::new()
        },
    );
    put("signal_unit", sample.recording.signal_unit.clone());
    put("measurement", plan.workflow.key().into());
    let monitor = plan.workflow == Workflow::SignalMonitor;
    if monitor {
        put("elapsed_s", csv_float(sample.elapsed));
        put("fast_axis", "elapsed_s".into());
        put("target_elapsed_s", csv_float(sample.target.fast));
    } else {
        put("fast_axis", plan.fast.name().into());
        if plan.workflow.is_2d() {
            put("slow_axis", plan.slow.name().into());
        }
        for (axis, value) in [
            (plan.fast, Some(sample.target.fast)),
            (plan.slow, sample.target.slow),
        ] {
            if let Some(value) = value {
                put(
                    &format!(
                        "target_{}_cor_{}",
                        axis.name(),
                        if axis == trkr_core::Axis::T {
                            "ps"
                        } else {
                            "um"
                        }
                    ),
                    csv_float(match axis {
                        Axis::T | Axis::X | Axis::Y => {
                            let index = match axis {
                                Axis::T => 0,
                                Axis::X => 1,
                                _ => 2,
                            };
                            sample.recording.canonical_target[index]
                                .or_else(|| {
                                    (plan.coordinate == Coordinate::Measurement).then_some(value)
                                })
                                .ok_or(
                                    "Raw-coordinate export requires resolved canonical targets",
                                )?
                        }
                        _ => value,
                    }),
                );
            }
        }
        if matches!(plan.fast, Axis::U | Axis::V)
            || (plan.workflow.is_2d() && matches!(plan.slow, Axis::U | Axis::V))
        {
            // Rotated moves command both X and Y. The orthogonal U/V target
            // is explicitly zero, not an unknown fixed Cartesian position.
            for (axis, field) in [(Axis::U, "target_u_cor_um"), (Axis::V, "target_v_cor_um")] {
                let value = if plan.fast == axis {
                    sample.target.fast
                } else if plan.workflow.is_2d() && plan.slow == axis {
                    sample.target.slow.ok_or("Missing slow target")?
                } else {
                    0.0
                };
                put(field, csv_float(value));
            }
        }
    }
    // Position already contains fresh readbacks from the connected rig. Export
    // every available axis, including fixed axes; never substitute scan targets.
    let corrected: [Option<f64>; 3] =
        std::array::from_fn(|i| sample.position.physical[i].map(|value| value - plan.zero[i]));
    for (i, axis) in ["t", "x", "y"].iter().enumerate() {
        let unit = if i == 0 { "ps" } else { "um" };
        if let Some(value) = sample.position.physical[i] {
            put(&format!("{axis}_{unit}"), csv_float(value));
            put(
                &format!("{axis}_cor_{unit}"),
                csv_float(corrected[i].unwrap()),
            );
        }
    }
    if let (Some(x), Some(y)) = (corrected[1], corrected[2]) {
        let (s, c) = plan.theta_deg.to_radians().sin_cos();
        put("u_cor_um", csv_float(x * c + y * s));
        put("v_cor_um", csv_float(-x * s + y * c));
    }
    put("spatial_theta_deg", csv_float(plan.theta_deg));
    put("X", csv_voltage(sample.signal.x_v)?);
    put("Y", csv_voltage(sample.signal.y_v)?);
    put("R", csv_voltage(sample.signal.r_v)?);
    put("Theta_deg", csv_float(sample.signal.theta_deg));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use trkr_core::Signal;
    use trkr_motion::Position;

    #[test]
    fn raw_input_targets_require_and_use_resolved_measurement_units() {
        for coordinate in [Coordinate::Interface, Coordinate::Instrument] {
            let plan = Plan {
                workflow: Workflow::Trkr,
                coordinate,
                zero: [-250., 0., 0.],
                ..Plan::default()
            };
            let mut sample = test_sample(&plan, [Some(-200.), None, None]);
            sample.target.fast = if coordinate == Coordinate::Interface {
                275.
            } else {
                1234.
            };
            assert!(
                row(&plan, &sample)
                    .unwrap_err()
                    .contains("resolved canonical targets")
            );
            sample.recording.canonical_target[0] = Some(50.);
            let saved = row(&plan, &sample).unwrap();
            assert_eq!(cell(&saved, "target_t_cor_ps"), "50.0");
            assert_eq!(cell(&saved, "t_cor_ps"), "50.0");
        }
    }

    #[test]
    fn output_collisions_preserve_existing_data_and_rollback_only_new_files() {
        for collision in ["run.csv", "run.run.json", "run.raw.csv"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("run.csv");
            let metadata = path.with_extension("run.json");
            let readings = path.with_extension("raw.csv");
            let collided = dir.path().join(collision);
            std::fs::write(&collided, "existing").unwrap();

            assert!(create_csv(&path, Source::Demo, &Plan::default()).is_err());
            assert_eq!(std::fs::read_to_string(&collided).unwrap(), "existing");
            for output in [&path, &metadata, &readings] {
                if output != &collided {
                    assert!(!output.exists(), "{}", output.display());
                }
            }
        }
    }

    #[test]
    fn csv_row_is_counted_when_later_timing_audit_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.csv");
        let plan = Plan::default();
        let mut recording = create_csv(&path, Source::Demo, &plan).unwrap();
        let run_id = recording.run_id().to_owned();
        recording.audit =
            RawWriter::open_readonly_for_test(&path.with_extension("raw.csv"), &run_id).unwrap();
        let sample = Sample {
            recording: Default::default(),
            timing: Default::default(),
            captured_at: None,
            elapsed: 0.0,
            target: plan.targets().unwrap()[0],
            signal: Signal::demo(0.0, 0.0, 0.0),
            position: Position::default(),
        };
        let mut rows = 0;
        assert!(
            recording
                .write_sample(&plan, &sample, std::time::Instant::now(), 1, &mut rows)
                .is_err()
        );
        assert_eq!(rows, 1);
        assert!(recording.save_failed());
        let mut reader = csv::Reader::from_path(path).unwrap();
        let record = reader.records().next().unwrap().unwrap();
        assert_eq!(
            &record[2], "",
            "unknown acquisition time must not become export time"
        );
    }

    #[test]
    fn metadata_includes_precision_and_audit_schemas() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.csv");
        let _recording = create_csv(&path, Source::Demo, &Plan::default()).unwrap();
        let metadata: Value =
            serde_json::from_slice(&std::fs::read(path.with_extension("run.json")).unwrap())
                .unwrap();
        assert_eq!(metadata["schema_version"], 3);
        assert_eq!(
            metadata["csv_numeric_format"],
            "shortest_roundtrip_scientific"
        );
        assert_eq!(metadata["precision_policy"], "no_accuracy_claim");
        assert_eq!(metadata["raw_csv"]["schema"], "raw_csv_v1");
        assert_eq!(metadata["raw_csv"]["filename"], "run.raw.csv");
        assert!(metadata.get("plan").is_some());
        assert!(metadata.get("source").is_some());
    }

    #[test]
    fn audit_records_raw_string_precision_and_finishes_durably() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.csv");
        let mut recording = create_csv(&path, Source::Demo, &Plan::default()).unwrap();
        recording
            .append_audit(&serde_json::json!({
                "event": "reading",
                "reply": "1.230000000000e-9\\r\\n"
            }))
            .unwrap();
        let readings = path.with_extension("raw.csv");
        assert!(
            std::fs::read_to_string(&readings)
                .unwrap()
                .contains("1.230000000000e-9")
        );
        recording.finish().unwrap();
        assert!(std::fs::metadata(readings).unwrap().len() > 0);
    }

    #[test]
    fn context_update_is_atomic_when_metadata_cannot_be_parsed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.csv");
        let mut recording = create_csv(&path, Source::Demo, &Plan::default()).unwrap();
        let metadata = path.with_extension("run.json");
        std::fs::write(&metadata, "initial bytes remain").unwrap();
        assert!(
            recording
                .set_context(&serde_json::json!({"device": "SR7265"}))
                .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(metadata).unwrap(),
            "initial bytes remain"
        );
        assert_eq!(
            csv::Reader::from_path(path.with_extension("raw.csv"))
                .unwrap()
                .records()
                .count(),
            0
        );
    }
    #[test]
    fn csv_float_preserves_python_repr_thresholds_sign_and_precision() {
        for (value, expected) in [
            (0., "0.0"),
            (-0., "-0.0"),
            (4., "4.0"),
            (1e-4, "0.0001"),
            (1e-5, "1e-05"),
            (1e15, "1000000000000000.0"),
            (1e16, "1e+16"),
            (-1e20, "-1e+20"),
            (1e-100, "1e-100"),
            (1.0000000000000002, "1.0000000000000002"),
        ] {
            assert_eq!(csv_float(value), expected);
        }
    }
    #[test]
    fn csv_voltage_is_shortest_roundtrip_scientific_for_extreme_finite_bits() {
        for value in [
            -0.0,
            f64::from_bits(1.0f64.to_bits() + 1),
            f64::from_bits(1),
            f64::MIN_POSITIVE,
            f64::MAX,
        ] {
            let text = csv_voltage(value).unwrap();
            assert!(text.contains('e'), "{text}");
            assert_eq!(
                text.parse::<f64>().unwrap().to_bits(),
                value.to_bits(),
                "{text}"
            );
        }
        assert_eq!(csv_voltage(-0.0).unwrap(), "-0e+00");
        assert_eq!(
            csv_voltage(f64::from_bits(1.0f64.to_bits() + 1)).unwrap(),
            "1.0000000000000002e+00"
        );
        assert_eq!(csv_voltage(f64::from_bits(1)).unwrap(), "5e-324");
        assert_eq!(csv_voltage(f64::MAX).unwrap(), "1.7976931348623157e+308");
    }
    #[test]
    fn row_rejects_nonfinite_raw_lockin_voltage() {
        let plan = Plan::default();
        let mut sample = Sample {
            recording: Default::default(),
            timing: Default::default(),
            captured_at: None,
            elapsed: 0.0,
            target: plan.targets().unwrap()[0],
            signal: Signal::default(),
            position: Position::default(),
        };
        sample.signal.x_v = f64::NAN;
        assert_eq!(
            row(&plan, &sample).unwrap_err(),
            "Non-finite raw lock-in voltage cannot be saved"
        );
    }
    #[test]
    fn monitor_retains_components_without_inventing_positions() {
        let p = Plan::default();
        let sample = Sample {
            recording: Default::default(),
            timing: Default::default(),
            captured_at: None,
            elapsed: 1.,
            target: p.targets().unwrap()[0],
            signal: Signal::new(1., 2., 3., 4.).unwrap(),
            position: Position::default(),
        };
        let r = row(&p, &sample).unwrap();
        let get = |k| &r[FIELDS.iter().position(|&v| v == k).unwrap()];
        assert_eq!(get("Theta_deg"), "4.0"); // Python CSV float representation.
        assert!(get("t_ps").is_empty());
    }
    #[test]
    fn actual_readback_is_exported_with_raw_values() {
        let p = Plan {
            workflow: Workflow::Trkr,
            zero: [-122., 0., 0.],
            ..Default::default()
        };
        let sample = Sample {
            recording: Default::default(),
            timing: Default::default(),
            captured_at: None,
            elapsed: 0.,
            target: p.targets().unwrap()[0],
            signal: Signal::default(),
            position: Position {
                physical: [Some(-171.99), None, None],
                delay_stage_mm: Some(275.),
                delay_stage_pulse: Some(1234),
                ..Default::default()
            },
        };
        let r = row(&p, &sample).unwrap();
        let get = |k| &r[FIELDS.iter().position(|&v| v == k).unwrap()];
        assert_eq!(get("t_ps"), "-171.99");
        assert_eq!(saved_raw_position(&sample)["delay_stage_pulse"], 1234);
        assert!(get("x_um").is_empty());
    }
    fn test_sample(plan: &Plan, physical: [Option<f64>; 3]) -> Sample {
        Sample {
            recording: Default::default(),
            timing: Default::default(),
            captured_at: None,
            elapsed: 0.0,
            target: plan.targets().unwrap()[0],
            signal: Signal::default(),
            position: Position {
                physical,
                delay_stage_mm: Some(12.5),
                delay_stage_pulse: Some(1234),
                x_scanner_mm: Some(0.02),
                y_scanner_deg: Some(0.03),
                ..Default::default()
            },
        }
    }
    fn saved_raw_position(sample: &Sample) -> Value {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("raw.csv");
        let mut writer = RawWriter::create(&path, "test").unwrap();
        writer
            .append(&serde_json::json!({"event":"reading","position":sample.position}))
            .unwrap();
        crate::raw_csv::test_events(&path)[0]["position"].clone()
    }
    fn cell<'a>(row: &'a [String], name: &str) -> &'a str {
        &row[FIELDS.iter().position(|field| *field == name).unwrap()]
    }
    #[test]
    fn every_workflow_exports_available_fixed_axis_readbacks_and_raw_units() {
        for (workflow, fast, slow) in [
            (Workflow::SignalMonitor, Axis::T, Axis::X),
            (Workflow::Trkr, Axis::T, Axis::X),
            (Workflow::Srkr, Axis::X, Axis::Y),
            (Workflow::Srkr, Axis::Y, Axis::X),
            (Workflow::Strkr, Axis::T, Axis::X),
            (Workflow::Strkr, Axis::Y, Axis::T),
            (Workflow::Srkr2d, Axis::X, Axis::Y),
        ] {
            let plan = Plan {
                workflow,
                fast,
                slow,
                zero: [-250., 10., 20.],
                ..Default::default()
            };
            let sample = test_sample(&plan, [Some(-200.), Some(12.), Some(24.)]);
            let r = row(&plan, &sample).unwrap();
            for (field, value) in [
                ("t_ps", -200.),
                ("t_cor_ps", 50.),
                ("x_um", 12.),
                ("x_cor_um", 2.),
                ("y_um", 24.),
                ("y_cor_um", 4.),
            ] {
                assert_eq!(
                    cell(&r, field).parse::<f64>().unwrap(),
                    value,
                    "{workflow:?}: {field}"
                );
            }
            let raw = saved_raw_position(&sample);
            assert_eq!(raw["delay_stage_pulse"], 1234);
            for (field, value) in [
                ("delay_stage_mm", 12.5),
                ("x_scanner_mm", 0.02),
                ("y_scanner_deg", 0.03),
            ] {
                assert_eq!(raw[field].as_f64(), Some(value));
            }
            if workflow == Workflow::Srkr {
                assert!(cell(&r, "target_t_cor_ps").is_empty());
            }
        }
    }
    #[test]
    fn rotated_scans_distinguish_commanded_uv_from_actual_readback() {
        for (workflow, fast, slow) in [
            (Workflow::Srkr, Axis::U, Axis::X),
            (Workflow::Srkr, Axis::V, Axis::X),
            (Workflow::Srkr2d, Axis::U, Axis::V),
            (Workflow::Srkr2d, Axis::V, Axis::U),
            (Workflow::Strkr, Axis::T, Axis::U),
            (Workflow::Strkr, Axis::V, Axis::T),
        ] {
            let plan = Plan {
                workflow,
                fast,
                slow,
                theta_deg: 90.,
                zero: [-250., 10., 20.],
                fast_range: trkr_core::Range {
                    min: 3.,
                    max: 3.,
                    step: 1.,
                },
                slow_range: trkr_core::Range {
                    min: 5.,
                    max: 5.,
                    step: 1.,
                },
                ..Default::default()
            };
            let sample = test_sample(&plan, [Some(-200.), Some(12.), Some(24.)]);
            let r = row(&plan, &sample).unwrap();
            assert!((cell(&r, "u_cor_um").parse::<f64>().unwrap() - 4.).abs() < 1e-12);
            assert!((cell(&r, "v_cor_um").parse::<f64>().unwrap() + 2.).abs() < 1e-12);
            assert_eq!(cell(&r, "spatial_theta_deg"), "90.0");
            for (axis, field) in [(Axis::U, "target_u_cor_um"), (Axis::V, "target_v_cor_um")] {
                let target = if fast == axis {
                    3.
                } else if workflow.is_2d() && slow == axis {
                    5.
                } else {
                    0.
                };
                assert_eq!(cell(&r, field).parse::<f64>().unwrap(), target);
            }
        }
    }
    #[test]
    fn missing_positions_stay_empty_instead_of_becoming_targets_or_zero() {
        let plan = Plan {
            workflow: Workflow::Srkr,
            fast: Axis::U,
            theta_deg: 37.,
            ..Default::default()
        };
        for physical in [
            [None, None, None],
            [None, Some(3.), None],
            [None, None, Some(4.)],
        ] {
            let sample = Sample {
                position: Position {
                    physical,
                    ..Default::default()
                },
                ..test_sample(&plan, physical)
            };
            let r = row(&plan, &sample).unwrap();
            for field in ["t_ps", "t_cor_ps", "u_cor_um", "v_cor_um"] {
                assert!(cell(&r, field).is_empty(), "{field}");
            }
            assert!(!cell(&r, "target_u_cor_um").is_empty());
            assert_eq!(cell(&r, "target_v_cor_um"), "0.0");
        }
    }
}
