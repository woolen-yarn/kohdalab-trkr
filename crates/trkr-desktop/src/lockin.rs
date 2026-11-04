//! Read-only lock-in amplifier protocols used by the desktop application.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use trkr_core::{DisplayMode, Signal};

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
        if model == Model::Sr7265 {
            value["input_mode"] = json!(query_value(io, "IMODE\r")?);
        }
        Ok(value)
    })
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
pub fn status_recorded_cancellable(
    model: Model,
    io: &mut impl CancellableIo,
    cancel: &std::sync::atomic::AtomicBool,
) -> RecordedOperation<Value> {
    status_recorded(model, &mut Cancellable { io, cancel })
}
pub fn settings_recorded_cancellable(
    model: Model,
    io: &mut impl CancellableIo,
    cancel: &std::sync::atomic::AtomicBool,
) -> RecordedOperation<Value> {
    settings_recorded(model, &mut Cancellable { io, cancel })
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
    let (sensitivity, time_constant, ref_freq, coupling, slope, ac_gain, overload, available) =
        match model {
            Model::Sr830 => (
                indexed(io, "SENS?\n", &SR_SENS)?,
                indexed(io, "OFLT?\n", &SR_TC)?,
                Some(query_value(io, "FREQ?\n")?),
                coupling(io, "ICPL?\n")?,
                slope4(io, "OFSL?\n")?,
                Value::Null,
                overload_sr830(io)?,
                model.descriptor().available(&SR_SENS, &SR_TC),
            ),
            Model::Sr7265 => {
                let imode = integer(query_value(io, "IMODE\r")?, "SR7265 IMODE")?;
                let sensitivity_table = sr7265_sensitivity(imode)?;
                input_mode = json!(imode);
                (
                    query_value(io, "SEN.\r")?,
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
                indexed(io, "SEN\r", &S5210_SENS)?,
                indexed(io, "TC\r", &S5210_TC)?,
                Some(query_value(io, "FRQ\r")? / 1000.0),
                Value::String("AC".into()),
                slope2(io, "XDB\r")?,
                Value::Null,
                overload_sr5210(io)?,
                model.descriptor().available(&S5210_SENS, &S5210_TC),
            ),
            Model::Li5640 => (
                indexed(io, "VSEN?\n", &SR_SENS)?,
                indexed(io, "TCON?\n", &SR_TC)?,
                // Reading reference frequency through OTYP would temporarily
                // replace the selected DATA1/DATA2 output. Leave it unknown.
                None,
                coupling(io, "ICPL?\n")?,
                slope4(io, "SLOP?\n")?,
                Value::Null,
                overload_li5640(io)?,
                model.descriptor().available(&SR_SENS, &SR_TC),
            ),
        };
    Ok(
        json!({"Sensitivity": sensitivity, "Time Constant": time_constant, "Ref. Freq": ref_freq, "Coupling": coupling, "Slope": slope, "AC Gain": ac_gain, "Overload": overload, "input_mode":input_mode, "available": available}),
    )
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
            write_index(io, "VSEN", value, &SR_SENS, descriptor.termination)
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

pub fn auto(io: &mut impl Io, model: Model, action: &str) -> Result<(), String> {
    let descriptor = model.descriptor();
    let command = match action {
        "phase" => descriptor.auto_phase,
        "sensitivity" => descriptor.auto_sensitivity,
        "measure" => descriptor.auto_measure,
        _ => return Err(format!("Unknown {} auto action: {action}", model.name())),
    };
    io.write(&format!("{command}{}", descriptor.termination))
}

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
    let sensitivity = sensitivity(query_value(io, "SEN\r")?)?;
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

/// Explicit user mode selection, used at connect/mode change, never per sample.
pub fn select_mode(
    model: Model,
    io: &mut impl CancellableIo,
    mode: DisplayMode,
) -> Result<(), String> {
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
    let changed = first != selected || second != selected || output != [1., 2.];
    let result = (|| {
        if first != selected {
            io.write(&format!("DDEF 1,{selected}\n"))?;
        }
        if second != selected {
            io.write(&format!("DDEF 2,{selected}\n"))?;
        }
        if output != [1., 2.] {
            io.write("OTYP 1,2\n")?;
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
fn verify_li5640_mode(io: &mut impl Io, mode: DisplayMode) -> Result<(), String> {
    let selected = if mode == DisplayMode::Xy { 0 } else { 1 };
    let output = query_values(io, "OTYP?\n", 2)?;
    let first = integer(query_value(io, "DDEF? 1\n")?, "DDEF? 1")?;
    let second = integer(query_value(io, "DDEF? 2\n")?, "DDEF? 2")?;
    if output != [1., 2.] || first != selected || second != selected {
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
    let values = query_values(io, "DOUT?\n", 2)?;
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

fn sensitivity(index: f64) -> Result<f64, String> {
    const VALUES: [f64; 16] = [
        100e-9, 300e-9, 1e-6, 3e-6, 10e-6, 30e-6, 100e-6, 300e-6, 1e-3, 3e-3, 10e-3, 30e-3, 100e-3,
        300e-3, 1.0, 3.0,
    ];
    let index = integer(index, "SR5210 SEN")?;
    VALUES
        .get(index as usize)
        .copied()
        .ok_or_else(|| format!("Unexpected SR5210 sensitivity index: {index}"))
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

    struct Fake {
        replies: VecDeque<Result<String, String>>,
        commands: Vec<String>,
        fail_write: Option<&'static str>,
        invalidated: bool,
    }
    impl Fake {
        fn new(replies: &[&str]) -> Self {
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
        let mut io = Fake::new(&["0", "0", "0", "0", "0"]);
        io.fail_write = Some("OTYP 3\n");
        let snapshot = settings_recorded(Model::Li5640, &mut io);
        assert!(snapshot.result.as_ref().unwrap()["Ref. Freq"].is_null());
        assert!(
            snapshot
                .exchanges
                .iter()
                .all(|event| event.operation == "query")
        );
        assert!(
            !io.commands
                .iter()
                .any(|command| command.starts_with("OTYP"))
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
                .any(|command| command.starts_with("OTYP"))
        );
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
            let mut io = Fake::new(&[]);
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
        let mut io = Fake::new(&["8", "10000,-5000", "2500,12345"]);
        let got = read(Model::Sr5210, &mut io).unwrap();
        assert_eq!(
            (got.x_v, got.y_v, got.r_v, got.theta_deg),
            (0.001, -0.0005, 0.00025, 12.345)
        );
        assert_eq!(io.commands, ["SEN\r", "XY\r", "MP\r"]);
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
            (DisplayMode::Xy, "0", "1,2"),
            (DisplayMode::RTheta, "1", "3,40"),
        ] {
            let mut io = Fake::new(&["1,2", defs, defs, values, "1,2", defs, defs, values]);
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
        let mut io = Fake::new(&["3", "1", "1", "1,2", "0", "0"]);
        select_mode(Model::Li5640, &mut io, DisplayMode::Xy).unwrap();
        assert_eq!(
            io.commands
                .iter()
                .filter(|c| !c.contains('?'))
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["DDEF 1,0\n", "DDEF 2,0\n", "OTYP 1,2\n"]
        );
        let mut correct = Fake::new(&["1,2", "1", "1", "1,2", "1", "1"]);
        select_mode(Model::Li5640, &mut correct, DisplayMode::RTheta).unwrap();
        assert!(correct.commands.iter().all(|c| c.contains('?')));
    }
    #[test]
    fn li5640_refuses_wrong_display_or_output_selection_instead_of_mislabeling_or_mutating() {
        for replies in [["1,2", "1", "1"], ["2,1", "0", "0"]] {
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
        let mut io = Fake::new(&["1,2", "1", "1"]);
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
        for failed in ["DDEF 1,0\n", "DDEF 2,0\n", "OTYP 1,2\n"] {
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
        let mut io = Fake::new(&["3", "1", "1", "1,2", "1", "0", "3", "1", "1"]);
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
        let mut io = Fake::new(&["3", "1", "1", "1,2", "1", "0"]);
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
        let mut io = Fake::new(&["0", "0", "0", "0", "0"]);
        let settings = settings(&mut io, Model::Li5640).unwrap();
        assert!(settings["Ref. Freq"].is_null());
        assert!(
            io.commands
                .iter()
                .all(|c| c.contains('?') && !c.starts_with("OTYP"))
        );
    }

    #[test]
    fn recorded_reads_match_direct_reads_without_extra_io_for_every_model() {
        let cases: [(Model, &[&str]); 4] = [
            (Model::Sr830, &["1,2,3,4"]),
            (Model::Sr7265, &["1,2", "3,4"]),
            (Model::Sr5210, &["8", "10000,-5000", "2500,12345"]),
            (Model::Li5640, &["1,2", "0", "0", "1,2"]),
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
        let mut io = Fake::new(&["1,2", "0", "0"]);
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
        let mut io = Fake::new(&["0", "10", "123.4", "1", "2", "3"]);
        let got = settings(&mut io, Model::Sr830).unwrap();
        assert_eq!(got["Sensitivity"], json!(2e-9));
        assert_eq!(got["Time Constant"], json!(1.0));
        assert_eq!(got["Coupling"], json!("DC"));
        assert_eq!(got["Slope"], json!(18.0));
        assert_eq!(got["AC Gain"], Value::Null);
        assert_eq!(
            io.commands,
            [
                "SENS?\n", "OFLT?\n", "FREQ?\n", "ICPL?\n", "OFSL?\n", "LIAS?\n"
            ]
        );

        set_setting(&mut io, Model::Sr830, "Sensitivity", &json!(1e-6)).unwrap();
        auto(&mut io, Model::Sr830, "measure").unwrap();
        assert_eq!(&io.commands[6..], ["SENS 8\n", "AGAN\n"]);
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
