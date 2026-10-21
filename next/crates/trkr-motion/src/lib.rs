//! Portable motion configuration and protocol exports.
pub mod protocol;

use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicBool;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct MotionConfig {
    #[serde(default)]
    pub delay: Option<DelayConfig>,
    #[serde(default)]
    pub x: Option<ScannerConfig>,
    #[serde(default)]
    pub y: Option<ScannerConfig>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DelayConfig {
    #[serde(default = "shot", alias = "delay_stage_controller")]
    pub controller: String,
    #[serde(default)]
    pub stage: Option<String>,
    #[serde(default)]
    pub port: String,
    #[serde(default = "dbaud")]
    pub baudrate: u32,
    #[serde(default = "one")]
    pub timeout: f64,
    #[serde(default = "axis", alias = "default_axis")]
    pub controller_axis: u8,
    #[serde(default = "axis")]
    pub axis_count: u8,
    #[serde(default)]
    pub min_pulse: Option<i64>,
    #[serde(default)]
    pub max_pulse: Option<i64>,
    #[serde(default)]
    pub travel_mm: Option<f64>,
    #[serde(default)]
    pub pos_um_per_pulse: Option<f64>,
    #[serde(default)]
    pub screw_lead_mm_per_rev: Option<f64>,
    #[serde(default)]
    pub step_angle_deg: Option<f64>,
    #[serde(default = "crlf")]
    pub write_termination: String,
    #[serde(default = "crlf")]
    pub read_termination: String,
    #[serde(default)]
    pub zero_pos_mm: Option<f64>,
    #[serde(default)]
    pub direction: i32,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ScannerConfig {
    #[serde(default = "cc", alias = "scanner_controller")]
    pub controller: String,
    #[serde(default)]
    pub actuator: Option<String>,
    #[serde(default)]
    pub port: String,
    #[serde(default = "sbaud")]
    pub baudrate: u32,
    #[serde(default = "one")]
    pub timeout: f64,
    #[serde(default = "axis_text", deserialize_with = "text_or_number")]
    pub axis: String,
    #[serde(default = "axis")]
    pub controller_address: u8,
    #[serde(default)]
    pub pos_unit: Option<String>,
    #[serde(default)]
    pub min_pos: Option<f64>,
    #[serde(default)]
    pub max_pos: Option<f64>,
    #[serde(default)]
    pub origin_pos: Option<f64>,
    #[serde(
        default = "one",
        alias = "sample_um_per_actuator_mm",
        alias = "sample_um_per_actuator_deg"
    )]
    pub sample_um_per_unit: f64,
    #[serde(default)]
    pub pos_digits: Option<u8>,
    #[serde(default)]
    pub resolution: Option<f64>,
    #[serde(default)]
    pub min_step: Option<f64>,
    #[serde(default)]
    pub ensure_closed_loop_on_move: Option<bool>,
    #[serde(default)]
    pub software_hysteresis: Hysteresis,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Hysteresis {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, alias = "pre_move_um", alias = "approach_distance_um")]
    pub distance_um: Option<f64>,
    #[serde(default)]
    pub direction: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Position {
    pub physical: [Option<f64>; 3],
    pub delay_stage_mm: Option<f64>,
    pub delay_stage_pulse: Option<i64>,
    pub x_scanner_mm: Option<f64>,
    pub x_scanner_deg: Option<f64>,
    pub y_scanner_mm: Option<f64>,
    pub y_scanner_deg: Option<f64>,
}

/// Delay-stage scale resolved for a particular controller connection.
///
/// `microstep_division` is deliberately optional in configuration hints: a
/// disconnected controller has not reported its division yet, so callers must
/// not silently label a full step as one pulse.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DelayCalibration {
    /// Full normalized configuration identity. Consumers can compare this to
    /// their normalized requested configuration before using cached runtime
    /// microstep data.
    #[serde(default)]
    pub configuration: Option<DelayConfig>,
    pub controller: String,
    pub stage: Option<String>,
    pub port: String,
    #[serde(default)]
    pub configured_pos_um_per_pulse: Option<f64>,
    #[serde(default)]
    pub effective_pos_um_per_pulse: Option<f64>,
    #[serde(default)]
    pub effective_ps_per_pulse: Option<f64>,
    #[serde(default)]
    pub microstep_division: Option<u32>,
    #[serde(default)]
    pub full_step_ps: Option<f64>,
}
fn shot() -> String {
    "SHOT302GS".into()
}
fn cc() -> String {
    "CONEXCC".into()
}
fn dbaud() -> u32 {
    9600
}
fn sbaud() -> u32 {
    921600
}
fn one() -> f64 {
    1.0
}
fn axis() -> u8 {
    1
}
fn axis_text() -> String {
    "1".into()
}
fn text_or_number<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    match v {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Number(n) => Ok(n.to_string()),
        _ => Err(serde::de::Error::custom("axis must be string or number")),
    }
}
impl Default for DelayConfig {
    fn default() -> Self {
        Self {
            controller: shot(),
            stage: None,
            port: String::new(),
            baudrate: dbaud(),
            timeout: one(),
            controller_axis: 1,
            axis_count: 1,
            min_pulse: Some(0),
            max_pulse: None,
            travel_mm: None,
            pos_um_per_pulse: None,
            screw_lead_mm_per_rev: None,
            step_angle_deg: None,
            write_termination: crlf(),
            read_termination: crlf(),
            zero_pos_mm: None,
            direction: 0,
        }
    }
}
impl Default for ScannerConfig {
    fn default() -> Self {
        Self {
            controller: cc(),
            actuator: None,
            port: String::new(),
            baudrate: sbaud(),
            timeout: one(),
            axis: axis_text(),
            controller_address: 1,
            pos_unit: None,
            min_pos: None,
            max_pos: None,
            origin_pos: None,
            sample_um_per_unit: 1.0,
            pos_digits: None,
            resolution: None,
            min_step: None,
            ensure_closed_loop_on_move: None,
            software_hysteresis: Hysteresis::default(),
        }
    }
}

fn crlf() -> String {
    "\r\n".into()
}

use fs2::FileExt;
use protocol::{Bus, DelayModel, ScannerModel, ScannerProtocol};
use serialport::SerialPort;
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{Read, Write},
    sync::atomic::Ordering,
    thread,
    time::{Duration, Instant},
};
const LIGHT_MM_PER_PS: f64 = 0.299_792_458;

/// Lists serial ports visible to the operating system without opening them.
pub fn available_ports() -> Result<Vec<String>, String> {
    let mut ports = serialport::available_ports()
        .map_err(|error| format!("serial port enumeration failed: {error}"))?
        .into_iter()
        .map(|port| port.port_name)
        .collect::<Vec<_>>();
    ports.sort();
    ports.dedup();
    Ok(ports)
}

fn positive(v: f64, name: &str) -> Result<(), String> {
    if v.is_finite() && v > 0. {
        Ok(())
    } else {
        Err(format!("{name} must be finite and positive"))
    }
}
fn finite(v: f64, name: &str) -> Result<(), String> {
    if v.is_finite() {
        Ok(())
    } else {
        Err(format!("{name} must be finite"))
    }
}
fn canonical(s: &str) -> String {
    s.to_ascii_uppercase().replace(['-', '_', ' '], "")
}
impl DelayConfig {
    pub fn normalized(&self) -> Result<Self, String> {
        let mut c = self.clone();
        c.controller = canonical(&c.controller);
        let model = c.model()?;
        if let Some(stage) = &c.stage {
            let (travel, lead, shot_only) = match canonical(stage).as_str() {
                "SGSP46500" => (500., 10., true),
                "OSMS2035" => (35., 1., false),
                "OSMS26300" => (300., 2., false),
                "OSMS33600" => (600., 10., true),
                _ => return Err(format!("Unknown delay stage {stage}")),
            };
            if shot_only && model != DelayModel::Shot302Gs {
                return Err(format!("{stage} requires SHOT302GS"));
            }
            c.travel_mm.get_or_insert(travel);
            c.screw_lead_mm_per_rev.get_or_insert(lead);
            c.step_angle_deg.get_or_insert(0.72);
        }
        if c.port.trim().is_empty() {
            return Err("Delay stage serial port is required".into());
        }
        positive(c.timeout, "Serial timeout")?;
        if c.timeout > 3600. {
            return Err("Serial timeout exceeds one hour".into());
        }
        if ![0, 1].contains(&c.direction) {
            return Err("Delay direction must be 0 or 1".into());
        }
        if c.baudrate == 0
            || c.axis_count == 0
            || c.controller_axis == 0
            || c.controller_axis > c.axis_count
        {
            return Err("Invalid delay-stage baud rate or axis".into());
        }
        if c.write_termination.is_empty()
            || c.read_termination.is_empty()
            || !c.write_termination.is_ascii()
            || !c.read_termination.is_ascii()
        {
            return Err("Serial terminators must be nonempty ASCII".into());
        }
        if let Some(v) = c.pos_um_per_pulse {
            positive(v, "pos_um_per_pulse")?;
        } else {
            positive(
                c.screw_lead_mm_per_rev
                    .ok_or("Specify delay stage calibration or preset")?,
                "Screw lead",
            )?;
            positive(
                c.step_angle_deg.ok_or("Specify step_angle_deg")?,
                "Step angle",
            )?;
        }
        if c.max_pulse.is_none() && c.travel_mm.is_none() {
            return Err("Delay travel limit is required".into());
        }
        if let Some(t) = c.travel_mm {
            positive(t, "Stage travel")?;
        }
        if let Some(z) = c.zero_pos_mm {
            finite(z, "Delay origin")?;
        }
        if c.max_pulse.is_some_and(|v| v <= c.min_pulse.unwrap_or(0)) {
            return Err("Invalid delay pulse limits".into());
        }
        Ok(c)
    }
    fn model(&self) -> Result<DelayModel, String> {
        match canonical(&self.controller).as_str() {
            "SHOT302GS" => Ok(DelayModel::Shot302Gs),
            "GSC01" => Ok(DelayModel::Gsc01),
            "GSC01A" => Ok(DelayModel::Gsc01A),
            _ => Err(format!("Unsupported delay controller {}", self.controller)),
        }
    }
    fn um_per_pulse(&self, micro: u32) -> Result<f64, String> {
        let v = self.pos_um_per_pulse.unwrap_or_else(|| {
            1000. * self.screw_lead_mm_per_rev.unwrap_or(0.) * self.step_angle_deg.unwrap_or(0.)
                / (360. * micro as f64)
        });
        positive(v, "Delay pulse calibration")?;
        Ok(v)
    }
    fn limits(&self, micro: u32) -> Result<(f64, f64), String> {
        let unit = self.um_per_pulse(micro)? / 1000.;
        let min = self.min_pulse.unwrap_or(0) as f64 * unit;
        let max = self
            .max_pulse
            .map(|p| p as f64 * unit)
            .or(self.travel_mm)
            .ok_or("Missing stage upper limit")?;
        if min >= max {
            return Err("Invalid delay travel limits".into());
        }
        Ok((min, max))
    }
    fn origin(&self, micro: u32) -> Result<f64, String> {
        let (min, max) = self.limits(micro)?;
        let zero = self.zero_pos_mm.unwrap_or((min + max) / 2.);
        if !(min..=max).contains(&zero) {
            return Err("Delay origin outside travel".into());
        }
        Ok(zero)
    }
    fn sign(&self) -> f64 {
        if self.direction == 0 { 1. } else { -1. }
    }
    fn ps_from_mm(&self, mm: f64, micro: u32) -> Result<f64, String> {
        finite(mm, "Stage position")?;
        Ok(self.sign() * 2. * (mm - self.origin(micro)?) / LIGHT_MM_PER_PS)
    }
    fn to_mm(&self, ps: f64, micro: u32) -> Result<f64, String> {
        finite(ps, "Delay position")?;
        Ok(self.origin(micro)? + self.sign() * ps * LIGHT_MM_PER_PS / 2.)
    }
    fn pulse(&self, ps: f64, micro: u32) -> Result<i64, String> {
        let mm = self.to_mm(ps, micro)?;
        let (min, max) = self.limits(micro)?;
        if mm < min - 1e-12 || mm > max + 1e-12 {
            return Err(format!(
                "Delay target {ps} ps is outside travel {min}..{max} mm"
            ));
        }
        let pulse = (mm * 1000. / self.um_per_pulse(micro)?).round_ties_even();
        if pulse < i64::MIN as f64 || pulse >= i64::MAX as f64 {
            return Err("Delay pulse overflow".into());
        }
        Ok(pulse as i64)
    }
    pub fn measurement_ps_from_mm(&self, mm: f64) -> Result<f64, String> {
        self.normalized()?.ps_from_mm(mm, 1)
    }
    pub fn mm_from_measurement_ps(&self, ps: f64) -> Result<f64, String> {
        self.normalized()?.to_mm(ps, 1)
    }
    /// Scale available before a stage is opened. The controller-reported
    /// microstep division is intentionally absent here.
    pub fn calibration_hint(&self) -> Result<DelayCalibration, String> {
        let config = self.normalized()?;
        let full_step_um = match (config.screw_lead_mm_per_rev, config.step_angle_deg) {
            (Some(lead), Some(angle)) => Some(1000. * lead * angle / 360.),
            _ => None,
        };
        let full_step_ps = full_step_um.map(|um| (2. * um / 1000. / LIGHT_MM_PER_PS).abs());
        Ok(DelayCalibration {
            configuration: Some(config.clone()),
            controller: config.controller,
            stage: config.stage,
            port: config.port,
            configured_pos_um_per_pulse: config.pos_um_per_pulse,
            effective_pos_um_per_pulse: config.pos_um_per_pulse,
            effective_ps_per_pulse: config
                .pos_um_per_pulse
                .map(|um| (2. * um / 1000. / LIGHT_MM_PER_PS).abs()),
            microstep_division: None,
            full_step_ps,
        })
    }
}
impl ScannerConfig {
    pub fn normalized(&self) -> Result<Self, String> {
        let mut c = self.clone();
        c.controller = canonical(&c.controller);
        let model = c.model()?;
        if let Some(actuator) = &c.actuator {
            let descriptor = actuator_descriptor(actuator)
                .ok_or_else(|| format!("Unknown scanner actuator {actuator}"))?;
            if model != descriptor.controller {
                return Err(format!("{actuator} controller mismatch"));
            }
            c.pos_unit.get_or_insert(descriptor.native_unit.into());
            c.min_pos.get_or_insert(descriptor.minimum);
            c.max_pos.get_or_insert(descriptor.maximum);
            c.resolution.get_or_insert(descriptor.resolution);
        }
        if c.port.trim().is_empty() {
            return Err("Scanner serial port is required".into());
        }
        positive(c.timeout, "Serial timeout")?;
        if c.timeout > 3600. {
            return Err("Serial timeout exceeds one hour".into());
        }
        if c.baudrate == 0 || c.controller_address == 0 {
            return Err("Invalid scanner baud rate or address".into());
        }
        if model.capabilities().uses_named_axis {
            c.axis = match c.axis.to_ascii_uppercase().as_str() {
                "1" | "U" => "U".into(),
                "2" | "V" => "V".into(),
                _ => return Err("CONEXAGAP axis must be U/V or 1/2".into()),
            };
        } else {
            let axis = c
                .axis
                .parse::<u8>()
                .map_err(|_| "CONEXCC axis must be a positive integer")?;
            if axis == 0 {
                return Err("CONEXCC axis must be positive".into());
            }
        }
        let min = c.min_pos.ok_or("Scanner minimum position is required")?;
        let max = c.max_pos.ok_or("Scanner maximum position is required")?;
        finite(min, "Scanner limit")?;
        finite(max, "Scanner limit")?;
        if min >= max {
            return Err("Invalid scanner travel limits".into());
        }
        let origin = c.origin_pos.unwrap_or((min + max) / 2.);
        finite(origin, "Scanner origin")?;
        if !(min..=max).contains(&origin) {
            return Err("Scanner origin outside travel".into());
        }
        c.origin_pos = Some(origin);
        finite(c.sample_um_per_unit, "Scanner sample scale")?;
        if c.sample_um_per_unit == 0. {
            return Err("Scanner sample scale must be nonzero".into());
        }
        if let Some(r) = c.resolution.or(c.min_step) {
            positive(r, "Scanner resolution")?;
        }
        if c.pos_digits.is_some_and(|d| d > 12) {
            return Err("Scanner pos_digits must be <=12".into());
        }
        if let Some(distance) = c.software_hysteresis.distance_um {
            finite(distance, "Hysteresis distance")?;
            if distance < 0. {
                return Err("Hysteresis distance must be nonnegative".into());
            }
        }
        c.hysteresis_sign()?;
        Ok(c)
    }
    fn model(&self) -> Result<ScannerModel, String> {
        match canonical(&self.controller).as_str() {
            "CONEXCC" => Ok(ScannerModel::ConexCc),
            "CONEXAGAP" => Ok(ScannerModel::ConexAgap),
            _ => Err(format!(
                "Unsupported scanner controller {}",
                self.controller
            )),
        }
    }
    fn protocol(&self) -> ScannerProtocol {
        ScannerProtocol {
            model: self.model().expect("validated model"),
            address: self.controller_address,
            axis: self.axis.clone(),
            ensure_closed_loop: self.ensure_closed_loop_on_move.unwrap_or(true),
        }
    }
    fn origin(&self) -> f64 {
        self.origin_pos
            .unwrap_or((self.min_pos.unwrap_or(0.) + self.max_pos.unwrap_or(0.)) / 2.)
    }
    pub fn sample_um_from_raw(&self, raw: f64) -> f64 {
        (raw - self.origin()) * self.sample_um_per_unit
    }
    pub fn raw_from_sample_um(&self, um: f64) -> f64 {
        self.origin() + um / self.sample_um_per_unit
    }
    fn digits(&self) -> usize {
        self.pos_digits.unwrap_or_else(|| {
            self.resolution
                .or(self.min_step)
                .map(|r| (-r.log10()).ceil().max(0.) as u8)
                .unwrap_or(4)
        }) as usize
    }
    fn raw_target(&self, um: f64) -> Result<f64, String> {
        finite(um, "Scanner target")?;
        let raw = self.raw_from_sample_um(um);
        let min = self.min_pos.ok_or("Missing scanner minimum")?;
        let max = self.max_pos.ok_or("Missing scanner maximum")?;
        if raw < min - 1e-12 || raw > max + 1e-12 {
            return Err(format!(
                "Scanner target {um} µm ({raw}) outside {min}..{max}"
            ));
        }
        let factor = 10_f64.powi(self.digits() as i32);
        Ok((raw * factor).round_ties_even() / factor)
    }
    fn hysteresis_sign(&self) -> Result<f64, String> {
        match self
            .software_hysteresis
            .direction
            .as_deref()
            .unwrap_or("negative")
            .to_ascii_lowercase()
            .as_str()
        {
            "negative" | "negative_to_target" | "minus" | "-" => Ok(-1.),
            "positive" | "positive_to_target" | "plus" | "+" => Ok(1.),
            _ => Err("Invalid hysteresis direction".into()),
        }
    }
    fn approach(&self, um: f64) -> Result<Option<f64>, String> {
        self.approach_with_distance(um, self.software_hysteresis.distance_um.unwrap_or(100.))
    }
    fn approach_with_distance(&self, um: f64, distance: f64) -> Result<Option<f64>, String> {
        if !self.model()?.capabilities().supports_software_hysteresis
            || !self.software_hysteresis.enabled
        {
            return Ok(None);
        }
        if distance == 0. {
            return Ok(None);
        }
        let pre = um + self.hysteresis_sign()? * distance;
        self.raw_target(pre)?;
        Ok(Some(pre))
    }
}

/// Defaults that belong to a physical actuator rather than a particular
/// experiment configuration.  New actuators extend this table without
/// duplicating their limits and native unit through normalization code.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ActuatorDescriptor {
    native_unit: &'static str,
    minimum: f64,
    maximum: f64,
    resolution: f64,
    controller: ScannerModel,
}

fn actuator_descriptor(actuator: &str) -> Option<ActuatorDescriptor> {
    match canonical(actuator).as_str() {
        "TRA12CC" => Some(ActuatorDescriptor {
            native_unit: "mm",
            minimum: 0.,
            maximum: 12.,
            resolution: 0.0002,
            controller: ScannerModel::ConexCc,
        }),
        "AGM100D" => Some(ActuatorDescriptor {
            native_unit: "deg",
            minimum: -0.75,
            maximum: 0.75,
            resolution: 0.001,
            controller: ScannerModel::ConexAgap,
        }),
        _ => None,
    }
}
impl MotionConfig {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(c) = &self.delay {
            c.normalized()?;
        }
        for c in [&self.x, &self.y].into_iter().flatten() {
            c.normalized()?;
        }
        Ok(())
    }
    pub fn validate_targets(&self, targets: &[[Option<f64>; 3]]) -> Result<(), String> {
        self.validate()?;
        let delay = self
            .delay
            .as_ref()
            .map(DelayConfig::normalized)
            .transpose()?;
        let scanners = [
            self.x.as_ref().map(ScannerConfig::normalized).transpose()?,
            self.y.as_ref().map(ScannerConfig::normalized).transpose()?,
        ];
        for target in targets {
            for (i, value) in target.iter().enumerate() {
                if let Some(v) = value {
                    finite(*v, "Motion target")?;
                    if i == 0 {
                        let c = delay.as_ref().ok_or("No delay stage configured")?;
                        if c.max_pulse.is_none() && c.min_pulse.unwrap_or(0) == 0 {
                            let mm = c.to_mm(*v, 1)?;
                            if mm < 0. || mm > c.travel_mm.unwrap_or(0.) {
                                return Err("Delay target outside stage travel".into());
                            }
                        }
                    } else {
                        scanners[i - 1]
                            .as_ref()
                            .ok_or("No scanner configured")?
                            .raw_target(*v)?;
                    }
                }
            }
        }
        Ok(())
    }
}
struct Port {
    serial: Box<dyn SerialPort>,
    _lease: File,
    write_end: Vec<u8>,
    read_end: Vec<u8>,
    timeout: Duration,
    poisoned: bool,
}
impl Port {
    fn open(
        name: &str,
        baud: u32,
        timeout: f64,
        write_end: &str,
        read_end: &str,
    ) -> Result<Self, String> {
        let normalized = name.trim().to_ascii_uppercase().replace("\\\\.\\", "");
        let key = normalized
            .bytes()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let path = std::env::temp_dir().join(format!("kohdalab-motion-{key}.lock"));
        let lease = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        lease.try_lock_exclusive().map_err(|_| {
            format!("Serial port {name} is already used by another measurement process")
        })?;
        let mut serial = serialport::new(name, baud)
            .dtr_on_open(true)
            .timeout(Duration::from_millis(100))
            .open()
            .map_err(|e| format!("Open {name}: {e}"))?;
        // Match pyserial's default control lines, including NI USB-232 adapters.
        serial
            .write_data_terminal_ready(true)
            .map_err(|e| format!("Set DTR on {name}: {e}"))?;
        serial
            .write_request_to_send(true)
            .map_err(|e| format!("Set RTS on {name}: {e}"))?;
        Ok(Self {
            serial,
            _lease: lease,
            write_end: write_end.as_bytes().to_vec(),
            read_end: read_end.as_bytes().to_vec(),
            timeout: Duration::from_secs_f64(timeout),
            poisoned: false,
        })
    }
    fn frame(&mut self, command: &str) -> Result<(), String> {
        if !command.is_ascii() || command.contains(['\r', '\n']) {
            return Err("Controller command must be a single ASCII line".into());
        }
        self.serial
            .write_all(command.as_bytes())
            .and_then(|()| self.serial.write_all(&self.write_end))
            .and_then(|()| self.serial.flush())
            .map_err(|e| format!("Serial write: {e}"))
    }
}
impl Bus for Port {
    fn stop_query(&mut self, command: &str) -> Result<String, String> {
        if self.poisoned {
            self.frame(command)?;
            Err("STOP was sent but acknowledgement cannot be confirmed; reconnect the serial session".into())
        } else {
            self.query(command)
        }
    }

    fn query(&mut self, command: &str) -> Result<String, String> {
        if self.poisoned {
            return Err("Serial session failed; disconnect and reconnect".into());
        }
        let result = (|| {
            self.serial
                .clear(serialport::ClearBuffer::Input)
                .map_err(|e| e.to_string())?;
            self.frame(command)?;
            let start = Instant::now();
            let mut bytes = Vec::new();
            while start.elapsed() < self.timeout {
                let mut b = [0u8; 1];
                match self.serial.read(&mut b) {
                    Ok(1) => {
                        bytes.push(b[0]);
                        if bytes.len() > 4096 {
                            return Err("Serial response exceeds 4096 bytes".into());
                        }
                        if bytes.ends_with(&self.read_end) {
                            bytes.truncate(bytes.len() - self.read_end.len());
                            if !bytes.is_ascii() {
                                return Err("Non-ASCII serial response".into());
                            }
                            return String::from_utf8(bytes)
                                .map(|s| s.trim().to_owned())
                                .map_err(|_| "Non-ASCII serial response".into());
                        }
                    }
                    Ok(_) => {}
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                        ) => {}
                    Err(e) => return Err(format!("Serial read: {e}")),
                }
            }
            Err(format!("Serial timeout for {command}"))
        })();
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    fn write(&mut self, command: &str) -> Result<(), String> {
        self.frame(command)
    }
}
struct Delay {
    config: DelayConfig,
    port: usize,
    micro: u32,
}
struct Scanner {
    config: ScannerConfig,
    port: usize,
    prepared: bool,
}

/// A validated two-leg scanner move.  The optional first leg is the existing
/// backlash-compensation approach; keeping it with its target ensures callers
/// that validate, scan, and return use the same ordering rules.
#[derive(Clone, Copy, Debug)]
struct MovePlan {
    target: [Option<f64>; 3],
    approach: [Option<f64>; 3],
}

pub struct MotionRig {
    ports: Vec<Box<dyn Bus + Send>>,
    delay: Option<Delay>,
    scanners: [Option<Scanner>; 2],
}
impl MotionRig {
    pub fn open(config: &MotionConfig) -> Result<Self, String> {
        config.validate()?;
        let mut rig = Self {
            ports: vec![],
            delay: None,
            scanners: [None, None],
        };
        let mut ports: HashMap<String, (usize, u32, f64, bool)> = HashMap::new();
        if let Some(c) = &config.delay {
            let c = c.normalized()?;
            let mut port = Port::open(
                &c.port,
                c.baudrate,
                c.timeout,
                &c.write_termination,
                &c.read_termination,
            )?;
            let micro = protocol::delay_microstep(&mut port, c.model()?, c.controller_axis)?;
            c.origin(micro)?;
            ports.insert(
                c.port.trim().to_ascii_uppercase(),
                (0, c.baudrate, c.timeout, true),
            );
            rig.ports.push(Box::new(port));
            rig.delay = Some(Delay {
                config: c,
                port: 0,
                micro,
            });
        }
        for (i, c) in [&config.x, &config.y].into_iter().enumerate() {
            if let Some(c) = c {
                let c = c.normalized()?;
                let key = c.port.trim().to_ascii_uppercase();
                let index = if let Some((index, baud, timeout, delay)) = ports.get(&key) {
                    if *delay || *baud != c.baudrate || *timeout != c.timeout {
                        return Err("Conflicting settings for shared serial port".into());
                    }
                    *index
                } else {
                    let index = rig.ports.len();
                    rig.ports.push(Box::new(Port::open(
                        &c.port, c.baudrate, c.timeout, "\r\n", "\r\n",
                    )?));
                    ports.insert(key, (index, c.baudrate, c.timeout, false));
                    index
                };
                if let Some(other) = &rig.scanners[0]
                    && other.port == index
                    && other.config.controller_address == c.controller_address
                    && (!c.model()?.capabilities().uses_named_axis || other.config.axis == c.axis)
                {
                    return Err("X and Y refer to the same scanner axis".into());
                }
                rig.scanners[i] = Some(Scanner {
                    config: c,
                    port: index,
                    prepared: false,
                });
            }
        }
        rig.position()?;
        Ok(rig)
    }
    /// Returns the scale actually in use by the connected delay controller.
    pub fn delay_calibration(&self) -> Result<Option<DelayCalibration>, String> {
        let Some(delay) = &self.delay else {
            return Ok(None);
        };
        let config = &delay.config;
        let effective_pos_um_per_pulse = config.um_per_pulse(delay.micro)?;
        let effective_ps_per_pulse =
            (2. * effective_pos_um_per_pulse / 1000. / LIGHT_MM_PER_PS).abs();
        Ok(Some(DelayCalibration {
            configuration: Some(config.clone()),
            controller: config.controller.clone(),
            stage: config.stage.clone(),
            port: config.port.clone(),
            configured_pos_um_per_pulse: config.pos_um_per_pulse,
            effective_pos_um_per_pulse: Some(effective_pos_um_per_pulse),
            effective_ps_per_pulse: Some(effective_ps_per_pulse),
            microstep_division: Some(delay.micro),
            full_step_ps: Some(effective_ps_per_pulse * delay.micro as f64),
        }))
    }
    pub fn to_physical(
        &self,
        target: [Option<f64>; 3],
        coordinate: &str,
        zero: [f64; 3],
    ) -> Result<[Option<f64>; 3], String> {
        let mut out = target;
        for (i, v) in target.into_iter().enumerate() {
            if let Some(v) = v {
                finite(v, "Motion target")?;
                out[i] = Some(match coordinate {
                    "measurement" => {
                        finite(zero[i], "Measurement origin")?;
                        v + zero[i]
                    }
                    "interface" | "instrument" if i == 0 => {
                        let d = self.delay.as_ref().ok_or("Delay stage not connected")?;
                        let mm = if coordinate == "instrument" {
                            if v.fract() != 0. {
                                return Err(
                                    "Delay stage instrument target must be an integer pulse".into(),
                                );
                            }
                            v * d.config.um_per_pulse(d.micro)? / 1000.
                        } else {
                            v
                        };
                        d.config.ps_from_mm(mm, d.micro)?
                    }
                    "interface" | "instrument" => self.scanners[i - 1]
                        .as_ref()
                        .ok_or("Scanner not connected")?
                        .config
                        .sample_um_from_raw(v),
                    _ => return Err("Unknown coordinate".into()),
                });
            }
        }
        Ok(out)
    }
    pub fn validate_targets(&self, targets: &[[Option<f64>; 3]]) -> Result<(), String> {
        let mut previous = [None; 3];
        for target in targets {
            for (i, v) in target.iter().enumerate() {
                if let Some(v) = v {
                    let minimum = if i == 0 {
                        let d = self.delay.as_ref().ok_or("Delay stage not connected")?;
                        d.config.pulse(*v, d.micro)?;
                        2. * d.config.um_per_pulse(d.micro)? / 1000. / LIGHT_MM_PER_PS
                    } else {
                        let s = self.scanners[i - 1]
                            .as_ref()
                            .ok_or("Scanner not connected")?;
                        s.config.raw_target(*v)?;
                        s.config.resolution.or(s.config.min_step).unwrap_or(0.)
                            * s.config.sample_um_per_unit.abs()
                    };
                    if let Some(prev) = previous[i] {
                        let difference: f64 = *v - prev;
                        if difference.abs() > 1e-12 && difference.abs() + 1e-12 < minimum {
                            return Err(format!(
                                "Axis {i} step below controller resolution {minimum}"
                            ));
                        }
                    }
                    previous[i] = Some(*v);
                }
            }
        }
        Ok(())
    }
    pub fn validate_moves(&self, moves: &[([Option<f64>; 3], [bool; 3])]) -> Result<(), String> {
        for (target, approach) in moves {
            self.plan_move(*target, *approach)?;
        }
        Ok(())
    }
    /// Validate the whole return (including its approach) before scanning starts.
    pub fn validate_return(&self, target: [Option<f64>; 3]) -> Result<(), String> {
        self.plan_move(target, [false, true, true])?;
        Ok(())
    }

    /// Builds the existing compensation-then-target sequence without issuing
    /// I/O.  Validation users and execution users deliberately share it so a
    /// future workflow cannot accept a path that execution would reject.
    fn plan_move(
        &self,
        target: [Option<f64>; 3],
        use_approach: [bool; 3],
    ) -> Result<MovePlan, String> {
        self.validate_targets(&[target])?;
        let mut approach = [None; 3];
        for i in 0..2 {
            if use_approach[i + 1]
                && let (Some(scanner), Some(value)) = (&self.scanners[i], target[i + 1])
            {
                approach[i + 1] = scanner.config.approach(value)?;
            }
        }
        Ok(MovePlan { target, approach })
    }
    pub fn position(&mut self) -> Result<Position, String> {
        let mut out = Position::default();
        if let Some(d) = &self.delay {
            let pulse = protocol::delay_position(
                &mut self.ports[d.port],
                d.config.controller_axis,
                d.config.axis_count,
            )?;
            let mm = pulse as f64 * d.config.um_per_pulse(d.micro)? / 1000.;
            out.physical[0] = Some(d.config.ps_from_mm(mm, d.micro)?);
            out.delay_stage_mm = Some(mm);
            out.delay_stage_pulse = Some(pulse);
        }
        for (i, s) in self.scanners.iter().enumerate() {
            if let Some(s) = s {
                let raw = s.config.protocol().position(&mut self.ports[s.port])?;
                out.physical[i + 1] = Some(s.config.sample_um_from_raw(raw));
                match (i, s.config.pos_unit.as_deref()) {
                    (0, Some("deg")) => out.x_scanner_deg = Some(raw),
                    (0, _) => out.x_scanner_mm = Some(raw),
                    (1, Some("deg")) => out.y_scanner_deg = Some(raw),
                    _ => out.y_scanner_mm = Some(raw),
                }
            }
        }
        Ok(out)
    }
    fn start(&mut self, targets: [Option<f64>; 3], cancel: &AtomicBool) -> Result<(), String> {
        if cancel.load(Ordering::Acquire) {
            return Err("motion cancelled".into());
        }
        if let (Some(d), Some(v)) = (&self.delay, targets[0]) {
            protocol::delay_start(
                &mut self.ports[d.port],
                d.config.model()?,
                d.config.controller_axis,
                d.config.pulse(v, d.micro)?,
            )?;
        }
        for (i, v) in targets[1..].iter().enumerate() {
            if let Some(v) = v {
                let s = self.scanners[i].as_ref().ok_or("Scanner not connected")?;
                let raw = s.config.raw_target(*v)?;
                let axis = if s.config.model()?.capabilities().uses_named_axis {
                    s.config.axis.as_str()
                } else {
                    ""
                };
                self.ports[s.port].write(&format!(
                    "{}PA{axis}{raw:.digits$}",
                    s.config.controller_address,
                    digits = s.config.digits()
                ))?;
            }
        }
        Ok(())
    }
    fn wait(
        &mut self,
        targets: [Option<f64>; 3],
        cancel: &AtomicBool,
        timeout: Duration,
        verify_delay: bool,
        progress: &mut Option<&mut dyn FnMut(Position) -> Result<(), String>>,
    ) -> Result<(), String> {
        let start = Instant::now();
        let mut last_position = None;
        loop {
            if cancel.load(Ordering::Acquire) {
                return Err("motion cancelled".into());
            }
            let mut ready = true;
            if targets[0].is_some()
                && let Some(d) = &self.delay
            {
                let reached = if verify_delay {
                    protocol::delay_position(
                        &mut self.ports[d.port],
                        d.config.controller_axis,
                        d.config.axis_count,
                    )? == d
                        .config
                        .pulse(targets[0].ok_or("Missing delay target")?, d.micro)?
                } else {
                    true
                };
                ready &= reached && protocol::delay_ready(&mut self.ports[d.port])?;
            }
            for (i, v) in targets[1..].iter().enumerate() {
                if v.is_some() {
                    let s = self.scanners[i].as_ref().ok_or("Scanner not connected")?;
                    ready &= s.config.protocol().ready(&mut self.ports[s.port])?;
                }
            }
            if progress.is_some()
                && last_position
                    .is_none_or(|last: Instant| last.elapsed() >= Duration::from_millis(100))
            {
                let position = self.position()?;
                if let Some(emit) = progress.as_mut() {
                    emit(position)?;
                }
                last_position = Some(Instant::now());
            }
            // Stop may arrive while the callback reads another instrument.
            if cancel.load(Ordering::Acquire) {
                return Err("motion cancelled".into());
            }
            if ready {
                return Ok(());
            }
            if start.elapsed() >= timeout {
                return Err("Motion timed out".into());
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
    pub fn move_to(
        &mut self,
        targets: [Option<f64>; 3],
        approach: [bool; 3],
        cancel: &AtomicBool,
    ) -> Result<Position, String> {
        self.move_to_with_progress(targets, approach, cancel, None)
    }
    /// Reads actual encoder positions while waiting. Callbacks run synchronously;
    /// any callback I/O must observe the same cancellation token promptly.
    pub fn move_to_with_progress(
        &mut self,
        targets: [Option<f64>; 3],
        approach: [bool; 3],
        cancel: &AtomicBool,
        progress: Option<&mut dyn FnMut(Position) -> Result<(), String>>,
    ) -> Result<Position, String> {
        self.move_with_approach(targets, approach, cancel, progress)
    }
    pub fn return_to_with_progress(
        &mut self,
        targets: [Option<f64>; 3],
        cancel: &AtomicBool,
        progress: Option<&mut dyn FnMut(Position) -> Result<(), String>>,
    ) -> Result<Position, String> {
        self.move_to_with_progress(targets, [false, true, true], cancel, progress)
    }
    fn move_with_approach(
        &mut self,
        targets: [Option<f64>; 3],
        approach: [bool; 3],
        cancel: &AtomicBool,
        mut progress: Option<&mut dyn FnMut(Position) -> Result<(), String>>,
    ) -> Result<Position, String> {
        let plan = self.plan_move(targets, approach)?;
        self.validate_targets(&[plan.approach])?;
        let result = (|| {
            if cancel.load(Ordering::Acquire) {
                return Err("motion cancelled".into());
            }
            for (i, v) in targets[1..].iter().enumerate() {
                if v.is_some() {
                    let s = self.scanners[i].as_mut().ok_or("Scanner not connected")?;
                    let mut protocol = s.config.protocol();
                    protocol.ensure_closed_loop &= !s.prepared;
                    protocol.prepare(&mut self.ports[s.port], cancel)?;
                    s.prepared = true;
                }
            }
            if plan.approach.iter().any(Option::is_some) {
                self.start(plan.approach, cancel)?;
                self.wait(
                    plan.approach,
                    cancel,
                    Duration::from_secs(30),
                    true,
                    &mut progress,
                )?;
            }
            self.start(plan.target, cancel)?;
            self.wait(
                plan.target,
                cancel,
                Duration::from_secs(if plan.target[0].is_some() { 240 } else { 30 }),
                true,
                &mut progress,
            )?;
            let actual = self.position()?;
            for (i, v) in plan.target.iter().enumerate() {
                if let Some(v) = v {
                    if i == 0 {
                        let d = self.delay.as_ref().ok_or("Delay not connected")?;
                        if actual.delay_stage_pulse != Some(d.config.pulse(*v, d.micro)?) {
                            return Err("Delay stage readback does not match target pulse".into());
                        }
                    } else {
                        let s = self.scanners[i - 1]
                            .as_ref()
                            .ok_or("Scanner not connected")?;
                        let expected = s.config.raw_target(*v)?;
                        let measured = s.config.raw_from_sample_um(
                            actual.physical[i].ok_or("Missing position readback")?,
                        );
                        if (expected - measured).abs()
                            > s.config
                                .resolution
                                .or(s.config.min_step)
                                .unwrap_or(0.0001)
                                .max(0.0001)
                        {
                            return Err("Scanner readback does not match target".into());
                        }
                    }
                }
            }
            Ok(actual)
        })();
        if let Err(error) = result {
            return match self.stop() {
                Ok(()) => Err(error),
                Err(stop) => Err(format!("{error}; STOP failed: {stop}")),
            };
        }
        result
    }
    pub fn stop(&mut self) -> Result<(), String> {
        let mut errors = vec![];
        if let Some(d) = &self.delay
            && let Err(e) = protocol::delay_stop(
                &mut self.ports[d.port],
                d.config.model()?,
                d.config.controller_axis,
            )
        {
            errors.push(e)
        }
        for s in self.scanners.iter().flatten() {
            if let Err(e) = s.config.protocol().stop(&mut self.ports[s.port]) {
                errors.push(e)
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
    pub fn initialize(
        &mut self,
        axis: usize,
        home: bool,
        cancel: &AtomicBool,
    ) -> Result<Position, String> {
        self.initialize_with_progress(axis, home, cancel, None)
    }
    /// Python GUI initialization: reference/enable, then move to calibrated
    /// instrument zero. Low-level initialization remains nonmoving when requested.
    pub fn initialize_with_origin_progress(
        &mut self,
        axis: usize,
        home: bool,
        move_to_origin: bool,
        cancel: &AtomicBool,
        mut progress: Option<&mut dyn FnMut(Position) -> Result<(), String>>,
    ) -> Result<Position, String> {
        if !move_to_origin {
            return self.initialize_with_progress(axis, home, cancel, progress);
        }
        if axis > 2 {
            return Err("Unknown axis".into());
        }
        let targets = std::array::from_fn(|i| (i == axis).then_some(0.));
        let approach = [false, true, true];
        self.validate_moves(&[(targets, approach)])?;
        let mut forward = |position| match progress.as_mut() {
            Some(callback) => callback(position),
            None => Ok(()),
        };
        self.initialize_with_progress(axis, home, cancel, Some(&mut forward))?;
        self.move_to_with_progress(targets, approach, cancel, Some(&mut forward))
    }
    pub fn initialize_with_progress(
        &mut self,
        axis: usize,
        home: bool,
        cancel: &AtomicBool,
        mut progress: Option<&mut dyn FnMut(Position) -> Result<(), String>>,
    ) -> Result<Position, String> {
        if axis > 2 {
            return Err("Unknown axis".into());
        }
        let result = (|| {
            if cancel.load(Ordering::Acquire) {
                return Err("motion cancelled".into());
            }
            let mut target = [None; 3];
            target[axis] = Some(0.);
            if axis == 0 {
                let d = self.delay.as_mut().ok_or("Delay stage not connected")?;
                // Refresh calibration even when homing was explicitly disabled.
                d.micro = protocol::delay_microstep(
                    &mut self.ports[d.port],
                    d.config.model()?,
                    d.config.controller_axis,
                )?;
                d.config.origin(d.micro)?;
                if home {
                    protocol::delay_home(
                        &mut self.ports[d.port],
                        d.config.model()?,
                        d.config.controller_axis,
                    )?;
                    self.wait(
                        target,
                        cancel,
                        Duration::from_secs(240),
                        false,
                        &mut progress,
                    )?;
                }
            } else {
                let s = self.scanners[axis - 1]
                    .as_ref()
                    .ok_or("Scanner not connected")?;
                if home && s.config.model()?.capabilities().supports_home {
                    s.config.protocol().home(&mut self.ports[s.port])?;
                    self.wait(
                        target,
                        cancel,
                        Duration::from_secs(60),
                        false,
                        &mut progress,
                    )?;
                } else {
                    s.config
                        .protocol()
                        .prepare(&mut self.ports[s.port], cancel)?;
                }
            }
            if axis > 0 {
                self.scanners[axis - 1]
                    .as_mut()
                    .ok_or("Scanner not connected")?
                    .prepared = false;
            }
            self.position()
        })();
        if let Err(error) = result {
            return match self.stop() {
                Ok(()) => Err(error),
                Err(stop) => Err(format!("{error}; STOP failed: {stop}")),
            };
        }
        result
    }
}

#[cfg(test)]
#[path = "tests/config.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/motion.rs"]
mod motion_fake_bus_tests;
