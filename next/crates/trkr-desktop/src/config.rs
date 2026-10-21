//! Reads and updates the existing Python JSON profiles without dropping user keys.
use crate::{engine::HardwareConfig, lockin::Model};
use serde_json::{Value, json};
use std::path::Path;
use trkr_core::{Axis, Coordinate, Plan, Range, Workflow};
use trkr_motion::MotionConfig;

/// Bundle data belongs in Contents/Resources on macOS; portable data is beside
/// the executable. Keeping it out of MacOS also preserves the app code seal.
pub fn resources_at(executable_directory: &Path) -> std::path::PathBuf {
    let resources = executable_directory.join("../Resources");
    if executable_directory
        .file_name()
        .is_some_and(|n| n == "MacOS")
        && resources.is_dir()
    {
        resources
    } else {
        executable_directory.to_path_buf()
    }
}
pub fn application_resources() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(resources_at))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}
#[derive(Clone, Debug)]
pub struct Settings {
    pub raw: Value,
}
impl Default for Settings {
    fn default() -> Self {
        let mut settings = Self {
            raw: serde_json::from_str(include_str!("../assets/default.json"))
                .expect("embedded profile"),
        };
        settings.normalize_profile_aliases();
        settings
    }
}
impl Settings {
    pub fn load(path: &Path) -> Result<Self, String> {
        let mut raw: Value = serde_json::from_slice(
            &std::fs::read(path).map_err(|e| format!("Load {}: {e}", path.display()))?,
        )
        .map_err(|e| e.to_string())?;
        if !raw.is_object() {
            return Err("Configuration must be a JSON object".into());
        }
        normalize_measurement_alias(&mut raw);
        for path in [
            "/coordinates",
            "/coordinates/spatial",
            "/instruments",
            "/measurements",
            "/measurements/move_abs",
            "/measurements/move_abs/zero",
        ] {
            if raw.pointer(path).is_some_and(|v| !v.is_object()) {
                return Err(format!("Profile {path} must be an object"));
            }
        }
        for workflow in Workflow::ALL {
            for suffix in ["", "/scan", "/scan/ranges"] {
                let path = format!("/measurements/{}{suffix}", workflow.key());
                if raw.pointer(&path).is_some_and(|v| !v.is_object()) {
                    return Err(format!("Profile {path} must be an object"));
                }
            }
        }
        normalize_profile_aliases(&mut raw);
        let settings = Self { raw };
        settings.hardware()?;
        for workflow in Workflow::ALL {
            settings.plan(workflow)?;
        }
        Ok(settings)
    }
    /// Normalize recognized Python-era hardware aliases in memory before UI
    /// hydration or a later Save. Unknown profile metadata is untouched.
    pub fn normalize_profile_aliases(&mut self) {
        normalize_profile_aliases(&mut self.raw);
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.hardware()?;
        let data = serde_json::to_vec_pretty(&self.raw).map_err(|e| e.to_string())?;
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, data).map_err(|e| format!("Save {}: {e}", path.display()))
    }
    pub fn hardware(&self) -> Result<HardwareConfig, String> {
        validate_measurement_device_selections(&self.raw)?;
        let instruments = &self.raw["instruments"];
        for (group, keys) in [("delay_stage", &["t"][..]), ("scanner", &["x", "y"][..])] {
            if let Some(devices) = instruments[group].as_object()
                && !devices.is_empty()
                && !keys.iter().any(|key| devices.contains_key(*key))
            {
                return Err(format!(
                    "Portable GUI requires canonical instruments.{group} keys {}; named-device selection is not supported",
                    keys.join("/")
                ));
            }
        }
        let lockin = device(&instruments["lockin"], "main").ok_or("No main lock-in configured")?;
        let name = lockin
            .get("model")
            .or_else(|| lockin.get("lockin_model"))
            .and_then(Value::as_str)
            .unwrap_or("SR7265");
        let model = Model::ALL
            .into_iter()
            .find(|m| m.name().eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("Unsupported lock-in: {name}"))?;
        let resource = lockin["resource"].as_str().unwrap_or("GPIB0::12::INSTR");
        let address = parse_gpib(resource)?;
        let delay = device(&instruments["delay_stage"], "t")
            .map(|v| {
                serde_json::from_value(v.clone())
                    .map_err(|e| format!("Delay-stage configuration: {e}"))
            })
            .transpose()?;
        let x = device(&instruments["scanner"], "x")
            .map(|v| {
                serde_json::from_value(v.clone())
                    .map_err(|e| format!("X scanner configuration: {e}"))
            })
            .transpose()?;
        let y = instruments["scanner"]
            .get("y")
            .map(|v| {
                serde_json::from_value(v.clone())
                    .map_err(|e| format!("Y scanner configuration: {e}"))
            })
            .transpose()?;
        let motion = MotionConfig { delay, x, y };
        motion.validate()?;
        Ok(HardwareConfig {
            model,
            address,
            motion,
        })
    }
    pub fn plan(&self, workflow: Workflow) -> Result<Plan, String> {
        let v = &self.raw["measurements"][workflow.key()];
        let z = &self.raw["measurements"]["move_abs"]["zero"];
        let zero = [
            number(z, "t_ps", 0.)?,
            number(z, "x_um", 0.)?,
            number(z, "y_um", 0.)?,
        ];
        let scan = &v["scan"];
        let fast = match workflow {
            Workflow::SignalMonitor | Workflow::Trkr => Axis::T,
            Workflow::Srkr => axis(scan["axis"].as_str().unwrap_or("x"))?,
            Workflow::Strkr => axis(scan["fast_axis"].as_str().unwrap_or("t"))?,
            Workflow::Srkr2d => axis(scan["fast_axis"].as_str().unwrap_or("x"))?,
        };
        let slow = axis(
            scan["slow_axis"]
                .as_str()
                .unwrap_or(if workflow == Workflow::Srkr2d {
                    "y"
                } else {
                    "x"
                }),
        )?;
        let fast_range = if workflow.is_2d() {
            range(&scan["ranges"][fast.name()], fast)?
        } else {
            range(scan, fast)?
        };
        let slow_range = range(&scan["ranges"][slow.name()], slow)?;
        let interval_s = number(
            v,
            if workflow == Workflow::SignalMonitor {
                "interval_s"
            } else {
                "wait_s"
            },
            if workflow == Workflow::SignalMonitor {
                1.
            } else {
                2.
            },
        )?;
        let n_points = v
            .get("n_points")
            .map(|x| x.as_u64().ok_or("n_points must be a positive integer"))
            .transpose()?
            .unwrap_or(360) as usize;
        let coordinate = coordinate(
            v["coordinate"].as_str().unwrap_or("measurement"),
            workflow,
            fast,
            &self.raw,
        )?;
        let returns = match v.get("return_to_zero") {
            Some(Value::Bool(b)) => [*b, *b],
            Some(Value::Object(m)) => [
                m.get("fast_axis").and_then(Value::as_bool).unwrap_or(true),
                m.get("slow_axis").and_then(Value::as_bool).unwrap_or(true),
            ],
            None => [true; 2],
            _ => return Err("return_to_zero must be a boolean or per-axis object".into()),
        };
        let theta_deg = number(&self.raw["coordinates"]["spatial"], "theta_deg", 0.)?;
        let plan = Plan {
            workflow,
            fast,
            slow,
            fast_range,
            slow_range,
            interval_s,
            n_points,
            theta_deg,
            zero,
            coordinate,
            return_to_zero: returns,
            return_fast_between_lines: match v.get("return_fast_between_lines") {
                None => true,
                Some(value) => value
                    .as_bool()
                    .ok_or("return_fast_between_lines must be boolean")?,
            },
        };
        plan.targets()?;
        Ok(plan)
    }
    pub fn set_plan(&mut self, p: &Plan) -> Result<(), String> {
        p.targets()?;
        self.raw["coordinates"]["spatial"]["theta_deg"] = json!(p.theta_deg);
        for (i, key) in ["t_ps", "x_um", "y_um"].into_iter().enumerate() {
            self.raw["measurements"]["move_abs"]["zero"][key] = json!(p.zero[i]);
        }
        let v = &mut self.raw["measurements"][p.workflow.key()];
        if !v.is_object() {
            *v = json!({});
        }
        v["coordinate"] = serde_json::to_value(p.coordinate).map_err(|e| e.to_string())?;
        v["return_fast_between_lines"] = json!(p.return_fast_between_lines);
        if p.workflow == Workflow::SignalMonitor {
            v["interval_s"] = json!(p.interval_s);
            v["n_points"] = json!(p.n_points);
        } else {
            v["wait_s"] = json!(p.interval_s);
            v["return_to_zero"] = if p.workflow.is_2d() {
                json!({"fast_axis":p.return_to_zero[0],"slow_axis":p.return_to_zero[1]})
            } else {
                json!(p.return_to_zero[0])
            };
            if p.workflow.is_2d() {
                v["scan"]["fast_axis"] = json!(p.fast.name());
                v["scan"]["slow_axis"] = json!(p.slow.name());
                v["scan"]["ranges"][p.fast.name()] = json!(p.fast_range);
                v["scan"]["ranges"][p.slow.name()] = json!(p.slow_range);
            } else {
                for (key, value) in [
                    ("min", p.fast_range.min),
                    ("max", p.fast_range.max),
                    ("step", p.fast_range.step),
                ] {
                    v["scan"][key] = json!(value);
                }
                if p.workflow == Workflow::Srkr {
                    v["scan"]["axis"] = json!(p.fast.name());
                }
            }
        }
        Ok(())
    }
}
fn device<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.get(key)
}
fn number(v: &Value, key: &str, default: f64) -> Result<f64, String> {
    let n = match v.get(key) {
        None => default,
        Some(x) => x
            .as_f64()
            .ok_or_else(|| format!("{key} must be a number"))?,
    };
    if n.is_finite() {
        Ok(n)
    } else {
        Err(format!("{key} must be finite"))
    }
}
fn axis(s: &str) -> Result<Axis, String> {
    match s.to_ascii_lowercase().as_str() {
        "t" => Ok(Axis::T),
        "x" => Ok(Axis::X),
        "y" => Ok(Axis::Y),
        "u" => Ok(Axis::U),
        "v" => Ok(Axis::V),
        _ => Err(format!("Unsupported axis: {s}")),
    }
}
fn coordinate(s: &str, workflow: Workflow, fast: Axis, raw: &Value) -> Result<Coordinate, String> {
    let coordinate = s.trim().to_ascii_lowercase();
    match workflow {
        Workflow::Trkr => delay_coordinate(&coordinate),
        Workflow::Srkr if !matches!(fast, Axis::U | Axis::V) => {
            let raw_unit = if matches!(coordinate.as_str(), "mm" | "pos_mm" | "deg" | "pos_deg") {
                Some(scanner_raw_unit(raw, fast)?)
            } else {
                None
            };
            scanner_coordinate(&coordinate, raw_unit)
        }
        // STRKR/SRKR 2D and rotated scans retain the core measurement-only
        // validation.  Do not turn legacy raw-unit names into a bypass.
        _ => generic_coordinate(&coordinate),
    }
}
fn generic_coordinate(s: &str) -> Result<Coordinate, String> {
    match s {
        "measurement" => Ok(Coordinate::Measurement),
        "interface" | "control" => Ok(Coordinate::Interface),
        "instrument" | "device" => Ok(Coordinate::Instrument),
        _ => Err(format!("Unsupported coordinate: {s}")),
    }
}
fn delay_coordinate(s: &str) -> Result<Coordinate, String> {
    match s {
        "measurement" | "ps" | "t_ps" => Ok(Coordinate::Measurement),
        "interface" | "control" | "mm" | "pos_mm" => Ok(Coordinate::Interface),
        "instrument" | "device" | "pulse" => Ok(Coordinate::Instrument),
        _ => Err(format!("Unsupported TRKR coordinate: {s}")),
    }
}
fn scanner_coordinate(s: &str, raw_unit: Option<&str>) -> Result<Coordinate, String> {
    match s {
        "measurement" | "um" | "sample_um" => Ok(Coordinate::Measurement),
        // Python's scanner compatibility layer deliberately treats these as
        // the controller interface coordinate because scanners have no
        // separate pulse/instrument coordinate.
        "interface" | "control" | "instrument" | "device" => Ok(Coordinate::Interface),
        "mm" | "pos_mm" | "deg" | "pos_deg" => {
            let alias_unit = if matches!(s, "mm" | "pos_mm") {
                "mm"
            } else {
                "deg"
            };
            let raw_unit = raw_unit.expect("raw scanner aliases request a control unit");
            if alias_unit == raw_unit {
                Ok(Coordinate::Interface)
            } else {
                Err(format!(
                    "SRKR coordinate {s} does not match this scanner's {raw_unit} control unit"
                ))
            }
        }
        _ => Err(format!("Unsupported SRKR coordinate: {s}")),
    }
}
fn normalize_measurement_alias(raw: &mut Value) {
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
fn normalize_profile_aliases(raw: &mut Value) {
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
    let controller = value
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_uppercase();
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
fn canonical_controller(value: &str) -> String {
    value
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_uppercase()
}
fn scanner_raw_unit(raw: &Value, axis: Axis) -> Result<&'static str, String> {
    let key = match axis {
        Axis::X => "x",
        Axis::Y => "y",
        _ => return Err("SRKR raw scanner coordinates require X or Y".into()),
    };
    let scanner = raw
        .pointer(&format!("/instruments/scanner/{key}"))
        .ok_or_else(|| format!("Missing instruments.scanner.{key}"))?;
    let config: trkr_motion::ScannerConfig = serde_json::from_value(scanner.clone())
        .map_err(|error| format!("{key} scanner configuration: {error}"))?;
    match config
        .normalized()?
        .pos_unit
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "mm" => Ok("mm"),
        "deg" => Ok("deg"),
        unit => Err(format!("Unsupported {key} scanner control unit: {unit}")),
    }
}
fn validate_measurement_device_selections(raw: &Value) -> Result<(), String> {
    for (workflow, key) in Workflow::ALL
        .into_iter()
        .map(|w| (w.name(), w.key()))
        .chain([("Manual Move", "move_abs")])
    {
        let settings = &raw["measurements"][key];
        validate_default_selection(
            raw,
            workflow,
            settings,
            "lockin_key",
            "lockin",
            "lockin",
            "main",
        )?;
        validate_default_selection(
            raw,
            workflow,
            settings,
            "delay_stage_key",
            "delay_stage",
            "delay_stage",
            "t",
        )?;
        if let Some(scanners) = settings
            .get("scanner_keys")
            .or_else(|| settings.get("scanners"))
            .filter(|value| !value.is_null())
        {
            let scanners = scanners
                .as_object()
                .ok_or_else(|| format!("{} scanners must be an object keyed by x/y", workflow))?;
            for (axis, selected) in scanners {
                if selected.is_null() {
                    continue;
                }
                if !matches!(axis.as_str(), "x" | "y") {
                    return Err(format!(
                        "{} scanner selection has unsupported axis {axis}",
                        workflow
                    ));
                }
                let selected = selected.as_str().ok_or_else(|| {
                    format!("{} scanner selection for {axis} must be a string", workflow)
                })?;
                if selected != axis {
                    return Err(format!(
                        "{} selects scanner {selected:?} for {axis}; portable GUI supports only instruments.scanner.{axis}",
                        workflow
                    ));
                }
                if raw
                    .pointer(&format!("/instruments/scanner/{axis}"))
                    .is_none()
                {
                    return Err(format!(
                        "{} selects missing instruments.scanner.{axis}",
                        workflow
                    ));
                }
            }
        }
    }
    Ok(())
}
fn validate_default_selection(
    raw: &Value,
    workflow: &str,
    settings: &Value,
    primary: &str,
    alias: &str,
    kind: &str,
    default: &str,
) -> Result<(), String> {
    let Some(selected) = settings
        .get(primary)
        .or_else(|| settings.get(alias))
        .filter(|value| !value.is_null())
    else {
        return Ok(());
    };
    let selected = selected
        .as_str()
        .ok_or_else(|| format!("{} {primary} must be a string", workflow))?;
    if selected != default {
        return Err(format!(
            "{} selects {kind} {selected:?}; portable GUI supports only instruments.{kind}.{default}",
            workflow
        ));
    }
    if raw
        .pointer(&format!("/instruments/{kind}/{default}"))
        .is_none()
    {
        return Err(format!(
            "{} selects missing instruments.{kind}.{default}",
            workflow
        ));
    }
    Ok(())
}
fn range(v: &Value, axis: Axis) -> Result<Range, String> {
    let d = if axis == Axis::T {
        [-50., 300., 5.]
    } else {
        [-30., 30., 1.]
    };
    Ok(Range {
        min: number(v, "min", d[0])?,
        max: number(v, "max", d[1])?,
        step: number(v, "step", d[2])?,
    })
}
fn parse_gpib(resource: &str) -> Result<u8, String> {
    let s = resource.to_ascii_uppercase();
    let fields: Vec<_> = s.split("::").collect();
    if fields.len() != 3 || fields[0] != "GPIB0" || fields[2] != "INSTR" {
        return Err("Portable lock-in resource must be GPIB0::<1–30>::INSTR".into());
    }
    let address = fields[1]
        .parse::<u8>()
        .map_err(|_| "Invalid GPIB primary address")?;
    if !(1..=30).contains(&address) {
        return Err("GPIB primary address must be 1–30".into());
    }
    Ok(address)
}

#[cfg(test)]
mod tests {
    #[test]
    fn noncanonical_only_devices_are_never_selected_by_map_order() {
        for group in ["lockin", "delay_stage", "scanner"] {
            let mut settings = Settings::default();
            let key = match group {
                "lockin" => "main",
                "delay_stage" => "t",
                _ => "x",
            };
            let device = settings.raw["instruments"][group][key].clone();
            settings.raw["instruments"][group] = json!({"other":device});
            assert!(
                settings.hardware().is_err(),
                "silently selected {group}.other"
            );
        }
        let mut settings = Settings::default();
        settings.raw["instruments"]["scanner"]
            .as_object_mut()
            .unwrap()
            .remove("x");
        let config = settings.hardware().unwrap();
        assert!(config.motion.x.is_none());
        assert!(config.motion.y.is_some());
    }
    #[test]
    fn null_legacy_device_selectors_mean_unspecified_without_falling_back_to_alias() {
        let mut settings = Settings::default();
        settings.raw["measurements"]["srkr"]["lockin_key"] = Value::Null;
        settings.raw["measurements"]["srkr"]["lockin"] = json!("ignored-alias");
        settings.raw["measurements"]["srkr"]["scanner_keys"] = Value::Null;
        settings.raw["measurements"]["srkr"]["scanners"] = json!({"x":"ignored-alias"});
        assert!(settings.hardware().is_ok());
        settings.raw["measurements"]["srkr"]["scanner_keys"] = json!({"x":null,"y":null});
        assert!(settings.hardware().is_ok());
    }
    #[test]
    fn app_bundle_uses_resources_and_portable_uses_executable_directory() {
        let root = tempfile::tempdir().unwrap();
        let macos = root.path().join("Contents/MacOS");
        std::fs::create_dir_all(root.path().join("Contents/Resources")).unwrap();
        std::fs::create_dir_all(&macos).unwrap();
        assert!(super::resources_at(&macos).ends_with("MacOS/../Resources"));
        let portable = root.path().join("portable");
        assert_eq!(super::resources_at(&portable), portable);
    }

    use super::*;
    #[test]
    fn profile_plan_roundtrip_preserves_custom_keys() {
        let mut s = Settings::default();
        s.raw["custom"] = json!({"keep":true});
        for workflow in Workflow::ALL {
            let p = s.plan(workflow).unwrap();
            s.set_plan(&p).unwrap();
            assert_eq!(s.plan(workflow).unwrap(), p);
        }
        assert_eq!(s.raw["custom"]["keep"], true);
    }
    #[test]
    fn return_reset_choice_survives_profile_roundtrip() {
        let mut s = Settings::default();
        let mut p = s.plan(Workflow::Strkr).unwrap();
        p.return_fast_between_lines = false;
        s.set_plan(&p).unwrap();
        assert_eq!(s.plan(Workflow::Strkr).unwrap(), p);
    }
    #[test]
    fn rejects_direct_usb_and_nonintegral_count() {
        assert!(parse_gpib("USB0::12::INSTR").is_err());
        assert!(parse_gpib("GPIB0::0::INSTR").is_err());
        let mut s = Settings::default();
        s.raw["measurements"]["signal_monitor"]["n_points"] = json!(1.5);
        assert!(s.plan(Workflow::SignalMonitor).is_err());
    }
    #[test]
    fn legacy_trkr_coordinate_aliases_preserve_their_units() {
        let mut settings = Settings::default();
        for (alias, expected) in [
            ("ps", Coordinate::Measurement),
            ("t_ps", Coordinate::Measurement),
            ("mm", Coordinate::Interface),
            ("pos_mm", Coordinate::Interface),
            ("pulse", Coordinate::Instrument),
            ("control", Coordinate::Interface),
            ("device", Coordinate::Instrument),
        ] {
            settings.raw["measurements"]["trkr"]["coordinate"] = json!(alias);
            assert_eq!(
                settings.plan(Workflow::Trkr).unwrap().coordinate,
                expected,
                "{alias}"
            );
        }
    }
    #[test]
    fn legacy_srkr_coordinate_aliases_follow_the_actual_scanner_unit() {
        let mut settings = Settings::default();
        settings.raw["instruments"]["scanner"]["x"]["controller"] = json!("CONEXCC");
        settings.raw["instruments"]["scanner"]["x"]["actuator"] = json!("TRA12CC");
        settings.raw["instruments"]["scanner"]["x"]["axis"] = json!(1);
        for (alias, expected) in [
            ("um", Coordinate::Measurement),
            ("sample_um", Coordinate::Measurement),
            ("mm", Coordinate::Interface),
            ("pos_mm", Coordinate::Interface),
            ("instrument", Coordinate::Interface),
            ("device", Coordinate::Interface),
        ] {
            settings.raw["measurements"]["srkr"]["coordinate"] = json!(alias);
            assert_eq!(
                settings.plan(Workflow::Srkr).unwrap().coordinate,
                expected,
                "{alias}"
            );
        }
        settings.raw["measurements"]["srkr"]["coordinate"] = json!("deg");
        assert!(
            settings
                .plan(Workflow::Srkr)
                .unwrap_err()
                .contains("does not match")
        );

        settings.raw["instruments"]["scanner"]["x"]["controller"] = json!("CONEXAGAP");
        settings.raw["instruments"]["scanner"]["x"]["actuator"] = json!("AG-M100D");
        settings.raw["instruments"]["scanner"]["x"]["axis"] = json!("U");
        settings.raw["measurements"]["srkr"]["coordinate"] = json!("pos_deg");
        assert_eq!(
            settings.plan(Workflow::Srkr).unwrap().coordinate,
            Coordinate::Interface
        );
        settings.raw["measurements"]["srkr"]["coordinate"] = json!("pos_mm");
        assert!(
            settings
                .plan(Workflow::Srkr)
                .unwrap_err()
                .contains("does not match")
        );
    }
    #[test]
    fn non_measurement_2d_and_rotated_aliases_remain_rejected() {
        let mut settings = Settings::default();
        settings.raw["measurements"]["strkr"]["coordinate"] = json!("device");
        assert!(
            settings
                .plan(Workflow::Strkr)
                .unwrap_err()
                .contains("measurement coordinates")
        );
        settings.raw["measurements"]["srkr"]["scan"]["axis"] = json!("u");
        settings.raw["measurements"]["srkr"]["coordinate"] = json!("control");
        assert!(
            settings
                .plan(Workflow::Srkr)
                .unwrap_err()
                .contains("measurement coordinates")
        );
    }
    #[test]
    fn explicit_legacy_device_selectors_require_the_portable_defaults() {
        let mut settings = Settings::default();
        settings.raw["measurements"]["trkr"]["lockin"] = json!("main");
        settings.raw["measurements"]["trkr"]["delay_stage_key"] = json!("t");
        settings.raw["measurements"]["srkr"]["scanners"] = json!({"x": "x", "y": "y"});
        assert!(settings.hardware().is_ok());

        settings.raw["measurements"]["trkr"]["lockin_key"] = json!("other");
        assert!(
            settings
                .hardware()
                .unwrap_err()
                .contains("supports only instruments.lockin.main")
        );
        settings.raw["measurements"]["trkr"]["lockin_key"] = json!("main");
        settings.raw["measurements"]["srkr"]["scanner_keys"] = json!({"x": "other"});
        assert!(
            settings
                .hardware()
                .unwrap_err()
                .contains("supports only instruments.scanner.x")
        );
    }
    #[test]
    fn profile_alias_normalization_uses_the_actual_scanner_unit_and_canonical_wins() {
        let mut settings = Settings::default();
        let scanner = settings.raw["instruments"]["scanner"]["x"]
            .as_object_mut()
            .unwrap();
        scanner.insert("controller".into(), json!("CONEXAGAP"));
        scanner.insert("scanner_controller".into(), json!("CONEX-CC"));
        scanner.insert("actuator".into(), json!("AG-M100D"));
        scanner.insert("sample_um_per_unit".into(), json!(17.));
        scanner.insert("sample_um_per_actuator_mm".into(), json!(400.));
        scanner.insert("sample_um_per_actuator_deg".into(), json!(500.));
        scanner.insert(
            "software_hysteresis".into(),
            json!({"distance_um": 7., "pre_move_um": 9., "direction": "negative", "approach": "positive"}),
        );
        scanner.insert("unknown_metadata".into(), json!({"keep": true}));

        settings.normalize_profile_aliases();
        let scanner = &settings.raw["instruments"]["scanner"]["x"];
        assert_eq!(scanner["controller"], "CONEXAGAP");
        assert_eq!(scanner["sample_um_per_unit"], 17.);
        assert_eq!(scanner["software_hysteresis"]["distance_um"], 7.);
        assert_eq!(scanner["software_hysteresis"]["direction"], "negative");
        assert_eq!(scanner["unknown_metadata"]["keep"], true);
        for alias in [
            "scanner_controller",
            "sample_um_per_actuator_mm",
            "sample_um_per_actuator_deg",
        ] {
            assert!(scanner.get(alias).is_none(), "{alias}");
        }
        assert!(scanner["software_hysteresis"].get("pre_move_um").is_none());
        assert!(scanner["software_hysteresis"].get("approach").is_none());

        let mut legacy = Settings::default();
        let scanner = legacy.raw["instruments"]["scanner"]["x"]
            .as_object_mut()
            .unwrap();
        scanner.remove("sample_um_per_unit");
        scanner.insert("controller".into(), json!("CONEX-CC"));
        scanner.insert("actuator".into(), json!("TRA12CC"));
        scanner.insert("sample_um_per_actuator_mm".into(), json!(321.));
        scanner.insert("sample_um_per_actuator_deg".into(), json!(999.));
        scanner.insert(
            "software_hysteresis".into(),
            json!({"approach_distance_um": 12., "pre_move_um": 99.}),
        );
        legacy.normalize_profile_aliases();
        assert_eq!(
            legacy.raw["instruments"]["scanner"]["x"]["controller"],
            "CONEXCC"
        );
        assert_eq!(
            legacy.raw["instruments"]["scanner"]["x"]["sample_um_per_unit"],
            321.
        );
        assert_eq!(
            legacy.raw["instruments"]["scanner"]["x"]["software_hysteresis"]["distance_um"],
            12.
        );
    }
    #[test]
    fn lockin_model_alias_normalizes_without_overriding_a_canonical_model() {
        let mut settings = Settings::default();
        let lockin = settings.raw["instruments"]["lockin"]["main"]
            .as_object_mut()
            .unwrap();
        lockin.remove("model");
        lockin.insert("lockin_model".into(), json!("sr830"));
        settings.normalize_profile_aliases();
        assert_eq!(
            settings.raw["instruments"]["lockin"]["main"]["model"],
            "sr830"
        );
        assert!(
            settings.raw["instruments"]["lockin"]["main"]
                .get("lockin_model")
                .is_none()
        );

        settings.raw["instruments"]["lockin"]["main"] =
            json!({"model":"SR7265", "lockin_model":"SR830"});
        settings.normalize_profile_aliases();
        assert_eq!(
            settings.raw["instruments"]["lockin"]["main"]["model"],
            "SR7265"
        );
    }
    #[test]
    fn manual_device_selectors_cannot_silently_move_a_different_device() {
        for (field, selection) in [
            ("delay_stage_key", json!("other")),
            ("delay_stage", json!("other")),
            ("scanner_keys", json!({"x":"other"})),
            ("scanners", json!({"y":"other"})),
        ] {
            let mut settings = Settings::default();
            settings.raw["measurements"]["move_abs"][field] = selection;
            let error = settings.hardware().unwrap_err();
            assert!(error.contains("Manual Move"), "{field}: {error}");
        }
        let mut settings = Settings::default();
        settings.raw["measurements"]["move_abs"]["delay_stage_key"] = json!("t");
        settings.raw["measurements"]["move_abs"]["scanner_keys"] = json!({"x":"x","y":"y"});
        assert!(settings.hardware().is_ok());
    }
    #[test]
    fn singular_measurement_root_preserves_ranges_zero_and_metadata_on_load() {
        let mut raw = Settings::default().raw;
        let measurements = raw.as_object_mut().unwrap().remove("measurements").unwrap();
        raw["measurement"] = measurements;
        raw["measurement"]["trkr"]["scan"] = json!({"min":17.,"max":23.,"step":3.});
        raw["measurement"]["move_abs"]["zero"] = json!({"t_ps":123.,"x_um":45.,"y_um":67.});
        raw["measurement"]["private_metadata"] = json!({"keep":true});
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), serde_json::to_vec(&raw).unwrap()).unwrap();
        let loaded = Settings::load(file.path()).unwrap();
        let plan = loaded.plan(Workflow::Trkr).unwrap();
        assert_eq!(
            plan.fast_range,
            Range {
                min: 17.,
                max: 23.,
                step: 3.
            }
        );
        assert_eq!(plan.zero, [123., 45., 67.]);
        assert_eq!(loaded.raw["measurements"]["private_metadata"]["keep"], true);
        assert!(loaded.raw.get("measurement").is_none());

        raw["measurements"] = Settings::default().raw["measurements"].clone();
        std::fs::write(file.path(), serde_json::to_vec(&raw).unwrap()).unwrap();
        assert_eq!(
            Settings::load(file.path())
                .unwrap()
                .plan(Workflow::Trkr)
                .unwrap()
                .zero,
            Settings::default().plan(Workflow::Trkr).unwrap().zero
        );
        raw.as_object_mut().unwrap().remove("measurements");
        raw["measurement"] = json!("invalid");
        std::fs::write(file.path(), serde_json::to_vec(&raw).unwrap()).unwrap();
        assert!(Settings::load(file.path()).is_err());
    }
    #[test]
    fn measurement_coordinate_profiles_do_not_require_scanners() {
        let mut settings = Settings::default();
        settings.raw["instruments"]["scanner"] = json!({});
        assert!(settings.hardware().is_ok());
        for workflow in Workflow::ALL {
            assert!(settings.plan(workflow).is_ok(), "{}", workflow.name());
        }
    }
}
