//! Differential fixtures frozen from the v0.2.0 Python measurement helpers.
//!
//! The fixture is generated from archived pure modules only.  It deliberately
//! contains both the Python rows as returned and their archived CSV-boundary
//! representation, so a behavioral difference is visible instead of being
//! hidden by a new golden.

use crate::engine::{self, Sample};
use serde_json::Value;
use trkr_core::{Axis, Coordinate, Plan, Range, Signal, Target, Workflow};
use trkr_motion::Position;

const FIXTURE: &str = include_str!("../../../tests/fixtures/python_measurement_reference.json");

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("valid frozen Python parity fixture")
}

fn axis(value: &str) -> Axis {
    match value {
        "t" => Axis::T,
        "x" => Axis::X,
        "y" => Axis::Y,
        "u" => Axis::U,
        "v" => Axis::V,
        _ => panic!("unknown fixture axis {value}"),
    }
}

fn workflow(value: &str) -> Workflow {
    match value {
        "signal_monitor" => Workflow::SignalMonitor,
        "trkr" => Workflow::Trkr,
        "srkr" => Workflow::Srkr,
        "strkr" => Workflow::Strkr,
        "srkr_2d" => Workflow::Srkr2d,
        _ => panic!("unknown fixture workflow {value}"),
    }
}

fn number(object: &serde_json::Map<String, Value>, key: &str) -> f64 {
    object[key]
        .as_f64()
        .unwrap_or_else(|| panic!("fixture {key} must be a number"))
}

fn optional_number(object: &serde_json::Map<String, Value>, key: &str) -> Option<f64> {
    object.get(key).and_then(Value::as_f64)
}

fn scanner_value(position: &serde_json::Map<String, Value>, axis: &str, unit: &str) -> Option<f64> {
    optional_number(position, &format!("{axis}_scanner_{unit}")).or_else(|| {
        (position
            .get(&format!("scanner_{axis}_unit"))
            .and_then(Value::as_str)
            == Some(unit))
        .then(|| optional_number(position, &format!("scanner_{axis}_value")))
        .flatten()
    })
}

fn plan(case: &serde_json::Map<String, Value>) -> Plan {
    let plan = case["plan"].as_object().expect("fixture plan object");
    let zero: [f64; 3] = plan["zero"]
        .as_array()
        .expect("fixture zero array")
        .iter()
        .map(|value| value.as_f64().expect("numeric zero"))
        .collect::<Vec<_>>()
        .try_into()
        .expect("three fixture zero coordinates");
    Plan {
        workflow: workflow(plan["workflow"].as_str().expect("fixture workflow")),
        fast: axis(plan["fast"].as_str().expect("fixture fast axis")),
        slow: axis(plan["slow"].as_str().expect("fixture slow axis")),
        fast_range: Range {
            min: 0.0,
            max: 0.0,
            step: 1.0,
        },
        slow_range: Range {
            min: 0.0,
            max: 0.0,
            step: 1.0,
        },
        interval_s: 0.0,
        n_points: 1,
        theta_deg: number(plan, "theta_deg"),
        zero,
        coordinate: Coordinate::Measurement,
        return_to_zero: [true, true],
        return_fast_between_lines: true,
    }
}

fn sample(case: &serde_json::Map<String, Value>) -> Sample {
    let sample = case["sample"].as_object().expect("fixture sample object");
    let corrected: [f64; 3] = sample["corrected"]
        .as_array()
        .expect("fixture corrected array")
        .iter()
        .map(|value| value.as_f64().expect("numeric corrected coordinate"))
        .collect::<Vec<_>>()
        .try_into()
        .expect("three fixture corrected coordinates");
    let fallback_physical: [Option<f64>; 3] = sample["physical"]
        .as_array()
        .expect("fixture physical array")
        .iter()
        .map(Value::as_f64)
        .collect::<Vec<_>>()
        .try_into()
        .expect("three fixture physical coordinates");
    let signal = sample["signal"].as_object().expect("fixture signal object");
    let position = sample["position"]
        .as_object()
        .expect("fixture position object");
    // 2D Python rows carry measured positions separately from their targets.
    // Older 1D fixture cases provide only the measured fallback array.
    let measured_physical = ["t_ps", "x_um", "y_um"].map(|key| {
        optional_number(position, key).or_else(|| {
            let index = match key {
                "t_ps" => 0,
                "x_um" => 1,
                "y_um" => 2,
                _ => unreachable!(),
            };
            fallback_physical[index]
        })
    });
    let target_physical = fallback_physical.map(Option::unwrap_or_default);
    Sample {
        elapsed: number(sample, "elapsed_s"),
        target: Target {
            fast: number(sample, "target_fast"),
            slow: optional_number(sample, "target_slow"),
            corrected,
            physical: target_physical,
            line: 0,
        },
        signal: Signal::new(
            number(signal, "X"),
            number(signal, "Y"),
            number(signal, "R"),
            number(signal, "Theta"),
        )
        .expect("fixture signal is valid"),
        position: Position {
            physical: measured_physical,
            delay_stage_mm: optional_number(position, "delay_stage_mm"),
            delay_stage_pulse: position.get("delay_stage_pulse").and_then(Value::as_i64),
            x_scanner_mm: scanner_value(position, "x", "mm"),
            x_scanner_deg: scanner_value(position, "x", "deg"),
            y_scanner_mm: scanner_value(position, "y", "mm"),
            y_scanner_deg: scanner_value(position, "y", "deg"),
        },
    }
}

fn csv_value(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        _ => panic!("fixture CSV value must be scalar"),
    }
}

#[test]
fn range_points_match_frozen_python_build_range_points() {
    let fixture = fixture();
    for case in fixture["range_cases"].as_array().expect("range cases") {
        let case = case.as_object().expect("range case object");
        let actual = Range {
            min: number(case, "min"),
            max: number(case, "max"),
            step: number(case, "step"),
        }
        .points()
        .expect("valid frozen Python range");
        let expected: Vec<f64> = case["points"]
            .as_array()
            .expect("fixture points")
            .iter()
            .map(|value| value.as_f64().expect("numeric fixture point"))
            .collect();
        assert_eq!(actual, expected, "{}", case["id"]);
    }
}

#[test]
fn compatible_csv_rows_replay_with_acquisition_timing_preserved() {
    for case in fixture()["row_cases"].as_array().expect("row cases") {
        let case = case.as_object().expect("row case object");
        let plan = plan(case);
        let sample = sample(case);
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut writer = csv::Writer::from_path(file.path()).unwrap();
        writer.write_record(engine::FIELDS).unwrap();
        writer
            .write_record(engine::row(&plan, &sample).unwrap())
            .unwrap();
        writer.flush().unwrap();
        let replay = crate::replay_data::load(file.path()).unwrap();
        assert_eq!(replay.plan.workflow, plan.workflow);
        assert_eq!(replay.samples.len(), 1);
        assert_eq!(replay.samples[0].elapsed, sample.elapsed);
        assert_eq!(replay.plan.fast, plan.fast);
        if plan.workflow.is_2d() {
            assert_eq!(replay.plan.slow, plan.slow);
        }
    }
}

#[test]
fn rows_match_common_columns_at_frozen_python_csv_boundary() {
    let fixture = fixture();
    for case in fixture["row_cases"].as_array().expect("row cases") {
        let case = case.as_object().expect("row case object");
        let case_id = case["id"].as_str().expect("fixture row id");
        let row = engine::row(&plan(case), &sample(case)).expect("fixture row renders");
        let expected = case["expected_csv"]
            .as_object()
            .expect("expected Python CSV row");
        for (column, expected) in expected {
            // Python and Rust both stamp export time independently.  Every
            // remaining v0.2.0 column is compared; Rust-only U/V columns are
            // intentionally not in this legacy fixture.
            if column == "timestamp" {
                continue;
            }
            let index = engine::FIELDS
                .iter()
                .position(|field| *field == column)
                .unwrap_or_else(|| panic!("{case_id}: missing legacy column {column}"));
            assert_eq!(row[index], csv_value(expected), "{case_id}: {column}");
        }
    }
}
