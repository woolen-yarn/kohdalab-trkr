//! CSV schema, Python-compatible number formatting and run metadata.
//! Instrument ownership and acquisition scheduling remain in the parent engine.
use super::{Sample, Source, axis_mask, coordinate_name};
use serde_json::Value;
use std::{
    fs::{File, OpenOptions},
    path::Path,
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
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&meta).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
pub(super) fn create_csv(
    path: &Path,
    source: Source,
    plan: &Plan,
) -> Result<csv::Writer<File>, String> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let metadata_path = path.with_extension("run.json");
    if metadata_path == path {
        return Err("CSV path must not have the .run.json extension".into());
    }
    let metadata = serde_json::json!({
        "application": "kohdalab-trkr", "version": env!("CARGO_PKG_VERSION"),
        "simulation": matches!(source, Source::Demo), "source": format!("{source:?}"),
        "plan": plan, "timestamp": utc_now()?,
    });
    let bytes = serde_json::to_vec_pretty(&metadata).map_err(|e| e.to_string())?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("Create {}: {e}", path.display()))?;
    let mut metadata_created = false;
    let result = (|| {
        use std::io::Write;
        let mut meta = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&metadata_path)
            .map_err(|e| format!("Create run metadata: {e}"))?;
        metadata_created = true;
        meta.write_all(&bytes).map_err(|e| e.to_string())?;
        let mut writer = csv::Writer::from_writer(file);
        writer.write_record(FIELDS).map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())?;
        Ok(writer)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(path);
        if metadata_created {
            let _ = std::fs::remove_file(metadata_path);
        }
    }
    result
}

fn utc_now() -> Result<String, String> {
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

fn csv_voltage(value: f64) -> String {
    let text = format!("{value:.6e}");
    let Some((mantissa, exponent)) = text.split_once('e') else {
        return text;
    };
    let exponent: i32 = exponent.parse().expect("Rust scientific exponent");
    format!("{mantissa}e{exponent:+03}")
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
    put("X_V", csv_voltage(sample.signal.x_v));
    put("Y_V", csv_voltage(sample.signal.y_v));
    put("R_V", csv_voltage(sample.signal.r_v));
    put("Theta_deg", csv_float(sample.signal.theta_deg));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use trkr_core::Signal;
    use trkr_motion::Position;

    #[test]
    fn metadata_collision_preserves_existing_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.csv");
        let meta = path.with_extension("run.json");
        std::fs::write(&meta, "existing").unwrap();
        assert!(create_csv(&path, Source::Demo, &Plan::default()).is_err());
        assert!(!path.exists());
        assert_eq!(std::fs::read_to_string(meta).unwrap(), "existing");
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
