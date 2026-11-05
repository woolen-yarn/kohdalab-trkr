//! Stable, selectable device inventory for the desktop profile.
//!
//! The legacy profile keeps one lock-in, delay stage, and X/Y scanner under
//! `instruments`.  This module keeps that projection available while letting a
//! later UI select a named member of each device class.  It deliberately does
//! no I/O and is independent of the worker ownership model.

use crate::lockin::Model;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use trkr_motion::{DelayConfig, ScannerConfig};

const INVENTORY_KEY: &str = "device_inventory";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct DeviceIds {
    pub lockin: String,
    pub delay: String,
    pub scanner: String,
}

impl Default for DeviceIds {
    fn default() -> Self {
        Self {
            lockin: "lockin-1".into(),
            delay: "delay-1".into(),
            scanner: "scanner-1".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LockinEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub label: String,
    #[serde(with = "model_serde")]
    pub model: Model,
    pub address: u8,
    #[serde(default)]
    pub settings: Value,
}

impl LockinEntry {
    pub fn label_text(&self) -> String {
        if self.label.trim().is_empty() {
            self.name
                .strip_prefix("Lock-in ")
                .unwrap_or(&self.name)
                .to_owned()
        } else {
            self.label.trim().to_owned()
        }
    }
    pub fn display_name(&self) -> String {
        format!("Lock-in: {}", self.label_text())
    }

    pub fn new(id: impl Into<String>, name: impl Into<String>, model: Model, address: u8) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            label: String::new(),
            model,
            address,
            settings: json!({}),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DelayEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub label: String,
    pub config: DelayConfig,
    #[serde(default)]
    pub zero_ps: f64,
    /// Original device object, retained in memory so a UI edit does not erase
    /// an extension written by another compatible client.
    #[serde(skip)]
    raw_config: Option<Value>,
}

impl DelayEntry {
    pub fn label_text(&self) -> String {
        if self.label.trim().is_empty() {
            self.name
                .strip_prefix("Delay ")
                .unwrap_or(&self.name)
                .to_owned()
        } else {
            self.label.trim().to_owned()
        }
    }
    pub fn display_name(&self) -> String {
        format!("Delay: {}", self.label_text())
    }

    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        config: DelayConfig,
        zero_ps: f64,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            label: String::new(),
            config,
            zero_ps,
            raw_config: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScannerEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub x: Option<ScannerConfig>,
    #[serde(default)]
    pub y: Option<ScannerConfig>,
    #[serde(default)]
    pub zero_um: [f64; 2],
    #[serde(skip)]
    raw_x: Option<Value>,
    #[serde(skip)]
    raw_y: Option<Value>,
}

impl ScannerEntry {
    pub fn label_text(&self) -> String {
        if self.label.trim().is_empty() {
            self.name
                .strip_prefix("Scanner ")
                .unwrap_or(&self.name)
                .to_owned()
        } else {
            self.label.trim().to_owned()
        }
    }
    pub fn display_name(&self) -> String {
        format!("Scanner: {}", self.label_text())
    }

    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        x: Option<ScannerConfig>,
        y: Option<ScannerConfig>,
        zero_um: [f64; 2],
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            label: String::new(),
            x,
            y,
            zero_um,
            raw_x: None,
            raw_y: None,
        }
    }
}

/// Stable presentation key. Reordering never changes acquisition/device identities.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct PanelKey {
    pub kind: String,
    pub id: String,
}
impl PanelKey {
    pub fn new(kind: &str, id: &str) -> Self {
        Self {
            kind: kind.into(),
            id: id.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Inventory {
    pub lockins: Vec<LockinEntry>,
    pub delays: Vec<DelayEntry>,
    pub scanners: Vec<ScannerEntry>,
    pub selected: DeviceIds,
    pub panel_order: Vec<PanelKey>,
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            lockins: vec![LockinEntry::new("lockin-1", "Lock-in 1", Model::Sr7265, 12)],
            delays: Vec::new(),
            scanners: Vec::new(),
            panel_order: Vec::new(),
            selected: DeviceIds {
                delay: String::new(),
                scanner: String::new(),
                ..DeviceIds::default()
            },
        }
    }
}

impl Inventory {
    pub fn ordered_panels(&self) -> Vec<PanelKey> {
        let all: Vec<_> = self
            .lockins
            .iter()
            .map(|d| PanelKey::new("lockin", &d.id))
            .chain(self.delays.iter().map(|d| PanelKey::new("delay", &d.id)))
            .chain(
                self.scanners
                    .iter()
                    .map(|d| PanelKey::new("scanner", &d.id)),
            )
            .collect();
        let available: HashSet<_> = all.iter().collect();
        let mut seen = HashSet::new();
        self.panel_order
            .iter()
            .filter(|key| available.contains(key))
            .chain(all.iter())
            .filter(|key| seen.insert((*key).clone()))
            .cloned()
            .collect()
    }

    pub fn reorder_panel(&mut self, source: &PanelKey, target: &PanelKey, after: bool) -> bool {
        let before = self.ordered_panels();
        let Some(source_index) = before.iter().position(|key| key == source) else {
            return false;
        };
        if source == target || !before.contains(target) {
            return false;
        }
        let mut order = before.clone();
        let moved = order.remove(source_index);
        let target_index = order
            .iter()
            .position(|key| key == target)
            .expect("known target");
        order.insert(target_index + usize::from(after), moved);
        if order == before {
            return false;
        }
        self.panel_order = order;
        true
    }

    /// Read the current multi-device schema, or create its exact in-memory
    /// equivalent from a Python-era / portable legacy profile.
    pub fn from_profile(raw: &Value) -> Result<Self, String> {
        let mut inventory = match raw.get(INVENTORY_KEY) {
            Some(value) => serde_json::from_value::<Inventory>(value.clone())
                .map_err(|error| format!("{INVENTORY_KEY}: {error}"))?,
            None => Self::from_legacy(raw)?,
        };
        inventory.capture_raw_config(raw);
        inventory.validate()?;
        Ok(inventory)
    }

    fn from_legacy(raw: &Value) -> Result<Self, String> {
        let lockin = raw.pointer("/instruments/lockin/main");
        let (model, address) = match lockin {
            Some(value) => (model_from_value(value)?, gpib_address(value)?),
            None => (Model::Sr7265, 12),
        };
        let delay = raw
            .pointer("/instruments/delay_stage/t")
            .map(|value| {
                let config = serde_json::from_value(value.clone())
                    .map_err(|error| format!("Delay-stage configuration: {error}"))?;
                let mut entry = DelayEntry::new(
                    "delay-1",
                    "Delay 1",
                    config,
                    number(raw.pointer("/measurements/move_abs/zero"), "t_ps")?,
                );
                entry.raw_config = Some(value.clone());
                Ok::<DelayEntry, String>(entry)
            })
            .transpose()?;
        let scanner = match raw.pointer("/instruments/scanner") {
            Some(value) if value.is_object() => {
                let x = parse_scanner(value.get("x"), "X")?;
                let y = parse_scanner(value.get("y"), "Y")?;
                if x.is_none() && y.is_none() {
                    None
                } else {
                    let zero = raw.pointer("/measurements/move_abs/zero");
                    let mut entry = ScannerEntry::new(
                        "scanner-1",
                        "Scanner 1",
                        x,
                        y,
                        [number(zero, "x_um")?, number(zero, "y_um")?],
                    );
                    entry.raw_x = value.get("x").cloned();
                    entry.raw_y = value.get("y").cloned();
                    Some(entry)
                }
            }
            _ => None,
        };
        let mut lockin_entry = LockinEntry::new("lockin-1", "Lock-in 1", model, address);
        lockin_entry.settings = lockin.cloned().unwrap_or_else(|| json!({}));
        Ok(Self {
            lockins: vec![lockin_entry],
            delays: delay.into_iter().collect(),
            scanners: scanner.into_iter().collect(),
            panel_order: Vec::new(),
            selected: DeviceIds {
                lockin: "lockin-1".into(),
                delay: if raw.pointer("/instruments/delay_stage/t").is_some() {
                    "delay-1".into()
                } else {
                    String::new()
                },
                scanner: if raw.pointer("/instruments/scanner/x").is_some()
                    || raw.pointer("/instruments/scanner/y").is_some()
                {
                    "scanner-1".into()
                } else {
                    String::new()
                },
            },
        })
    }

    /// Write only the active devices into the legacy locations.  Other top
    /// level profile data and inactive inventory members are untouched.
    pub fn project_selected(&self, raw: &mut Value) -> Result<(), String> {
        self.validate()?;
        let lockin = self.lockin()?;
        let mut lockin_value = if lockin.settings.is_object() {
            lockin.settings.clone()
        } else {
            json!({})
        };
        lockin_value["model"] = json!(lockin.model.name());
        lockin_value["resource"] = json!(format!("GPIB0::{}::INSTR", lockin.address));
        set(raw, &["instruments", "lockin", "main"], lockin_value);
        let delay = self.delay()?;
        set_optional(
            raw,
            &["instruments", "delay_stage", "t"],
            delay.map(delay_value),
        );
        let scanner = self.scanner()?;
        set_optional(
            raw,
            &["instruments", "scanner", "x"],
            scanner.and_then(|entry| {
                entry
                    .x
                    .as_ref()
                    .map(|config| merge_config(entry.raw_x.as_ref(), config))
            }),
        );
        set_optional(
            raw,
            &["instruments", "scanner", "y"],
            scanner.and_then(|entry| {
                entry
                    .y
                    .as_ref()
                    .map(|config| merge_config(entry.raw_y.as_ref(), config))
            }),
        );
        if let Some(delay) = delay {
            set(
                raw,
                &["measurements", "move_abs", "zero", "t_ps"],
                json!(delay.zero_ps),
            );
        }
        if let Some(scanner) = scanner {
            set(
                raw,
                &["measurements", "move_abs", "zero", "x_um"],
                json!(scanner.zero_um[0]),
            );
            set(
                raw,
                &["measurements", "move_abs", "zero", "y_um"],
                json!(scanner.zero_um[1]),
            );
        }
        raw[INVENTORY_KEY] = self.inventory_value()?;
        Ok(())
    }

    /// Take edits made through the existing legacy controls back into the
    /// selected entries. This keeps a temporary mixed UI from losing an
    /// unselected device or profile extension.
    pub fn absorb_selected(&mut self, raw: &Value) -> Result<(), String> {
        let lockin = self.selected_lockin_mut()?;
        if let Some(value) = raw.pointer("/instruments/lockin/main") {
            lockin.model = model_from_value(value)?;
            lockin.address = gpib_address(value)?;
            lockin.settings = value.clone();
        }
        let zero = raw.pointer("/measurements/move_abs/zero");
        if let Some(entry) = self.selected_delay_mut()? {
            if let Some(value) = raw.pointer("/instruments/delay_stage/t") {
                entry.config = serde_json::from_value(value.clone())
                    .map_err(|error| format!("Delay-stage configuration: {error}"))?;
                entry.raw_config = Some(value.clone());
            }
            entry.zero_ps = number(zero, "t_ps")?;
        }
        if let Some(entry) = self.selected_scanner_mut()? {
            entry.x = parse_scanner(raw.pointer("/instruments/scanner/x"), "X")?;
            entry.y = parse_scanner(raw.pointer("/instruments/scanner/y"), "Y")?;
            entry.raw_x = raw.pointer("/instruments/scanner/x").cloned();
            entry.raw_y = raw.pointer("/instruments/scanner/y").cloned();
            entry.zero_um = [number(zero, "x_um")?, number(zero, "y_um")?];
        }
        self.validate()
    }

    pub fn lockin(&self) -> Result<&LockinEntry, String> {
        find(&self.lockins, &self.selected.lockin, "lock-in")
    }
    pub fn delay(&self) -> Result<Option<&DelayEntry>, String> {
        optional_find(&self.delays, &self.selected.delay, "delay")
    }
    pub fn scanner(&self) -> Result<Option<&ScannerEntry>, String> {
        optional_find(&self.scanners, &self.selected.scanner, "scanner")
    }

    fn selected_lockin_mut(&mut self) -> Result<&mut LockinEntry, String> {
        let id = self.selected.lockin.clone();
        find_mut(&mut self.lockins, &id, "lock-in")
    }
    fn selected_delay_mut(&mut self) -> Result<Option<&mut DelayEntry>, String> {
        let id = self.selected.delay.clone();
        optional_find_mut(&mut self.delays, &id, "delay")
    }
    fn selected_scanner_mut(&mut self) -> Result<Option<&mut ScannerEntry>, String> {
        let id = self.selected.scanner.clone();
        optional_find_mut(&mut self.scanners, &id, "scanner")
    }

    pub fn validate(&self) -> Result<(), String> {
        unique(self.lockins.iter().map(|entry| &entry.id), "lock-in")?;
        unique(self.delays.iter().map(|entry| &entry.id), "delay")?;
        unique(self.scanners.iter().map(|entry| &entry.id), "scanner")?;
        self.lockin()?;
        self.delay()?;
        self.scanner()?;
        let mut gpib = HashSet::new();
        for entry in &self.lockins {
            if !(1..=30).contains(&entry.address) {
                return Err(format!("Lock-in {} GPIB address must be 1..30", entry.id));
            }
            if !gpib.insert(entry.address) {
                return Err(format!("Duplicate GPIB address {}", entry.address));
            }
        }
        let mut delay_ports = HashSet::new();
        for entry in &self.delays {
            finite(entry.zero_ps, &format!("Delay {} zero", entry.id))?;
            let config = &entry.config;
            if let Some(key) = delay_key(config)
                && !delay_ports.insert(key.clone())
            {
                return Err(format!("Duplicate delay controller axis {key}"));
            }
        }
        let mut scanner_axes = HashSet::new();
        let mut scanner_ports: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for entry in &self.scanners {
            finite(entry.zero_um[0], &format!("Scanner {} X zero", entry.id))?;
            finite(entry.zero_um[1], &format!("Scanner {} Y zero", entry.id))?;
            for config in [&entry.x, &entry.y].into_iter().flatten() {
                if let Some((port, controller, axis)) = scanner_key(config) {
                    if delay_ports
                        .iter()
                        .any(|delay| delay.starts_with(&format!("{port}|")))
                    {
                        return Err(format!(
                            "Serial port {port} cannot be shared by a delay stage and scanner"
                        ));
                    }
                    if let Some(previous) = scanner_ports.insert(port.clone(), controller.clone())
                        && previous != controller
                    {
                        return Err(format!(
                            "Serial port {port} has incompatible scanner controller families"
                        ));
                    }
                    let key = format!("{port}|{controller}|{axis}");
                    if !scanner_axes.insert(key.clone()) {
                        return Err(format!("Duplicate scanner endpoint {key}"));
                    }
                }
            }
        }
        Ok(())
    }

    fn capture_raw_config(&mut self, raw: &Value) {
        if raw.get(INVENTORY_KEY).is_none() {
            return;
        }
        let Some(root) = raw.get(INVENTORY_KEY) else {
            return;
        };
        for entry in &mut self.delays {
            entry.raw_config = array(root, "delays")
                .find(|value| value.get("id").and_then(Value::as_str) == Some(entry.id.as_str()))
                .and_then(|value| value.get("config").cloned());
        }
        for entry in &mut self.scanners {
            if let Some(value) = array(root, "scanners")
                .find(|value| value.get("id").and_then(Value::as_str) == Some(entry.id.as_str()))
            {
                entry.raw_x = value.get("x").cloned();
                entry.raw_y = value.get("y").cloned();
            }
        }
    }

    fn inventory_value(&self) -> Result<Value, String> {
        let mut value = serde_json::to_value(self).map_err(|error| error.to_string())?;
        value["panel_order"] = json!(self.ordered_panels());
        let delays = value
            .get_mut("delays")
            .and_then(Value::as_array_mut)
            .expect("serialized inventory delays");
        for (target, entry) in delays.iter_mut().zip(&self.delays) {
            target["config"] = merge_config(entry.raw_config.as_ref(), &entry.config);
        }
        let scanners = value
            .get_mut("scanners")
            .and_then(Value::as_array_mut)
            .expect("serialized inventory scanners");
        for (target, entry) in scanners.iter_mut().zip(&self.scanners) {
            if let Some(config) = &entry.x {
                target["x"] = merge_config(entry.raw_x.as_ref(), config);
            }
            if let Some(config) = &entry.y {
                target["y"] = merge_config(entry.raw_y.as_ref(), config);
            }
        }
        Ok(value)
    }
}

fn array<'a>(value: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}
fn parse_scanner(value: Option<&Value>, label: &str) -> Result<Option<ScannerConfig>, String> {
    value
        .map(|value| {
            serde_json::from_value(value.clone())
                .map_err(|error| format!("{label} scanner configuration: {error}"))
        })
        .transpose()
}
fn finite(value: f64, label: &str) -> Result<(), String> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(format!("{label} must be finite"))
    }
}
fn unique<'a>(ids: impl Iterator<Item = &'a String>, kind: &str) -> Result<(), String> {
    let mut seen = HashSet::new();
    for id in ids {
        if id.trim().is_empty() {
            return Err(format!("{kind} ID must not be empty"));
        }
        if !seen.insert(id) {
            return Err(format!("Duplicate {kind} ID {id}"));
        }
    }
    Ok(())
}
fn find<'a, T: HasId>(items: &'a [T], id: &str, kind: &str) -> Result<&'a T, String> {
    if id.is_empty() {
        return Err(format!("Select a {kind}"));
    }
    items
        .iter()
        .find(|entry| entry.id() == id)
        .ok_or_else(|| format!("Selected {kind} {id} does not exist"))
}
fn optional_find<'a, T: HasId>(
    items: &'a [T],
    id: &str,
    kind: &str,
) -> Result<Option<&'a T>, String> {
    if id.is_empty() {
        Ok(None)
    } else {
        find(items, id, kind).map(Some)
    }
}
fn find_mut<'a, T: HasId>(items: &'a mut [T], id: &str, kind: &str) -> Result<&'a mut T, String> {
    if id.is_empty() {
        return Err(format!("Select a {kind}"));
    }
    items
        .iter_mut()
        .find(|entry| entry.id() == id)
        .ok_or_else(|| format!("Selected {kind} {id} does not exist"))
}
fn optional_find_mut<'a, T: HasId>(
    items: &'a mut [T],
    id: &str,
    kind: &str,
) -> Result<Option<&'a mut T>, String> {
    if id.is_empty() {
        Ok(None)
    } else {
        find_mut(items, id, kind).map(Some)
    }
}
trait HasId {
    fn id(&self) -> &str;
}
impl HasId for LockinEntry {
    fn id(&self) -> &str {
        &self.id
    }
}
impl HasId for DelayEntry {
    fn id(&self) -> &str {
        &self.id
    }
}
impl HasId for ScannerEntry {
    fn id(&self) -> &str {
        &self.id
    }
}

fn delay_key(config: &DelayConfig) -> Option<String> {
    let port = port(&config.port)?;
    Some(format!(
        "{port}|{}|{}",
        canonical(&config.controller),
        config.controller_axis
    ))
}
fn scanner_key(config: &ScannerConfig) -> Option<(String, String, String)> {
    let port = port(&config.port)?;
    let controller = canonical(&config.controller);
    let axis = if controller == "CONEXAGAP" {
        let uppercase_axis = config.axis.to_ascii_uppercase();
        let axis = match uppercase_axis.as_str() {
            "1" | "U" => "U",
            "2" | "V" => "V",
            other => other,
        };
        format!("{}|{axis}", config.controller_address)
    } else {
        config.controller_address.to_string()
    };
    Some((port, controller, axis))
}
fn port(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_ascii_uppercase())
}
fn canonical(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_uppercase)
        .collect()
}

fn delay_value(entry: &DelayEntry) -> Value {
    merge_config(entry.raw_config.as_ref(), &entry.config)
}
fn merge_config<T: Serialize>(original: Option<&Value>, config: &T) -> Value {
    let mut output = original.cloned().unwrap_or_else(|| json!({}));
    if !output.is_object() {
        output = json!({});
    }
    let fields = serde_json::to_value(config).expect("configuration serializes");
    output
        .as_object_mut()
        .expect("object")
        .extend(fields.as_object().expect("configuration object").clone());
    output
}
fn set(root: &mut Value, path: &[&str], value: Value) {
    let mut current = root.as_object_mut().expect("profile object");
    for key in &path[..path.len() - 1] {
        current = current
            .entry(*key)
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .expect("profile path object");
    }
    current.insert(path[path.len() - 1].into(), value);
}
fn set_optional(root: &mut Value, path: &[&str], value: Option<Value>) {
    if let Some(value) = value {
        set(root, path, value);
    } else if let Some(parent) = pointer_object_mut(root, &path[..path.len() - 1]) {
        parent.remove(path[path.len() - 1]);
    }
}
fn pointer_object_mut<'a>(
    root: &'a mut Value,
    path: &[&str],
) -> Option<&'a mut Map<String, Value>> {
    let mut current = root;
    for key in path {
        current = current.get_mut(*key)?;
    }
    current.as_object_mut()
}
fn number(value: Option<&Value>, key: &str) -> Result<f64, String> {
    let number = value
        .and_then(|value| value.get(key))
        .and_then(Value::as_f64)
        .unwrap_or(0.);
    finite(number, key)?;
    Ok(number)
}
fn gpib_address(value: &Value) -> Result<u8, String> {
    let resource = value
        .get("resource")
        .and_then(Value::as_str)
        .unwrap_or("GPIB0::12::INSTR");
    let mut fields = resource.split("::");
    let _ = fields.next();
    let address = fields
        .next()
        .and_then(|value| value.parse::<u8>().ok())
        .ok_or_else(|| format!("Invalid GPIB resource {resource}"))?;
    if !(1..=30).contains(&address) {
        return Err(format!("GPIB address must be 1..30: {resource}"));
    }
    Ok(address)
}
fn model_from_value(value: &Value) -> Result<Model, String> {
    let name = value
        .get("model")
        .or_else(|| value.get("lockin_model"))
        .and_then(Value::as_str)
        .unwrap_or("SR7265");
    Model::ALL
        .into_iter()
        .find(|model| model.name().eq_ignore_ascii_case(name))
        .ok_or_else(|| format!("Unsupported lock-in: {name}"))
}

mod model_serde {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        model: &Model,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(model.name())
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Model, D::Error> {
        let name = String::deserialize(deserializer)?;
        Model::ALL
            .into_iter()
            .find(|model| model.name().eq_ignore_ascii_case(&name))
            .ok_or_else(|| serde::de::Error::custom(format!("Unsupported lock-in: {name}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy() -> Value {
        json!({"instruments":{"lockin":{"main":{"model":"SR7265","resource":"GPIB0::12::INSTR"}},"delay_stage":{"t":{"controller":"SHOT302GS","stage":"SGSP46-500","port":"COM6","direction":1}},"scanner":{"x":{"controller":"CONEXCC","actuator":"TRA12CC","port":"COM5","axis":1,"sample_um_per_unit":582.},"y":{"controller":"CONEXCC","actuator":"TRA12CC","port":"COM4","axis":1,"sample_um_per_unit":412.}}},"measurements":{"move_abs":{"zero":{"t_ps":2.,"x_um":3.,"y_um":4.}}}})
    }

    #[test]
    fn legacy_projection_preserves_unknown_device_fields() {
        let mut profile = legacy();
        profile["instruments"]["scanner"]["x"]["operator_note"] = json!("keep");
        let inventory = Inventory::from_profile(&profile).unwrap();
        inventory.project_selected(&mut profile).unwrap();
        let reloaded = Inventory::from_profile(&profile).unwrap();
        reloaded.project_selected(&mut profile).unwrap();
        assert_eq!(
            profile["instruments"]["scanner"]["x"]["operator_note"],
            "keep"
        );
        assert_eq!(
            profile[INVENTORY_KEY]["scanners"][0]["x"]["operator_note"],
            "keep"
        );
        assert_eq!(profile[INVENTORY_KEY]["selected"]["scanner"], "scanner-1");
    }

    #[test]
    fn selected_projection_uses_one_device_of_each_kind() {
        let mut inventory = Inventory::from_profile(&legacy()).unwrap();
        let mut other = inventory.delays[0].clone();
        other.id = "delay-2".into();
        other.name = "Delay 2".into();
        other.config.port = "COM7".into();
        other.zero_ps = 22.;
        inventory.delays.push(other);
        inventory.selected.delay = "delay-2".into();
        let mut profile = legacy();
        inventory.project_selected(&mut profile).unwrap();
        assert_eq!(profile["instruments"]["delay_stage"]["t"]["port"], "COM7");
        assert_eq!(profile["measurements"]["move_abs"]["zero"]["t_ps"], 22.);
    }

    #[test]
    fn rejects_duplicate_hardware_and_unknown_selection() {
        let mut inventory = Inventory::from_profile(&legacy()).unwrap();
        let mut duplicate = inventory.lockins[0].clone();
        duplicate.id = "lockin-2".into();
        inventory.lockins.push(duplicate);
        assert!(inventory.validate().unwrap_err().contains("Duplicate GPIB"));
        inventory.lockins.pop();
        inventory.selected.scanner = "missing".into();
        assert!(inventory.validate().unwrap_err().contains("does not exist"));
    }
    #[test]
    fn presentation_order_and_names_do_not_reassign_hardware() {
        let mut raw = legacy();
        let mut inventory = Inventory::from_profile(&raw).unwrap();
        inventory.project_selected(&mut raw).unwrap();
        let instruments = raw["instruments"].clone();
        let zero = raw["measurements"]["move_abs"]["zero"].clone();
        let selected = inventory.selected.clone();
        let scanner = PanelKey::new("scanner", "scanner-1");
        let lockin = PanelKey::new("lockin", "lockin-1");
        assert!(inventory.reorder_panel(&scanner, &lockin, false));
        inventory.scanners[0].label = "Pump".into();
        assert_eq!(inventory.scanners[0].display_name(), "Scanner: Pump");
        inventory.project_selected(&mut raw).unwrap();
        let restored = Inventory::from_profile(&raw).unwrap();
        assert_eq!(restored.ordered_panels()[0], scanner);
        assert_eq!(restored.selected, selected);
        assert_eq!(raw["instruments"], instruments);
        assert_eq!(raw["measurements"]["move_abs"]["zero"], zero);
        assert_eq!(restored.scanners[0].display_name(), "Scanner: Pump");
        assert_eq!(restored.delays[0].display_name(), "Delay: 1");
        assert!(!inventory.reorder_panel(&scanner, &scanner, true));
        assert!(!inventory.reorder_panel(&scanner, &PanelKey::new("scanner", "missing"), true));
    }

    #[test]
    fn stale_or_duplicate_panel_order_is_repaired_without_losing_new_entries() {
        let mut inventory = Inventory::from_profile(&legacy()).unwrap();
        let scanner = PanelKey::new("scanner", "scanner-1");
        inventory.panel_order = vec![
            PanelKey::new("scanner", "deleted"),
            scanner.clone(),
            scanner.clone(),
        ];
        let order = inventory.ordered_panels();
        assert_eq!(order.len(), 3);
        assert_eq!(order[0], scanner);
        let mut raw = legacy();
        inventory.project_selected(&mut raw).unwrap();
        assert_eq!(
            raw["device_inventory"]["panel_order"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        inventory.scanners.clear();
        inventory.selected.scanner.clear();
        inventory.project_selected(&mut raw).unwrap();
        assert!(
            !raw["device_inventory"]["panel_order"]
                .to_string()
                .contains("scanner")
        );
    }
}
