//! Compatibility boundary for unversioned user profiles.
//!
//! Revision 1 is the profile shape accepted by the original portable Rust
//! application.  It intentionally has no `schema_version` field: user JSON
//! may contain arbitrary metadata, which must never be interpreted as a
//! schema declaration.  Future profile revisions belong here and must keep a
//! migration from this accepted shape.

use serde_json::{Value, json};
use trkr_core::Workflow;

/// Current compatibility contract for unversioned JSON profiles.
#[cfg(test)]
pub const PROFILE_REVISION: u32 = 1;

/// Reject malformed structural containers before profile hydration.
///
/// This keeps the exact legacy paths and error messages while allowing all
/// unrecognised metadata to pass through unchanged.
pub(super) fn validate_profile_shape(raw: &Value) -> Result<(), String> {
    for path in [
        "/coordinates",
        "/coordinates/spatial",
        "/instruments",
        "/measurements",
        "/measurements/move_abs",
        "/measurements/move_abs/zero",
    ] {
        if raw.pointer(path).is_some_and(|value| !value.is_object()) {
            return Err(format!("Profile {path} must be an object"));
        }
    }
    for workflow in Workflow::ALL {
        for suffix in ["", "/scan", "/scan/ranges"] {
            let path = format!("/measurements/{}{suffix}", workflow.key());
            if raw.pointer(&path).is_some_and(|value| !value.is_object()) {
                return Err(format!("Profile {path} must be an object"));
            }
        }
    }
    Ok(())
}

/// Normalize recognised Python-era keys in memory without dropping arbitrary
/// profile metadata. This is revision-1 compatibility, not a persisted schema
/// marker.
pub(super) fn normalize_profile_aliases(raw: &mut Value) {
    normalize_measurement_alias(raw);
    let Some(instruments) = raw.get_mut("instruments").and_then(Value::as_object_mut) else {
        return;
    };
    if let Some(delays) = instruments
        .get_mut("delay_stage")
        .and_then(Value::as_object_mut)
    {
        for delay in delays.values_mut() {
            let Some(delay) = delay.as_object_mut() else {
                continue;
            };
            canonicalize_field(delay, "controller", &["delay_stage_controller"]);
            canonicalize_controller(delay, "controller");
        }
    }
    if let Some(lockins) = instruments.get_mut("lockin").and_then(Value::as_object_mut) {
        for lockin in lockins.values_mut() {
            if let Some(lockin) = lockin.as_object_mut() {
                canonicalize_field(lockin, "model", &["lockin_model"]);
            }
        }
    }
    if let Some(scanners) = instruments
        .get_mut("scanner")
        .and_then(Value::as_object_mut)
    {
        for scanner in scanners.values_mut() {
            let Some(scanner) = scanner.as_object_mut() else {
                continue;
            };
            canonicalize_field(scanner, "controller", &["scanner_controller"]);
            canonicalize_controller(scanner, "controller");
            canonicalize_scanner_scale(scanner);
            if let Some(hysteresis) = scanner
                .get_mut("software_hysteresis")
                .and_then(Value::as_object_mut)
            {
                canonicalize_field(
                    hysteresis,
                    "distance_um",
                    &["approach_distance_um", "pre_move_um"],
                );
                canonicalize_field(hysteresis, "direction", &["approach"]);
                // Superseded development-only option: all approaches share distance_um.
                hysteresis.remove("return_distance_um");
            }
        }
    }
}

pub(super) fn normalize_measurement_alias(raw: &mut Value) {
    if let Some(object) = raw.as_object_mut() {
        // Python normalize_config() accepted the singular root spelling.
        // Never silently discard its scan ranges or calibrated experiment zero.
        if !object.contains_key("measurements")
            && let Some(legacy) = object.get("measurement").cloned()
        {
            object.insert("measurements".into(), legacy);
        }
        object.remove("measurement");
    }
}

fn canonicalize_field(
    object: &mut serde_json::Map<String, Value>,
    canonical: &str,
    aliases: &[&str],
) {
    let canonical_value = object
        .get(canonical)
        .filter(|value| !value.is_null())
        .cloned();
    let alias_value = aliases
        .iter()
        .find_map(|alias| object.get(*alias).filter(|value| !value.is_null()).cloned());
    if let Some(value) = canonical_value.or(alias_value) {
        object.insert(canonical.into(), value);
    } else {
        object.remove(canonical);
    }
    for alias in aliases {
        object.remove(*alias);
    }
}

fn canonicalize_controller(object: &mut serde_json::Map<String, Value>, field: &str) {
    let Some(value) = object.get(field).and_then(Value::as_str) else {
        return;
    };
    let controller = canonical_controller(value);
    object.insert(field.into(), json!(controller));
}

fn canonicalize_scanner_scale(scanner: &mut serde_json::Map<String, Value>) {
    let canonical = scanner
        .get("sample_um_per_unit")
        .filter(|value| !value.is_null())
        .cloned();
    let raw_unit = scanner_control_unit(scanner);
    let aliases = match raw_unit.as_deref() {
        Some("deg") => ["sample_um_per_actuator_deg", "sample_um_per_actuator_mm"],
        _ => ["sample_um_per_actuator_mm", "sample_um_per_actuator_deg"],
    };
    let alias = aliases
        .iter()
        .find_map(|key| scanner.get(*key).filter(|value| !value.is_null()).cloned());
    if let Some(value) = canonical.or(alias) {
        scanner.insert("sample_um_per_unit".into(), value);
    } else {
        scanner.remove("sample_um_per_unit");
    }
    for alias in ["sample_um_per_actuator_mm", "sample_um_per_actuator_deg"] {
        scanner.remove(alias);
    }
}

fn scanner_control_unit(scanner: &serde_json::Map<String, Value>) -> Option<String> {
    if let Some(unit) = scanner.get("pos_unit").and_then(Value::as_str) {
        return Some(unit.trim().to_ascii_lowercase().replace('/', "_"));
    }
    let actuator = scanner
        .get("actuator")
        .and_then(Value::as_str)
        .map(canonical_controller);
    match actuator.as_deref() {
        Some("TRA12CC") => Some("mm".into()),
        Some("AGM100D") => Some("deg".into()),
        _ => match scanner
            .get("controller")
            .and_then(Value::as_str)
            .map(canonical_controller)
            .as_deref()
        {
            Some("CONEXCC") => Some("mm".into()),
            Some("CONEXAGAP") => Some("deg".into()),
            _ => None,
        },
    }
}

pub(super) fn canonical_controller(value: &str) -> String {
    value
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use serde_json::json;
    use trkr_core::Workflow;

    #[test]
    fn revision_one_keeps_unknown_metadata_outside_the_schema() {
        let mut raw = json!({
            "measurement": {"private_metadata": {"schema_version": 99}},
            "metadata": {"schema_version": "vendor-defined"}
        });
        normalize_profile_aliases(&mut raw);
        assert_eq!(PROFILE_REVISION, 1);
        assert_eq!(
            raw["measurements"]["private_metadata"]["schema_version"],
            99
        );
        assert_eq!(raw["metadata"]["schema_version"], "vendor-defined");
        assert!(raw.get("measurement").is_none());
    }

    #[test]
    fn structural_errors_keep_the_legacy_path_and_text() {
        let raw = json!({"measurements": {"trkr": {"scan": []}}});
        assert_eq!(
            validate_profile_shape(&raw).unwrap_err(),
            "Profile /measurements/trkr/scan must be an object"
        );
    }

    #[test]
    fn revision_one_profile_defaults_remain_hydratable() {
        let settings = Settings::default();
        let hardware = settings.hardware().unwrap();
        assert_eq!(hardware.model.name(), "SR7265");
        assert_eq!(hardware.address, 12);
        let plan = settings.plan(Workflow::SignalMonitor).unwrap();
        assert_eq!(plan.interval_s, 1.);
        assert_eq!(plan.n_points, 360);
    }
}
