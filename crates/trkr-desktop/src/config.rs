//! Reads and updates the existing Python JSON profiles without dropping user keys.
pub(crate) mod compatibility;

use crate::{engine::HardwareConfig, lockin::Model};
use compatibility::{
    normalize_measurement_alias, normalize_profile_aliases, validate_profile_shape,
};
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
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

pub const APPLICATION: &str = "kohdalab-trkr";

/// Per-user paths owned by TRKR.  The constructor accepts the `.kohdalab`
/// directory rather than reading the process environment so tests and callers
/// can remain isolated from an operator's home directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserConfigPaths {
    kohdalab_dir: PathBuf,
}

impl UserConfigPaths {
    pub fn from_kohdalab_dir(kohdalab_dir: impl Into<PathBuf>) -> Self {
        Self {
            kohdalab_dir: kohdalab_dir.into(),
        }
    }

    pub fn for_current_user() -> Self {
        let userprofile = std::env::var_os("USERPROFILE");
        let home = std::env::var_os("HOME");
        let home = home_from_environment(userprofile.as_deref(), home.as_deref(), cfg!(windows));
        Self::from_kohdalab_dir(home.join(".kohdalab"))
    }

    pub fn production(&self) -> PathBuf {
        self.kohdalab_dir.join("trkr/config.json")
    }

    pub fn demo(&self) -> PathBuf {
        self.kohdalab_dir.join("trkr/demo.json")
    }
}

fn home_from_environment(
    userprofile: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
    windows: bool,
) -> PathBuf {
    let selected = if windows {
        userprofile.or(home)
    } else {
        home.or(userprofile)
    };
    selected
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

#[derive(Debug)]
pub enum StartupSettings {
    Loaded { settings: Settings, path: PathBuf },
}

/// Load normal TRKR settings without connecting any hardware.
pub fn load_startup(paths: &UserConfigPaths) -> Result<StartupSettings, String> {
    let production = paths.production();
    if regular_file_exists(&production, "Configuration")? {
        return Settings::load(&production)
            .map(|settings| StartupSettings::Loaded {
                settings,
                path: production.clone(),
            })
            .map_err(|error| {
                format!(
                    "Configuration is invalid ({}): {error}",
                    production.display()
                )
            });
    }

    // Materialize the embedded safe profile on the first normal launch.  The
    // create-new link keeps this race-safe: another process may win, but an
    // existing operator file is never replaced by defaults.
    create_default_config(&production)?;
    let display = production.display().to_string();
    Settings::load(&production)
        .map(|settings| StartupSettings::Loaded {
            settings,
            path: production,
        })
        .map_err(|error| format!("Configuration is invalid ({display}): {error}"))
}

fn create_default_config(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|error| {
        format!(
            "Create configuration directory {}: {error}",
            parent.display()
        )
    })?;
    let mut raw = Settings::default().raw;
    raw["application"] = json!(APPLICATION);
    let data = serde_json::to_vec_pretty(&raw).map_err(|error| error.to_string())?;
    let temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| format!("Stage default configuration {}: {error}", path.display()))?;
    std::fs::write(temporary.path(), &data)
        .map_err(|error| format!("Stage default configuration {}: {error}", path.display()))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| format!("Stage default configuration {}: {error}", path.display()))?;
    match temporary.persist_noclobber(path) {
        Ok(_) => Ok(()),
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(format!("Create default configuration {}: {}", path.display(), error.error)),
    }
}

/// Load the optional, separate Demo profile. Demo profiles need only be JSON
/// objects because bundled replay presets deliberately omit hardware fields.
pub fn load_demo_startup(paths: &UserConfigPaths) -> Result<Option<PathBuf>, String> {
    let demo = paths.demo();
    if regular_file_exists(&demo, "Demo configuration")? {
        validate_demo_file(&demo)?;
        return Ok(Some(demo));
    }
    Ok(None)
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
        let raw: Value = serde_json::from_slice(
            &std::fs::read(path).map_err(|e| format!("Load {}: {e}", path.display()))?,
        )
        .map_err(|e| e.to_string())?;
        Self::from_value(raw)
    }
    pub fn from_value(mut raw: Value) -> Result<Self, String> {
        if !raw.is_object() {
            return Err("Configuration must be a JSON object".into());
        }
        validate_application(&raw)?;
        normalize_measurement_alias(&mut raw);
        validate_profile_shape(&raw)?;
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
        validate_application(&self.raw)?;
        self.hardware()?;
        if regular_file_exists(path, "Configuration")? {
            let existing = read_config_value(path, "Configuration").map_err(|e| {
                format!(
                    "Refusing to overwrite invalid configuration {}: {e}",
                    path.display()
                )
            })?;
            Self::from_value(existing).map_err(|e| {
                format!(
                    "Refusing to overwrite invalid configuration {}: {e}",
                    path.display()
                )
            })?;
        }
        let mut raw = self.raw.clone();
        raw["application"] = json!(APPLICATION);
        Self::from_value(raw.clone())?;
        persist_value(path, &raw)
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
            experiment: match self.raw.get("experiment") {
                None => Default::default(),
                Some(Value::Object(value)) => value.clone(),
                Some(_) => return Err("experiment must be an object".into()),
            },
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
        self.raw["experiment"] = json!(p.experiment);
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

fn regular_file_exists(path: &Path, label: &str) -> Result<bool, String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(true),
        Ok(_) => Err(format!("{label} is not a regular file: {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("Cannot read {label} {}: {error}", path.display())),
    }
}

fn read_config_value(path: &Path, label: &str) -> Result<Value, String> {
    serde_json::from_slice(
        &std::fs::read(path)
            .map_err(|error| format!("Read {label} {}: {error}", path.display()))?,
    )
    .map_err(|error| format!("Invalid {label} {}: {error}", path.display()))
}

fn validate_demo_value(raw: &Value) -> Result<(), String> {
    if !raw.is_object() {
        return Err("Demo configuration must be a JSON object".into());
    }
    validate_application(raw)
}

fn validate_demo_file(path: &Path) -> Result<(), String> {
    let raw = read_config_value(path, "Demo configuration")?;
    validate_demo_value(&raw)
}

fn stage_value(path: &Path, raw: &Value) -> Result<tempfile::NamedTempFile, String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let parent = parent.unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|error| {
        format!(
            "Create configuration directory {}: {error}",
            parent.display()
        )
    })?;
    let data = serde_json::to_vec_pretty(raw).map_err(|error| error.to_string())?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| format!("Stage configuration {}: {error}", path.display()))?;
    temporary
        .write_all(&data)
        .map_err(|error| format!("Stage configuration {}: {error}", path.display()))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| format!("Stage configuration {}: {error}", path.display()))?;
    Ok(temporary)
}

fn persist_value(path: &Path, raw: &Value) -> Result<(), String> {
    stage_value(path, raw)?
        .persist(path)
        .map_err(|error| format!("Save {}: {}", path.display(), error.error))?;
    Ok(())
}

fn validate_application(raw: &Value) -> Result<(), String> {
    if let Some(marker) = raw.get("application") {
        let marker = marker
            .as_str()
            .ok_or("Configuration application marker must be a string")?;
        if marker != APPLICATION {
            return Err(format!(
                "Configuration belongs to {marker}, not {APPLICATION}"
            ));
        }
    } else if looks_like_iv_profile(raw) {
        return Err(
            "Configuration appears to be a KohdaLab IV profile, not a KohdaLab TRKR profile".into(),
        );
    }
    Ok(())
}

fn looks_like_iv_profile(raw: &Value) -> bool {
    let has_source = raw.get("source").is_some()
        || raw.get("source_model").is_some()
        || raw.get("source_resource").is_some();
    let has_meter = raw.get("meter").is_some()
        || raw.get("meter_model").is_some()
        || raw.get("meter_resource").is_some();
    let paired_instruments = raw
        .get("instruments")
        .and_then(Value::as_object)
        .is_some_and(|instruments| {
            instruments.contains_key("source") && instruments.contains_key("meter")
        });
    (has_source && has_meter) || paired_instruments
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
#[path = "config/tests.rs"]
mod tests;
