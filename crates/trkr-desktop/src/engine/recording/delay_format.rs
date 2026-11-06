//! Presentation formatting for delay values in the human-facing CSV.
//!
//! The raw CSV remains the source of the exact acquired binary values.  This
//! formatter only makes the ordinary CSV convenient to read while retaining
//! the resolution reported by an observed controller.
use super::csv_float;
use serde::Serialize;
use trkr_core::{Axis, Coordinate, Plan};
use trkr_motion::DelayCalibration;

/// Formatting precision derived from the controller calibration used for a run.
///
/// `None` means use the normal shortest round-trip CSV representation.  It is
/// intentionally also the default for disconnected or merely configured stages:
/// a configuration hint is not an observed pulse resolution.
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub(crate) struct DelayCsvFormat {
    pub readback_decimals: Option<usize>,
    pub target_decimals: Option<usize>,
    pub pulse_resolution_ps: Option<f64>,
}

impl DelayCsvFormat {
    pub(crate) fn from_calibration(calibration: Option<&DelayCalibration>, plan: &Plan) -> Self {
        let Some(calibration) = calibration else {
            return Self::default();
        };
        let (Some(microstep_division), Some(pulse_resolution_ps)) = (
            calibration.microstep_division,
            calibration.effective_ps_per_pulse,
        ) else {
            return Self::default();
        };
        if microstep_division == 0 || !pulse_resolution_ps.is_finite() || pulse_resolution_ps <= 0.
        {
            return Self::default();
        }

        let readback_decimals = decimals_for_grid(pulse_resolution_ps);
        let target_decimals = readback_decimals.and_then(|readback| {
            let input_decimals = match plan.coordinate {
                Coordinate::Measurement => time_input_decimals(plan),
                Coordinate::Interface => interface_input_decimals(plan, calibration),
                Coordinate::Instrument => Some(readback),
            }?;
            let decimals = input_decimals.max(readback);
            (decimals <= 15).then_some(decimals)
        });

        Self {
            readback_decimals,
            target_decimals,
            pulse_resolution_ps: Some(pulse_resolution_ps),
        }
    }

    pub(crate) fn readback(self, value: f64) -> String {
        fixed_or_shortest(value, self.readback_decimals)
    }

    pub(crate) fn target(self, value: f64) -> String {
        fixed_or_shortest(value, self.target_decimals)
    }
}

fn time_input_decimals(plan: &Plan) -> Option<usize> {
    if plan.workflow == trkr_core::Workflow::SignalMonitor {
        return Some(0);
    }
    [
        (plan.fast == Axis::T).then_some(plan.fast_range),
        (plan.workflow.is_2d() && plan.slow == Axis::T).then_some(plan.slow_range),
    ]
    .into_iter()
    .flatten()
    .map(|range| {
        [range.min, range.max, range.step]
            .into_iter()
            .map(decimal_precision)
            .max()
            .unwrap_or(0)
    })
    .max()
    .or(Some(0))
}

fn interface_input_decimals(plan: &Plan, calibration: &DelayCalibration) -> Option<usize> {
    let pos_mm_per_pulse = calibration.effective_pos_um_per_pulse? / 1000.;
    let ps_per_pulse = calibration.effective_ps_per_pulse?;
    if !pos_mm_per_pulse.is_finite()
        || pos_mm_per_pulse <= 0.
        || !ps_per_pulse.is_finite()
        || ps_per_pulse <= 0.
    {
        return None;
    }
    let input = time_input_decimals(plan)?;
    // A shortest decimal input with `input` fractional places describes this
    // grid in mm.  Convert that grid through pulses to canonical ps and retain
    // one guard decimal for the conversion.
    let grid_mm = 10_f64.powi(-(input as i32));
    let grid_ps = grid_mm / pos_mm_per_pulse * ps_per_pulse;
    decimals_for_grid(grid_ps)
}

fn decimals_for_grid(grid: f64) -> Option<usize> {
    if !grid.is_finite() || grid <= 0. {
        return None;
    }
    let decimals = ((-grid.log10()).ceil() + 1.).max(0.);
    (decimals.is_finite() && decimals <= 15.).then_some(decimals as usize)
}

/// Fractional places in Rust's shortest round-trip decimal spelling of a f64.
fn decimal_precision(value: f64) -> usize {
    if !value.is_finite() {
        return 16;
    }
    let text = format!("{value:?}");
    let (mantissa, exponent) = text
        .trim_start_matches('-')
        .split_once('e')
        .map(|(mantissa, exponent)| (mantissa, exponent.parse::<i32>().unwrap_or(0)))
        .unwrap_or((text.trim_start_matches('-'), 0));
    let fraction = mantissa
        .split_once('.')
        .map(|(_, fraction)| fraction.trim_end_matches('0').len() as i32)
        .unwrap_or(0);
    (fraction - exponent).max(0) as usize
}

fn fixed_or_shortest(value: f64, decimals: Option<usize>) -> String {
    let Some(decimals) = decimals.filter(|_| value.is_finite()) else {
        return csv_float(value);
    };
    let mut text = format!("{value:.decimals$}");
    if text.starts_with("-0")
        && text[1..]
            .chars()
            .all(|character| character == '0' || character == '.')
    {
        text.remove(0);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use trkr_core::{Range, Workflow};

    fn calibration(ps: f64, pos_um: f64) -> DelayCalibration {
        calibration_with_micro(ps, pos_um, Some(20))
    }

    fn calibration_with_micro(
        ps: f64,
        pos_um: f64,
        microstep_division: Option<u32>,
    ) -> DelayCalibration {
        DelayCalibration {
            configuration: None,
            controller: "test".into(),
            stage: None,
            port: "test".into(),
            configured_pos_um_per_pulse: Some(pos_um),
            effective_pos_um_per_pulse: Some(pos_um),
            effective_ps_per_pulse: Some(ps),
            microstep_division,
            full_step_ps: Some(ps * 20.),
        }
    }

    fn t_plan() -> Plan {
        Plan {
            workflow: Workflow::Trkr,
            fast: Axis::T,
            ..Plan::default()
        }
    }

    #[test]
    fn sgsp_20um_microstep_resolution_and_round_trips_get_guard_decimal() {
        const LIGHT_MM_PER_PS: f64 = 0.299_792_458;
        let plan = t_plan();
        let micro_20 = 2. * 0.001 / LIGHT_MM_PER_PS;
        let micro_80 = 2. * 0.000_25 / LIGHT_MM_PER_PS;
        let three_round_trips = 6. * 0.001 / LIGHT_MM_PER_PS;
        let mut sgsp_micro_80 = calibration_with_micro(micro_80, 0.25, Some(80));
        sgsp_micro_80.full_step_ps = Some(micro_80 * 80.);
        assert_eq!(
            DelayCsvFormat::from_calibration(Some(&calibration(micro_20, 1.)), &plan)
                .readback_decimals,
            Some(4)
        );
        assert_eq!(
            DelayCsvFormat::from_calibration(Some(&sgsp_micro_80), &plan).readback_decimals,
            Some(4)
        );
        assert_eq!(
            DelayCsvFormat::from_calibration(Some(&calibration(three_round_trips, 1.)), &plan)
                .readback_decimals,
            Some(3)
        );
    }

    #[test]
    fn negative_and_near_zero_values_are_fixed_without_negative_zero() {
        let format = DelayCsvFormat {
            readback_decimals: Some(3),
            target_decimals: Some(3),
            pulse_resolution_ps: Some(0.02),
        };
        assert_eq!(format.readback(-0.0001), "0.000");
        assert_eq!(format.target(-1.2344), "-1.234");
    }

    #[test]
    fn fine_nominal_input_is_preserved_above_coarse_motion_step() {
        let mut plan = t_plan();
        plan.fast_range = Range {
            min: 0.000001,
            max: 20.000001,
            step: 10.,
        };
        let format = DelayCsvFormat::from_calibration(Some(&calibration(0.02, 1.)), &plan);
        assert_eq!(format.target_decimals, Some(6));
        assert_eq!(format.target(0.000001), "0.000001");
    }

    #[test]
    fn tiny_measurement_steps_remain_distinct() {
        let mut plan = t_plan();
        plan.fast_range = Range {
            min: 0.,
            max: 0.000000002,
            step: 0.000000001,
        };
        let format = DelayCsvFormat::from_calibration(Some(&calibration(0.02, 1.)), &plan);
        assert_eq!(format.target_decimals, Some(9));
        assert_ne!(format.target(0.), format.target(0.000000001));
    }

    #[test]
    fn interface_grid_is_converted_through_the_observed_scale() {
        let mut plan = t_plan();
        plan.coordinate = Coordinate::Interface;
        plan.fast_range = Range {
            min: 0.000001,
            max: 0.000003,
            step: 0.000001,
        };
        // 0.001 mm per pulse and 0.02 ps per pulse: 1 nm maps to 2e-5 ps.
        let format = DelayCsvFormat::from_calibration(Some(&calibration(0.02, 1.)), &plan);
        assert_eq!(format.target_decimals, Some(6));
    }

    #[test]
    fn absent_or_unrepresentable_precision_uses_round_trip_output() {
        let plan = t_plan();
        assert_eq!(
            DelayCsvFormat::from_calibration(None, &plan).readback(1.25),
            csv_float(1.25)
        );
        let format = DelayCsvFormat::from_calibration(Some(&calibration(1e-20, 1.)), &plan);
        assert_eq!(format.readback_decimals, None);
        assert_eq!(format.readback(1.25), csv_float(1.25));

        let mut fine_plan = t_plan();
        fine_plan.fast_range.step = 1e-16;
        let format = DelayCsvFormat::from_calibration(Some(&calibration(0.02, 1.)), &fine_plan);
        assert_eq!(format.target_decimals, None);
        assert_eq!(format.target(1.25), csv_float(1.25));
    }

    #[test]
    fn invalid_or_unobserved_calibration_falls_back_to_round_trip_output() {
        let plan = t_plan();
        for calibration in [
            calibration_with_micro(0.02, 1., None),
            calibration_with_micro(0.02, 1., Some(0)),
            calibration(f64::NAN, 1.),
            calibration(0., 1.),
        ] {
            let format = DelayCsvFormat::from_calibration(Some(&calibration), &plan);
            assert_eq!(format.readback_decimals, None);
            assert_eq!(format.readback(1.25), csv_float(1.25));
        }
    }

    #[test]
    fn only_active_t_axes_contribute_nominal_precision() {
        let mut strkr = t_plan();
        strkr.workflow = Workflow::Strkr;
        strkr.fast = Axis::X;
        strkr.slow = Axis::T;
        strkr.slow_range = Range {
            min: 0.000001,
            max: 0.000002,
            step: 0.000001,
        };
        assert_eq!(
            DelayCsvFormat::from_calibration(Some(&calibration(0.02, 1.)), &strkr).target_decimals,
            Some(6)
        );

        let mut inactive_slow = t_plan();
        inactive_slow.slow = Axis::T;
        inactive_slow.slow_range.step = 1e-16;
        assert_eq!(
            DelayCsvFormat::from_calibration(Some(&calibration(0.02, 1.)), &inactive_slow)
                .target_decimals,
            Some(3)
        );

        let mut monitor = t_plan();
        monitor.workflow = Workflow::SignalMonitor;
        monitor.fast_range.step = 1e-16;
        assert_eq!(
            DelayCsvFormat::from_calibration(Some(&calibration(0.02, 1.)), &monitor)
                .target_decimals,
            Some(3)
        );
    }

    #[test]
    fn rounding_error_is_below_a_twentieth_of_each_pulse_resolution() {
        for (q, value) in [(0.02, 12.34567), (10., 12.49), (20., 12.49)] {
            let format = DelayCsvFormat::from_calibration(Some(&calibration(q, 1.)), &t_plan());
            let rounded = format.readback(value).parse::<f64>().unwrap();
            assert!((rounded - value).abs() <= q / 20., "q={q}");
        }
        assert_eq!(decimal_precision(10.), 0);
    }
}
