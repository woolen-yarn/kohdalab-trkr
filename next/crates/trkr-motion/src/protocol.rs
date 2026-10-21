//! Controller protocol helpers.  The `Bus` implementation owns framing (CRLF),
//! timeouts, port leases, and poisoning; this module owns controller syntax.
use std::sync::atomic::{AtomicBool, Ordering};

pub trait Bus {
    fn query(&mut self, command: &str) -> Result<String, String>;
    fn write(&mut self, command: &str) -> Result<(), String>;
    fn stop_query(&mut self, command: &str) -> Result<String, String> {
        self.query(command)
    }
}

impl<T: Bus + ?Sized> Bus for Box<T> {
    fn stop_query(&mut self, command: &str) -> Result<String, String> {
        (**self).stop_query(command)
    }
    fn query(&mut self, command: &str) -> Result<String, String> {
        (**self).query(command)
    }
    fn write(&mut self, command: &str) -> Result<(), String> {
        (**self).write(command)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DelayModel {
    Shot302Gs,
    Gsc01,
    Gsc01A,
}

pub fn delay_microstep(bus: &mut impl Bus, model: DelayModel, axis: u8) -> Result<u32, String> {
    require_axis(axis)?;
    let command = match model {
        DelayModel::Shot302Gs | DelayModel::Gsc01A => format!("?:S{axis}"),
        DelayModel::Gsc01 => "?:MS".to_owned(),
    };
    let mut last = String::new();
    for _ in 0..3 {
        let response = bus.query(&command)?;
        last = response.clone();
        let number = response
            .split(|character: char| !character.is_ascii_digit())
            .rfind(|part| !part.is_empty())
            .and_then(|part| part.parse::<u32>().ok());
        if let Some(value) = number {
            return match model {
                DelayModel::Gsc01 => match value {
                    0 => Ok(1),
                    1 => Ok(2),
                    _ => Err(format!(
                        "Unsupported GSC01 microstep mode from ?MS: {response}"
                    )),
                },
                _ if value > 0 => Ok(value),
                _ => Err(format!("Invalid delay microstep response: {response}")),
            };
        }
    }
    let name = match model {
        DelayModel::Shot302Gs => "SHOT-302GS ?S",
        DelayModel::Gsc01 => "GSC01 ?MS",
        DelayModel::Gsc01A => "GSC-01A ?S",
    };
    Err(format!("Unexpected {name} response: {last}"))
}

pub fn delay_position(bus: &mut impl Bus, axis: u8, axis_count: u8) -> Result<i64, String> {
    require_axis(axis)?;
    if axis_count == 0 || axis > axis_count {
        return Err(format!("Axis {axis} is outside 1..{axis_count}"));
    }
    let response = bus.query("Q:")?;
    let values: Vec<&str> = response.split(',').map(str::trim).collect();
    if values.len() < usize::from(axis_count) {
        return Err(format!("Unexpected delay Q response: {response:?}"));
    }
    let value = values[usize::from(axis - 1)].replace(' ', "");
    value
        .parse::<i64>()
        .map_err(|_| format!("Unexpected delay Q response: {response:?}"))
}

pub fn delay_start(
    bus: &mut impl Bus,
    _model: DelayModel,
    axis: u8,
    target_pulse: i64,
) -> Result<(), String> {
    require_axis(axis)?;
    let sign = if target_pulse >= 0 { '+' } else { '-' };
    ack(
        bus.query(&format!("A:{axis}{sign}P{}", target_pulse.unsigned_abs()))?,
        "A",
    )?;
    ack(bus.query("G:")?, "G")
}

pub fn delay_ready(bus: &mut impl Bus) -> Result<bool, String> {
    match bus.query("!:")?.trim().to_ascii_uppercase().as_str() {
        "R" => Ok(true),
        "B" => Ok(false),
        value => Err(format!("Unexpected delay ! response: {value:?}")),
    }
}

pub fn delay_stop(bus: &mut impl Bus, model: DelayModel, axis: u8) -> Result<(), String> {
    require_axis(axis)?;
    let command = match model {
        DelayModel::Gsc01A => format!("L:{axis}"),
        DelayModel::Shot302Gs | DelayModel::Gsc01 => "L:E".to_owned(),
    };
    ack(bus.stop_query(&command)?, "L")
}

pub fn delay_home(bus: &mut impl Bus, _model: DelayModel, axis: u8) -> Result<(), String> {
    require_axis(axis)?;
    ack(bus.query(&format!("H:{axis}"))?, "H")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScannerModel {
    ConexAgap,
    ConexCc,
}

/// Behavioural properties shared by every controller of a scanner model.
///
/// Keeping these properties next to the protocol model gives configuration,
/// initialization, and motion planning one source of truth when another
/// scanner controller is added.  Command syntax remains in `ScannerProtocol`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScannerCapabilities {
    /// The controller accepts an OR/home command as part of initialization.
    pub supports_home: bool,
    /// The controller addresses a physical axis by a named suffix (U/V).
    pub uses_named_axis: bool,
    /// The controller can use the software approach move used for backlash
    /// compensation in sample coordinates.
    pub supports_software_hysteresis: bool,
}

impl ScannerModel {
    pub const fn capabilities(self) -> ScannerCapabilities {
        match self {
            Self::ConexAgap => ScannerCapabilities {
                supports_home: false,
                uses_named_axis: true,
                supports_software_hysteresis: false,
            },
            Self::ConexCc => ScannerCapabilities {
                supports_home: true,
                uses_named_axis: false,
                supports_software_hysteresis: true,
            },
        }
    }
}

#[derive(Clone, Debug)]
pub struct ScannerProtocol {
    pub model: ScannerModel,
    pub address: u8,
    pub axis: String,
    pub ensure_closed_loop: bool,
}

impl ScannerProtocol {
    fn command(&self, body: &str) -> String {
        format!("{}{}", self.address, body)
    }

    fn axis(&self) -> Result<&str, String> {
        if !self.model.capabilities().uses_named_axis {
            return Ok("");
        }
        match self.axis.trim().to_ascii_uppercase().as_str() {
            "U" => Ok("U"),
            "V" => Ok("V"),
            other => Err(format!("Unsupported CONEX-AGAP axis: {other}")),
        }
    }

    pub fn position(&self, bus: &mut impl Bus) -> Result<f64, String> {
        let body = match self.model {
            ScannerModel::ConexCc => "TP".to_owned(),
            ScannerModel::ConexAgap => format!("TP{}", self.axis()?),
        };
        let response = bus.query(&self.command(&body))?;
        let prefix = self.command(&body);
        let value = response
            .strip_prefix(&prefix)
            .ok_or_else(|| format!("Unexpected scanner TP response: {response}"))?
            .parse::<f64>()
            .map_err(|_| format!("Unexpected scanner TP response: {response}"))?;
        if value.is_finite() {
            Ok(value)
        } else {
            Err("scanner returned non-finite position".into())
        }
    }

    pub fn state(&self, bus: &mut impl Bus) -> Result<String, String> {
        let response = bus.query(&self.command("TS"))?;
        let prefix = self.command("TS");
        let payload = response
            .strip_prefix(&prefix)
            .ok_or_else(|| format!("Unexpected scanner TS response: {response:?}"))?;
        if payload.len() != 6 || !payload.as_bytes().iter().all(u8::is_ascii_hexdigit) {
            return Err(format!("Unexpected scanner TS response: {response:?}"));
        }
        let error = &payload[..4];
        if error != "0000" {
            return Err(format!("scanner error {error}"));
        }
        Ok(payload[4..].to_ascii_uppercase())
    }

    pub fn prepare(&self, bus: &mut impl Bus, cancel: &AtomicBool) -> Result<(), String> {
        if cancel.load(Ordering::Acquire) {
            return Err("motion cancelled".into());
        }
        let state = self.state(bus)?;
        match self.model {
            ScannerModel::ConexAgap => {
                if agap_ready(&state) {
                    return Ok(());
                }
                if !matches!(state.as_str(), "3C" | "3D") {
                    return Err(format!(
                        "CONEX-AGAP is not ready for motion (state {state})"
                    ));
                }
                bus.write(&self.command("MM1"))?;
                self.wait_ready_after_enable(bus, cancel, true)
            }
            ScannerModel::ConexCc => {
                if cc_ready(&state) {
                    if !self.ensure_closed_loop {
                        return Ok(());
                    }
                    bus.write(&self.command("MM0"))?;
                    self.wait_state(bus, cancel, &["3C", "3D", "3E", "3F"])?;
                    self.enable_cc_closed_loop(bus, cancel)
                } else if matches!(state.as_str(), "3C" | "3D" | "3E" | "3F") {
                    self.enable_cc_closed_loop(bus, cancel)
                } else if matches!(
                    state.as_str(),
                    "0A" | "0B" | "0C" | "0D" | "0E" | "0F" | "10"
                ) {
                    Err("CONEX-CC is not referenced; run HOME/OR before moving".into())
                } else {
                    Err(format!("CONEX-CC is not ready for motion (state {state})"))
                }
            }
        }
    }

    fn enable_cc_closed_loop(&self, bus: &mut impl Bus, cancel: &AtomicBool) -> Result<(), String> {
        let response = bus.query(&self.command("SC?"))?;
        let prefix = self.command("SC");
        let value = response
            .strip_prefix(&prefix)
            .ok_or_else(|| format!("Unexpected SC response: {response}"))?;
        if value.trim() != "1" {
            bus.write(&self.command("SC1"))?;
        }
        bus.write(&self.command("MM1"))?;
        self.wait_state(bus, cancel, &["32", "33", "34", "36", "37", "38"])
    }

    fn wait_ready_after_enable(
        &self,
        bus: &mut impl Bus,
        cancel: &AtomicBool,
        agap: bool,
    ) -> Result<(), String> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if cancel.load(Ordering::Acquire) {
                return Err("motion cancelled".into());
            }
            let state = self.state(bus)?;
            if (agap && agap_ready(&state)) || (!agap && cc_ready(&state)) {
                return Ok(());
            }
            if !(matches!(state.as_str(), "3C" | "3D" | "3E" | "3F")) {
                return Err(format!(
                    "scanner failed to enter READY state (state {state})"
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        Err("scanner failed to enter READY state".into())
    }

    fn wait_state(
        &self,
        bus: &mut impl Bus,
        cancel: &AtomicBool,
        expected: &[&str],
    ) -> Result<(), String> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if cancel.load(Ordering::Acquire) {
                return Err("motion cancelled".into());
            }
            if expected.contains(&self.state(bus)?.as_str()) {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        Err("scanner state transition timed out".into())
    }

    pub fn start(&self, bus: &mut impl Bus, raw: f64) -> Result<(), String> {
        if !raw.is_finite() {
            return Err("scanner absolute target must be finite".into());
        }
        let body = match self.model {
            ScannerModel::ConexCc => format!("PA{raw:.4}"),
            ScannerModel::ConexAgap => format!("PA{}{raw:.4}", self.axis()?),
        };
        bus.write(&self.command(&body))
    }

    pub fn ready(&self, bus: &mut impl Bus) -> Result<bool, String> {
        let state = self.state(bus)?;
        match self.model {
            ScannerModel::ConexCc if cc_ready(&state) => Ok(true),
            ScannerModel::ConexAgap if agap_ready(&state) => Ok(true),
            ScannerModel::ConexCc if matches!(state.as_str(), "28" | "1E") => Ok(false),
            ScannerModel::ConexAgap if matches!(state.as_str(), "28" | "29" | "46") => Ok(false),
            _ => Err(format!("scanner motion stopped in unsafe state {state}")),
        }
    }

    pub fn stop(&self, bus: &mut impl Bus) -> Result<(), String> {
        let body = match self.model {
            ScannerModel::ConexCc => "ST".to_owned(),
            ScannerModel::ConexAgap => format!("ST{}", self.axis()?),
        };
        bus.write(&self.command(&body))
    }

    pub fn home(&self, bus: &mut impl Bus) -> Result<(), String> {
        let body = match self.model {
            ScannerModel::ConexCc => "OR".to_owned(),
            ScannerModel::ConexAgap => format!("OR{}", self.axis()?),
        };
        bus.write(&self.command(&body))
    }
}

fn require_axis(axis: u8) -> Result<(), String> {
    if axis == 0 {
        Err("Axis must be at least 1".into())
    } else {
        Ok(())
    }
}
fn ack(response: String, command: &str) -> Result<(), String> {
    if response.is_empty()
        || response.to_ascii_uppercase().contains("NG")
        || response.to_ascii_uppercase().contains("ERR")
    {
        Err(format!("{command} command failed: {response}"))
    } else {
        Ok(())
    }
}
fn cc_ready(state: &str) -> bool {
    matches!(state, "32" | "33" | "34" | "36" | "37" | "38")
}
fn agap_ready(state: &str) -> bool {
    matches!(state, "32" | "33" | "34" | "35" | "36")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    struct FakeBus {
        writes: Vec<String>,
        replies: VecDeque<Result<String, String>>,
    }
    impl FakeBus {
        fn new(replies: &[&str]) -> Self {
            Self {
                writes: vec![],
                replies: replies.iter().map(|s| Ok((*s).into())).collect(),
            }
        }
    }
    impl Bus for FakeBus {
        fn query(&mut self, c: &str) -> Result<String, String> {
            self.writes.push(c.into());
            self.replies
                .pop_front()
                .unwrap_or_else(|| Err("missing reply".into()))
        }
        fn write(&mut self, c: &str) -> Result<(), String> {
            self.writes.push(c.into());
            Ok(())
        }
    }
    #[test]
    fn shot_start_transcript() {
        let mut b = FakeBus::new(&["OK", "OK"]);
        delay_start(&mut b, DelayModel::Shot302Gs, 1, -9).unwrap();
        assert_eq!(b.writes, ["A:1-P9", "G:"]);
    }
    #[test]
    fn microstep_tables() {
        let mut b = FakeBus::new(&["MS=1"]);
        assert_eq!(delay_microstep(&mut b, DelayModel::Gsc01, 1).unwrap(), 2);
        assert_eq!(b.writes, ["?:MS"]);
    }
    #[test]
    fn cc_prepare_transcript() {
        let p = ScannerProtocol {
            model: ScannerModel::ConexCc,
            address: 1,
            axis: "1".into(),
            ensure_closed_loop: true,
        };
        let mut b = FakeBus::new(&["1TS000033", "1TS00003C", "1SC0", "1TS000032"]);
        p.prepare(&mut b, &AtomicBool::new(false)).unwrap();
        assert_eq!(
            b.writes,
            ["1TS", "1MM0", "1TS", "1SC?", "1SC1", "1MM1", "1TS"]
        );
    }
    #[test]
    fn agap_start_has_axis() {
        let p = ScannerProtocol {
            model: ScannerModel::ConexAgap,
            address: 1,
            axis: "U".into(),
            ensure_closed_loop: false,
        };
        let mut b = FakeBus::new(&[]);
        p.start(&mut b, 0.5).unwrap();
        assert_eq!(b.writes, ["1PAU0.5000"]);
    }

    #[test]
    fn position_rejects_nonfinite_and_malformed_responses() {
        let p = ScannerProtocol {
            model: ScannerModel::ConexCc,
            address: 1,
            axis: "1".into(),
            ensure_closed_loop: false,
        };
        let mut nonfinite = FakeBus::new(&["1TPNaN"]);
        assert!(
            p.position(&mut nonfinite)
                .unwrap_err()
                .contains("non-finite")
        );
        let mut malformed = FakeBus::new(&["bad"]);
        assert!(
            p.position(&mut malformed)
                .unwrap_err()
                .contains("Unexpected")
        );
    }

    #[test]
    fn status_rejects_error_and_non_hex_payload() {
        let p = ScannerProtocol {
            model: ScannerModel::ConexAgap,
            address: 1,
            axis: "U".into(),
            ensure_closed_loop: false,
        };
        let mut error = FakeBus::new(&["1TS002028"]);
        assert!(p.state(&mut error).unwrap_err().contains("0020"));
        let mut malformed = FakeBus::new(&["1TS0000ZZ"]);
        assert!(p.state(&mut malformed).unwrap_err().contains("Unexpected"));
    }

    #[test]
    fn ready_sets_are_controller_specific() {
        let cc = ScannerProtocol {
            model: ScannerModel::ConexCc,
            address: 1,
            axis: "1".into(),
            ensure_closed_loop: false,
        };
        let agap = ScannerProtocol {
            model: ScannerModel::ConexAgap,
            axis: "U".into(),
            ..cc.clone()
        };
        let mut cc_bus = FakeBus::new(&["1TS000037"]);
        assert!(cc.ready(&mut cc_bus).unwrap());
        let mut agap_bus = FakeBus::new(&["1TS000037"]);
        assert!(agap.ready(&mut agap_bus).is_err());
    }

    #[test]
    fn cancelled_prepare_sends_no_command() {
        let p = ScannerProtocol {
            model: ScannerModel::ConexAgap,
            address: 1,
            axis: "U".into(),
            ensure_closed_loop: false,
        };
        let mut bus = FakeBus::new(&[]);
        assert!(p.prepare(&mut bus, &AtomicBool::new(true)).is_err());
        assert!(bus.writes.is_empty());
    }

    #[test]
    fn configuration_state_does_not_auto_home() {
        let p = ScannerProtocol {
            model: ScannerModel::ConexAgap,
            address: 1,
            axis: "U".into(),
            ensure_closed_loop: false,
        };
        let mut bus = FakeBus::new(&["1TS000014"]);
        assert!(p.prepare(&mut bus, &AtomicBool::new(false)).is_err());
        assert_eq!(bus.writes, ["1TS"]);
    }

    #[test]
    fn scanner_capabilities_describe_existing_model_contracts() {
        assert_eq!(
            ScannerModel::ConexCc.capabilities(),
            ScannerCapabilities {
                supports_home: true,
                uses_named_axis: false,
                supports_software_hysteresis: true,
            }
        );
        assert_eq!(
            ScannerModel::ConexAgap.capabilities(),
            ScannerCapabilities {
                supports_home: false,
                uses_named_axis: true,
                supports_software_hysteresis: false,
            }
        );
    }
}
