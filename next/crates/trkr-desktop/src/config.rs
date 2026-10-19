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
        Self {
            raw: serde_json::from_str(include_str!("../assets/default.json"))
                .expect("embedded profile"),
        }
    }
}
impl Settings {
    pub fn load(path: &Path) -> Result<Self, String> {
        let raw: Value = serde_json::from_slice(
            &std::fs::read(path).map_err(|e| format!("Load {}: {e}", path.display()))?,
        )
        .map_err(|e| e.to_string())?;
        if !raw.is_object() {
            return Err("Configuration must be a JSON object".into());
        }
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
        let settings = Self { raw };
        settings.hardware()?;
        for workflow in Workflow::ALL {
            settings.plan(workflow)?;
        }
        Ok(settings)
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
        let instruments = &self.raw["instruments"];
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
        let coordinate = coordinate(v["coordinate"].as_str().unwrap_or("measurement"))?;
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
        .or_else(|| v.as_object().and_then(|m| m.values().next()))
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
fn coordinate(s: &str) -> Result<Coordinate, String> {
    match s.to_ascii_lowercase().as_str() {
        "measurement" => Ok(Coordinate::Measurement),
        "interface" | "control" => Ok(Coordinate::Interface),
        "instrument" | "device" => Ok(Coordinate::Instrument),
        _ => Err(format!("Unsupported coordinate: {s}")),
    }
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
}
