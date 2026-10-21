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
            let (unit, min, max, res, required) = match canonical(actuator).as_str() {
                "TRA12CC" => ("mm", 0., 12., 0.0002, ScannerModel::ConexCc),
                "AGM100D" => ("deg", -0.75, 0.75, 0.001, ScannerModel::ConexAgap),
                _ => return Err(format!("Unknown scanner actuator {actuator}")),
            };
            if model != required {
                return Err(format!("{actuator} controller mismatch"));
            }
            c.pos_unit.get_or_insert(unit.into());
            c.min_pos.get_or_insert(min);
            c.max_pos.get_or_insert(max);
            c.resolution.get_or_insert(res);
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
        if model == ScannerModel::ConexAgap {
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
        if self.model()? != ScannerModel::ConexCc || !self.software_hysteresis.enabled {
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
                    && (c.model()? != ScannerModel::ConexAgap || other.config.axis == c.axis)
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
            self.validate_targets(&[*target])?;
            for i in 0..2 {
                if approach[i + 1]
                    && let (Some(s), Some(v)) = (&self.scanners[i], target[i + 1])
                {
                    s.config.approach(v)?;
                }
            }
        }
        Ok(())
    }
    /// Validate the whole return (including its approach) before scanning starts.
    pub fn validate_return(&self, target: [Option<f64>; 3]) -> Result<(), String> {
        self.validate_targets(&[target])?;
        for (i, value) in target[1..].iter().enumerate() {
            if let (Some(scanner), Some(value)) = (&self.scanners[i], value) {
                scanner.config.approach(*value)?;
            }
        }
        Ok(())
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
                let axis = if s.config.model()? == ScannerModel::ConexAgap {
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
        self.validate_targets(&[targets])?;
        let mut pre = [None; 3];
        for i in 0..2 {
            if approach[i + 1]
                && let (Some(s), Some(v)) = (&self.scanners[i], targets[i + 1])
            {
                pre[i + 1] = s.config.approach(v)?;
            }
        }
        self.validate_targets(&[pre])?;
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
            if pre.iter().any(Option::is_some) {
                self.start(pre, cancel)?;
                self.wait(pre, cancel, Duration::from_secs(30), true, &mut progress)?;
            }
            self.start(targets, cancel)?;
            self.wait(
                targets,
                cancel,
                Duration::from_secs(if targets[0].is_some() { 240 } else { 30 }),
                true,
                &mut progress,
            )?;
            let actual = self.position()?;
            for (i, v) in targets.iter().enumerate() {
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
                if home && s.config.model()? == ScannerModel::ConexCc {
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
mod tests {
    use super::*;
    fn stage() -> DelayConfig {
        DelayConfig {
            port: "test".into(),
            stage: Some("SGSP46-500".into()),
            direction: 1,
            ..Default::default()
        }
        .normalized()
        .unwrap()
    }
    fn scanner() -> ScannerConfig {
        ScannerConfig {
            port: "test".into(),
            controller: "CONEXAGAP".into(),
            actuator: Some("AG-M100D".into()),
            sample_um_per_unit: 412.,
            axis: "V".into(),
            ..Default::default()
        }
        .normalized()
        .unwrap()
    }
    #[test]
    fn delay_optical_path_and_runtime_microstep() {
        let c = stage();
        assert!((c.ps_from_mm(200., 10).unwrap() - 333.564095198152).abs() < 1e-9);
        assert!((c.to_mm(-122., 10).unwrap() - 268.287339938).abs() < 1e-9);
        assert_eq!(c.pulse(0., 10).unwrap(), 125000);
        assert_eq!(c.um_per_pulse(10).unwrap(), 2.);
        assert!(c.pulse(2000., 10).is_err());
    }
    #[test]
    fn delay_midpoint_uses_runtime_pulse_limits() {
        let c = DelayConfig {
            port: "test".into(),
            max_pulse: Some(10000),
            pos_um_per_pulse: Some(0.5),
            ..Default::default()
        }
        .normalized()
        .unwrap();
        assert_eq!(c.origin(8).unwrap(), 2.5);
        assert_eq!(c.pulse(0., 8).unwrap(), 5000);
    }
    #[test]
    fn scanner_scale_has_no_extra_thousand() {
        let c = scanner();
        assert_eq!(c.sample_um_from_raw(0.5), 206.);
        assert_eq!(c.raw_from_sample_um(-103.), -0.25);
        assert!(c.raw_target(310.).is_err());
        assert_eq!(c.raw_target(10.).unwrap(), 0.024);
        assert_eq!(c.digits(), 3);
    }
    #[test]
    fn scanner_negative_scale_and_preset_alias() {
        let mut c = scanner();
        c.sample_um_per_unit = -412.;
        assert_eq!(c.sample_um_from_raw(-0.5), 206.);
        assert_eq!(c.raw_from_sample_um(206.), -0.5);
        let c:ScannerConfig=serde_json::from_str(r#"{"scanner_controller":"CONEX-AGAP","port":"COM2","actuator":"AG-M100D","axis":2,"sample_um_per_unit":412}"#).unwrap();
        assert_eq!(c.normalized().unwrap().axis, "V");
    }
    #[test]
    fn hysteresis_applies_only_to_cc_and_is_bounded() {
        let mut c = scanner();
        c.software_hysteresis = Hysteresis {
            enabled: true,
            distance_um: Some(5.),
            direction: None,
        };
        assert_eq!(c.approach(-309.).unwrap(), None);
        let mut c = ScannerConfig {
            port: "test".into(),
            actuator: Some("TRA12CC".into()),
            sample_um_per_unit: 10.,
            ..Default::default()
        }
        .normalized()
        .unwrap();
        c.software_hysteresis = Hysteresis {
            enabled: true,
            distance_um: Some(10.),
            direction: None,
        };
        assert!(c.approach(-60.).is_err());
        assert_eq!(c.approach(0.).unwrap(), Some(-10.));
    }
    #[test]
    fn rejects_invalid_calibrations_before_serial_io() {
        let mut c = scanner();
        c.sample_um_per_unit = 0.;
        assert!(c.normalized().is_err());
        let mut d = stage();
        d.step_angle_deg = Some(f64::NAN);
        assert!(d.normalized().is_err());
        assert!(
            MotionConfig {
                delay: None,
                x: Some(scanner()),
                y: None
            }
            .validate_targets(&[[None, Some(400.), None]])
            .is_err()
        );
    }
}

#[cfg(test)]
mod motion_fake_bus_tests {
    use super::*;
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };

    struct ScriptedBus {
        log: Arc<Mutex<Vec<String>>>,
        replies: Arc<Mutex<VecDeque<Result<String, String>>>>,
        fail_write: Option<String>,
    }

    impl ScriptedBus {
        fn new(log: Arc<Mutex<Vec<String>>>, replies: &[&str]) -> Self {
            Self {
                log,
                replies: Arc::new(Mutex::new(
                    replies.iter().map(|reply| Ok((*reply).into())).collect(),
                )),
                fail_write: None,
            }
        }

        fn failing_on(mut self, command: &str) -> Self {
            self.fail_write = Some(command.into());
            self
        }
    }

    impl Bus for ScriptedBus {
        fn query(&mut self, command: &str) -> Result<String, String> {
            self.log.lock().unwrap().push(command.into());
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(format!("unexpected query {command}")))
        }

        fn write(&mut self, command: &str) -> Result<(), String> {
            self.log.lock().unwrap().push(command.into());
            if self.fail_write.as_deref() == Some(command) {
                Err(format!("scripted write failure for {command}"))
            } else {
                Ok(())
            }
        }
    }

    fn agap(axis: &str) -> ScannerConfig {
        ScannerConfig {
            port: "fake".into(),
            controller: "CONEXAGAP".into(),
            actuator: Some("AG-M100D".into()),
            axis: axis.into(),
            sample_um_per_unit: 1.0,
            ..Default::default()
        }
        .normalized()
        .unwrap()
    }

    fn cc_with_hysteresis() -> ScannerConfig {
        ScannerConfig {
            port: "fake".into(),
            controller: "CONEXCC".into(),
            actuator: Some("TRA12CC".into()),
            sample_um_per_unit: 1.0,
            software_hysteresis: Hysteresis {
                enabled: true,
                distance_um: Some(1.0),
                direction: Some("negative".into()),
            },
            ..Default::default()
        }
        .normalized()
        .unwrap()
    }

    fn two_scanner_rig(bus: ScriptedBus) -> MotionRig {
        MotionRig {
            ports: vec![Box::new(bus)],
            delay: None,
            scanners: [
                Some(Scanner {
                    config: agap("U"),
                    port: 0,
                    prepared: false,
                }),
                Some(Scanner {
                    config: agap("V"),
                    port: 0,
                    prepared: false,
                }),
            ],
        }
    }

    #[test]
    fn move_starts_both_scanners_before_polling() {
        let log = Arc::new(Mutex::new(vec![]));
        let bus = ScriptedBus::new(
            log.clone(),
            &[
                "1TS000032",
                "1TS000032", // prepare X/Y
                "1TS000032",
                "1TS000032", // wait X/Y
                "1TPU0.000",
                "1TPV0.000", // readback X/Y
            ],
        );
        let mut rig = two_scanner_rig(bus);

        rig.move_to(
            [None, Some(0.0), Some(0.0)],
            [false; 3],
            &AtomicBool::new(false),
        )
        .unwrap();

        assert_eq!(
            *log.lock().unwrap(),
            vec![
                "1TS",
                "1TS",
                "1PAU0.000",
                "1PAV0.000",
                "1TS",
                "1TS",
                "1TPU",
                "1TPV",
            ]
        );
    }

    #[test]
    fn manual_move_reports_actual_position_before_completion() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut rig = two_scanner_rig(ScriptedBus::new(
            log.clone(),
            &[
                "1TS000032",
                "1TS000032", // prepare
                "1TS000028",
                "1TS000028", // still moving
                "1TPU0.001",
                "1TPV0.002", // intermediate encoder readback
                "1TS000032",
                "1TS000032", // ready
                "1TPU0.010",
                "1TPV0.020", // final readback
            ],
        ));
        let mut positions = vec![];
        let final_position = rig
            .move_to_with_progress(
                [None, Some(0.010), Some(0.020)],
                [false; 3],
                &AtomicBool::new(false),
                Some(&mut |position| {
                    positions.push(position);
                    Ok(())
                }),
            )
            .unwrap();
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].physical, [None, Some(0.001), Some(0.002)]);
        assert_eq!(final_position.physical, [None, Some(0.010), Some(0.020)]);
        assert!(!log.lock().unwrap().iter().any(|cmd| cmd.starts_with("1ST")));
    }

    #[test]
    fn failed_live_callback_stops_motion_before_another_poll() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut rig = two_scanner_rig(ScriptedBus::new(
            log.clone(),
            &["1TS000032", "1TS000028", "1TPU0.001", "1TPV0.002"],
        ));
        let error = rig
            .move_to_with_progress(
                [None, Some(0.010), None],
                [false; 3],
                &AtomicBool::new(false),
                Some(&mut |_| Err("live lock-in read failed".into())),
            )
            .unwrap_err();
        assert_eq!(error, "live lock-in read failed");
        assert!(
            log.lock()
                .unwrap()
                .ends_with(&["1STU".into(), "1STV".into()])
        );
    }

    #[test]
    fn cancellation_from_progress_stops_every_axis() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut rig = two_scanner_rig(ScriptedBus::new(
            log.clone(),
            &["1TS000032", "1TS000028", "1TPU0.001", "1TPV0.002"],
        ));
        let cancel = AtomicBool::new(false);
        let result = rig.move_to_with_progress(
            [None, Some(0.010), None],
            [false; 3],
            &cancel,
            Some(&mut |_| {
                cancel.store(true, Ordering::Release);
                Ok(())
            }),
        );
        assert!(result.unwrap_err().contains("cancelled"));
        assert!(
            log.lock()
                .unwrap()
                .ends_with(&["1STU".into(), "1STV".into()])
        );
    }

    #[test]
    fn initialize_delay_without_home_refreshes_calibration_without_motion() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut rig = MotionRig {
            ports: vec![Box::new(ScriptedBus::new(log.clone(), &["4", "100"]))],
            delay: Some(Delay {
                config: DelayConfig {
                    port: "fake".into(),
                    stage: Some("SGSP46-500".into()),
                    ..Default::default()
                }
                .normalized()
                .unwrap(),
                port: 0,
                micro: 1,
            }),
            scanners: [None, None],
        };
        let position = rig.initialize(0, false, &AtomicBool::new(false)).unwrap();
        assert_eq!(position.delay_stage_pulse, Some(100));
        assert_eq!(
            rig.delay_calibration().unwrap().unwrap().microstep_division,
            Some(4)
        );
        assert_eq!(*log.lock().unwrap(), vec!["?:S1", "Q:"]);
    }

    #[test]
    fn initialize_agap_enables_closed_loop_without_home_command() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut rig = two_scanner_rig(ScriptedBus::new(
            log.clone(),
            &["1TS00003C", "1TS000032", "1TPU0.0", "1TPV0.0"],
        ));
        rig.initialize(1, false, &AtomicBool::new(false)).unwrap();
        assert_eq!(
            *log.lock().unwrap(),
            vec!["1TS", "1MM1", "1TS", "1TPU", "1TPV"]
        );
    }

    #[test]
    fn initialize_cc_homes_and_reports_intermediate_position() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut rig = MotionRig {
            ports: vec![Box::new(ScriptedBus::new(
                log.clone(),
                &["1TS00001E", "1TP1.0", "1TS000032", "1TP0.0"],
            ))],
            delay: None,
            scanners: [
                Some(Scanner {
                    config: cc_with_hysteresis(),
                    port: 0,
                    prepared: true,
                }),
                None,
            ],
        };
        let mut positions = vec![];
        rig.initialize_with_progress(
            1,
            true,
            &AtomicBool::new(false),
            Some(&mut |p| {
                positions.push(p);
                Ok(())
            }),
        )
        .unwrap();
        assert_eq!(log.lock().unwrap()[0], "1OR");
        assert_eq!(positions[0].x_scanner_mm, Some(1.0));
        assert!(!rig.scanners[0].as_ref().unwrap().prepared);
    }

    fn return_rig(log: Arc<Mutex<Vec<String>>>, replies: &[&str]) -> MotionRig {
        let mut shared_compensation = cc_with_hysteresis();
        shared_compensation.software_hysteresis.distance_um = Some(100.);
        let config = ScannerConfig {
            sample_um_per_unit: 100.,
            ensure_closed_loop_on_move: Some(false),
            ..shared_compensation
        }
        .normalized()
        .unwrap();
        MotionRig {
            ports: vec![Box::new(ScriptedBus::new(log, replies))],
            delay: None,
            scanners: [
                Some(Scanner {
                    config,
                    port: 0,
                    prepared: false,
                }),
                None,
            ],
        }
    }
    #[test]
    fn full_initialization_homes_then_moves_to_calibrated_instrument_zero() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut rig = return_rig(
            log.clone(),
            &[
                "1TS000032",
                "1TP0.0",
                "1TP0.0", // home reference is raw0, not measurement0
                "1TS000032",
                "1TS000032",
                "1TP5.0", // common 100 µm approach before instrument zero
                "1TS000032",
                "1TP6.0",
                "1TP6.0",
            ],
        );
        let position = rig
            .initialize_with_origin_progress(1, true, true, &AtomicBool::new(false), None)
            .unwrap();
        assert_eq!(position.physical[1], Some(0.));
        let commands = log.lock().unwrap();
        assert_eq!(commands[0], "1OR");
        let positioning: Vec<_> = commands
            .iter()
            .filter(|c| c.starts_with("1PA"))
            .map(String::as_str)
            .collect();
        assert_eq!(positioning, ["1PA5.0000", "1PA6.0000"]);
        assert!(commands.iter().any(|c| c == "1PA6.0000"));
        assert!(!commands.iter().any(|c| c == "1PA0.0000"));
    }
    #[test]
    fn initialization_checks_zero_approach_limits_before_homing() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut rig = return_rig(log.clone(), &[]);
        rig.scanners[0]
            .as_mut()
            .unwrap()
            .config
            .software_hysteresis
            .distance_um = Some(2000.);
        assert!(
            rig.initialize_with_origin_progress(1, true, true, &AtomicBool::new(false), None)
                .is_err()
        );
        assert!(log.lock().unwrap().is_empty());
    }
    #[test]
    fn stop_on_the_ready_callback_cancels_low_level_initialization_too() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut rig = return_rig(log.clone(), &["1TS000032", "1TP0.0"]);
        let cancel = AtomicBool::new(false);
        let result = rig.initialize_with_progress(
            1,
            true,
            &cancel,
            Some(&mut |_| {
                cancel.store(true, Ordering::Release);
                Ok(())
            }),
        );
        assert!(result.unwrap_err().contains("cancelled"));
        assert_eq!(log.lock().unwrap().last().unwrap(), "1ST");
    }
    #[test]
    fn cancelled_home_does_not_start_the_following_origin_move() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut rig = return_rig(log.clone(), &["1TS000032", "1TP0.0", "1TP0.0"]);
        let cancel = AtomicBool::new(false);
        let result = rig.initialize_with_origin_progress(
            1,
            true,
            true,
            &cancel,
            Some(&mut |_| {
                cancel.store(true, Ordering::Release);
                Ok(())
            }),
        );
        assert!(result.unwrap_err().contains("cancelled"));
        let commands = log.lock().unwrap();
        assert!(commands.iter().all(|c| !c.starts_with("1PA")));
        assert_eq!(commands.last().unwrap(), "1ST");
    }
    #[test]
    fn spatial_return_goes_100_um_below_target_before_reaching_zero() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut rig = return_rig(
            log.clone(),
            &["1TS000032", "1TS000032", "1TS000032", "1TP6.0"],
        );
        rig.validate_return([None, Some(0.), None]).unwrap();
        let position = rig
            .return_to_with_progress([None, Some(0.), None], &AtomicBool::new(false), None)
            .unwrap();
        assert_eq!(position.physical[1], Some(0.));
        assert_eq!(
            *log.lock().unwrap(),
            vec!["1TS", "1PA5.0000", "1TS", "1PA6.0000", "1TS", "1TP"]
        );
        // Scan starts and completion returns use the same compensation distance.
        assert_eq!(
            rig.scanners[0]
                .as_ref()
                .unwrap()
                .config
                .approach(0.)
                .unwrap(),
            Some(-100.)
        );
    }
    #[test]
    fn return_rejects_out_of_range_approach_before_any_motion() {
        let log = Arc::new(Mutex::new(vec![]));
        let rig = return_rig(log.clone(), &[]);
        assert!(rig.validate_return([None, Some(-550.), None]).is_err());
        assert!(log.lock().unwrap().is_empty());
    }
    #[test]
    fn stop_during_return_approach_does_not_continue_to_zero() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut rig = return_rig(log.clone(), &["1TS000032", "1TS000032", "1TP5.0"]);
        let cancel = AtomicBool::new(false);
        let result = rig.return_to_with_progress(
            [None, Some(0.), None],
            &cancel,
            Some(&mut |_| {
                cancel.store(true, Ordering::Release);
                Ok(())
            }),
        );
        assert!(result.unwrap_err().contains("cancelled"));
        let commands = log.lock().unwrap();
        assert!(commands.iter().any(|c| c == "1PA5.0000"));
        assert!(!commands.iter().any(|c| c == "1PA6.0000"));
        assert_eq!(commands.last().unwrap(), "1ST");
    }
    #[test]
    fn successive_scan_lines_repeat_compensation_without_an_origin_move() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut rig = return_rig(
            log.clone(),
            &[
                "1TS000032",
                "1TS000032",
                "1TS000032",
                "1TP6.2",
                "1TS000032",
                "1TS000032",
                "1TP6.4",
                "1TS000032",
                "1TS000032",
                "1TS000032",
                "1TP6.2",
            ],
        );
        let cancel = AtomicBool::new(false);
        rig.move_to([None, Some(20.), None], [false, true, false], &cancel)
            .unwrap();
        rig.move_to([None, Some(40.), None], [false; 3], &cancel)
            .unwrap();
        rig.move_to([None, Some(20.), None], [false, true, false], &cancel)
            .unwrap();
        let commands = log.lock().unwrap();
        let positions: Vec<_> = commands
            .iter()
            .filter(|command| command.starts_with("1PA"))
            .map(String::as_str)
            .collect();
        assert_eq!(
            positions,
            [
                "1PA5.2000",
                "1PA6.2000",
                "1PA6.4000",
                "1PA5.2000",
                "1PA6.2000"
            ]
        );
    }
    #[test]
    fn shared_distance_is_configurable_validated_and_has_no_effect_on_agap() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut rig = return_rig(log, &[]);
        let config = &mut rig.scanners[0].as_mut().unwrap().config;
        config.software_hysteresis.distance_um = None;
        assert_eq!(config.approach(0.).unwrap(), Some(-100.));
        config.software_hysteresis.distance_um = Some(25.);
        assert_eq!(config.approach(0.).unwrap(), Some(-25.));
        config.software_hysteresis.direction = Some("positive".into());
        assert_eq!(config.approach(0.).unwrap(), Some(25.));
        config.software_hysteresis.distance_um = Some(-1.);
        assert!(config.clone().normalized().is_err());
        let mut config = agap("U");
        config.software_hysteresis.enabled = true;
        assert_eq!(config.approach(0.).unwrap(), None);
    }
    #[test]
    fn interface_scan_return_still_uses_saved_measurement_origin() {
        let log = Arc::new(Mutex::new(vec![]));
        let rig = return_rig(log, &[]);
        let returning = rig
            .to_physical([None, Some(0.), None], "measurement", [0., 38.237, 420.487])
            .unwrap();
        assert_eq!(returning, [None, Some(38.237), None]);
        assert_ne!(
            returning,
            rig.to_physical([None, Some(0.), None], "interface", [0., 38.237, 420.487])
                .unwrap()
        );
    }
    #[test]
    fn failed_second_scanner_start_stops_all_scanners() {
        let log = Arc::new(Mutex::new(vec![]));
        let bus =
            ScriptedBus::new(log.clone(), &["1TS000032", "1TS000032"]).failing_on("1PAV0.000");
        let mut rig = two_scanner_rig(bus);

        assert!(
            rig.move_to(
                [None, Some(0.0), Some(0.0)],
                [false; 3],
                &AtomicBool::new(false),
            )
            .is_err()
        );
        assert_eq!(
            *log.lock().unwrap(),
            vec!["1TS", "1TS", "1PAU0.000", "1PAV0.000", "1STU", "1STV"]
        );
    }

    #[test]
    fn cancelled_move_sends_stop_without_starting_motion() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut rig = two_scanner_rig(ScriptedBus::new(log.clone(), &[]));

        assert!(
            rig.move_to(
                [None, Some(0.0), Some(0.0)],
                [false; 3],
                &AtomicBool::new(true),
            )
            .is_err()
        );
        assert_eq!(*log.lock().unwrap(), vec!["1STU", "1STV"]);
    }

    #[test]
    fn delay_readback_mismatch_stops_motion() {
        let log = Arc::new(Mutex::new(vec![]));
        let stage = DelayConfig {
            port: "fake".into(),
            stage: Some("SGSP46-500".into()),
            direction: 1,
            ..Default::default()
        }
        .normalized()
        .unwrap();
        let mut rig = MotionRig {
            ports: vec![Box::new(ScriptedBus::new(
                log.clone(),
                &["OK", "OK", "12500", "R", "12501", "OK"],
            ))],
            delay: Some(Delay {
                config: stage,
                port: 0,
                micro: 1,
            }),
            scanners: [None, None],
        };

        assert!(
            rig.move_to([Some(0.0), None, None], [false; 3], &AtomicBool::new(false))
                .unwrap_err()
                .contains("readback")
        );
        let commands = log.lock().unwrap();
        assert!(commands.iter().any(|command| command == "A:1+P12500"));
        assert_eq!(commands.last().unwrap(), "L:E");
    }

    #[test]
    fn out_of_bounds_hysteresis_approach_performs_no_io() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut rig = MotionRig {
            ports: vec![Box::new(ScriptedBus::new(log.clone(), &[]))],
            delay: None,
            scanners: [
                Some(Scanner {
                    config: cc_with_hysteresis(),
                    port: 0,
                    prepared: false,
                }),
                None,
            ],
        };

        assert!(
            rig.move_to(
                [None, Some(-6.0), None],
                [false, true, false],
                &AtomicBool::new(false),
            )
            .is_err()
        );
        assert!(log.lock().unwrap().is_empty());
    }

    #[test]
    fn runtime_coordinates_convert_measurement_interface_and_instrument_values() {
        let log = Arc::new(Mutex::new(vec![]));
        let stage = DelayConfig {
            port: "delay".into(),
            stage: Some("SGSP46-500".into()),
            direction: 1,
            ..Default::default()
        }
        .normalized()
        .unwrap();
        let x = ScannerConfig {
            port: "x".into(),
            controller: "CONEXCC".into(),
            actuator: Some("TRA12CC".into()),
            sample_um_per_unit: 1.0,
            ..Default::default()
        }
        .normalized()
        .unwrap();
        let rig = MotionRig {
            ports: vec![
                Box::new(ScriptedBus::new(log.clone(), &[])),
                Box::new(ScriptedBus::new(log, &[])),
            ],
            delay: Some(Delay {
                config: stage.clone(),
                port: 0,
                micro: 1,
            }),
            scanners: [
                Some(Scanner {
                    config: x,
                    port: 1,
                    prepared: false,
                }),
                None,
            ],
        };

        assert_eq!(
            rig.to_physical(
                [Some(1.0), Some(2.0), None],
                "measurement",
                [10.0, 20.0, 30.0],
            )
            .unwrap(),
            [Some(11.0), Some(22.0), None]
        );
        let interface = rig
            .to_physical([Some(250.0), Some(7.0), None], "interface", [0.0; 3])
            .unwrap();
        assert!((interface[0].unwrap() - stage.ps_from_mm(250.0, 1).unwrap()).abs() < 1e-12);
        assert_eq!(interface[1], Some(1.0));
        let instrument = rig
            .to_physical([Some(12_500.0), Some(7.0), None], "instrument", [0.0; 3])
            .unwrap();
        assert!((instrument[0].unwrap() - stage.ps_from_mm(250.0, 1).unwrap()).abs() < 1e-12);
        assert_eq!(instrument[1], Some(1.0));
        assert!(
            rig.to_physical([Some(0.5), None, None], "instrument", [0.; 3])
                .is_err()
        );
    }
    #[test]
    fn delay_waits_for_target_readback_before_ready_completion() {
        let log = Arc::new(Mutex::new(vec![]));
        let config = DelayConfig {
            port: "fake".into(),
            stage: Some("SGSP46-500".into()),
            direction: 1,
            ..Default::default()
        }
        .normalized()
        .unwrap();
        let mut rig = MotionRig {
            ports: vec![Box::new(ScriptedBus::new(
                log.clone(),
                &["OK", "OK", "12000", "12500", "R", "12500"],
            ))],
            delay: Some(Delay {
                config,
                port: 0,
                micro: 1,
            }),
            scanners: [None, None],
        };
        let position = rig
            .move_to([Some(0.), None, None], [false; 3], &AtomicBool::new(false))
            .unwrap();
        assert_eq!(position.delay_stage_pulse, Some(12500));
        assert_eq!(
            *log.lock().unwrap(),
            ["A:1+P12500", "G:", "Q:", "Q:", "!:", "Q:"]
        );
    }

    #[test]
    fn delay_calibration_keeps_microstep_unknown_until_connected() {
        let config = DelayConfig {
            port: "COM6".into(),
            stage: Some("SGSP46-500".into()),
            pos_um_per_pulse: Some(0.5),
            ..Default::default()
        };
        let hint = config.calibration_hint().unwrap();
        assert_eq!(hint.microstep_division, None);
        assert_eq!(hint.configured_pos_um_per_pulse, Some(0.5));
        assert!(hint.effective_ps_per_pulse.unwrap() > 0.);
        assert_eq!(hint.configuration, Some(config.normalized().unwrap()));

        let rig = MotionRig {
            ports: vec![],
            delay: Some(Delay {
                config: config.normalized().unwrap(),
                port: 0,
                micro: 8,
            }),
            scanners: [None, None],
        };
        let actual = rig.delay_calibration().unwrap().unwrap();
        assert_eq!(actual.microstep_division, Some(8));
        assert_eq!(actual.configuration, Some(config.normalized().unwrap()));
        assert_eq!(actual.effective_pos_um_per_pulse, Some(0.5));
        assert_eq!(
            actual.full_step_ps.unwrap(),
            actual.effective_ps_per_pulse.unwrap() * 8.
        );
    }
}
