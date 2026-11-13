//! Read-only lock-in amplifier protocols used by the desktop application.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use trkr_core::{DisplayMode, Signal};

mod reference;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Model {
    Sr830,
    Sr7265,
    Sr5210,
    Li5640,
}

/// Static model capabilities. Protocol-specific reads stay in their drivers;
/// settings/UI and auto actions share this one definition.
pub struct ModelDescriptor {
    pub name: &'static str,
    termination: &'static str,
    auto_phase: &'static str,
    auto_sensitivity: &'static str,
    auto_measure: &'static str,
    coupling: &'static [&'static str],
    slope: &'static [i32],
    ac_gain: &'static [f64],
}
const MODELS: [ModelDescriptor; 4] = [
    ModelDescriptor {
        name: "SR830",
        termination: "\n",
        auto_phase: "APHS",
        auto_sensitivity: "AGAN",
        auto_measure: "AGAN",
        coupling: &["AC", "DC"],
        slope: &[6, 12, 18, 24],
        ac_gain: &[],
    },
    ModelDescriptor {
        name: "SR7265",
        termination: "\r",
        auto_phase: "AQN",
        auto_sensitivity: "AS",
        auto_measure: "ASM",
        coupling: &["AC", "DC"],
        slope: &[6, 12, 18, 24],
        ac_gain: &AC_GAIN,
    },
    ModelDescriptor {
        name: "SR5210",
        termination: "\r",
        auto_phase: "AQN",
        auto_sensitivity: "AS",
        auto_measure: "ASM",
        coupling: &["AC"],
        slope: &[6, 12],
        ac_gain: &[],
    },
    ModelDescriptor {
        name: "LI5640",
        termination: "\n",
        auto_phase: "APHS",
        auto_sensitivity: "ASEN",
        auto_measure: "ASEN",
        coupling: &["AC", "DC"],
        slope: &[6, 12, 18, 24],
        ac_gain: &[],
    },
];
impl ModelDescriptor {
    /// Signal-channel AC gain (dB) exists only on the SR7265. It is a
    /// different control from the output filter slope (dB/octave).
    pub const fn has_ac_gain(&self) -> bool {
        !self.ac_gain.is_empty()
    }
    fn available(&self, sensitivity: &[f64], time_constant: &[f64]) -> Value {
        json!({"sensitivity": sensitivity, "time_constant": time_constant, "coupling": self.coupling, "slope": self.slope, "ac_gain": self.ac_gain})
    }
}
impl Model {
    pub const ALL: [Self; 4] = [Self::Sr830, Self::Sr7265, Self::Sr5210, Self::Li5640];
    pub const fn descriptor(self) -> &'static ModelDescriptor {
        &MODELS[match self {
            Self::Sr830 => 0,
            Self::Sr7265 => 1,
            Self::Sr5210 => 2,
            Self::Li5640 => 3,
        }]
    }
    pub const fn name(self) -> &'static str {
        self.descriptor().name
    }
}

pub trait Io {
    fn query(&mut self, command: &str) -> Result<String, String>;
    fn write(&mut self, command: &str) -> Result<(), String>;
    fn serial_poll(&mut self) -> Result<Option<u8>, String> {
        Ok(None)
    }
    fn invalidate_serial_session(&mut self) {}
}

/// One command exchanged while acquiring a lock-in sample.
///
/// Query replies are recorded exactly as supplied by the transport, before any
/// trimming or numeric parsing.  This makes the saved acquisition provenance
/// independent of the display precision used by the application.
#[derive(Debug, Serialize)]
pub struct Exchange {
    pub operation: &'static str,
    pub command: String,
    pub reply: Option<String>,
    pub error: Option<String>,
}

/// A lock-in read and the transport exchanges that produced it.
///
/// `result` retains the exact behavior of [`read`].  Exchanges remain
/// available when a transport or parsing error prevents a signal from being
/// produced.
pub struct RecordedOperation<T> {
    pub result: Result<T, String>,
    pub exchanges: Vec<Exchange>,
}

/// Run [`read`] while retaining the exact commands, replies, and errors used
/// by that read.  The recording adapter only observes the existing `Io`
/// calls, so it never performs additional instrument I/O.
pub type RecordedRead = RecordedOperation<Signal>;
#[cfg(test)]
pub fn read_recorded(model: Model, io: &mut impl Io) -> RecordedRead {
    recorded(io, |io| read(model, io))
}
#[cfg(test)]
pub fn settings_recorded(model: Model, io: &mut impl Io) -> RecordedOperation<Value> {
    recorded(io, |io| settings_impl(io, model))
}

pub fn status_recorded(model: Model, io: &mut impl Io) -> RecordedOperation<Value> {
    recorded(io, |io| {
        let mut value = match model {
            Model::Sr830 => overload_sr830(io)?,
            Model::Sr7265 => overload_sr7265(io)?,
            Model::Sr5210 => overload_sr5210(io)?,
            Model::Li5640 => overload_li5640(io)?,
        };
        if matches!(model, Model::Sr830 | Model::Sr7265) {
            let command = if model == Model::Sr830 {
                "ISRC?\n"
            } else {
                "IMODE\r"
            };
            value["input_mode"] = json!(query_value(io, command)?);
        }
        if model == Model::Li5640 {
            value.as_object_mut().expect("status object").extend(
                li5640_unit_context(io)?
                    .as_object()
                    .expect("unit context")
                    .clone(),
            );
        }
        Ok(value)
    })
}

/// Unit qualification observes the device, never a profile assumption.
pub fn measured_unit(model: Model, status: Option<&Value>) -> &'static str {
    let Some(status) = status else {
        return "unknown";
    };
    match model {
        Model::Sr830 => match status["input_mode"].as_f64() {
            Some(0. | 1.) => "V",
            Some(2. | 3.) => "A",
            _ => "unknown",
        },
        Model::Sr7265 if status["input_mode"].as_f64() == Some(0.) => "V",
        Model::Sr5210 => match sr5210_input_mode(status) {
            Some(0) => "V",
            Some(2 | 4) => "A",
            _ => "unknown",
        },
        Model::Li5640 => match status["signal_unit"].as_str() {
            Some("V") => "V",
            Some("A") => "A",
            _ => "unknown",
        },
        _ => "unknown",
    }
}
pub fn sr5210_input_mode(status: &Value) -> Option<u8> {
    let byte = u8::try_from(status["overload_byte"].as_u64()?).ok()?;
    Some(byte & 6)
}

fn sr5210_input_scale(mode: u8) -> Option<f64> {
    match mode {
        0 => Some(1.0),
        2 => Some(1e-8),
        4 => Some(1e-6),
        _ => None,
    }
}

fn li5640_unit_context(io: &mut impl Io) -> Result<Value, String> {
    let input = integer(query_value(io, "ISRC?\n")?, "ISRC?")?;
    let normalization = integer(query_value(io, "NORM?\n")?, "NORM?")?;
    let ratio = integer(query_value(io, "RAT?\n")?, "RAT?")?;
    let input_unit = match input {
        0 | 1 => "V",
        2 | 3 => "A",
        _ => "unknown",
    };
    let unit = if normalization != 0 || ratio != 0 {
        "unknown"
    } else {
        input_unit
    };
    Ok(
        json!({"input_mode":input,"normalization_mode":normalization,"ratio_mode":ratio,
        "signal_unit":unit,"input_unit":input_unit,"unit_qualification":"queried_input_and_display_mode"}),
    )
}

struct Recording<'a, T> {
    io: &'a mut T,
    exchanges: Vec<Exchange>,
}

impl<T: Io> Io for Recording<'_, T> {
    fn query(&mut self, command: &str) -> Result<String, String> {
        match self.io.query(command) {
            Ok(reply) => {
                self.exchanges.push(Exchange {
                    operation: "query",
                    command: command.into(),
                    reply: Some(reply.clone()),
                    error: None,
                });
                Ok(reply)
            }
            Err(error) => {
                self.exchanges.push(Exchange {
                    operation: "query",
                    command: command.into(),
                    reply: None,
                    error: Some(error.clone()),
                });
                Err(error)
            }
        }
    }

    fn write(&mut self, command: &str) -> Result<(), String> {
        match self.io.write(command) {
            Ok(()) => {
                self.exchanges.push(Exchange {
                    operation: "write",
                    command: command.into(),
                    reply: None,
                    error: None,
                });
                Ok(())
            }
            Err(error) => {
                self.exchanges.push(Exchange {
                    operation: "write",
                    command: command.into(),
                    reply: None,
                    error: Some(error.clone()),
                });
                Err(error)
            }
        }
    }
}

fn recorded<T: Io, R>(
    io: &mut T,
    operation: impl FnOnce(&mut Recording<'_, T>) -> Result<R, String>,
) -> RecordedOperation<R> {
    let mut recording = Recording {
        io,
        exchanges: Vec::new(),
    };
    let result = operation(&mut recording);
    RecordedOperation {
        result,
        exchanges: recording.exchanges,
    }
}

impl Io for trkr_transport::Gpib {
    fn query(&mut self, command: &str) -> Result<String, String> {
        trkr_transport::Gpib::query(self, command)
    }

    fn write(&mut self, command: &str) -> Result<(), String> {
        trkr_transport::Gpib::write(self, command)
    }
    fn serial_poll(&mut self) -> Result<Option<u8>, String> {
        trkr_transport::Gpib::serial_poll(self)
    }
    fn invalidate_serial_session(&mut self) {
        trkr_transport::Gpib::invalidate(self);
    }
}

pub trait CancellableIo: Io {
    fn invalidate(&mut self);
    fn query_cancellable(
        &mut self,
        command: &str,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<String, String>;
    fn write_cancellable(
        &mut self,
        command: &str,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<(), String>;
}
impl CancellableIo for trkr_transport::Gpib {
    fn invalidate(&mut self) {
        trkr_transport::Gpib::invalidate(self);
    }
    fn query_cancellable(
        &mut self,
        command: &str,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<String, String> {
        trkr_transport::Gpib::query_cancellable(self, command, cancel)
    }
    fn write_cancellable(
        &mut self,
        command: &str,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<(), String> {
        trkr_transport::Gpib::write_cancellable(self, command, cancel)
    }
}

struct Cancellable<'a, T> {
    io: &'a mut T,
    cancel: &'a std::sync::atomic::AtomicBool,
}
impl<T: CancellableIo> Io for Cancellable<'_, T> {
    fn query(&mut self, command: &str) -> Result<String, String> {
        self.io.query_cancellable(command, self.cancel)
    }
    fn write(&mut self, command: &str) -> Result<(), String> {
        self.io.write_cancellable(command, self.cancel)
    }
}

/// Display reads during motion/settling observe Stop even during a helper
/// response. LI5640 acquisition itself never changes the selected displays.
#[cfg(test)]
pub fn read_cancellable(
    model: Model,
    io: &mut impl CancellableIo,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Signal, String> {
    read_selected_cancellable(model, io, cancel, DisplayMode::Xy)
}
pub fn read_selected_cancellable(
    model: Model,
    io: &mut impl CancellableIo,
    cancel: &std::sync::atomic::AtomicBool,
    mode: DisplayMode,
) -> Result<Signal, String> {
    read_selected(model, &mut Cancellable { io, cancel }, mode)
}

/// Recorded acquisition queries observe Stop without losing raw evidence.
#[cfg(test)]
pub fn read_recorded_cancellable(
    model: Model,
    io: &mut impl CancellableIo,
    cancel: &std::sync::atomic::AtomicBool,
) -> RecordedRead {
    read_selected_recorded_cancellable(model, io, cancel, DisplayMode::Xy)
}
pub fn read_selected_recorded_cancellable(
    model: Model,
    io: &mut impl CancellableIo,
    cancel: &std::sync::atomic::AtomicBool,
    mode: DisplayMode,
) -> RecordedRead {
    recorded(&mut Cancellable { io, cancel }, |io| {
        read_selected(model, io, mode)
    })
}
pub fn li5640_unit_recorded_cancellable(
    io: &mut impl CancellableIo,
    cancel: &std::sync::atomic::AtomicBool,
) -> RecordedOperation<Value> {
    recorded(&mut Cancellable { io, cancel }, |io| {
        li5640_unit_context(io)
    })
}
pub fn same_li5640_unit_context(before: &Value, after: &Value) -> bool {
    ["input_mode", "normalization_mode", "ratio_mode"]
        .into_iter()
        .all(|key| before.get(key).is_some() && before.get(key) == after.get(key))
}

pub fn status_recorded_cancellable(
    model: Model,
    io: &mut impl CancellableIo,
    cancel: &std::sync::atomic::AtomicBool,
) -> RecordedOperation<Value> {
    status_recorded(model, &mut Cancellable { io, cancel })
}
pub fn run_settings_recorded_cancellable(
    model: Model,
    io: &mut impl CancellableIo,
    cancel: &std::sync::atomic::AtomicBool,
) -> RecordedOperation<Value> {
    recorded(&mut Cancellable { io, cancel }, |io| {
        let mut settings = settings_impl(io, model)?;
        settings["reference"] = reference::read(io, model)?;
        Ok(settings)
    })
}

#[cfg(test)]
pub fn read(model: Model, io: &mut impl Io) -> Result<Signal, String> {
    read_selected(model, io, DisplayMode::Xy)
}
pub fn read_selected(model: Model, io: &mut impl Io, mode: DisplayMode) -> Result<Signal, String> {
    match model {
        Model::Sr830 => read_sr830(io),
        Model::Sr7265 => read_sr7265(io),
        Model::Sr5210 => read_sr5210(io),
        Model::Li5640 => read_li5640(io, mode),
    }
}

// LI5640 manual §4.6: 50 fA–1 µA at 10^6 V/A; 5 fA–10 nA at 10^8 V/A.
fn li5640_sensitivity_table(input: i32) -> Result<(&'static [f64], i32, &'static str), String> {
    match input {
        0 | 1 => Ok((&SR_SENS, 0, "VSEN")),
        2 => Ok((&LI_CURRENT_SENS[3..], 4, "ISEN")),
        3 => Ok((&LI_CURRENT_SENS[..20], 1, "ISEN")),
        _ => Err("Unsupported LI5640 input mode".into()),
    }
}
const LI_CURRENT_SENS: [f64; 26] = [
    5e-15, 1e-14, 2e-14, 5e-14, 1e-13, 2e-13, 5e-13, 1e-12, 2e-12, 5e-12, 1e-11, 2e-11, 5e-11,
    1e-10, 2e-10, 5e-10, 1e-9, 2e-9, 5e-9, 1e-8, 2e-8, 5e-8, 1e-7, 2e-7, 5e-7, 1e-6,
];
const SR_SENS: [f64; 27] = [
    2e-9, 5e-9, 10e-9, 20e-9, 50e-9, 100e-9, 200e-9, 500e-9, 1e-6, 2e-6, 5e-6, 10e-6, 20e-6, 50e-6,
    100e-6, 200e-6, 500e-6, 1e-3, 2e-3, 5e-3, 10e-3, 20e-3, 50e-3, 100e-3, 200e-3, 500e-3, 1.0,
];
const SR_TC: [f64; 20] = [
    10e-6, 30e-6, 100e-6, 300e-6, 1e-3, 3e-3, 10e-3, 30e-3, 100e-3, 300e-3, 1.0, 3.0, 10.0, 30.0,
    100.0, 300.0, 1e3, 3e3, 10e3, 30e3,
];
const S7265_TC: [f64; 30] = [
    10e-6, 20e-6, 40e-6, 80e-6, 160e-6, 320e-6, 640e-6, 5e-3, 10e-3, 20e-3, 50e-3, 100e-3, 200e-3,
    500e-3, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1e3, 2e3, 5e3, 10e3, 20e3, 50e3,
    100e3,
];
const S5210_SENS: [f64; 16] = [
    100e-9, 300e-9, 1e-6, 3e-6, 10e-6, 30e-6, 100e-6, 300e-6, 1e-3, 3e-3, 10e-3, 30e-3, 100e-3,
    300e-3, 1.0, 3.0,
];
const S5210_TC: [f64; 14] = [
    1e-3, 3e-3, 10e-3, 30e-3, 100e-3, 300e-3, 1.0, 3.0, 10.0, 30.0, 100.0, 300.0, 1e3, 3e3,
];
const AC_GAIN: [f64; 10] = [0.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0];

/// Snapshot the controls exposed by the selected lock-in.  Fields unsupported
/// by a model are represented by JSON null, while their available list is
/// empty.  This lets the GUI share one settings panel across all models.
pub fn settings(io: &mut impl Io, model: Model) -> Result<Value, String> {
    settings_impl(io, model)
}
fn settings_impl(io: &mut impl Io, model: Model) -> Result<Value, String> {
    let mut input_mode = Value::Null;
    let li_context = if model == Model::Li5640 {
        Some(li5640_unit_context(io)?)
    } else {
        None
    };
    let (sensitivity, time_constant, ref_freq, coupling, slope, ac_gain, overload, available) =
        match model {
            Model::Sr830 => {
                // ISRC 0/1 are voltage inputs (A, A-B); 2/3 are current
                // inputs.  The volt table is only qualified for voltage input;
                // current-input sensitivity is reported as unqualified rather
                // than converted with an unverified table.
                let input = integer(query_value(io, "ISRC?\n")?, "ISRC?")?;
                input_mode = json!(input);
                let volts = matches!(input, 0 | 1);
                let index = integer(query_value(io, "SENS?\n")?, "SENS?\n")?;
                let sensitivity = if volts {
                    json!(table_value(index, &SR_SENS, 0, "SENS?\n")?)
                } else {
                    Value::Null
                };
                (
                    sensitivity,
                    indexed(io, "OFLT?\n", &SR_TC)?,
                    Some(query_value(io, "FREQ?\n")?),
                    coupling(io, "ICPL?\n")?,
                    slope4(io, "OFSL?\n")?,
                    Value::Null,
                    overload_sr830(io)?,
                    model
                        .descriptor()
                        .available(if volts { &SR_SENS[..] } else { &[][..] }, &SR_TC),
                )
            }
            Model::Sr7265 => {
                let imode = integer(query_value(io, "IMODE\r")?, "SR7265 IMODE")?;
                let sensitivity_table = sr7265_sensitivity(imode)?;
                input_mode = json!(imode);
                (
                    json!(query_value(io, "SEN.\r")?),
                    query_value(io, "TC.\r")?,
                    Some(query_value(io, "FRQ.\r")?),
                    coupling(io, "CP\r")?,
                    slope4(io, "SLOPE\r")?,
                    json!(query_value(io, "ACGAIN\r")? * 10.0),
                    overload_sr7265(io)?,
                    model.descriptor().available(sensitivity_table, &S7265_TC),
                )
            }
            Model::Sr5210 => (
                json!(indexed(io, "SEN\r", &S5210_SENS)?),
                indexed(io, "TC\r", &S5210_TC)?,
                Some(query_value(io, "FRQ\r")? / 1000.0),
                Value::String("AC".into()),
                slope2(io, "XDB\r")?,
                Value::Null,
                overload_sr5210(io)?,
                model.descriptor().available(&S5210_SENS, &S5210_TC),
            ),
            Model::Li5640 => {
                let context = li_context.as_ref().expect("LI5640 context");
                let input = integer(
                    context["input_mode"]
                        .as_f64()
                        .ok_or("Missing LI5640 input mode")?,
                    "ISRC?",
                )?;
                let (table, base, command) = li5640_sensitivity_table(input)?;
                let sensitivity = table_value(
                    integer(
                        query_value(io, &format!("{command}?\n"))?,
                        "LI5640 sensitivity",
                    )?,
                    table,
                    base,
                    "LI5640 sensitivity",
                )?;
                (
                    json!(sensitivity),
                    indexed(io, "TCON?\n", &SR_TC)?,
                    li5640_reference_frequency(io)?,
                    coupling(io, "ICPL?\n")?,
                    slope4(io, "SLOP?\n")?,
                    Value::Null,
                    overload_li5640(io)?,
                    model.descriptor().available(table, &SR_TC),
                )
            }
        };
    let sr5210_sensitivity = sensitivity.as_f64().unwrap_or_default();
    let (mut sensitivity, mut available) = (sensitivity, available);
    if model == Model::Sr5210 {
        // The SR5210 `N` status byte reports current-input conversion
        // (bits 1/2).  Its volt table is not qualified for current input, so
        // the value is withheld and sensitivity changes are disabled.
        let current = overload["overload_byte"].as_i64().unwrap_or(0) & 6;
        input_mode = json!(current);
        if current != 0 {
            sensitivity = Value::Null;
            available["sensitivity"] = json!([]);
        }
    }
    let resolution = if model == Model::Sr5210 {
        sr5210_input_mode(&overload)
            .and_then(sr5210_input_scale)
            .map(|scale| sr5210_sensitivity * scale / 10_000.0)
    } else {
        None
    };
    let mut value = json!({"Sensitivity": sensitivity, "Time Constant": time_constant, "Ref. Freq": ref_freq, "Coupling": coupling, "Slope": slope, "AC Gain": ac_gain, "Overload": overload, "input_mode":input_mode, "available": available});
    if model == Model::Sr5210 {
        value["Signal Resolution"] = json!(resolution);
    }
    if let Some(context) = li_context {
        value
            .as_object_mut()
            .expect("settings object")
            .extend(context.as_object().expect("unit context").clone());
    }
    Ok(value)
}

pub fn set_setting(
    io: &mut impl Io,
    model: Model,
    setting: &str,
    value: &Value,
) -> Result<(), String> {
    let descriptor = model.descriptor();
    match (model, setting) {
        (Model::Sr830, "Sensitivity") => {
            let input = integer(query_value(io, "ISRC?\n")?, "ISRC?")?;
            if !matches!(input, 0 | 1) {
                return Err(format!(
                    "SR830 sensitivity changes are supported only for voltage input; ISRC? reports {input}"
                ));
            }
            write_index(io, "SENS", value, &SR_SENS, descriptor.termination)
        }
        (Model::Sr830, "Time Constant") => {
            write_index(io, "OFLT", value, &SR_TC, descriptor.termination)
        }
        (Model::Sr830, "Coupling") => write_coupling(
            io,
            "ICPL",
            value,
            descriptor.coupling,
            descriptor.termination,
        ),
        (Model::Sr830, "Slope") => {
            write_slope(io, "OFSL", value, descriptor.slope, descriptor.termination)
        }
        (Model::Sr7265, "Sensitivity") => {
            let mode = integer(query_value(io, "IMODE\r")?, "SR7265 IMODE")?;
            write_index_base(
                io,
                "SEN",
                value,
                sr7265_sensitivity(mode)?,
                if mode == 2 { 7 } else { 1 },
                descriptor.termination,
            )
        }
        (Model::Sr7265, "Time Constant") => {
            write_index(io, "TC", value, &S7265_TC, descriptor.termination)
        }
        (Model::Sr7265, "Coupling") => {
            write_coupling(io, "CP", value, descriptor.coupling, descriptor.termination)
        }
        (Model::Sr7265, "Slope") => {
            write_slope(io, "SLOPE", value, descriptor.slope, descriptor.termination)
        }
        (Model::Sr7265, "AC Gain") => write_index(
            io,
            "ACGAIN",
            value,
            descriptor.ac_gain,
            descriptor.termination,
        ),
        (Model::Sr5210, "Sensitivity") => {
            write_index(io, "SEN", value, &S5210_SENS, descriptor.termination)
        }
        (Model::Sr5210, "Time Constant") => {
            write_index(io, "TC", value, &S5210_TC, descriptor.termination)
        }
        (Model::Sr5210, "Coupling")
            if value.as_str().is_some_and(|v| {
                descriptor
                    .coupling
                    .iter()
                    .any(|choice| v.trim().eq_ignore_ascii_case(choice))
            }) =>
        {
            Ok(())
        }
        (Model::Sr5210, "Slope") => {
            write_slope(io, "XDB", value, descriptor.slope, descriptor.termination)
        }
        (Model::Li5640, "Sensitivity") => {
            let input = integer(query_value(io, "ISRC?\n")?, "ISRC?")?;
            let (table, base, command) = li5640_sensitivity_table(input)?;
            write_index_base(
                io,
                command,
                value,
                table,
                base as usize,
                descriptor.termination,
            )
        }
        (Model::Li5640, "Time Constant") => {
            write_index(io, "TCON", value, &SR_TC, descriptor.termination)
        }
        (Model::Li5640, "Coupling") => write_coupling(
            io,
            "ICPL",
            value,
            descriptor.coupling,
            descriptor.termination,
        ),
        (Model::Li5640, "Slope") => {
            write_slope(io, "SLOP", value, descriptor.slope, descriptor.termination)
        }
        _ => Err(format!(
            "{} does not support setting {setting:?}",
            model.name()
        )),
    }
}

/// Canonical setting key accepted by [`set_setting`] and its `available` list.
pub fn setting_key(setting: &str) -> Option<(&'static str, &'static str)> {
    Some(match setting {
        "sensitivity" | "Sensitivity" => ("Sensitivity", "sensitivity"),
        "time_constant" | "Time Constant" => ("Time Constant", "time_constant"),
        "coupling" | "Coupling" => ("Coupling", "coupling"),
        // Output (post-PSD) low-pass filter roll-off in dB/octave.
        "slope" | "Slope" => ("Slope", "slope"),
        // SR7265 signal-channel AC gain in dB; unrelated to the filter slope.
        "ac_gain" | "AC Gain" => ("AC Gain", "ac_gain"),
        _ => return None,
    })
}

fn same_setting(requested: &Value, actual: &Value) -> bool {
    match (requested, actual) {
        (Value::String(a), Value::String(b)) => a.trim().eq_ignore_ascii_case(b.trim()),
        _ => match (requested.as_f64(), actual.as_f64()) {
            // Readbacks are either table values or the instrument's own
            // floating-point formatting (SR7265 `SEN.`/`TC.`); adjacent
            // table entries differ by at least 25 %.
            (Some(a), Some(b)) if a.is_finite() && b.is_finite() => {
                (a - b).abs() <= a.abs().max(b.abs()).max(1e-30) * 1e-3
            }
            _ => false,
        },
    }
}

/// Change one setting and confirm it from the instrument.
///
/// The current settings are read first so the requested value is validated
/// against the table that applies to the *observed* input mode (SR7265
/// `IMODE`, LI5640 `ISRC?`, SR830 `ISRC?`) and model (AC gain exists only on
/// the SR7265).  After the write the settings are read again; the change is
/// accepted only when the input mode is unchanged and the readback equals the
/// request.  The verified readback is returned so callers never display a
/// requested value the instrument did not adopt.
pub fn apply_setting(
    io: &mut impl Io,
    model: Model,
    setting: &str,
    value: &Value,
) -> Result<Value, String> {
    let (key, available_key) = setting_key(setting)
        .ok_or_else(|| format!("{} does not support setting {setting:?}", model.name()))?;
    if key == "AC Gain" && model.descriptor().ac_gain.is_empty() {
        return Err(format!(
            "{} has no AC gain setting (filter slope is a separate dB/oct setting)",
            model.name()
        ));
    }
    let before = settings_impl(io, model)?;
    let allowed = before["available"][available_key]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    if allowed.is_empty() {
        return Err(format!(
            "{} {key} cannot be changed in the current instrument state (input mode {})",
            model.name(),
            before["input_mode"]
        ));
    }
    if !allowed.iter().any(|choice| same_setting(value, choice)) {
        return Err(format!(
            "Unsupported {} {key} for the current input mode: {value}",
            model.name()
        ));
    }
    set_setting(io, model, key, value)?;
    if model == Model::Sr5210 {
        wait_sr5210_ready(io, false)?;
    }
    let after = settings_impl(io, model)?;
    if after["input_mode"] != before["input_mode"] {
        return Err(format!(
            "{} input mode changed during the {key} change ({} -> {}); verify the instrument",
            model.name(),
            before["input_mode"],
            after["input_mode"]
        ));
    }
    if !same_setting(value, &after[key]) {
        return Err(format!(
            "{} {key} readback {} does not match the requested {value}",
            model.name(),
            after[key]
        ));
    }
    Ok(after)
}

pub fn auto(io: &mut impl Io, model: Model, action: &str) -> Result<(), String> {
    let descriptor = model.descriptor();
    let command = match action {
        "phase" => descriptor.auto_phase,
        "sensitivity" => descriptor.auto_sensitivity,
        "measure" => descriptor.auto_measure,
        _ => return Err(format!("Unknown {} auto action: {action}", model.name())),
    };
    if model == Model::Sr830 && action == "phase" {
        return auto_phase_sr830(io);
    }
    if model == Model::Sr5210 {
        if action == "phase" && query_value(io, "FRQ\r")? <= 0.0 {
            return Err("SR5210 Auto Phase requires a locked reference".into());
        }
        let status = io
            .serial_poll()
            .map_err(|error| {
                format!(
                    "SR5210 automatic settings require a serial-poll-capable VISA helper: {error}"
                )
            })?
            .ok_or("SR5210 automatic settings require a serial-poll-capable VISA helper")?;
        if status & 0x27 != 1 || (action == "phase" && status & 8 != 0) {
            return Err(format!(
                "SR5210 is not ready for automatic adjustment (GPIB status 0x{status:02x})"
            ));
        }
    }
    io.write(&format!("{command}{}", descriptor.termination))
        .map_err(|error| format!("transport: {error}"))?;
    if model == Model::Sr5210 {
        wait_sr5210_ready(io, true)?;
    }
    Ok(())
}

// SR830 manual pp. 4-23 and 5-11: APHS can leave the phase shift
// unchanged if the instrument rejects an unstable phase.
fn auto_phase_sr830(io: &mut impl Io) -> Result<(), String> {
    let status = overload_sr830(io)?;
    if status["reference_unlock"] == true || status["overload"] == true {
        return Err("SR830 Auto Phase requires a locked reference and no overload".into());
    }
    let before = query_value(io, "PHAS?\n")?;
    io.write("APHS\n")
        .map_err(|error| format!("transport: {error}"))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        match io
            .serial_poll()
            .map_err(|error| format!("transport: {error}"))?
        {
            Some(status) if status & 2 == 0 => {
                if std::time::Instant::now() >= deadline {
                    io.invalidate_serial_session();
                    return Err("transport: SR830 Auto Phase completion timed out".into());
                }
            }
            _ => break,
        }
    }
    let after = query_value(io, "PHAS?\n")?;
    if ((after - before + 180.).rem_euclid(360.) - 180.).abs() < 0.005 {
        return Err("SR830 Auto Phase could not be confirmed: phase shift unchanged. Check for an unstable or weak signal (PhAS bAd).".into());
    }
    Ok(())
}

// Model 5210 manual §6.3.12: bit 0 is command complete and bit 5 can
// indicate an auto function in progress. ST cannot replace a serial poll.
fn wait_sr5210_ready(io: &mut impl Io, automatic: bool) -> Result<(), String> {
    use std::time::{Duration, Instant};

    let deadline = Instant::now()
        + if automatic {
            Duration::from_secs(45)
        } else {
            Duration::from_secs(2)
        };
    let interval = if automatic { 50 } else { 10 };
    let mut observed_execution = false;
    loop {
        std::thread::sleep(Duration::from_millis(interval));
        let Some(status) = io
            .serial_poll()
            .map_err(|error| format!("transport: {error}"))?
        else {
            if automatic {
                io.invalidate_serial_session();
                return Err("transport: SR5210 serial polling became unavailable".into());
            }
            return Ok(());
        };
        if status & 6 != 0 {
            return Err(format!(
                "SR5210 rejected the command (GPIB status 0x{status:02x})"
            ));
        }
        if automatic && status & 8 != 0 {
            io.invalidate_serial_session();
            return Err("SR5210 reference unlocked during the automatic operation".into());
        }
        let ready = status & 0x21 == 1;
        if ready && (!automatic || observed_execution) {
            return Ok(());
        }
        observed_execution |= !ready;
        if Instant::now() >= deadline {
            io.invalidate_serial_session();
            return Err("transport: SR5210 did not finish the command before the deadline".into());
        }
    }
}

const LI5640_OUTPUT: [f64; 3] = [1., 2., 3.];

const S7265_CURRENT_1: [f64; 27] = [
    2e-15, 5e-15, 10e-15, 20e-15, 50e-15, 100e-15, 200e-15, 500e-15, 1e-12, 2e-12, 5e-12, 10e-12,
    20e-12, 50e-12, 100e-12, 200e-12, 500e-12, 1e-9, 2e-9, 5e-9, 10e-9, 20e-9, 50e-9, 100e-9,
    200e-9, 500e-9, 1e-6,
];
const S7265_CURRENT_2: [f64; 21] = [
    2e-15, 5e-15, 10e-15, 20e-15, 50e-15, 100e-15, 200e-15, 500e-15, 1e-12, 2e-12, 5e-12, 10e-12,
    20e-12, 50e-12, 100e-12, 200e-12, 500e-12, 1e-9, 2e-9, 5e-9, 10e-9,
];

fn indexed(io: &mut impl Io, command: &str, table: &[f64]) -> Result<f64, String> {
    table_value(
        integer(query_value(io, command)?, command)?,
        table,
        0,
        command,
    )
}
fn table_value(index: i32, table: &[f64], base: i32, label: &str) -> Result<f64, String> {
    index
        .checked_sub(base)
        .and_then(|i| table.get(i as usize))
        .copied()
        .ok_or_else(|| format!("Unexpected {label} index: {index}"))
}
fn sr7265_sensitivity(mode: i32) -> Result<&'static [f64], String> {
    match mode {
        0 => Ok(&SR_SENS),
        1 => Ok(&S7265_CURRENT_1),
        2 => Ok(&S7265_CURRENT_2),
        _ => Err(format!("Unsupported SR7265 IMODE for sensitivity: {mode}")),
    }
}
fn coupling(io: &mut impl Io, command: &str) -> Result<Value, String> {
    match integer(query_value(io, command)?, command)? {
        0 => Ok(Value::String("AC".into())),
        1 => Ok(Value::String("DC".into())),
        value => Err(format!("Unexpected coupling mode: {value}")),
    }
}
fn slope4(io: &mut impl Io, command: &str) -> Result<f64, String> {
    table_value(
        integer(query_value(io, command)?, command)?,
        &[6.0, 12.0, 18.0, 24.0],
        0,
        "slope",
    )
}
fn slope2(io: &mut impl Io, command: &str) -> Result<f64, String> {
    table_value(
        integer(query_value(io, command)?, command)?,
        &[6.0, 12.0],
        0,
        "slope",
    )
}
fn write_index(
    io: &mut impl Io,
    command: &str,
    value: &Value,
    table: &[f64],
    term: &str,
) -> Result<(), String> {
    write_index_base(io, command, value, table, 0, term)
}
fn write_index_base(
    io: &mut impl Io,
    command: &str,
    value: &Value,
    table: &[f64],
    base: usize,
    term: &str,
) -> Result<(), String> {
    let value = value
        .as_f64()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("{command} requires a finite numeric value"))?;
    let index = table
        .iter()
        .position(|candidate| (candidate - value).abs() <= candidate.abs().max(1e-15) * 1e-9)
        .ok_or_else(|| format!("Unsupported {command} value: {value}"))?;
    io.write(&format!("{command} {}{term}", index + base))
}
fn write_coupling(
    io: &mut impl Io,
    command: &str,
    value: &Value,
    choices: &[&str],
    term: &str,
) -> Result<(), String> {
    let index = value
        .as_str()
        .and_then(|value| {
            choices
                .iter()
                .position(|choice| value.trim().eq_ignore_ascii_case(choice))
        })
        .ok_or("Coupling must be AC or DC")?;
    io.write(&format!("{command} {index}{term}"))
}
fn write_slope(
    io: &mut impl Io,
    command: &str,
    value: &Value,
    choices: &[i32],
    term: &str,
) -> Result<(), String> {
    let value = value
        .as_i64()
        .ok_or_else(|| format!("{command} requires an integer slope"))? as i32;
    let index = choices
        .iter()
        .position(|choice| *choice == value)
        .ok_or_else(|| format!("Unsupported {command} slope: {value}"))?;
    io.write(&format!("{command} {index}{term}"))
}
fn overload_sr830(io: &mut impl Io) -> Result<Value, String> {
    let value = integer(query_value(io, "LIAS?\n")?, "LIAS?")?;
    Ok(
        json!({"overload": value & 1 != 0, "input_overload": value & 1 != 0, "filter_overload": value & 2 != 0, "reference_unlock": value & 8 != 0, "range_changed": value & 16 != 0, "time_constant_changed": value & 32 != 0, "data_storage_triggered": value & 64 != 0, "overload_byte": value}),
    )
}
fn overload_sr7265(io: &mut impl Io) -> Result<Value, String> {
    let value = integer(query_value(io, "N\r")?, "N")?;
    let overload = value & (2 | 4 | 8 | 16 | 64) != 0;
    Ok(
        json!({"overload": overload, "ch1_output_overload": value & 2 != 0, "ch2_output_overload": value & 4 != 0, "y_output_overload": value & 8 != 0, "x_output_overload": value & 16 != 0, "input_overload": value & 64 != 0, "reference_unlock": value & 128 != 0, "overload_byte": value}),
    )
}
fn overload_sr5210(io: &mut impl Io) -> Result<Value, String> {
    let value = integer(query_value(io, "N\r")?, "N")?;
    let overload = value & (8 | 16 | 32 | 64) != 0;
    Ok(
        json!({"overload": overload, "current_mode_1e8": value & 2 != 0, "current_mode_1e6": value & 4 != 0, "y_output_overload": value & 8 != 0, "x_output_overload": value & 16 != 0, "psd_overload": value & 32 != 0, "raw_input_overload": value & 64 != 0, "input_overload": overload, "reference_unlock": value & 128 != 0, "overload_byte": value}),
    )
}
fn overload_li5640(io: &mut impl Io) -> Result<Value, String> {
    let value = integer(query_value(io, "OVCR?\n")?, "OVCR?")?;
    let overload = value & 63 != 0;
    Ok(
        json!({"overload": overload, "input_overload": overload, "raw_input_overload": value & 1 != 0, "middle_stage_overload": value & 2 != 0, "data1_display_overload": value & 4 != 0, "data2_display_overload": value & 8 != 0, "data1_ratio_display_overload": value & 16 != 0, "data2_ratio_display_overload": value & 32 != 0, "overload_byte": value}),
    )
}
fn read_sr830(io: &mut impl Io) -> Result<Signal, String> {
    let values = query_values(io, "SNAP?1,2,3,4\n", 4)?;
    signal(values[0], values[1], values[2], values[3])
}

fn read_sr7265(io: &mut impl Io) -> Result<Signal, String> {
    // The SR7265 can queue XY. and MP. in one write.  The portable GPIB
    // transport deliberately exposes one response per query, so use the
    // Python driver's documented two-query fallback instead.
    let xy = query_values(io, "XY.\r", 2);
    if let Err(error) = &xy
        && is_transport_error(error)
    {
        return Err(error.clone());
    }
    let mp = query_values(io, "MP.\r", 2);
    match (xy, mp) {
        (Ok(xy), Ok(mp)) => signal(xy[0], xy[1], mp[0], mp[1]),
        (Err(error), _) | (_, Err(error)) if is_transport_error(&error) => Err(error),
        _ => signal(
            query_value(io, "X.\r")?,
            query_value(io, "Y.\r")?,
            query_value(io, "MAG.\r")?,
            query_value(io, "PHA.\r")?,
        ),
    }
}

fn read_sr5210(io: &mut impl Io) -> Result<Signal, String> {
    let sensitivity = indexed(io, "SEN\r", &S5210_SENS)?;
    let status = overload_sr5210(io)?;
    let input_scale = sr5210_input_mode(&status)
        .and_then(sr5210_input_scale)
        .ok_or("SR5210 reports an invalid input mode")?;
    let sensitivity = sensitivity * input_scale;
    let xy = query_values(io, "XY\r", 2);
    if let Err(error) = &xy
        && is_transport_error(error)
    {
        return Err(error.clone());
    }
    let mp = query_values(io, "MP\r", 2);
    let (x, y, r, theta_mdeg) = match (xy, mp) {
        (Ok(xy), Ok(mp)) => (xy[0], xy[1], mp[0], mp[1]),
        (Err(error), _) | (_, Err(error)) if is_transport_error(&error) => return Err(error),
        _ => (
            query_value(io, "X\r")?,
            query_value(io, "Y\r")?,
            query_value(io, "MAG\r")?,
            query_value(io, "PHA\r")?,
        ),
    };
    signal(
        scale_5210(x, sensitivity),
        scale_5210(y, sensitivity),
        scale_5210(r, sensitivity),
        theta_mdeg / 1000.0,
    )
}

fn sr830_displays(io: &mut impl Io) -> Result<[[i32; 2]; 2], String> {
    let mut displays = [[0; 2]; 2];
    for (index, display) in displays.iter_mut().enumerate() {
        let command = format!("DDEF? {}\n", index + 1);
        let reply = query_values(io, &command, 2)?;
        *display = [integer(reply[0], &command)?, integer(reply[1], &command)?];
        if !(0..=4).contains(&display[0]) || !(0..=2).contains(&display[1]) {
            return Err(format!("Invalid SR830 display settings: {display:?}"));
        }
    }
    Ok(displays)
}

fn select_sr830_display(io: &mut impl CancellableIo, mode: DisplayMode) -> Result<(), String> {
    let before = sr830_displays(io)?;
    let selected = if mode == DisplayMode::Xy { 0 } else { 1 };
    // Preserve each channel's ratio and independent analog output selection.
    let requested = before.map(|[_, ratio]| [selected, ratio]);
    if requested == before {
        return Ok(());
    }
    for (index, display) in before.iter().enumerate() {
        if display[0] != selected {
            let channel = index + 1;
            if query_value(io, &format!("FPOP? {channel}\n"))? != 1. {
                return Err(format!(
                    "SR830 CH{channel} output follows the display or could not be verified; display mode was not changed"
                ));
            }
        }
    }
    let mut apply = |from: [[i32; 2]; 2], target: [[i32; 2]; 2]| {
        for (index, ([old, _], [display, ratio])) in from.into_iter().zip(target).enumerate() {
            if old != display {
                io.write(&format!("DDEF {},{display},{ratio}\n", index + 1))?;
            }
        }
        if sr830_displays(io)? != target {
            return Err("SR830 display readback does not match requested mode".into());
        }
        Ok(())
    };
    if let Err(error) = apply(before, requested) {
        if let Err(restore) = apply(requested, before) {
            io.invalidate();
            return Err(format!(
                "{error}; SR830 display restoration failed: {restore}"
            ));
        }
        return Err(error);
    }
    Ok(())
}

/// Explicit user mode selection, used at connect/mode change, never per sample.
pub fn select_mode(
    model: Model,
    io: &mut impl CancellableIo,
    mode: DisplayMode,
) -> Result<(), String> {
    if model == Model::Sr830 {
        return select_sr830_display(io, mode);
    }
    if model != Model::Li5640 {
        return Ok(());
    }
    let selected = if mode == DisplayMode::Xy { 0 } else { 1 };
    let output_text = io.query("OTYP?\n").map_err(|e| format!("transport: {e}"))?;
    let count = output_text.trim().split(',').count();
    if !(1..=6).contains(&count) {
        return Err("Unexpected LI5640 output selection length".into());
    }
    let output = parse_values(&output_text, count, "OTYP?")?;
    let first = integer(query_value(io, "DDEF? 1\n")?, "DDEF? 1")?;
    let second = integer(query_value(io, "DDEF? 2\n")?, "DDEF? 2")?;
    let changed = first != selected || second != selected || output != LI5640_OUTPUT;
    let result = (|| {
        if first != selected {
            io.write(&format!("DDEF 1,{selected}\n"))?;
        }
        if second != selected {
            io.write(&format!("DDEF 2,{selected}\n"))?;
        }
        if output != LI5640_OUTPUT {
            io.write("OTYP 1,2,3\n")?;
        }
        if changed {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        verify_li5640_mode(io, mode)
    })();
    if let Err(error) = result {
        // Explicit selection is a transaction. A partial configuration must
        // either restore and verify the previous state or require reconnection.
        let mut failures = Vec::new();
        for command in [
            format!("OTYP {}\n", output_text.trim()),
            format!("DDEF 1,{first}\n"),
            format!("DDEF 2,{second}\n"),
        ] {
            if let Err(e) = io.write(&command) {
                failures.push(e);
            }
        }
        if failures.is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(20));
            let restored = (|| {
                let restored_output = query_values(io, "OTYP?\n", count)?;
                let restored_first = integer(query_value(io, "DDEF? 1\n")?, "DDEF? 1")?;
                let restored_second = integer(query_value(io, "DDEF? 2\n")?, "DDEF? 2")?;
                if restored_output != output || restored_first != first || restored_second != second
                {
                    return Err("LI5640 previous display/output settings not restored".into());
                }
                Ok(())
            })();
            if let Err(e) = restored {
                failures.push(e);
            }
        }
        if failures.is_empty() {
            return Err(format!(
                "{error}; previous LI5640 display/output settings restored"
            ));
        }
        io.invalidate();
        return Err(format!(
            "{error}; LI5640 recovery required: reconnect and verify display/output settings; restoration: {}",
            failures.join("; ")
        ));
    }
    Ok(())
}
// OTYP slot 3 is the measured reference frequency. FREQ? reports the
// internal oscillator setpoint, which can differ from an external reference.
fn li5640_reference_frequency(io: &mut impl Io) -> Result<Option<f64>, String> {
    let output = io.query("OTYP?\n").map_err(|e| format!("transport: {e}"))?;
    let output = output.trim();
    let mapping = parse_values(output, output.split(',').count(), "OTYP?")?;
    if mapping != LI5640_OUTPUT {
        return Ok(None);
    }
    let values = query_values(io, "DOUT?\n", LI5640_OUTPUT.len())?;
    Ok((values[2] > 0.0).then_some(values[2]))
}

fn verify_li5640_mode(io: &mut impl Io, mode: DisplayMode) -> Result<(), String> {
    let selected = if mode == DisplayMode::Xy { 0 } else { 1 };
    let output = query_values(io, "OTYP?\n", LI5640_OUTPUT.len())?;
    let first = integer(query_value(io, "DDEF? 1\n")?, "DDEF? 1")?;
    let second = integer(query_value(io, "DDEF? 2\n")?, "DDEF? 2")?;
    if output != LI5640_OUTPUT || first != selected || second != selected {
        return Err(format!(
            "LI5640 display/output mode differs from selected {mode:?}; reselect the mode before reading"
        ));
    }
    Ok(())
}
fn read_li5640(io: &mut impl Io, mode: DisplayMode) -> Result<Signal, String> {
    // Guard against front-panel/another client's mode changes. Never silently
    // relabel DATA1/DATA2, change the display, or derive unmeasured components.
    verify_li5640_mode(io, mode)?;
    let values = query_values(io, "DOUT?\n", LI5640_OUTPUT.len())?;
    Signal::from_pair(mode, values[0], values[1])
}

fn query_value(io: &mut impl Io, command: &str) -> Result<f64, String> {
    let response = io
        .query(command)
        .map_err(|error| format!("transport: {error}"))?;
    parse_values(&response, 1, command).map(|values| values[0])
}

fn query_values(io: &mut impl Io, command: &str, count: usize) -> Result<Vec<f64>, String> {
    let response = io
        .query(command)
        .map_err(|error| format!("transport: {error}"))?;
    parse_values(&response, count, command)
}

fn parse_values(response: &str, count: usize, command: &str) -> Result<Vec<f64>, String> {
    let fields: Vec<_> = response.trim().split(',').map(str::trim).collect();
    if fields.len() != count || fields.iter().any(|field| field.is_empty()) {
        return Err(format!(
            "Unexpected response for {command:?}: {response:?} (expected {count} numeric values)"
        ));
    }
    fields
        .into_iter()
        .map(|field| finite(field, command))
        .collect()
}

fn finite(value: &str, context: &str) -> Result<f64, String> {
    let value = value
        .parse::<f64>()
        .map_err(|_| format!("Unexpected numeric value for {context:?}: {value:?}"))?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(format!("Non-finite numeric value for {context:?}"))
    }
}

fn integer(value: f64, context: &str) -> Result<i32, String> {
    if value.is_finite()
        && value.fract() == 0.0
        && value >= i32::MIN as f64
        && value <= i32::MAX as f64
    {
        Ok(value as i32)
    } else {
        Err(format!(
            "Unexpected non-integer value for {context:?}: {value}"
        ))
    }
}

fn scale_5210(raw: f64, sensitivity: f64) -> f64 {
    raw * sensitivity / 10_000.0
}

fn signal(x: f64, y: f64, r: f64, theta: f64) -> Result<Signal, String> {
    Signal::new(x, y, r, theta)
}

// Only parsing failures may safely select a lower-level protocol.  Gpib kills
// its helper after transport failure, so retrying a command would be invalid.
fn is_transport_error(error: &str) -> bool {
    error.starts_with("transport: ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    pub(super) struct Fake {
        replies: VecDeque<Result<String, String>>,
        commands: Vec<String>,
        fail_write: Option<&'static str>,
        invalidated: bool,
    }
    impl Fake {
        pub(super) fn new(replies: &[&str]) -> Self {
            Self {
                replies: replies.iter().map(|v| Ok((*v).into())).collect(),
                commands: vec![],
                fail_write: None,
                invalidated: false,
            }
        }
    }
    impl Io for Fake {
        fn query(&mut self, command: &str) -> Result<String, String> {
            self.commands.push(command.into());
            self.replies
                .pop_front()
                .unwrap_or_else(|| Err("missing reply".into()))
        }
        fn write(&mut self, command: &str) -> Result<(), String> {
            self.commands.push(command.into());
            if self.fail_write == Some(command) {
                Err("write failed".into())
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn automatic_li5640_settings_snapshot_never_changes_output_type() {
        let mut io = Fake::new(&[
            "0",
            "0",
            "0",
            "0",
            "0",
            "1,2,3",
            "0.0218E-06,-0.1310E-06,464.2E+00",
            "0",
            "0",
            "0",
        ]);
        io.fail_write = Some("OTYP 3\n");
        let snapshot = settings_recorded(Model::Li5640, &mut io);
        assert_eq!(snapshot.result.as_ref().unwrap()["Ref. Freq"], 464.2);
        assert!(
            snapshot
                .exchanges
                .iter()
                .all(|event| event.operation == "query")
        );
        assert!(
            !io.commands
                .iter()
                .any(|command| command.starts_with("OTYP "))
        );
        let mut failed = Fake::new(&[]);
        assert!(
            settings_recorded(Model::Li5640, &mut failed)
                .result
                .is_err()
        );
        assert!(
            !failed
                .commands
                .iter()
                .any(|command| command.starts_with("OTYP "))
        );
    }

    #[test]
    fn run_snapshot_adds_reference_provenance_without_changing_live_queries() {
        use std::sync::atomic::AtomicBool;
        let cancel = AtomicBool::new(false);
        let base = ["0", "0", "0", "0", "0", "1,2,3", "1,2,464.2", "0", "0", "0"];
        let mut live = Fake::new(&base);
        assert!(
            settings(&mut live, Model::Li5640)
                .unwrap()
                .get("reference")
                .is_none()
        );
        let mut replies = base.to_vec();
        replies.extend(["-12.3400\r\n", "0", "3"]);
        let mut run = Fake::new(&replies);
        let snapshot = run_settings_recorded_cancellable(Model::Li5640, &mut run, &cancel);
        let value = snapshot.result.unwrap();
        assert_eq!(value["reference"]["phase_deg"], -12.34);
        assert_eq!(value["reference"]["harmonic"], 3);
        assert_eq!(&run.commands[..live.commands.len()], live.commands);
        assert_eq!(
            snapshot.exchanges[base.len()].reply.as_deref(),
            Some("-12.3400\r\n")
        );
        assert!(snapshot.exchanges.iter().all(|e| e.operation == "query"));

        let mut failed = Fake::new(&base);
        let snapshot = run_settings_recorded_cancellable(Model::Li5640, &mut failed, &cancel);
        assert!(snapshot.result.is_err());
        assert_eq!(snapshot.exchanges.last().unwrap().command, "PHAS?\n");
        assert!(snapshot.exchanges.last().unwrap().error.is_some());

        let mut stopped = Fake::new(&replies);
        let snapshot =
            run_settings_recorded_cancellable(Model::Li5640, &mut stopped, &AtomicBool::new(true));
        assert!(snapshot.result.is_err());
        assert!(stopped.commands.is_empty());
    }

    #[test]
    fn li5640_units_follow_queried_input_mode_and_never_assume_normalized_values_are_volts() {
        for (input, norm, ratio, unit) in [
            ("0", "0", "0", "V"),
            ("1", "0", "0", "V"),
            ("2", "0", "0", "A"),
            ("3", "0", "0", "A"),
            ("0", "1", "0", "unknown"),
            ("0", "2", "0", "unknown"),
            ("0", "0", "1", "unknown"),
            ("9", "0", "0", "unknown"),
        ] {
            let mut io = Fake::new(&["0", input, norm, ratio]);
            let result = status_recorded(Model::Li5640, &mut io);
            let value = result.result.unwrap();
            assert_eq!(measured_unit(Model::Li5640, Some(&value)), unit);
            assert_eq!(io.commands, ["OVCR?\n", "ISRC?\n", "NORM?\n", "RAT?\n"]);
            assert!(
                result
                    .exchanges
                    .iter()
                    .all(|event| event.operation == "query")
            );
        }
        assert_eq!(measured_unit(Model::Li5640, None), "unknown");
    }

    #[test]
    fn li5640_current_input_uses_current_sensitivity_readback_and_setter() {
        let mut io = Fake::new(&["2", "0", "0", "26", "0", "1,2", "0", "0", "0"]);
        let value = settings(&mut io, Model::Li5640).unwrap();
        assert_eq!(value["signal_unit"], "A");
        assert_eq!(value["Sensitivity"], json!(1e-6));
        assert!(io.commands.iter().any(|c| c == "ISEN?\n"));
        assert!(!io.commands.iter().any(|c| c == "VSEN?\n"));
        let mut io = Fake::new(&["3"]);
        set_setting(&mut io, Model::Li5640, "Sensitivity", &json!(5e-15)).unwrap();
        assert_eq!(io.commands, ["ISRC?\n", "ISEN 1\n"]);
    }

    #[test]
    fn li5640_current_sensitivity_respects_the_queried_conversion_gain() {
        let (low_gain, base, _) = li5640_sensitivity_table(2).unwrap();
        assert_eq!(base, 4);
        assert_eq!(low_gain[0], 50e-15);
        assert_eq!(*low_gain.last().unwrap(), 1e-6);
        let (high_gain, base, _) = li5640_sensitivity_table(3).unwrap();
        assert_eq!(base, 1);
        assert_eq!(high_gain[0], 5e-15);
        assert_eq!(*high_gain.last().unwrap(), 10e-9);
        let mut io = Fake::new(&["2"]);
        assert!(set_setting(&mut io, Model::Li5640, "Sensitivity", &json!(5e-15)).is_err());
        assert_eq!(io.commands, ["ISRC?\n"]);
    }
    #[test]
    fn sr830_front_panel_switch_preserves_ratios_and_checks_both_channels() {
        let mut io = Fake::new(&["0,1", "0,2", "1", "1", "1,1", "1,2"]);
        select_mode(Model::Sr830, &mut io, DisplayMode::RTheta).unwrap();
        assert_eq!(
            io.commands,
            [
                "DDEF? 1\n",
                "DDEF? 2\n",
                "FPOP? 1\n",
                "FPOP? 2\n",
                "DDEF 1,1,1\n",
                "DDEF 2,1,2\n",
                "DDEF? 1\n",
                "DDEF? 2\n"
            ]
        );
    }

    #[test]
    fn sr830_refuses_display_changes_when_output_follows_display() {
        let mut io = Fake::new(&["0,1", "0,2", "1", "0"]);
        let error = select_mode(Model::Sr830, &mut io, DisplayMode::RTheta).unwrap_err();
        assert!(error.contains("CH2 output follows the display"));
        assert!(io.commands.iter().all(|command| command.contains('?')));
    }

    #[test]
    fn sr830_units_follow_input_and_rejected_auto_phase_is_reported() {
        for (input, unit) in [("0", "V"), ("1", "V"), ("2", "A"), ("3", "A")] {
            let mut io = Fake::new(&["0", input]);
            let status = status_recorded(Model::Sr830, &mut io).result.unwrap();
            assert_eq!(measured_unit(Model::Sr830, Some(&status)), unit);
        }
        let mut io = Fake::new(&["0", "2.5", "2.5"]);
        assert!(
            auto(&mut io, Model::Sr830, "phase")
                .unwrap_err()
                .contains("phase shift unchanged")
        );
    }

    #[test]
    fn sr5210_auto_phase_waits_for_serial_poll_and_requires_reference() {
        struct SerialFake {
            io: Fake,
            status: std::collections::VecDeque<u8>,
        }
        impl Io for SerialFake {
            fn query(&mut self, command: &str) -> Result<String, String> {
                self.io.query(command)
            }
            fn write(&mut self, command: &str) -> Result<(), String> {
                self.io.write(command)
            }
            fn serial_poll(&mut self) -> Result<Option<u8>, String> {
                self.io.commands.push("serial poll".into());
                Ok(Some(self.status.pop_front().expect("unexpected poll")))
            }
        }
        let mut io = SerialFake {
            io: Fake::new(&["485200"]),
            status: [1, 1, 32, 33, 1].into(),
        };
        auto(&mut io, Model::Sr5210, "phase").unwrap();
        assert_eq!(
            io.io.commands,
            [
                "FRQ\r",
                "serial poll",
                "AQN\r",
                "serial poll",
                "serial poll",
                "serial poll",
                "serial poll"
            ]
        );

        for status in [0, 3, 9, 33] {
            let mut busy = SerialFake {
                io: Fake::new(&["485200"]),
                status: [status].into(),
            };
            assert!(
                auto(&mut busy, Model::Sr5210, "phase")
                    .unwrap_err()
                    .contains("not ready")
            );
            assert_eq!(busy.io.commands, ["FRQ\r", "serial poll"]);
        }

        let mut unlocked = Fake::new(&["0"]);
        assert!(
            auto(&mut unlocked, Model::Sr5210, "phase")
                .unwrap_err()
                .contains("locked reference")
        );
        assert_eq!(unlocked.commands, ["FRQ\r"]);
    }

    #[test]
    fn every_model_preserves_auto_commands_and_rejects_unknown_actions_without_io() {
        let commands = [
            ["APHS\n", "AGAN\n", "AGAN\n"],
            ["AQN\r", "AS\r", "ASM\r"],
            ["AQN\r", "AS\r", "ASM\r"],
            ["APHS\n", "ASEN\n", "ASEN\n"],
        ];
        for (model, expected) in Model::ALL.into_iter().zip(commands) {
            let mut io = Fake::new(if model == Model::Sr830 {
                &["0", "0", "10"]
            } else if model == Model::Sr5210 {
                &["485000"]
            } else {
                &[]
            });
            let expected = if model == Model::Sr830 {
                vec![
                    "LIAS?\n", "PHAS?\n", "APHS\n", "PHAS?\n", "AGAN\n", "AGAN\n",
                ]
            } else if model == Model::Sr5210 {
                for action in ["phase", "sensitivity", "measure"] {
                    assert!(
                        auto(&mut io, model, action)
                            .unwrap_err()
                            .contains("serial-poll-capable")
                    );
                }
                assert_eq!(io.commands, ["FRQ\r"]);
                continue;
            } else {
                expected.to_vec()
            };
            for action in ["phase", "sensitivity", "measure"] {
                auto(&mut io, model, action).unwrap();
            }
            assert_eq!(io.commands, expected, "{model:?}");
            let error = auto(&mut io, model, "unknown").unwrap_err();
            assert_eq!(
                error,
                format!("Unknown {} auto action: unknown", model.name())
            );
            assert_eq!(io.commands, expected, "unknown actions must perform no I/O");
        }
    }

    #[test]
    fn every_model_propagates_read_failures_and_observes_cancellation() {
        for model in Model::ALL {
            let mut io = Fake::new(&[]);
            io.replies.push_back(Err("transport failure".into()));
            assert_eq!(
                read(model, &mut io).unwrap_err(),
                "transport: transport failure",
                "{model:?}"
            );
            let mut io = Fake::new(&[]);
            let cancel = std::sync::atomic::AtomicBool::new(true);
            let error = read_cancellable(model, &mut io, &cancel).unwrap_err();
            assert_eq!(error, "transport: cancelled", "{model:?}");
            assert!(io.commands.is_empty(), "cancelled read must not issue I/O");
            assert!(!io.invalidated);
        }
    }

    #[test]
    fn model_names_and_sr830_protocol() {
        assert_eq!(
            Model::ALL.map(Model::name),
            ["SR830", "SR7265", "SR5210", "LI5640"]
        );
        let mut io = Fake::new(&["1,2,3,4"]);
        let got = read(Model::Sr830, &mut io).unwrap();
        assert_eq!(
            (got.x_v, got.y_v, got.r_v, got.theta_deg),
            (1.0, 2.0, 3.0, 4.0)
        );
        assert_eq!(io.commands, ["SNAP?1,2,3,4\n"]);
    }

    #[test]
    fn sr7265_uses_cr_and_pair_queries() {
        let mut io = Fake::new(&["1,2", "3,4"]);
        let got = read(Model::Sr7265, &mut io).unwrap();
        assert_eq!(
            (got.x_v, got.y_v, got.r_v, got.theta_deg),
            (1.0, 2.0, 3.0, 4.0)
        );
        assert_eq!(io.commands, ["XY.\r", "MP.\r"]);
    }

    #[test]
    fn sr5210_scales_and_converts_millidegrees() {
        let mut io = Fake::new(&["8", "0", "10000,-5000", "2500,12345"]);
        let got = read(Model::Sr5210, &mut io).unwrap();
        assert_eq!(
            (got.x_v, got.y_v, got.r_v, got.theta_deg),
            (0.001, -0.0005, 0.00025, 12.345)
        );
        assert_eq!(io.commands, ["SEN\r", "N\r", "XY\r", "MP\r"]);
    }

    #[test]
    fn sr5210_current_modes_convert_full_scale_and_qualify_units() {
        for (status, expected) in [("4", 1e-9), ("2", 1e-11)] {
            let mut io = Fake::new(&["8", status, "10000,-5000", "2500,12345"]);
            let reading = read(Model::Sr5210, &mut io).unwrap();
            assert!((reading.x_v / expected - 1.).abs() < 1e-12);
            assert!((reading.y_v / expected + 0.5).abs() < 1e-12);
            assert!((reading.r_v / expected - 0.25).abs() < 1e-12);
            let status = status.parse::<u8>().unwrap();
            assert_eq!(
                measured_unit(Model::Sr5210, Some(&json!({"overload_byte":status}))),
                "A"
            );
        }
        assert_eq!(
            measured_unit(Model::Sr5210, Some(&json!({"overload_byte":6}))),
            "unknown"
        );
        assert_eq!(
            measured_unit(Model::Sr5210, Some(&json!({"overload_byte":0}))),
            "V"
        );
    }

    impl CancellableIo for Fake {
        fn invalidate(&mut self) {
            self.invalidated = true;
        }
        fn query_cancellable(
            &mut self,
            command: &str,
            cancel: &std::sync::atomic::AtomicBool,
        ) -> Result<String, String> {
            if cancel.load(std::sync::atomic::Ordering::Acquire) {
                return Err("cancelled".into());
            }
            self.query(command)
        }
        fn write_cancellable(
            &mut self,
            command: &str,
            cancel: &std::sync::atomic::AtomicBool,
        ) -> Result<(), String> {
            if cancel.load(std::sync::atomic::Ordering::Acquire) {
                return Err("cancelled".into());
            }
            self.write(command)
        }
    }
    #[test]
    fn li5640_reads_only_selected_pair_without_any_display_writes() {
        for (mode, defs, values) in [
            (DisplayMode::Xy, "0", "1,2,464.2"),
            (DisplayMode::RTheta, "1", "3,40,464.2"),
        ] {
            let mut io = Fake::new(&["1,2,3", defs, defs, values, "1,2,3", defs, defs, values]);
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let first = read_selected_cancellable(Model::Li5640, &mut io, &cancel, mode).unwrap();
            let second = read_selected_recorded_cancellable(Model::Li5640, &mut io, &cancel, mode)
                .result
                .unwrap();
            assert_eq!(first, second);
            assert_eq!(first.acquired_pair(), Some(mode));
            let unavailable = if mode == DisplayMode::Xy {
                [first.r_v, first.theta_deg]
            } else {
                [first.x_v, first.y_v]
            };
            assert!(unavailable.iter().all(|value| value.is_nan()));
            assert!(io.commands.iter().all(|c| c.contains('?')));
            assert_eq!(
                io.commands
                    .iter()
                    .filter(|c| c.as_str() == "DOUT?\n")
                    .count(),
                2
            );
        }
    }
    #[test]
    fn li5640_mode_selection_changes_only_at_explicit_configuration_and_verifies_readback() {
        let mut io = Fake::new(&["3", "1", "1", "1,2,3", "0", "0"]);
        select_mode(Model::Li5640, &mut io, DisplayMode::Xy).unwrap();
        assert_eq!(
            io.commands
                .iter()
                .filter(|c| !c.contains('?'))
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["DDEF 1,0\n", "DDEF 2,0\n", "OTYP 1,2,3\n"]
        );
        let mut correct = Fake::new(&["1,2,3", "1", "1", "1,2,3", "1", "1"]);
        select_mode(Model::Li5640, &mut correct, DisplayMode::RTheta).unwrap();
        assert!(correct.commands.iter().all(|c| c.contains('?')));
    }
    #[test]
    fn li5640_refuses_wrong_display_or_output_selection_instead_of_mislabeling_or_mutating() {
        for replies in [["1,2,3", "1", "1"], ["2,1,3", "0", "0"]] {
            let mut io = Fake::new(&replies);
            assert!(
                read(Model::Li5640, &mut io)
                    .unwrap_err()
                    .contains("differs from selected")
            );
            assert_eq!(io.commands, ["OTYP?\n", "DDEF? 1\n", "DDEF? 2\n"]);
        }
    }
    #[test]
    fn li5640_configuration_failure_does_not_produce_a_measurement() {
        let mut io = Fake::new(&["1,2,3", "1", "1"]);
        io.fail_write = Some("DDEF 1,0\n");
        assert!(
            select_mode(Model::Li5640, &mut io, DisplayMode::Xy)
                .unwrap_err()
                .contains("write failed")
        );
        assert!(!io.commands.iter().any(|c| c == "DOUT?\n"));
    }

    #[test]
    fn li5640_selection_failures_restore_and_verify_every_previous_setting() {
        for failed in ["DDEF 1,0\n", "DDEF 2,0\n", "OTYP 1,2,3\n"] {
            let mut io = Fake::new(&["3", "1", "1", "3", "1", "1"]);
            io.fail_write = Some(failed);
            let error = select_mode(Model::Li5640, &mut io, DisplayMode::Xy).unwrap_err();
            assert!(
                error.contains("previous LI5640 display/output settings restored"),
                "{failed}: {error}"
            );
            assert!(!io.invalidated);
            assert!(io.commands.ends_with(&[
                "OTYP 3\n".into(),
                "DDEF 1,1\n".into(),
                "DDEF 2,1\n".into(),
                "OTYP?\n".into(),
                "DDEF? 1\n".into(),
                "DDEF? 2\n".into()
            ]));
        }
        let mut io = Fake::new(&["3", "1", "1", "1,2,3", "1", "0", "3", "1", "1"]);
        assert!(
            select_mode(Model::Li5640, &mut io, DisplayMode::Xy)
                .unwrap_err()
                .contains("settings restored")
        );
        assert!(!io.invalidated);
    }
    #[test]
    fn li5640_rollback_waits_for_display_settings_to_settle_before_verification() {
        struct Delayed {
            inner: Fake,
            restored_at: Option<std::time::Instant>,
        }
        impl Io for Delayed {
            fn query(&mut self, command: &str) -> Result<String, String> {
                if self
                    .restored_at
                    .is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(20))
                {
                    return Err("display settings still settling".into());
                }
                self.inner.query(command)
            }
            fn write(&mut self, command: &str) -> Result<(), String> {
                let result = self.inner.write(command);
                if command == "DDEF 2,1\n" {
                    self.restored_at = Some(std::time::Instant::now());
                }
                result
            }
        }
        impl CancellableIo for Delayed {
            fn invalidate(&mut self) {
                self.inner.invalidated = true;
            }
            fn query_cancellable(
                &mut self,
                c: &str,
                cancel: &std::sync::atomic::AtomicBool,
            ) -> Result<String, String> {
                if cancel.load(std::sync::atomic::Ordering::Acquire) {
                    Err("cancelled".into())
                } else {
                    self.query(c)
                }
            }
            fn write_cancellable(
                &mut self,
                c: &str,
                cancel: &std::sync::atomic::AtomicBool,
            ) -> Result<(), String> {
                if cancel.load(std::sync::atomic::Ordering::Acquire) {
                    Err("cancelled".into())
                } else {
                    self.write(c)
                }
            }
        }
        let mut io = Delayed {
            inner: Fake::new(&["3", "1", "1", "3", "1", "1"]),
            restored_at: None,
        };
        io.inner.fail_write = Some("DDEF 2,0\n");
        let error = select_mode(Model::Li5640, &mut io, DisplayMode::Xy).unwrap_err();
        assert!(error.contains("settings restored"), "{error}");
        assert!(!io.inner.invalidated);
    }
    #[test]
    fn li5640_selection_failed_restoration_invalidates_and_still_attempts_all_restores() {
        let mut io = Fake::new(&["3", "1", "1", "1,2,3", "1", "0"]);
        io.fail_write = Some("OTYP 3\n");
        let error = select_mode(Model::Li5640, &mut io, DisplayMode::Xy).unwrap_err();
        assert!(error.contains("LI5640 recovery required"));
        assert!(io.invalidated);
        assert!(io.commands.ends_with(&[
            "OTYP 3\n".into(),
            "DDEF 1,1\n".into(),
            "DDEF 2,1\n".into()
        ]));
    }
    #[test]
    fn li5640_periodic_and_manual_settings_never_mutate_output_mapping() {
        let mut io = Fake::new(&[
            "0",
            "0",
            "0",
            "0",
            "0",
            "1,2,3",
            "0.0218E-06,-0.1310E-06,464.2E+00",
            "0",
            "0",
            "0",
        ]);
        let settings = settings(&mut io, Model::Li5640).unwrap();
        assert_eq!(settings["Ref. Freq"], 464.2);
        assert!(
            io.commands
                .iter()
                .all(|c| c.contains('?') && !c.starts_with("OTYP "))
        );
    }

    #[test]
    fn recorded_reads_match_direct_reads_without_extra_io_for_every_model() {
        let cases: [(Model, &[&str]); 4] = [
            (Model::Sr830, &["1,2,3,4"]),
            (Model::Sr7265, &["1,2", "3,4"]),
            (Model::Sr5210, &["8", "0", "10000,-5000", "2500,12345"]),
            (Model::Li5640, &["1,2,3", "0", "0", "1,2,464.2"]),
        ];
        for (model, replies) in cases {
            let mut direct_io = Fake::new(replies);
            let direct = read(model, &mut direct_io);
            let mut recorded_io = Fake::new(replies);
            let recorded = read_recorded(model, &mut recorded_io);

            assert_eq!(recorded.result, direct, "{model:?}");
            assert_eq!(recorded_io.commands, direct_io.commands, "{model:?}");
            assert_eq!(
                recorded
                    .exchanges
                    .iter()
                    .map(|exchange| exchange.command.as_str())
                    .collect::<Vec<_>>(),
                direct_io
                    .commands
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                "{model:?}"
            );
        }
    }

    #[test]
    fn recorded_read_preserves_raw_reply_precision_before_parsing() {
        let reply = "1.230000000000e-9,-2.5000000000000E+03,3.140000000000000,4.0000000000000\r\n";
        let mut io = Fake::new(&[reply]);
        let recorded = read_recorded(Model::Sr830, &mut io);

        assert!(recorded.result.is_ok());
        assert_eq!(recorded.exchanges.len(), 1);
        assert_eq!(recorded.exchanges[0].operation, "query");
        assert_eq!(recorded.exchanges[0].command, "SNAP?1,2,3,4\n");
        assert_eq!(recorded.exchanges[0].reply.as_deref(), Some(reply));
        assert_eq!(recorded.exchanges[0].error, None);
    }

    #[test]
    fn recorded_read_keeps_parse_and_transport_failures() {
        let parse_reply = "1.000000000000e-9,not-a-number,3,4\n";
        let mut parse_io = Fake::new(&[parse_reply]);
        let parsed = read_recorded(Model::Sr830, &mut parse_io);
        assert!(
            parsed
                .result
                .unwrap_err()
                .contains("Unexpected numeric value")
        );
        assert_eq!(parsed.exchanges.len(), 1);
        assert_eq!(parsed.exchanges[0].reply.as_deref(), Some(parse_reply));
        assert_eq!(parsed.exchanges[0].error, None);

        let mut transport_io = Fake::new(&[]);
        transport_io
            .replies
            .push_back(Err("helper disconnected".into()));
        let transported = read_recorded(Model::Sr7265, &mut transport_io);
        assert_eq!(
            transported.result.unwrap_err(),
            "transport: helper disconnected"
        );
        assert_eq!(transported.exchanges.len(), 1);
        assert_eq!(transported.exchanges[0].operation, "query");
        assert_eq!(transported.exchanges[0].command, "XY.\r");
        assert_eq!(transported.exchanges[0].reply, None);
        assert_eq!(
            transported.exchanges[0].error.as_deref(),
            Some("helper disconnected")
        );
    }

    #[test]
    fn recorded_read_keeps_the_sr7265_parse_fallback_order() {
        let replies = ["incomplete", "also incomplete", "1", "2", "3", "4"];
        let mut direct_io = Fake::new(&replies);
        let direct = read(Model::Sr7265, &mut direct_io);
        let mut recorded_io = Fake::new(&replies);
        let recorded = read_recorded(Model::Sr7265, &mut recorded_io);

        assert_eq!(recorded.result, direct);
        assert_eq!(recorded_io.commands, direct_io.commands);
        assert_eq!(
            recorded_io.commands,
            ["XY.\r", "MP.\r", "X.\r", "Y.\r", "MAG.\r", "PHA.\r"]
        );
        assert_eq!(
            recorded
                .exchanges
                .iter()
                .filter_map(|exchange| exchange.reply.as_deref())
                .collect::<Vec<_>>(),
            replies
        );
    }

    #[test]
    fn recorded_li5640_query_failure_keeps_raw_evidence_without_restoration_writes() {
        let mut io = Fake::new(&["1,2,3", "0", "0"]);
        io.replies.push_back(Err("cancelled".into()));
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let recorded = read_recorded_cancellable(Model::Li5640, &mut io, &cancel);
        assert!(recorded.result.unwrap_err().contains("cancelled"));
        assert_eq!(recorded.exchanges.len(), 4);
        assert_eq!(recorded.exchanges[3].command, "DOUT?\n");
        assert_eq!(recorded.exchanges[3].error.as_deref(), Some("cancelled"));
        assert!(recorded.exchanges.iter().all(|e| e.operation == "query"));
        assert!(!io.invalidated);
    }

    #[test]
    fn sr830_settings_and_controls_use_python_commands() {
        let mut io = Fake::new(&["0", "0", "10", "123.4", "1", "2", "3", "0"]);
        let got = settings(&mut io, Model::Sr830).unwrap();
        assert_eq!(got["Sensitivity"], json!(2e-9));
        assert_eq!(got["Time Constant"], json!(1.0));
        assert_eq!(got["Coupling"], json!("DC"));
        assert_eq!(got["Slope"], json!(18.0));
        assert_eq!(got["AC Gain"], Value::Null);
        assert_eq!(got["input_mode"], json!(0));
        assert_eq!(
            io.commands,
            [
                "ISRC?\n", "SENS?\n", "OFLT?\n", "FREQ?\n", "ICPL?\n", "OFSL?\n", "LIAS?\n"
            ]
        );

        set_setting(&mut io, Model::Sr830, "Sensitivity", &json!(1e-6)).unwrap();
        auto(&mut io, Model::Sr830, "measure").unwrap();
        assert_eq!(&io.commands[7..], ["ISRC?\n", "SENS 8\n", "AGAN\n"]);
    }

    /// One SR830 settings snapshot: ISRC, SENS, OFLT, FREQ, ICPL, OFSL, LIAS.
    fn sr830_snapshot(input: &str, sens: &str, tc: &str, slope: &str) -> [String; 7] {
        [input, sens, tc, "1000", "0", slope, "0"].map(str::to_owned)
    }
    fn fake_from(replies: Vec<String>) -> Fake {
        let replies: Vec<&str> = replies.iter().map(String::as_str).collect();
        Fake::new(&replies)
    }

    #[test]
    fn apply_setting_reads_writes_and_verifies_the_instrument_readback() {
        // Time constant 1 s is OFLT index 10; slope 24 dB/oct is OFSL 3.
        let mut replies = sr830_snapshot("0", "8", "9", "1").to_vec();
        replies.extend(sr830_snapshot("0", "8", "10", "1"));
        let mut io = fake_from(replies);
        let got = apply_setting(&mut io, Model::Sr830, "time_constant", &json!(1.0)).unwrap();
        assert_eq!(got["Time Constant"], json!(1.0));
        assert_eq!(io.commands[7], "OFLT 10\n");
        assert_eq!(io.commands.len(), 15, "pre-read, write, post-read only");

        // The instrument kept the previous slope: report, never assume.
        let mut replies = sr830_snapshot("0", "8", "10", "1").to_vec();
        replies.extend(sr830_snapshot("0", "8", "10", "1"));
        let mut io = fake_from(replies);
        let error = apply_setting(&mut io, Model::Sr830, "slope", &json!(24)).unwrap_err();
        assert!(error.contains("Slope readback 12.0"), "{error}");
        assert_eq!(io.commands[7], "OFSL 3\n");
    }

    #[test]
    fn apply_setting_validates_against_the_observed_input_mode_before_writing() {
        // SR7265 IMODE 2 (low-noise current) has no 1 V range.
        let mut io = Fake::new(&["2", "1e-9", "0.1", "1000", "0", "1", "0", "0"]);
        let error = apply_setting(&mut io, Model::Sr7265, "sensitivity", &json!(1.0)).unwrap_err();
        assert!(error.contains("current input mode"), "{error}");
        assert!(
            io.commands.iter().all(|c| !c.starts_with("SEN ")),
            "no write"
        );

        // SR830 current input: the volt table is not qualified, so refuse.
        let mut io = fake_from(sr830_snapshot("2", "8", "10", "1").to_vec());
        let got = settings(&mut io, Model::Sr830).unwrap();
        assert_eq!(got["Sensitivity"], Value::Null);
        let mut io = fake_from(sr830_snapshot("2", "8", "10", "1").to_vec());
        let error = apply_setting(&mut io, Model::Sr830, "sensitivity", &json!(1e-6)).unwrap_err();
        assert!(error.contains("cannot be changed"), "{error}");
        assert!(
            io.commands.iter().all(|c| !c.starts_with("SENS ")),
            "no write"
        );
        let mut io = Fake::new(&["3"]);
        assert!(set_setting(&mut io, Model::Sr830, "Sensitivity", &json!(1e-6)).is_err());
        assert_eq!(io.commands, ["ISRC?\n"]);
    }

    #[test]
    fn apply_setting_rejects_an_input_mode_change_between_read_and_readback() {
        let before = ["0", "1e-3", "0.1", "1000", "0", "1", "0", "0"];
        let after = ["1", "2e-3", "0.1", "1000", "0", "1", "0", "0"];
        let mut replies = before.to_vec();
        replies.push("0"); // IMODE queried by set_setting
        replies.extend(after);
        let mut io = Fake::new(&replies);
        let error = apply_setting(&mut io, Model::Sr7265, "sensitivity", &json!(2e-3)).unwrap_err();
        assert!(error.contains("input mode changed"), "{error}");
    }

    #[test]
    fn ac_gain_is_sr7265_only_and_distinct_from_filter_slope() {
        for model in [Model::Sr830, Model::Sr5210, Model::Li5640] {
            let mut io = Fake::new(&[]);
            let error = apply_setting(&mut io, model, "ac_gain", &json!(10.0)).unwrap_err();
            assert!(error.contains("no AC gain"), "{model:?}: {error}");
            assert!(io.commands.is_empty(), "rejected without I/O");
        }
        // SR7265: ACGAIN n is reported as n x 10 dB; slope stays a separate
        // dB/oct value read by SLOPE.
        let before = ["0", "1e-3", "0.1", "1000", "0", "1", "2", "0"];
        let after = ["0", "1e-3", "0.1", "1000", "0", "1", "3", "0"];
        let mut replies = before.to_vec();
        replies.extend(after);
        let mut io = Fake::new(&replies);
        let got = apply_setting(&mut io, Model::Sr7265, "ac_gain", &json!(30.0)).unwrap();
        assert_eq!(got["AC Gain"], json!(30.0));
        assert_eq!(got["Slope"], json!(12.0));
        assert_eq!(io.commands[8], "ACGAIN 3\r");
    }

    #[test]
    fn sr5210_current_input_withholds_volt_sensitivity() {
        // SEN, TC, FRQ, XDB, N (bit 2: current mode 10^6)
        let mut io = Fake::new(&["4", "6", "1000000", "1", "4"]);
        let got = settings(&mut io, Model::Sr5210).unwrap();
        assert_eq!(got["Sensitivity"], Value::Null);
        assert_eq!(got["available"]["sensitivity"], json!([]));
        assert_eq!(got["Time Constant"], json!(1.0));
    }

    #[test]
    fn sr7265_sensitivity_uses_imode_table_and_one_based_index() {
        let mut io = Fake::new(&["2"]);
        set_setting(&mut io, Model::Sr7265, "Sensitivity", &json!(2e-15)).unwrap();
        assert_eq!(io.commands, ["IMODE\r", "SEN 7\r"]);
    }

    #[test]
    fn sr7265_settings_are_physical_values_not_indices() {
        let mut io = Fake::new(&["0", "0.000002", "0.5", "123", "0", "1", "2", "0"]);
        let got = settings(&mut io, Model::Sr7265).unwrap();
        assert_eq!(got["Sensitivity"], json!(0.000002));
        assert_eq!(got["Time Constant"], json!(0.5));
        assert_eq!(got["AC Gain"], json!(20.0));
        assert_eq!(
            io.commands,
            [
                "IMODE\r", "SEN.\r", "TC.\r", "FRQ.\r", "CP\r", "SLOPE\r", "ACGAIN\r", "N\r"
            ]
        );
    }
}
