//! CSV schema, Python-compatible number formatting and run metadata.
//! Instrument ownership and acquisition scheduling remain in the parent engine.
use super::{Sample, Source, axis_mask, coordinate_name};
use serde_json::Value;
use std::{
    fs::{File, OpenOptions},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use trkr_core::{Plan, Workflow};

pub const FIELDS: &[&str] = &[
    "timestamp",
    "measurement",
    "fast_axis",
    "slow_axis",
    "target_elapsed_s",
    "target_t_cor_ps",
    "target_x_cor_um",
    "target_y_cor_um",
    "target_u_cor_um",
    "target_v_cor_um",
    "elapsed_s",
    "acquisition_elapsed_s",
    "t_cor_ps",
    "t_ps",
    "x_cor_um",
    "x_um",
    "y_cor_um",
    "y_um",
    "u_cor_um",
    "v_cor_um",
    "X_V",
    "Y_V",
    "R_V",
    "Theta_deg",
    "spatial_theta_deg",
    "coordinate",
    "delay_stage_mm",
    "delay_stage_pulse",
    "x_scanner_mm",
    "x_scanner_deg",
    "y_scanner_mm",
    "y_scanner_deg",
];

pub(super) struct Recording {
    csv: csv::Writer<File>,
    audit: BufWriter<File>,
    metadata_path: PathBuf,
}

impl Recording {
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

    /// Append one flushed JSONL provenance record.  Each row is flushed so a
    /// completed acquisition point never relies on a later buffer flush.
    pub(super) fn append_audit(&mut self, value: &Value) -> Result<(), String> {
        serde_json::to_writer(&mut self.audit, value).map_err(|e| e.to_string())?;
        self.audit.write_all(b"\n").map_err(|e| e.to_string())?;
        self.audit.flush().map_err(|e| e.to_string())
    }

    /// Attach immutable run context without rewriting the audit history.
    pub(super) fn set_context(&mut self, context: &Value) -> Result<(), String> {
        let mut metadata: Value =
            serde_json::from_slice(&std::fs::read(&self.metadata_path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let object = metadata
            .as_object_mut()
            .ok_or_else(|| "Run metadata must be a JSON object".to_string())?;
        object.insert("context".into(), context.clone());
        replace_json_atomically(&self.metadata_path, &metadata)?;
        self.append_audit(&serde_json::json!({"event": "context", "context": context}))
    }

    /// Durably finish both output streams once an acquisition is complete.
    /// Per-point audit writes intentionally flush but do not issue `sync_all`.
    pub(super) fn finish(&mut self) -> Result<(), String> {
        self.csv.flush().map_err(|e| e.to_string())?;
        self.csv.get_ref().sync_all().map_err(|e| e.to_string())?;
        self.audit.flush().map_err(|e| e.to_string())?;
        self.audit.get_ref().sync_all().map_err(|e| e.to_string())
    }
}

pub(super) fn finish_metadata(
    path: &Path,
    rows: usize,
    status: &str,
    error: Option<&String>,
) -> Result<(), String> {
    let path = path.with_extension("run.json");
    let mut meta: Value = serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    meta["rows"] = serde_json::json!(rows);
    meta["status"] = serde_json::json!(status);
    meta["error"] = serde_json::json!(error);
    meta["finished_at"] = serde_json::json!(utc_now()?);
    replace_json_atomically(&path, &meta)
}
pub(super) fn create_csv(path: &Path, source: Source, plan: &Plan) -> Result<Recording, String> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let metadata_path = path.with_extension("run.json");
    if metadata_path == path {
        return Err("CSV path must not have the .run.json extension".into());
    }
    let readings_path = path.with_extension("readings.jsonl");
    if readings_path == path || readings_path == metadata_path {
        return Err("CSV path must permit distinct metadata and readings files".into());
    }
    let metadata = serde_json::json!({
        "application": "kohdalab-trkr", "version": env!("CARGO_PKG_VERSION"),
        "simulation": matches!(source, Source::Demo), "source": format!("{source:?}"),
        "plan": plan, "timestamp": utc_now()?,
        "schema_version": 2,
        "csv_numeric_format": "shortest_roundtrip_scientific",
        "csv_scientific_fields": ["X_V", "Y_V", "R_V"],
        "other_numeric_fields": "shortest_roundtrip_python_compatible",
        "precision_policy": "no_accuracy_claim",
        "readings_schema_version": 1,
        "readings_file": readings_path.file_name().and_then(|name| name.to_str())
            .ok_or_else(|| "Readings filename must be valid UTF-8".to_string())?,
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
        let audit_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&readings_path)
            .map_err(|e| format!("Create readings audit: {e}"))?;
        readings_created = true;
        let mut writer = csv::Writer::from_writer(file);
        writer.write_record(FIELDS).map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())?;
        Ok(Recording {
            csv: writer,
            audit: BufWriter::new(audit_file),
            metadata_path: metadata_path.clone(),
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
fn csv_float(value: f64) -> String {
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
    put("timestamp", utc_now()?);
    put("measurement", plan.workflow.key().into());
    put("acquisition_elapsed_s", csv_float(sample.elapsed));
    let monitor = plan.workflow == Workflow::SignalMonitor;
    let exported_axes = match plan.workflow {
        Workflow::SignalMonitor => [false; 3],
        Workflow::Trkr => [true, false, false],
        Workflow::Srkr => axis_mask(plan.fast),
        Workflow::Strkr | Workflow::Srkr2d => [true; 3],
    };
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
                    csv_float(value),
                );
            }
        }
        for (i, axis) in ["t", "x", "y"].iter().enumerate() {
            let unit = if i == 0 { "ps" } else { "um" };
            if exported_axes[i]
                && let Some(value) = sample.position.physical[i]
            {
                put(&format!("{axis}_{unit}"), csv_float(value));
                put(
                    &format!("{axis}_cor_{unit}"),
                    csv_float(value - plan.zero[i]),
                );
            }
        }
        let (s, c) = plan.theta_deg.to_radians().sin_cos();
        let [_, x, y] = sample.target.corrected;
        put("u_cor_um", csv_float(x * c + y * s));
        put("v_cor_um", csv_float(-x * s + y * c));
        put("spatial_theta_deg", csv_float(plan.theta_deg));
        put("coordinate", coordinate_name(plan.coordinate).into());
    }
    for (axis, key, value) in [
        (0, "delay_stage_mm", sample.position.delay_stage_mm),
        (1, "x_scanner_mm", sample.position.x_scanner_mm),
        (1, "x_scanner_deg", sample.position.x_scanner_deg),
        (2, "y_scanner_mm", sample.position.y_scanner_mm),
        (2, "y_scanner_deg", sample.position.y_scanner_deg),
    ] {
        if exported_axes[axis]
            && let Some(value) = value
        {
            put(key, csv_float(value));
        }
    }
    if exported_axes[0]
        && let Some(value) = sample.position.delay_stage_pulse
    {
        put("delay_stage_pulse", value.to_string());
    }
    put("X_V", csv_voltage(sample.signal.x_v)?);
    put("Y_V", csv_voltage(sample.signal.y_v)?);
    put("R_V", csv_voltage(sample.signal.r_v)?);
    put("Theta_deg", csv_float(sample.signal.theta_deg));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use trkr_core::Signal;
    use trkr_motion::Position;

    #[test]
    fn output_collisions_preserve_existing_data_and_rollback_only_new_files() {
        for collision in ["run.csv", "run.run.json", "run.readings.jsonl"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("run.csv");
            let metadata = path.with_extension("run.json");
            let readings = path.with_extension("readings.jsonl");
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
    fn metadata_includes_precision_and_audit_schemas() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.csv");
        let _recording = create_csv(&path, Source::Demo, &Plan::default()).unwrap();
        let metadata: Value =
            serde_json::from_slice(&std::fs::read(path.with_extension("run.json")).unwrap())
                .unwrap();
        assert_eq!(metadata["schema_version"], 2);
        assert_eq!(
            metadata["csv_numeric_format"],
            "shortest_roundtrip_scientific"
        );
        assert_eq!(metadata["precision_policy"], "no_accuracy_claim");
        assert_eq!(metadata["readings_schema_version"], 1);
        assert_eq!(metadata["readings_file"], "run.readings.jsonl");
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
        let readings = path.with_extension("readings.jsonl");
        assert!(
            std::fs::read_to_string(&readings)
                .unwrap()
                .contains("1.230000000000e-9\\\\r\\\\n")
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
        assert!(
            std::fs::read_to_string(path.with_extension("readings.jsonl"))
                .unwrap()
                .is_empty()
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
        assert_eq!(get("delay_stage_pulse"), "1234");
        assert!(get("x_um").is_empty());
    }
}
