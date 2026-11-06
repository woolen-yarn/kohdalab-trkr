//! Read-only run-start reference settings. Never used by periodic GUI refresh.
//! Command sources and units are listed in docs/reference/measurement-integrity.md.
use super::{Io, Model, integer, query_value, query_values};
use serde_json::{Value, json};

pub(super) fn read(io: &mut impl Io, model: Model) -> Result<Value, String> {
    let (phase, source, harmonic, native_phase) = match model {
        Model::Sr830 => (
            query_value(io, "PHAS?\n")?,
            source(io, "FMOD?\n", &["external", "internal"])?,
            harmonic(io, "HARM?\n", 19999)?,
            Value::Null,
        ),
        Model::Sr7265 => (
            query_value(io, "REFP.\r")?,
            source(io, "IE\r", &["internal", "external_ttl", "external_analog"])?,
            harmonic(io, "REFN\r", 65535)?,
            Value::Null,
        ),
        Model::Li5640 => (
            query_value(io, "PHAS?\n")?,
            source(io, "RSRC?\n", &["external", "internal", "signal"])?,
            harmonic(io, "HARM?\n", 19999)?,
            Value::Null,
        ),
        Model::Sr5210 => {
            let values = query_values(io, "P\r", 2)?;
            let quadrant = integer(values[0], "5210 phase quadrant")?;
            let fine = integer(values[1], "5210 phase millidegrees")?;
            if !(0..=3).contains(&quadrant) || !(0..=100000).contains(&fine) {
                return Err("5210 reference phase is outside the documented range".into());
            }
            let source = source(io, "IE\r", &["external", "internal"])?;
            let double = integer(query_value(io, "F2F\r")?, "5210 harmonic mode")?;
            if !(0..=1).contains(&double) {
                return Err("5210 harmonic mode must be 0 or 1".into());
            }
            (
                f64::from(quadrant) * 90.0 + f64::from(fine) / 1000.0,
                source,
                double + 1,
                json!({"quadrant":quadrant,"within_quadrant_millidegrees":fine,
                    "conversion":"quadrant * 90 + millidegrees / 1000; no wrapping"}),
            )
        }
    };
    Ok(
        json!({"phase_deg":phase,"phase_origin":"instrument_reference_offset_not_signal_theta",
        "source":source,"harmonic":harmonic,"native_phase":native_phase,
        "qualification":"read_only_run_start_snapshot; not simultaneous and not a continuous settings trace"}),
    )
}

fn source(io: &mut impl Io, command: &str, names: &[&str]) -> Result<Value, String> {
    let code = integer(query_value(io, command)?, command)?;
    let name = usize::try_from(code)
        .ok()
        .and_then(|n| names.get(n))
        .ok_or_else(|| format!("Invalid reference source for {command:?}: {code}"))?;
    Ok(json!({"code":code,"name":name}))
}

fn harmonic(io: &mut impl Io, command: &str, maximum: i32) -> Result<i32, String> {
    let value = integer(query_value(io, command)?, command)?;
    if !(1..=maximum).contains(&value) {
        return Err(format!(
            "Invalid reference harmonic for {command:?}: {value}"
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lockin::{recorded, tests::Fake};

    #[test]
    fn reference_settings_use_model_specific_queries_and_keep_exact_replies() {
        for (model, replies, commands, phase, source, harmonic) in [
            (
                Model::Sr830,
                ["-12.3400\r\n", "0", "3"],
                ["PHAS?\n", "FMOD?\n", "HARM?\n"],
                -12.34,
                "external",
                3,
            ),
            (
                Model::Sr7265,
                ["+90.125\r", "0", "2"],
                ["REFP.\r", "IE\r", "REFN\r"],
                90.125,
                "internal",
                2,
            ),
            (
                Model::Li5640,
                ["-179.99\n", "2", "19999"],
                ["PHAS?\n", "RSRC?\n", "HARM?\n"],
                -179.99,
                "signal",
                19999,
            ),
            (
                Model::Sr5210,
                ["3,10000\r", "0", "1"],
                ["P\r", "IE\r", "F2F\r"],
                280.,
                "external",
                2,
            ),
        ] {
            let mut io = Fake::new(&replies);
            let recorded = recorded(&mut io, |io| read(io, model));
            let value = recorded.result.unwrap();
            assert_eq!(value["phase_deg"], phase);
            assert_eq!(value["source"]["name"], source);
            assert_eq!(value["harmonic"], harmonic);
            assert_eq!(
                recorded
                    .exchanges
                    .iter()
                    .map(|e| e.command.as_str())
                    .collect::<Vec<_>>(),
                commands
            );
            assert_eq!(recorded.exchanges[0].reply.as_deref(), Some(replies[0]));
            assert!(recorded.exchanges.iter().all(|e| e.operation == "query"));
        }
    }

    #[test]
    fn invalid_phase_source_and_harmonic_never_become_plausible_settings() {
        for (model, replies) in [
            (Model::Sr830, vec!["NaN"]),
            (Model::Sr7265, vec!["1", "9"]),
            (Model::Li5640, vec!["1", "0", "0"]),
            (Model::Sr5210, vec!["4,0"]),
            (Model::Sr5210, vec!["0,-1"]),
            (Model::Sr5210, vec!["0,0", "0", "2"]),
        ] {
            let mut io = Fake::new(&replies);
            assert!(read(&mut io, model).is_err());
        }
    }
}
