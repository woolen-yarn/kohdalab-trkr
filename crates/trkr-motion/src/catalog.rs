//! Embedded hardware presets used by configuration validation and the GUI.
//!
//! Experiment calibration deliberately stays in `DelayConfig`/`ScannerConfig`;
//! this module contains only manufacturer model limits and compatibility.

use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashSet},
    sync::OnceLock,
};

#[derive(Clone, Debug, PartialEq)]
pub struct DelayStagePreset {
    pub name: String,
    pub controllers: Vec<String>,
    pub travel_mm: f64,
    pub screw_lead_mm_per_rev: f64,
    pub step_angle_deg: f64,
}
impl DelayStagePreset {
    pub fn supports_controller(&self, controller: &str) -> bool {
        self.controllers
            .iter()
            .any(|item| canonical(item) == canonical(controller))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScannerActuatorPreset {
    pub name: String,
    pub controllers: Vec<String>,
    pub native_unit: String,
    pub minimum: f64,
    pub maximum: f64,
    pub resolution: f64,
}
impl ScannerActuatorPreset {
    pub fn supports_controller(&self, controller: &str) -> bool {
        self.controllers
            .iter()
            .any(|item| canonical(item) == canonical(controller))
    }
}

#[derive(Default)]
struct Catalog {
    stages: Vec<DelayStagePreset>,
    actuators: Vec<ScannerActuatorPreset>,
}

#[derive(Deserialize)]
struct RawDelayStage {
    controllers: Vec<String>,
    travel_mm: f64,
    screw_lead_mm_per_rev: f64,
    step_angle_deg: f64,
}
#[derive(Deserialize)]
struct RawActuator {
    controllers: Vec<String>,
    pos_unit: String,
    min_pos: f64,
    max_pos: f64,
    resolution: f64,
}

static CATALOG: OnceLock<Catalog> = OnceLock::new();

fn catalog() -> &'static Catalog {
    CATALOG.get_or_init(|| {
        parse_catalogs(
            include_str!("../assets/stages.toml"),
            include_str!("../assets/actuators.toml"),
        )
        .expect("embedded motion catalog must be valid")
    })
}

pub fn delay_stages() -> Vec<DelayStagePreset> {
    catalog().stages.clone()
}

pub fn delay_stage(name: &str) -> Option<DelayStagePreset> {
    catalog()
        .stages
        .iter()
        .find(|item| canonical(&item.name) == canonical(name))
        .cloned()
}

pub fn scanner_actuators() -> Vec<ScannerActuatorPreset> {
    catalog().actuators.clone()
}

/// Presets suitable for a controller, in catalog order for stable segmented UI.
pub fn scanner_actuators_for(controller: &str) -> Vec<ScannerActuatorPreset> {
    catalog()
        .actuators
        .iter()
        .filter(|item| item.supports_controller(controller))
        .cloned()
        .collect()
}

pub fn scanner_actuator(name: &str) -> Option<ScannerActuatorPreset> {
    catalog()
        .actuators
        .iter()
        .find(|item| canonical(&item.name) == canonical(name))
        .cloned()
}

fn canonical(value: &str) -> String {
    value.to_ascii_uppercase().replace(['-', '_', ' '], "")
}

fn parse_catalogs(stages: &str, actuators: &str) -> Result<Catalog, String> {
    let stages: BTreeMap<String, RawDelayStage> =
        toml::from_str(stages).map_err(|error| format!("invalid delay-stage TOML: {error}"))?;
    let actuators: BTreeMap<String, RawActuator> =
        toml::from_str(actuators).map_err(|error| format!("invalid actuator TOML: {error}"))?;

    let stages = stages
        .into_iter()
        .map(|(name, raw)| {
            validate_name(&name, "delay stage")?;
            validate_controllers(&raw.controllers, "delay stage", &KNOWN_DELAY_CONTROLLERS)?;
            positive(raw.travel_mm, "delay-stage travel_mm")?;
            positive(
                raw.screw_lead_mm_per_rev,
                "delay-stage screw_lead_mm_per_rev",
            )?;
            positive(raw.step_angle_deg, "delay-stage step_angle_deg")?;
            Ok(DelayStagePreset {
                name,
                controllers: raw.controllers,
                travel_mm: raw.travel_mm,
                screw_lead_mm_per_rev: raw.screw_lead_mm_per_rev,
                step_angle_deg: raw.step_angle_deg,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    validate_unique_names(&stages, |item| &item.name, "delay stage")?;

    let actuators = actuators
        .into_iter()
        .map(|(name, raw)| {
            validate_name(&name, "actuator")?;
            validate_controllers(&raw.controllers, "actuator", &KNOWN_SCANNER_CONTROLLERS)?;
            if raw.pos_unit.trim().is_empty() {
                return Err(format!("actuator {name} has an empty pos_unit"));
            }
            finite(raw.min_pos, "actuator min_pos")?;
            finite(raw.max_pos, "actuator max_pos")?;
            if raw.min_pos >= raw.max_pos {
                return Err(format!("actuator {name} has invalid native limits"));
            }
            positive(raw.resolution, "actuator resolution")?;
            Ok(ScannerActuatorPreset {
                name,
                controllers: raw.controllers,
                native_unit: raw.pos_unit,
                minimum: raw.min_pos,
                maximum: raw.max_pos,
                resolution: raw.resolution,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    validate_unique_names(&actuators, |item| &item.name, "actuator")?;

    Ok(Catalog { stages, actuators })
}

const KNOWN_DELAY_CONTROLLERS: [&str; 3] = ["SHOT302GS", "GSC01", "GSC01A"];
const KNOWN_SCANNER_CONTROLLERS: [&str; 2] = ["CONEXCC", "CONEXAGAP"];

fn validate_name(name: &str, kind: &str) -> Result<(), String> {
    if canonical(name).is_empty() {
        Err(format!("{kind} has an empty name"))
    } else {
        Ok(())
    }
}
fn validate_controllers(controllers: &[String], kind: &str, known: &[&str]) -> Result<(), String> {
    if controllers.is_empty() {
        return Err(format!("{kind} has no compatible controllers"));
    }
    let mut seen = HashSet::new();
    for controller in controllers {
        let name = canonical(controller);
        if !known.iter().any(|known| name == canonical(known)) {
            return Err(format!("{kind} has unknown controller {controller}"));
        }
        if !seen.insert(name) {
            return Err(format!("{kind} lists a controller more than once"));
        }
    }
    Ok(())
}
fn validate_unique_names<T>(
    values: &[T],
    name: impl Fn(&T) -> &String,
    kind: &str,
) -> Result<(), String> {
    let mut seen = HashSet::new();
    for item in values {
        if !seen.insert(canonical(name(item))) {
            return Err(format!("duplicate canonical {kind} name"));
        }
    }
    Ok(())
}
fn positive(value: f64, field: &str) -> Result<(), String> {
    if value.is_finite() && value > 0. {
        Ok(())
    } else {
        Err(format!("{field} must be finite and positive"))
    }
}
fn finite(value: f64, field: &str) -> Result<(), String> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(format!("{field} must be finite"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_filters_compatible_hardware() {
        assert_eq!(
            scanner_actuators_for("CONEXCC")
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            vec!["TRA12CC", "TRA25CC", "TRA6CC"]
        );
        assert_eq!(
            scanner_actuators_for("CONEXAGAP")
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            vec!["AG-M100D"]
        );
        assert!(
            delay_stage("SGSP46-500")
                .unwrap()
                .supports_controller("SHOT302GS")
        );
        assert!(
            !delay_stage("SGSP46-500")
                .unwrap()
                .supports_controller("GSC01")
        );
    }

    #[test]
    fn catalog_rejects_invalid_specs_and_unknown_controllers() {
        let valid_stage = r#"[A]
controllers = ["SHOT302GS"]
travel_mm = 1.0
screw_lead_mm_per_rev = 1.0
step_angle_deg = 1.0"#;
        let valid_actuator = r#"[A]
controllers = ["CONEXCC"]
pos_unit = "mm"
min_pos = 0.0
max_pos = 1.0
resolution = 0.1"#;
        assert!(parse_catalogs(valid_stage, valid_actuator).is_ok());
        assert!(
            parse_catalogs(
                valid_stage,
                valid_actuator.replace("CONEXCC", "UNKNOWN").as_str()
            )
            .is_err()
        );
        assert!(
            parse_catalogs(
                valid_stage
                    .replace("travel_mm = 1.0", "travel_mm = 0.0")
                    .as_str(),
                valid_actuator
            )
            .is_err()
        );
        assert!(
            parse_catalogs(
                valid_stage,
                valid_actuator
                    .replace("max_pos = 1.0", "max_pos = 0.0")
                    .as_str()
            )
            .is_err()
        );
        let duplicate_canonical_stage = format!(
            "{valid_stage}\n\n[A_]\ncontrollers = [\"SHOT302GS\"]\ntravel_mm = 1.0\nscrew_lead_mm_per_rev = 1.0\nstep_angle_deg = 1.0"
        );
        assert!(parse_catalogs(&duplicate_canonical_stage, valid_actuator).is_err());
        assert!(
            parse_catalogs(
                valid_stage,
                valid_actuator
                    .replace("[\"CONEXCC\"]", "[\"CONEXCC\", \"CONEX-CC\"]")
                    .as_str()
            )
            .is_err()
        );
    }
}
