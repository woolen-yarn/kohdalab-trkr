//! Read-only lock-in amplifier protocols used by the desktop application.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use trkr_core::Signal;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Model {
    Sr830,
    Sr7265,
    Sr5210,
    Li5640,
}

impl Model {
    pub const ALL: [Self; 4] = [Self::Sr830, Self::Sr7265, Self::Sr5210, Self::Li5640];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Sr830 => "SR830",
            Self::Sr7265 => "SR7265",
            Self::Sr5210 => "SR5210",
            Self::Li5640 => "LI5640",
        }
    }
}

pub trait Io {
    fn query(&mut self, command: &str) -> Result<String, String>;
    fn write(&mut self, command: &str) -> Result<(), String>;
}

impl Io for trkr_transport::Gpib {
    fn query(&mut self, command: &str) -> Result<String, String> {
        trkr_transport::Gpib::query(self, command)
    }

    fn write(&mut self, command: &str) -> Result<(), String> {
        trkr_transport::Gpib::write(self, command)
    }
}

pub fn read(model: Model, io: &mut impl Io) -> Result<Signal, String> {
    match model {
        Model::Sr830 => read_sr830(io),
        Model::Sr7265 => read_sr7265(io),
        Model::Sr5210 => read_sr5210(io),
        Model::Li5640 => read_li5640(io),
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
    let (sensitivity, time_constant, ref_freq, coupling, slope, ac_gain, overload, available) =
        match model {
            Model::Sr830 => (
                indexed(io, "SENS?\n", &SR_SENS)?,
                indexed(io, "OFLT?\n", &SR_TC)?,
                query_value(io, "FREQ?\n")?,
                coupling(io, "ICPL?\n")?,
                slope4(io, "OFSL?\n")?,
                Value::Null,
                overload_sr830(io)?,
                json!({"sensitivity": SR_SENS, "time_constant": SR_TC, "coupling": ["AC", "DC"], "slope": [6,12,18,24], "ac_gain": []}),
            ),
            Model::Sr7265 => {
                let imode = integer(query_value(io, "IMODE\r")?, "SR7265 IMODE")?;
                let sensitivity_table = sr7265_sensitivity(imode)?;
                (
                    query_value(io, "SEN.\r")?,
                    query_value(io, "TC.\r")?,
                    query_value(io, "FRQ.\r")?,
                    coupling(io, "CP\r")?,
                    slope4(io, "SLOPE\r")?,
                    json!(query_value(io, "ACGAIN\r")? * 10.0),
                    overload_sr7265(io)?,
                    json!({"sensitivity": sensitivity_table, "time_constant": S7265_TC, "coupling": ["AC", "DC"], "slope": [6,12,18,24], "ac_gain": AC_GAIN}),
                )
            }
            Model::Sr5210 => (
                indexed(io, "SEN\r", &S5210_SENS)?,
                indexed(io, "TC\r", &S5210_TC)?,
                query_value(io, "FRQ\r")? / 1000.0,
                Value::String("AC".into()),
                slope2(io, "XDB\r")?,
                Value::Null,
                overload_sr5210(io)?,
                json!({"sensitivity": S5210_SENS, "time_constant": S5210_TC, "coupling": ["AC"], "slope": [6,12], "ac_gain": []}),
            ),
            Model::Li5640 => (
                indexed(io, "VSEN?\n", &SR_SENS)?,
                indexed(io, "TCON?\n", &SR_TC)?,
                li5640_ref_freq(io)?,
                coupling(io, "ICPL?\n")?,
                slope4(io, "SLOP?\n")?,
                Value::Null,
                overload_li5640(io)?,
                json!({"sensitivity": SR_SENS, "time_constant": SR_TC, "coupling": ["AC", "DC"], "slope": [6,12,18,24], "ac_gain": []}),
            ),
        };
    Ok(
        json!({"Sensitivity": sensitivity, "Time Constant": time_constant, "Ref. Freq": ref_freq, "Coupling": coupling, "Slope": slope, "AC Gain": ac_gain, "Overload": overload, "available": available}),
    )
}

pub fn set_setting(
    io: &mut impl Io,
    model: Model,
    setting: &str,
    value: &Value,
) -> Result<(), String> {
    match (model, setting) {
        (Model::Sr830, "Sensitivity") => write_index(io, "SENS", value, &SR_SENS, "\n"),
        (Model::Sr830, "Time Constant") => write_index(io, "OFLT", value, &SR_TC, "\n"),
        (Model::Sr830, "Coupling") => write_coupling(io, "ICPL", value, "\n"),
        (Model::Sr830, "Slope") => write_slope(io, "OFSL", value, &[6, 12, 18, 24], "\n"),
        (Model::Sr7265, "Sensitivity") => {
            let mode = integer(query_value(io, "IMODE\r")?, "SR7265 IMODE")?;
            write_index_base(
                io,
                "SEN",
                value,
                sr7265_sensitivity(mode)?,
                if mode == 2 { 7 } else { 1 },
                "\r",
            )
        }
        (Model::Sr7265, "Time Constant") => write_index(io, "TC", value, &S7265_TC, "\r"),
        (Model::Sr7265, "Coupling") => write_coupling(io, "CP", value, "\r"),
        (Model::Sr7265, "Slope") => write_slope(io, "SLOPE", value, &[6, 12, 18, 24], "\r"),
        (Model::Sr7265, "AC Gain") => write_index(io, "ACGAIN", value, &AC_GAIN, "\r"),
        (Model::Sr5210, "Sensitivity") => write_index(io, "SEN", value, &S5210_SENS, "\r"),
        (Model::Sr5210, "Time Constant") => write_index(io, "TC", value, &S5210_TC, "\r"),
        (Model::Sr5210, "Coupling")
            if value
                .as_str()
                .is_some_and(|v| v.trim().eq_ignore_ascii_case("AC")) =>
        {
            Ok(())
        }
        (Model::Sr5210, "Slope") => write_slope(io, "XDB", value, &[6, 12], "\r"),
        (Model::Li5640, "Sensitivity") => write_index(io, "VSEN", value, &SR_SENS, "\n"),
        (Model::Li5640, "Time Constant") => write_index(io, "TCON", value, &SR_TC, "\n"),
        (Model::Li5640, "Coupling") => write_coupling(io, "ICPL", value, "\n"),
        (Model::Li5640, "Slope") => write_slope(io, "SLOP", value, &[6, 12, 18, 24], "\n"),
        _ => Err(format!(
            "{} does not support setting {setting:?}",
            model.name()
        )),
    }
}

pub fn auto(io: &mut impl Io, model: Model, action: &str) -> Result<(), String> {
    let (command, term) = match (model, action) {
        (Model::Sr830, "phase") => ("APHS", "\n"),
        (Model::Sr830, "sensitivity" | "measure") => ("AGAN", "\n"),
        (Model::Sr7265, "phase") => ("AQN", "\r"),
        (Model::Sr7265, "sensitivity") => ("AS", "\r"),
        (Model::Sr7265, "measure") => ("ASM", "\r"),
        (Model::Sr5210, "phase") => ("AQN", "\r"),
        (Model::Sr5210, "sensitivity") => ("AS", "\r"),
        (Model::Sr5210, "measure") => ("ASM", "\r"),
        (Model::Li5640, "phase") => ("APHS", "\n"),
        (Model::Li5640, "sensitivity" | "measure") => ("ASEN", "\n"),
        _ => return Err(format!("Unknown {} auto action: {action}", model.name())),
    };
    io.write(&format!("{command}{term}"))
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
    term: &str,
) -> Result<(), String> {
    let index = match value
        .as_str()
        .map(str::trim)
        .map(str::to_ascii_uppercase)
        .as_deref()
    {
        Some("AC") => 0,
        Some("DC") => 1,
        _ => return Err("Coupling must be AC or DC".into()),
    };
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
fn li5640_ref_freq(io: &mut impl Io) -> Result<f64, String> {
    let saved = io.query("OTYP?\n")?;
    if saved.trim().is_empty() {
        return Err("Unexpected empty response for \"OTYP?\"".into());
    }
    let result = (|| {
        io.write("OTYP 3\n")?;
        query_value(io, "DOUT?\n")
    })();
    let restore = io.write(&format!("OTYP {}\n", saved.trim()));
    match (result, restore) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(restore)) => Err(format!("{error}; restore LI5640 OTYP: {restore}")),
    }
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

fn read_li5640(io: &mut impl Io) -> Result<Signal, String> {
    // Do not alter instrument state until every value needed to restore it is
    // available.  This is stricter than the legacy driver and avoids leaving
    // a display changed when a settings query fails.
    let output_types = io.query("OTYP?\n")?;
    if output_types.trim().is_empty() {
        return Err("Unexpected empty response for \"OTYP?\"".into());
    }
    let display1 = integer(query_value(io, "DDEF? 1\n")?, "DDEF? 1")?;
    let display2 = integer(query_value(io, "DDEF? 2\n")?, "DDEF? 2")?;

    let result = (|| {
        let (x, y) = li5640_pair(io, 0, 0)?;
        let (r, theta) = li5640_pair(io, 1, 1)?;
        signal(x, y, r, theta)
    })();
    let restore = restore_li5640(io, &output_types, display1, display2);
    match (result, restore) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(restore_error)) => {
            Err(format!("{error}; restore LI5640 settings: {restore_error}"))
        }
    }
}

fn li5640_pair(io: &mut impl Io, first: i32, second: i32) -> Result<(f64, f64), String> {
    io.write(&format!("DDEF 1,{first}\n"))?;
    io.write(&format!("DDEF 2,{second}\n"))?;
    std::thread::sleep(std::time::Duration::from_millis(20));
    io.write("OTYP 1,2\n")?;
    let values = query_values(io, "DOUT?\n", 2)?;
    Ok((values[0], values[1]))
}

fn restore_li5640(
    io: &mut impl Io,
    output_types: &str,
    display1: i32,
    display2: i32,
) -> Result<(), String> {
    // Attempt every restoration so one failed write does not prevent the
    // remaining saved settings from being restored.
    let mut errors = Vec::new();
    if let Err(error) = io.write(&format!("OTYP {}\n", output_types.trim())) {
        errors.push(error);
    }
    if let Err(error) = io.write(&format!("DDEF 1,{display1}\n")) {
        errors.push(error);
    }
    if let Err(error) = io.write(&format!("DDEF 2,{display2}\n")) {
        errors.push(error);
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
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
    }
    impl Fake {
        fn new(replies: &[&str]) -> Self {
            Self {
                replies: replies.iter().map(|v| Ok((*v).into())).collect(),
                commands: vec![],
                fail_write: None,
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

    #[test]
    fn li5640_restores_saved_settings() {
        let mut io = Fake::new(&["9,8", "7", "6", "1,2", "3,4"]);
        let got = read(Model::Li5640, &mut io).unwrap();
        assert_eq!(
            (got.x_v, got.y_v, got.r_v, got.theta_deg),
            (1.0, 2.0, 3.0, 4.0)
        );
        assert_eq!(
            io.commands,
            [
                "OTYP?\n",
                "DDEF? 1\n",
                "DDEF? 2\n",
                "DDEF 1,0\n",
                "DDEF 2,0\n",
                "OTYP 1,2\n",
                "DOUT?\n",
                "DDEF 1,1\n",
                "DDEF 2,1\n",
                "OTYP 1,2\n",
                "DOUT?\n",
                "OTYP 9,8\n",
                "DDEF 1,7\n",
                "DDEF 2,6\n"
            ]
        );
    }

    #[test]
    fn li5640_does_not_mutate_without_complete_saved_state() {
        let mut io = Fake::new(&["9,8", "bad"]);
        assert!(read(Model::Li5640, &mut io).is_err());
        assert_eq!(io.commands, ["OTYP?\n", "DDEF? 1\n"]);
    }

    #[test]
    fn li5640_reports_restore_failure_after_attempting_every_restore() {
        let mut io = Fake::new(&["9,8", "7", "6", "1,2", "3,4"]);
        io.fail_write = Some("DDEF 1,7\n");
        let error = read(Model::Li5640, &mut io).unwrap_err();
        assert!(error.contains("write failed"));
        assert_eq!(io.commands.last(), Some(&"DDEF 2,6\n".to_string()));
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
