//! Optional optical-power monitoring. Power never normalizes the lock-in signal.
mod native;
pub mod resources;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

const IO_TIMEOUT: Duration = Duration::from_secs(1);
const LIVE_INTERVAL: Duration = Duration::from_millis(250);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
// A point read can queue behind one in-flight native live integration.
const NATIVE_REQUEST_TIMEOUT: Duration = Duration::from_secs(25);

/// Presentation precision shared by the GUI and analysis CSV. This is not a
/// sensor accuracy estimate; native replies and computed f64 values stay in raw.
pub const SIGNIFICANT_DIGITS: usize = 4;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub resource: String,
    /// Sensor responsivity setting, not a wavelength measured by this device.
    pub wavelength_nm: f64,
    /// Sample-plane power = meter power in W × this dimensionless factor.
    pub correction_factor: f64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            resource: String::new(),
            wavelength_nm: 800.,
            correction_factor: 1.,
        }
    }
}
impl Config {
    pub fn validate(&self, draft: bool) -> Result<(), String> {
        if !self.wavelength_nm.is_finite() || self.wavelength_nm <= 0. {
            return Err("Power-meter wavelength must be positive and finite (nm)".into());
        }
        if !self.correction_factor.is_finite() || self.correction_factor <= 0. {
            return Err("Power correction factor must be positive and finite".into());
        }
        if !self.resource.trim().is_empty() || !draft {
            resources::normalize(&self.resource)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reading {
    pub native_value: f64,
    pub native_unit: String,
    pub meter_w: f64,
    pub corrected_w: f64,
    pub wavelength_nm: f64,
    pub correction_factor: f64,
    pub host_started_at_utc: String,
    pub host_completed_at_utc: String,
    pub duration_s: f64,
    pub simulated: bool,
    pub device_settings: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub id: String,
    /// A new connection never shares its revision sequence with a previous worker.
    pub session_id: String,
    pub revision: u64,
    pub connected: bool,
    pub config: Config,
    pub identity: Value,
    pub reading: Option<Reading>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Exchange {
    pub command: String,
    pub reply: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Observation {
    pub result: Result<Reading, String>,
    pub exchanges: Vec<Exchange>,
}

trait Io: Send {
    fn query(&mut self, command: &str) -> Result<String, String>;
    fn write(&mut self, command: &str) -> Result<(), String>;
    fn is_poisoned(&self) -> bool {
        false
    }
}

/// A session owns either a SCPI/VISA transport or the manufacturer's native
/// driver. Keep acquisition scheduling independent of the device protocol.
trait MeterDevice: Send {
    fn identity(&self) -> &Value;
    fn apply_config(&mut self, config: &Config) -> Result<(), String>;
    fn read(&mut self) -> Observation;
    fn is_poisoned(&self) -> bool;
}

impl<T: Io> MeterDevice for Meter<T> {
    fn identity(&self) -> &Value {
        &self.identity
    }
    fn apply_config(&mut self, config: &Config) -> Result<(), String> {
        self.apply_config(config)
    }
    fn read(&mut self) -> Observation {
        self.read()
    }
    fn is_poisoned(&self) -> bool {
        self.io.is_poisoned()
    }
}

impl<T: MeterDevice + ?Sized> MeterDevice for Box<T> {
    fn identity(&self) -> &Value {
        (**self).identity()
    }
    fn apply_config(&mut self, config: &Config) -> Result<(), String> {
        (**self).apply_config(config)
    }
    fn read(&mut self) -> Observation {
        (**self).read()
    }
    fn is_poisoned(&self) -> bool {
        (**self).is_poisoned()
    }
}
impl Io for trkr_transport::UsbVisa {
    fn query(&mut self, command: &str) -> Result<String, String> {
        self.query(command)
    }
    fn write(&mut self, command: &str) -> Result<(), String> {
        self.write(command)
    }
    fn is_poisoned(&self) -> bool {
        self.is_poisoned()
    }
}

struct Meter<T: Io> {
    io: T,
    config: Config,
    identity: Value,
    exchanges: Vec<Exchange>,
}
impl<T: Io> Meter<T> {
    fn query(&mut self, command: &str) -> Result<String, String> {
        let result = self.io.query(&format!("{command}\n"));
        self.exchanges.push(Exchange {
            command: command.into(),
            reply: result.as_ref().ok().cloned(),
            error: result.as_ref().err().cloned(),
        });
        result
    }
    fn write(&mut self, command: &str) -> Result<(), String> {
        let result = self.io.write(&format!("{command}\n"));
        self.exchanges.push(Exchange {
            command: command.into(),
            reply: None,
            error: result.as_ref().err().cloned(),
        });
        result
    }
    fn number(&mut self, command: &str) -> Result<f64, String> {
        number(&self.query(command)?, command)
    }
    fn check_error(&mut self) -> Result<(), String> {
        let reply = self.query("SYST:ERR?")?;
        let code = reply
            .split(',')
            .next()
            .unwrap_or("")
            .trim()
            .parse::<i32>()
            .map_err(|_| format!("Invalid power-meter error reply: {reply}"))?;
        if code != 0 {
            return Err(format!("Power meter: {}", reply.trim()));
        }
        Ok(())
    }
    fn initialize(io: T, config: Config) -> Result<Self, String> {
        let mut meter = Self {
            io,
            config: config.clone(),
            identity: Value::Null,
            exchanges: Vec::new(),
        };
        let console = meter.query("*IDN?")?;
        let fields: Vec<_> = console.trim().split(',').collect();
        if fields.len() < 4
            || !fields[0].trim().eq_ignore_ascii_case("THORLABS")
            || !["PM100USB", "PM100D"]
                .iter()
                .any(|model| fields[1].trim().eq_ignore_ascii_case(model))
        {
            return Err(format!(
                "Expected a Thorlabs PM100USB/PM100D, received {}",
                console.trim()
            ));
        }
        let sensor = meter.query("SYST:SENS:IDN?")?;
        let sensor_fields = sensor_fields(&sensor)?;
        let flags = sensor_fields
            .last()
            .and_then(|v| v.trim().parse::<u32>().ok());
        if sensor_fields.len() != 6
            || sensor_fields[0].trim().is_empty()
            || flags.is_none_or(|f| f & 1 == 0)
        {
            return Err(format!("No supported power sensor: {}", sensor.trim()));
        }
        // Preserve device averaging/range/attenuation. The application multiplier
        // is applied exactly once, after the meter's reported power conversion.
        meter.write("CONF:POW")?;
        meter.apply_config(&config)?;
        let average = meter.number("SENS:AVER:COUN?")?;
        let attenuation = meter.number("SENS:CORR:LOSS:INP:MAGN?")?;
        let autorange = meter.query("SENS:POW:RANG:AUTO?")?;
        meter.check_error()?;
        meter.identity = serde_json::json!({"console_idn":console.trim(),"sensor_idn":sensor.trim(),
            "average_count":average,"device_attenuation_db":attenuation,"autorange":autorange.trim(),
            "transport":"USB VISA","model_qualification":"identity_query","initialization_exchanges":meter.exchanges});
        meter.exchanges.clear();
        Ok(meter)
    }
    fn apply_config(&mut self, config: &Config) -> Result<(), String> {
        config.validate(false)?;
        let minimum = self.number("SENS:CORR:WAV? MIN")?;
        let maximum = self.number("SENS:CORR:WAV? MAX")?;
        if minimum <= 0.
            || maximum < minimum
            || !(minimum..=maximum).contains(&config.wavelength_nm)
        {
            return Err(format!(
                "Power wavelength must be within the sensor range {minimum}–{maximum} nm"
            ));
        }
        self.write(&format!("SENS:CORR:WAV {}", config.wavelength_nm))?;
        let actual = self.number("SENS:CORR:WAV?")?;
        if !wavelength_matches(config.wavelength_nm, actual) {
            return Err(format!(
                "Power wavelength readback {actual} nm does not match {} nm",
                config.wavelength_nm
            ));
        }
        self.check_error()?;
        self.config = config.clone();
        Ok(())
    }
    fn read(&mut self) -> Observation {
        self.exchanges.clear();
        let started = utc_now();
        let begin = Instant::now();
        let result = (|| {
            let sensor = self.query("SYST:SENS:IDN?")?;
            if self.identity["sensor_idn"].as_str() != Some(sensor.trim()) {
                return Err("Power sensor changed; reconnect to confirm its calibration".into());
            }
            let relative = self.number("SENS:POW:REF:STAT?")?;
            if relative != 0. {
                return Err(
                    "Disable relative/delta power mode to record absolute optical power".into(),
                );
            }
            let wavelength = self.number("SENS:CORR:WAV?")?;
            if !wavelength_matches(self.config.wavelength_nm, wavelength) {
                return Err("Power-meter wavelength changed; apply settings or reconnect".into());
            }
            let unit_before = self.query("SENS:POW:UNIT?")?.trim().to_ascii_uppercase();
            let native = self.number("READ?")?;
            let completed = utc_now();
            let duration_s = begin.elapsed().as_secs_f64();
            let unit_after = self.query("SENS:POW:UNIT?")?.trim().to_ascii_uppercase();
            if unit_before != unit_after {
                return Err("Power-meter unit changed during acquisition".into());
            }
            let wavelength_after = self.number("SENS:CORR:WAV?")?;
            if !wavelength_matches(wavelength, wavelength_after) {
                return Err("Power-meter wavelength changed during acquisition".into());
            }
            let questionable = self.query("STAT:QUES:COND?")?;
            let flags = questionable
                .trim()
                .parse::<u32>()
                .map_err(|_| "Invalid power-meter condition register".to_string())?;
            if flags != 0 {
                return Err(format!(
                    "Power meter reports questionable data (condition {flags}); raw reading retained"
                ));
            }
            let range_w = self.number("SENS:POW:RANG?")?;
            let attenuation_db = self.number("SENS:CORR:LOSS:INP:MAGN?")?;
            let average_count = self.number("SENS:AVER:COUN?")?;
            self.check_error()?;
            let (meter_w, native_unit) = watts(native, &unit_before)?;
            let corrected_w = meter_w * self.config.correction_factor;
            if !corrected_w.is_finite() {
                return Err("Corrected optical power is not finite".into());
            }
            if meter_w != 0. && corrected_w == 0. {
                return Err("Corrected optical power underflowed; raw reading retained".into());
            }
            Ok(Reading {
                native_value: native,
                native_unit,
                meter_w,
                corrected_w,
                wavelength_nm: wavelength,
                correction_factor: self.config.correction_factor,
                host_started_at_utc: started,
                host_completed_at_utc: completed,
                duration_s,
                simulated: false,
                device_settings: serde_json::json!({"sensor_idn":sensor.trim(),"range_W":range_w,
                    "device_attenuation_db":attenuation_db,"average_count":average_count,
                    "relative_mode":false,"questionable_condition":flags}),
            })
        })();
        Observation {
            result,
            exchanges: std::mem::take(&mut self.exchanges),
        }
    }
}
fn number(reply: &str, quantity: &str) -> Result<f64, String> {
    reply
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("Invalid finite power-meter {quantity}: {}", reply.trim()))
}
fn sensor_fields(reply: &str) -> Result<Vec<String>, String> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_reader(reply.trim().as_bytes());
    reader
        .records()
        .next()
        .ok_or("Missing power sensor identity")?
        .map(|record| record.iter().map(str::to_owned).collect())
        .map_err(|e| format!("Invalid power sensor identity: {e}"))
}
fn wavelength_matches(expected: f64, actual: f64) -> bool {
    // Same absolute wavelength readback tolerance as PLE's fixed-wavelength
    // adapter; never silently relax a requested calibration setting.
    actual.is_finite() && actual > 0. && (actual - expected).abs() <= 1e-6
}
fn watts(native: f64, unit: &str) -> Result<(f64, String), String> {
    let (value, unit) = match unit {
        "W" => (native, "W"),
        "DBM" => (1e-3 * 10_f64.powf(native / 10.), "dBm"),
        _ => return Err(format!("Unknown power-meter unit {unit}")),
    };
    if !value.is_finite() {
        return Err("Optical power conversion overflowed".into());
    }
    if unit == "dBm" && value == 0. {
        return Err("Optical power conversion underflowed; raw reading retained".into());
    }
    Ok((value, unit.into()))
}
fn utc_now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("current UTC timestamp")
}

enum Request {
    Read(mpsc::SyncSender<Observation>),
    Configure(Config, mpsc::SyncSender<Result<(), String>>),
}

/// Clone under the mutex, then notify after releasing it. A consumer may read
/// status while processing a notification; keeping the guard would deadlock.
fn emit_status(state: &Mutex<Status>, emit: &impl Fn(Status)) {
    let snapshot = state.lock().unwrap().clone();
    emit(snapshot);
}

/// Owns a USB session on a dedicated thread so stage motion/lock-in cadence and
/// GUI repaints do not depend on the power sensor's integration time.
pub struct Controller {
    requests: mpsc::SyncSender<Request>,
    status: Arc<Mutex<Status>>,
    shutdown: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    request_timeout: Duration,
}
impl Controller {
    pub fn connect(
        id: String,
        config: Config,
        emit: impl Fn(Status) + Send + 'static,
    ) -> Result<Self, String> {
        Self::start(
            id,
            config,
            emit,
            |config| -> Result<Box<dyn MeterDevice>, String> {
                if trkr_transport::thorlabs::is_driver_resource(config.resource.trim()) {
                    Ok(Box::new(native::NativeMeter::connect(config)?))
                } else {
                    let io = trkr_transport::UsbVisa::connect(
                        &resources::normalize(&config.resource)?,
                        IO_TIMEOUT,
                    )?;
                    Ok(Box::new(Meter::initialize(io, config)?))
                }
            },
        )
    }
    fn start<T: MeterDevice + 'static>(
        id: String,
        config: Config,
        emit: impl Fn(Status) + Send + 'static,
        open: impl FnOnce(Config) -> Result<T, String> + Send + 'static,
    ) -> Result<Self, String> {
        config.validate(false)?;
        let request_timeout =
            if trkr_transport::thorlabs::is_driver_resource(config.resource.trim()) {
                NATIVE_REQUEST_TIMEOUT
            } else {
                REQUEST_TIMEOUT
            };
        let status = Arc::new(Mutex::new(Status {
            id,
            session_id: uuid::Uuid::new_v4().to_string(),
            revision: 0,
            connected: false,
            config: config.clone(),
            identity: Value::Null,
            reading: None,
            error: None,
        }));
        let state = status.clone();
        let shutdown = Arc::new(AtomicBool::new(false));
        let stop = shutdown.clone();
        let (requests, receive) = mpsc::sync_channel(1);
        let (ready, initialized) = mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || {
            let opened = open(config);
            if stop.load(Ordering::Acquire) {
                return;
            }
            let mut meter = match opened {
                Ok(meter) => meter,
                Err(error) => {
                    {
                        let mut state = state.lock().unwrap();
                        state.error = Some(error.clone());
                        state.revision += 1;
                    }
                    emit_status(&state, &emit);
                    let _ = ready.send(Err(error));
                    return;
                }
            };
            {
                let mut state = state.lock().unwrap();
                state.connected = true;
                state.identity = meter.identity().clone();
                state.revision += 1;
            }
            emit_status(&state, &emit);
            if ready.send(Ok(())).is_err() {
                return;
            }
            while !stop.load(Ordering::Acquire) {
                let request = match receive.recv_timeout(LIVE_INTERVAL) {
                    Ok(request) => Some(request),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                if stop.load(Ordering::Acquire) {
                    break;
                }
                match request {
                    Some(Request::Configure(config, reply)) => {
                        let result = meter.apply_config(&config);
                        if stop.load(Ordering::Acquire) {
                            break;
                        }
                        {
                            let mut state = state.lock().unwrap();
                            state.revision += 1;
                            state.reading = None;
                            state.error = result.as_ref().err().cloned();
                            if result.is_ok() {
                                state.config = config;
                            } else {
                                state.connected = false;
                            }
                        }
                        emit_status(&state, &emit);
                        let failed = result.is_err();
                        let _ = reply.send(result);
                        if failed {
                            break;
                        }
                    }
                    request => {
                        let observation = meter.read();
                        if stop.load(Ordering::Acquire) {
                            break;
                        }
                        let failed = meter.is_poisoned();
                        {
                            let mut state = state.lock().unwrap();
                            state.revision += 1;
                            state.reading = observation.result.as_ref().ok().cloned();
                            state.error = observation.result.as_ref().err().cloned();
                            state.connected = !failed;
                        }
                        emit_status(&state, &emit);
                        if let Some(Request::Read(reply)) = request {
                            let _ = reply.send(observation);
                        }
                        if failed {
                            break;
                        }
                    }
                }
            }
            {
                let mut state = state.lock().unwrap();
                state.connected = false;
                state.revision += 1;
            }
            if !stop.load(Ordering::Acquire) {
                emit_status(&state, &emit);
            }
        });
        let controller = Self {
            requests,
            status,
            shutdown,
            thread: Some(thread),
            request_timeout,
        };
        initialized
            .recv_timeout(Duration::from_secs(25))
            .map_err(|_| "Power-meter connection timed out".to_string())??;
        Ok(controller)
    }
    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }
    pub fn configure(&self, config: Config) -> Result<(), String> {
        config.validate(false)?;
        let previous = self.status();
        if !previous.connected {
            return Err("Reconnect the power meter before applying settings".into());
        }
        if resources::normalize(&previous.config.resource)?
            != resources::normalize(&config.resource)?
        {
            return Err("Disconnect the power meter before changing its resource".into());
        }
        let (reply, response) = mpsc::sync_channel(1);
        self.requests
            .try_send(Request::Configure(config, reply))
            .map_err(|_| "Power meter is busy".to_string())?;
        response
            .recv_timeout(self.request_timeout)
            .map_err(|_| "Power-meter setting acknowledgement timed out".to_string())?
    }
    pub fn read(&self, cancel: &AtomicBool) -> Observation {
        let failed = |message: String| Observation {
            result: Err(message),
            exchanges: Vec::new(),
        };
        if cancel.load(Ordering::Acquire) {
            return failed("Power read cancelled".into());
        }
        let status = self.status();
        if !status.connected {
            return failed(
                status
                    .error
                    .unwrap_or_else(|| "Power meter disconnected".into()),
            );
        }
        let (reply, response) = mpsc::sync_channel(1);
        if self.requests.try_send(Request::Read(reply)).is_err() {
            return failed("Power meter is busy".into());
        }
        let deadline = Instant::now() + self.request_timeout;
        loop {
            if cancel.load(Ordering::Acquire) {
                return failed("Power read cancelled".into());
            }
            if Instant::now() >= deadline {
                return failed("Power read timed out".into());
            }
            match response.recv_timeout(Duration::from_millis(20)) {
                Ok(observation) => return observation,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return failed(
                        self.status()
                            .error
                            .unwrap_or_else(|| "Power worker disconnected".into()),
                    );
                }
            }
        }
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            // A helper transfer is deadline bounded; never freeze application exit.
            let deadline = Instant::now() + Duration::from_millis(500);
            while !thread.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            if thread.is_finished() {
                let _ = thread.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn status_callback_runs_without_the_status_mutex() {
        let state = Mutex::new(Status {
            id: "test".into(),
            session_id: "session".into(),
            revision: 1,
            connected: false,
            config: Config::default(),
            identity: Value::Null,
            reading: None,
            error: None,
        });
        emit_status(&state, &|snapshot| {
            assert!(state.try_lock().is_ok());
            assert_eq!(snapshot.revision, 1);
        });
    }

    use super::*;
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::atomic::AtomicUsize;

    struct Fake {
        replies: BTreeMap<String, VecDeque<String>>,
        writes: Arc<Mutex<Vec<String>>>,
        reads: Arc<AtomicUsize>,
        wavelength: f64,
        read_delay: Duration,
    }
    impl Default for Fake {
        fn default() -> Self {
            Self {
                replies: BTreeMap::new(),
                writes: Arc::default(),
                reads: Arc::default(),
                wavelength: 800.,
                read_delay: Duration::ZERO,
            }
        }
    }
    impl Fake {
        fn with(mut self, command: &str, replies: &[&str]) -> Self {
            self.replies.insert(
                command.into(),
                replies.iter().map(|v| (*v).to_owned()).collect(),
            );
            self
        }
    }
    impl Io for Fake {
        fn query(&mut self, command: &str) -> Result<String, String> {
            assert!(command.ends_with('\n'));
            let command = command.trim();
            if let Some(reply) = self.replies.get_mut(command).and_then(VecDeque::pop_front) {
                return Ok(reply);
            }
            Ok(match command {
                "*IDN?" => "THORLABS,PM100D,P0001,2.0".into(),
                "SYST:SENS:IDN?" => "S120C,S1,2026-01-01,1,1,33".into(),
                "SENS:CORR:WAV? MIN" => "400".into(),
                "SENS:CORR:WAV? MAX" => "1100".into(),
                "SENS:CORR:WAV?" => self.wavelength.to_string(),
                "SENS:POW:UNIT?" => "W".into(),
                "SYST:ERR?" => "0,\"No error\"".into(),
                "SENS:POW:RANG:AUTO?" | "SENS:AVER:COUN?" => "1".into(),
                "SENS:POW:RANG?" => "1e-2".into(),
                "SENS:POW:REF:STAT?" | "STAT:QUES:COND?" | "SENS:CORR:LOSS:INP:MAGN?" => "0".into(),
                "READ?" => {
                    let n = self.reads.fetch_add(1, Ordering::AcqRel) + 1;
                    std::thread::sleep(self.read_delay);
                    format!("{}e-12", n)
                }
                _ => panic!("Unexpected query {command}"),
            })
        }
        fn write(&mut self, command: &str) -> Result<(), String> {
            assert!(command.ends_with('\n'));
            self.writes.lock().unwrap().push(command.trim().into());
            if let Some(value) = command.trim().strip_prefix("SENS:CORR:WAV ") {
                self.wavelength = value.parse().unwrap();
            }
            Ok(())
        }
    }
    fn config() -> Config {
        Config {
            resource: "USB0::0x1313::0x8078::TEST::INSTR".into(),
            wavelength_nm: 800.,
            correction_factor: 2.5,
        }
    }
    #[test]
    fn cancelling_a_pending_power_read_keeps_the_session_and_next_reply_separate() {
        let count = Arc::new(AtomicUsize::new(0));
        let observed = count.clone();
        let controller = Controller::start(
            "power-1".into(),
            config(),
            |_| {},
            move |config| {
                Meter::initialize(
                    Fake {
                        reads: observed,
                        read_delay: Duration::from_millis(120),
                        ..Default::default()
                    },
                    config,
                )
            },
        )
        .unwrap();
        let cancel = AtomicBool::new(false);
        std::thread::scope(|scope| {
            let attempt = scope.spawn(|| controller.read(&cancel));
            let deadline = Instant::now() + Duration::from_secs(2);
            while count.load(Ordering::Acquire) == 0 && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(count.load(Ordering::Acquire) > 0);
            cancel.store(true, Ordering::Release);
            assert!(
                attempt
                    .join()
                    .unwrap()
                    .result
                    .unwrap_err()
                    .contains("cancelled")
            );
        });
        let result = controller.read(&AtomicBool::new(false)).result.unwrap();
        assert!(result.native_value >= 2e-12);
        assert!(controller.status().connected);
        controller
            .configure(Config {
                correction_factor: 3.,
                ..config()
            })
            .unwrap();
        assert_eq!(controller.status().config.correction_factor, 3.);
        let reading = controller.read(&AtomicBool::new(false)).result.unwrap();
        assert_eq!(reading.corrected_w, reading.native_value * 3.);
    }

    #[test]
    fn corrects_native_power_once_and_retains_each_original_reply() {
        let fake = Fake::default().with("READ?", &["1.234567890123450e-12\r\n"]);
        let writes = fake.writes.clone();
        let mut meter = Meter::initialize(fake, config()).unwrap();
        let observation = meter.read();
        let reading = observation.result.unwrap();
        assert_eq!(reading.native_value, 1.23456789012345e-12);
        assert_eq!(reading.meter_w, reading.native_value);
        assert_eq!(reading.corrected_w, reading.meter_w * 2.5);
        assert_eq!(reading.wavelength_nm, 800.);
        assert_eq!(reading.native_unit, "W");
        // RFC 3339 permits variable fractional precision, so lexical order
        // does not reliably express chronological order (e.g. .12Z vs .123Z).
        let parse = |value: &str| {
            time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
                .unwrap()
        };
        assert!(parse(&reading.host_completed_at_utc) >= parse(&reading.host_started_at_utc));
        assert_eq!(
            observation
                .exchanges
                .iter()
                .find(|x| x.command == "READ?")
                .unwrap()
                .reply
                .as_deref(),
            Some("1.234567890123450e-12\r\n")
        );
        assert_eq!(
            *writes.lock().unwrap(),
            vec!["CONF:POW", "SENS:CORR:WAV 800"]
        );
    }
    #[test]
    fn dbm_is_converted_before_the_sample_plane_multiplier() {
        let fake = Fake::default()
            .with("SENS:POW:UNIT?", &["DBM", "DBM"])
            .with("READ?", &["-30.0"]);
        let reading = Meter::initialize(fake, config())
            .unwrap()
            .read()
            .result
            .unwrap();
        assert_eq!(reading.native_value, -30.);
        assert_eq!(reading.native_unit, "dBm");
        assert_eq!(reading.meter_w, 1e-6);
        assert_eq!(reading.corrected_w, 1e-6 * 2.5);
    }
    #[test]
    fn invalid_optical_observations_never_become_a_numeric_power() {
        for (command, replies) in [
            ("READ?", vec!["NaN"]),
            ("READ?", vec!["1,2"]),
            ("STAT:QUES:COND?", vec!["8"]),
            ("SENS:POW:REF:STAT?", vec!["1"]),
            ("SENS:POW:UNIT?", vec!["W", "DBM"]),
            ("SENS:POW:UNIT?", vec!["V", "V"]),
        ] {
            let mut meter = Meter::initialize(Fake::default(), config()).unwrap();
            meter.io = meter.io.with(command, &replies);
            assert!(meter.read().result.is_err(), "{command}");
        }
        let mut meter = Meter::initialize(Fake::default(), config()).unwrap();
        meter.io = meter
            .io
            .with("SYST:SENS:IDN?", &["S121C,REPLACED,today,1,1,33"]);
        assert!(meter.read().result.unwrap_err().contains("sensor changed"));
    }
    #[test]
    fn sensor_range_readback_and_positive_settings_are_enforced() {
        for factor in [0., -1., f64::INFINITY, f64::NAN] {
            assert!(
                Config {
                    correction_factor: factor,
                    ..config()
                }
                .validate(true)
                .is_err()
            );
        }
        assert!(Config::default().validate(true).is_ok());
        assert!(Config::default().validate(false).is_err());
        let fake = Fake::default();
        let writes = fake.writes.clone();
        assert!(
            Meter::initialize(
                fake,
                Config {
                    wavelength_nm: 1200.,
                    ..config()
                }
            )
            .is_err()
        );
        assert_eq!(*writes.lock().unwrap(), vec!["CONF:POW"]);
        let fake = Fake::default().with("SENS:CORR:WAV?", &["799.9"]);
        assert!(Meter::initialize(fake, config()).is_err());
        let fake = Fake::default().with("SYST:SENS:IDN?", &["None,0,none,0,0,0"]);
        assert!(Meter::initialize(fake, config()).is_err());
        assert_eq!(
            sensor_fields("S120C,S1,\"Calibrated, 2026\",1,1,33")
                .unwrap()
                .len(),
            6
        );
    }
    #[test]
    fn negative_noise_is_preserved_and_nonfinite_conversion_rejected() {
        assert_eq!(watts(-1e-12, "W").unwrap().0, -1e-12);
        assert!(watts(10000., "DBM").is_err());
        assert!(watts(-10000., "DBM").is_err());
        let mut tiny = Meter::initialize(
            Fake::default(),
            Config {
                correction_factor: f64::MIN_POSITIVE,
                ..config()
            },
        )
        .unwrap();
        tiny.io = tiny.io.with("READ?", &["1e-30"]);
        assert!(tiny.read().result.unwrap_err().contains("underflowed"));
        let mut meter = Meter::initialize(
            Fake::default(),
            Config {
                correction_factor: f64::MAX,
                ..config()
            },
        )
        .unwrap();
        meter.io = meter.io.with("READ?", &["2"]);
        assert!(meter.read().result.is_err());
    }
    #[test]
    fn monitor_runs_independently_and_point_reads_are_fresh() {
        let live = Arc::new(AtomicUsize::new(0));
        let observed = live.clone();
        let controller = Controller::start(
            "power-1".into(),
            config(),
            move |state| {
                if state.reading.is_some() {
                    observed.fetch_add(1, Ordering::AcqRel);
                }
            },
            |config| Meter::initialize(Fake::default(), config),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while live.load(Ordering::Acquire) < 2 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(live.load(Ordering::Acquire) >= 2);
        let prior = controller.status().reading.unwrap().native_value;
        let sample = controller.read(&AtomicBool::new(false)).result.unwrap();
        assert!(sample.native_value > prior);
        assert_eq!(sample.corrected_w, sample.native_value * 2.5);
        assert!(
            controller
                .read(&AtomicBool::new(true))
                .result
                .unwrap_err()
                .contains("cancelled")
        );
        assert!(controller.status().connected);
    }
}
