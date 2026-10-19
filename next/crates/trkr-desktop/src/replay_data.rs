//! Read completed Python or native TRKR CSV files for offline replay.
//!
//! This deliberately accepts the documented Python replay schema rather than
//! trying to infer meaning from arbitrary columns.  Actual readback positions
//! remain optional in [`Sample`]; a missing position is never converted to 0.
use crate::engine::Sample;
use csv::StringRecord;
use std::{collections::BTreeMap, fs::File, path::Path};
use trkr_core::{Axis, MAX_TOTAL_POINTS, Plan, Range, Signal, Target, Workflow, rotated_to_xy};
use trkr_motion::Position;

const MAX_FILE_ROWS: usize = MAX_TOTAL_POINTS;

#[derive(Clone, Debug)]
pub struct Dataset {
    pub plan: Plan,
    pub samples: Vec<Sample>,
}

pub fn load(path: &Path) -> Result<Dataset, String> {
    let file = File::open(path).map_err(|e| format!("Open {}: {e}", path.display()))?;
    let mut reader = csv::ReaderBuilder::new().flexible(false).from_reader(file);
    let headers = reader
        .headers()
        .map_err(|e| format!("Read CSV header: {e}"))?
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let name = name.trim_start_matches('\u{feff}').trim();
            if name.is_empty() {
                Err(format!("CSV header column {} is empty", i + 1))
            } else {
                Ok((name.to_owned(), i))
            }
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    if !headers.contains_key("measurement") {
        return Err("CSV is missing required column measurement".into());
    }

    let mut rows = Vec::new();
    for (index, record) in reader.records().enumerate() {
        let row = index + 2;
        if rows.len() == MAX_FILE_ROWS {
            return Err(format!("CSV has more than {MAX_FILE_ROWS} rows"));
        }
        rows.push(parse_row(
            record.map_err(|e| format!("Row {row}: {e}"))?,
            &headers,
            row,
        )?);
    }
    if rows.is_empty() {
        return Err("CSV has no measurement rows".into());
    }

    let workflow = rows[0].workflow;
    let axes = rows[0].axes;
    for row in &rows[1..] {
        if row.workflow != workflow {
            return Err(format!(
                "Row {}: measurement changes within CSV",
                row.number
            ));
        }
        if row.axes != axes {
            return Err(format!("Row {}: scan axes change within CSV", row.number));
        }
    }
    validate_axes(workflow, axes.0, axes.1, rows[0].number)?;
    let theta_deg = common_theta(&rows)?;
    let zero = infer_zero(&rows);
    let mut plan = Plan {
        workflow,
        fast: axes.0,
        slow: axes.1.unwrap_or(Axis::X),
        theta_deg,
        zero,
        ..Plan::default()
    };
    if workflow == Workflow::SignalMonitor {
        plan.n_points = rows.len();
        plan.interval_s = median_step(rows.iter().map(|r| r.elapsed).collect()).unwrap_or(0.0);
    } else {
        plan.fast_range = replay_range(rows.iter().map(|r| r.fast).collect(), "fast")?;
        if axes.1.is_some() {
            plan.slow_range = replay_range(rows.iter().filter_map(|r| r.slow).collect(), "slow")?;
        }
    }
    let mut slow_values = Vec::new();
    for row in &rows {
        if let Some(value) = row.slow
            && !slow_values
                .iter()
                .any(|known: &f64| (known - value).abs() < 1e-9)
        {
            slow_values.push(value);
        }
    }
    let samples = rows
        .into_iter()
        .map(|row| {
            let line = row
                .slow
                .and_then(|value| {
                    slow_values
                        .iter()
                        .position(|known| (*known - value).abs() < 1e-9)
                })
                .unwrap_or(0);
            row.into_sample(&plan, line)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Dataset { plan, samples })
}

#[derive(Clone)]
struct Row {
    number: usize,
    workflow: Workflow,
    axes: (Axis, Option<Axis>),
    elapsed: f64,
    fast: f64,
    slow: Option<f64>,
    corrected: [Option<f64>; 3],
    physical: [Option<f64>; 3],
    spatial_theta_deg: Option<f64>,
    signal: Signal,
    position: Position,
}

impl Row {
    fn into_sample(self, plan: &Plan, line: usize) -> Result<Sample, String> {
        let mut corrected = self.corrected.map(|v| v.unwrap_or(0.0));
        set_axis(&mut corrected, self.axes.0, self.fast);
        if let Some(axis) = self.axes.1 {
            set_axis(&mut corrected, axis, self.slow.unwrap());
        }
        let u = axis_value(self.axes.0, self.fast, Axis::U).or_else(|| {
            self.axes
                .1
                .zip(self.slow)
                .and_then(|(axis, value)| axis_value(axis, value, Axis::U))
        });
        let v = axis_value(self.axes.0, self.fast, Axis::V).or_else(|| {
            self.axes
                .1
                .zip(self.slow)
                .and_then(|(axis, value)| axis_value(axis, value, Axis::V))
        });
        if u.is_some() || v.is_some() {
            let (x, y) = rotated_to_xy(u.unwrap_or(0.0), v.unwrap_or(0.0), plan.theta_deg);
            corrected[1] = x;
            corrected[2] = y;
        }
        let physical = std::array::from_fn(|i| corrected[i] + plan.zero[i]);
        Ok(Sample {
            elapsed: self.elapsed,
            target: Target {
                fast: self.fast,
                slow: self.slow,
                corrected,
                physical,
                line,
            },
            signal: self.signal,
            position: self.position,
        })
    }
}

fn parse_row(
    record: StringRecord,
    h: &BTreeMap<String, usize>,
    row_number: usize,
) -> Result<Row, String> {
    let get = |key: &str| column(&record, h, key);
    let number = |key: &str| match get(key) {
        Some(v) => parse_number(v, row_number, key).map(Some),
        None => Ok(None),
    };
    let measurement =
        get("measurement").ok_or_else(|| format!("Row {row_number}: missing measurement"))?;
    let workflow = match measurement.to_ascii_lowercase().as_str() {
        "signal_monitor" => Workflow::SignalMonitor,
        "trkr" => Workflow::Trkr,
        "srkr" => Workflow::Srkr,
        "strkr" => Workflow::Strkr,
        "srkr_2d" => Workflow::Srkr2d,
        _ => {
            return Err(format!(
                "Row {row_number}: unsupported measurement {measurement:?}"
            ));
        }
    };
    let fast_axis = match workflow {
        Workflow::SignalMonitor | Workflow::Trkr => Axis::T,
        Workflow::Srkr => infer_srkr_axis(&record, h, row_number)?,
        Workflow::Strkr | Workflow::Srkr2d => axis(get("fast_axis"), row_number, "fast_axis")?,
    };
    let slow_axis = match workflow {
        Workflow::Strkr | Workflow::Srkr2d => {
            Some(axis(get("slow_axis"), row_number, "slow_axis")?)
        }
        _ => None,
    };
    let x = number("X_V")?;
    let y = number("Y_V")?;
    let r = number("R_V")?;
    let theta = number("Theta_deg")?;
    let signal = signal(x, y, r, theta, row_number)?;
    let corrected = [
        coordinate(&record, h, row_number, Axis::T)?,
        coordinate(&record, h, row_number, Axis::X)?,
        coordinate(&record, h, row_number, Axis::Y)?,
    ];
    let fast = if workflow == Workflow::SignalMonitor {
        number("target_elapsed_s")?
            .unwrap_or_else(|| number("elapsed_s").ok().flatten().unwrap_or(0.0))
    } else {
        coordinate(&record, h, row_number, fast_axis)?.ok_or_else(|| {
            format!(
                "Row {row_number}: missing replay position for fast axis {}",
                fast_axis.name()
            )
        })?
    };
    let slow = match slow_axis {
        Some(axis) => Some(coordinate(&record, h, row_number, axis)?.ok_or_else(|| {
            format!(
                "Row {row_number}: missing replay position for slow axis {}",
                axis.name()
            )
        })?),
        None => None,
    };
    let physical = [number("t_ps")?, number("x_um")?, number("y_um")?];
    Ok(Row {
        number: row_number,
        workflow,
        axes: (fast_axis, slow_axis),
        elapsed: number("elapsed_s")?.unwrap_or(0.0),
        fast,
        slow,
        corrected,
        physical,
        spatial_theta_deg: number("spatial_theta_deg")?,
        signal,
        position: Position {
            physical,
            delay_stage_mm: number("delay_stage_mm")?,
            delay_stage_pulse: integer(get("delay_stage_pulse"), row_number, "delay_stage_pulse")?,
            x_scanner_mm: number("x_scanner_mm")?,
            x_scanner_deg: number("x_scanner_deg")?,
            y_scanner_mm: number("y_scanner_mm")?,
            y_scanner_deg: number("y_scanner_deg")?,
        },
    })
}

fn column<'a>(record: &'a StringRecord, h: &BTreeMap<String, usize>, key: &str) -> Option<&'a str> {
    h.get(key)
        .and_then(|&i| record.get(i))
        .map(str::trim)
        .filter(|v| !v.is_empty())
}
fn first_number(
    record: &StringRecord,
    h: &BTreeMap<String, usize>,
    row: usize,
    keys: &[&str],
) -> Result<Option<f64>, String> {
    for key in keys {
        if let Some(value) = column(record, h, key) {
            return parse_number(value, row, key).map(Some);
        }
    }
    Ok(None)
}
fn coordinate(
    record: &StringRecord,
    h: &BTreeMap<String, usize>,
    row: usize,
    axis: Axis,
) -> Result<Option<f64>, String> {
    first_number(record, h, row, &coordinate_keys(axis))
}
fn coordinate_keys(axis: Axis) -> [&'static str; 3] {
    match axis {
        Axis::T => ["t_cor_ps", "target_t_cor_ps", "t_ps"],
        Axis::X => ["x_cor_um", "target_x_cor_um", "x_um"],
        Axis::Y => ["y_cor_um", "target_y_cor_um", "y_um"],
        Axis::U => ["u_cor_um", "target_u_cor_um", "u_um"],
        Axis::V => ["v_cor_um", "target_v_cor_um", "v_um"],
    }
}
fn parse_number(value: &str, row: usize, key: &str) -> Result<f64, String> {
    let n = value
        .parse::<f64>()
        .map_err(|_| format!("Row {row}: {key} must be a finite number"))?;
    if n.is_finite() {
        Ok(n)
    } else {
        Err(format!("Row {row}: {key} must be finite"))
    }
}
fn integer(value: Option<&str>, row: usize, key: &str) -> Result<Option<i64>, String> {
    value
        .map(|v| {
            v.parse::<i64>()
                .map_err(|_| format!("Row {row}: {key} must be an integer"))
        })
        .transpose()
}
fn axis(value: Option<&str>, row: usize, key: &str) -> Result<Axis, String> {
    match value.unwrap_or("").to_ascii_lowercase().as_str() {
        "t" => Ok(Axis::T),
        "x" => Ok(Axis::X),
        "y" => Ok(Axis::Y),
        "u" => Ok(Axis::U),
        "v" => Ok(Axis::V),
        _ => Err(format!("Row {row}: invalid {key}")),
    }
}
fn infer_srkr_axis(
    record: &StringRecord,
    h: &BTreeMap<String, usize>,
    row: usize,
) -> Result<Axis, String> {
    if let Ok(axis @ (Axis::X | Axis::Y | Axis::U | Axis::V)) =
        axis(column(record, h, "fast_axis"), row, "fast_axis")
    {
        return Ok(axis);
    }
    let populated: Vec<_> = [Axis::X, Axis::Y, Axis::U, Axis::V]
        .into_iter()
        .filter(|&axis| {
            coordinate_keys(axis)
                .iter()
                .any(|key| column(record, h, key).is_some())
        })
        .collect();
    match populated.as_slice() {
        [axis] => Ok(*axis),
        _ => Err(format!("Row {row}: cannot determine the SRKR scan axis")),
    }
}
fn signal(
    x: Option<f64>,
    y: Option<f64>,
    r: Option<f64>,
    theta: Option<f64>,
    row: usize,
) -> Result<Signal, String> {
    let (x, y) = match (x, y, r, theta) {
        (Some(x), Some(y), _, _) => (x, y),
        (None, None, Some(r), Some(theta)) if r >= 0. => {
            let a = theta.to_radians();
            (r * a.cos(), r * a.sin())
        }
        _ => return Err(format!("Row {row}: require X_V/Y_V or R_V/Theta_deg")),
    };
    let r = r.unwrap_or_else(|| x.hypot(y));
    if r < 0. {
        return Err(format!("Row {row}: R_V must be non-negative"));
    }
    let theta = theta.unwrap_or_else(|| y.atan2(x).to_degrees());
    Signal::new(x, y, r, theta).map_err(|e| format!("Row {row}: {e}"))
}
fn validate_axes(
    workflow: Workflow,
    fast: Axis,
    slow: Option<Axis>,
    row: usize,
) -> Result<(), String> {
    let ok = match workflow {
        Workflow::SignalMonitor => slow.is_none(),
        Workflow::Trkr => fast == Axis::T && slow.is_none(),
        Workflow::Srkr => fast != Axis::T && slow.is_none(),
        Workflow::Strkr => matches!(
            (fast, slow),
            (Axis::T, Some(Axis::X | Axis::Y | Axis::U | Axis::V))
                | (Axis::X | Axis::Y | Axis::U | Axis::V, Some(Axis::T))
        ),
        Workflow::Srkr2d => matches!(
            (fast, slow),
            (Axis::X, Some(Axis::Y))
                | (Axis::Y, Some(Axis::X))
                | (Axis::U, Some(Axis::V))
                | (Axis::V, Some(Axis::U))
        ),
    };
    if ok {
        Ok(())
    } else {
        Err(format!(
            "Row {row}: invalid {} axes {}/{}",
            workflow.key(),
            fast.name(),
            slow.map(Axis::name).unwrap_or("-")
        ))
    }
}
fn common_theta(rows: &[Row]) -> Result<f64, String> {
    let mut values = rows.iter().filter_map(|r| r.spatial_theta_deg);
    let Some(theta) = values.next() else {
        return Ok(0.0);
    };
    if values.any(|value| (value - theta).abs() > 1e-9) {
        return Err("spatial_theta_deg changes within CSV".into());
    }
    Ok(theta)
}
fn infer_zero(rows: &[Row]) -> [f64; 3] {
    std::array::from_fn(|i| {
        rows.iter()
            .find_map(|r| r.physical[i].zip(r.corrected[i]).map(|(p, c)| p - c))
            .unwrap_or(0.0)
    })
}
fn set_axis(corrected: &mut [f64; 3], axis: Axis, value: f64) {
    match axis {
        Axis::T => corrected[0] = value,
        Axis::X => corrected[1] = value,
        Axis::Y => corrected[2] = value,
        Axis::U | Axis::V => {}
    }
}
fn axis_value(axis: Axis, value: f64, wanted: Axis) -> Option<f64> {
    (axis == wanted).then_some(value)
}
fn replay_range(mut values: Vec<f64>, name: &str) -> Result<Range, String> {
    values.sort_by(f64::total_cmp);
    values.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    if values.is_empty() {
        return Err(format!("No {name} coordinates"));
    }
    if values.len() == 1 {
        return Ok(Range {
            min: values[0],
            max: values[0],
            step: 1.0,
        });
    }
    let step = values
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .filter(|spacing| *spacing > 0.)
        .min_by(f64::total_cmp)
        .ok_or_else(|| format!("No {name} coordinate spacing"))?;
    Ok(Range {
        min: values[0],
        max: *values.last().unwrap(),
        step,
    })
}
fn median_step(mut values: Vec<f64>) -> Option<f64> {
    values.sort_by(f64::total_cmp);
    let mut ds: Vec<f64> = values
        .windows(2)
        .map(|v| v[1] - v[0])
        .filter(|v| *v > 0.)
        .collect();
    ds.sort_by(f64::total_cmp);
    ds.get(ds.len() / 2).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn csv(body: &str) -> tempfile::NamedTempFile {
        let f = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(f.path(), body).unwrap();
        f
    }
    fn header() -> &'static str {
        "measurement,fast_axis,slow_axis,target_elapsed_s,target_t_cor_ps,target_x_cor_um,target_y_cor_um,target_u_cor_um,target_v_cor_um,t_cor_ps,x_cor_um,y_cor_um,t_ps,x_um,y_um,X_V,Y_V,R_V,Theta_deg,spatial_theta_deg,elapsed_s,u_cor_um,v_cor_um,u_um,v_um\n"
    }
    fn row(measurement: &str, fast: &str, slow: &str) -> Vec<String> {
        let mut v = vec![String::new(); 25];
        v[0] = measurement.into();
        v[1] = fast.into();
        v[2] = slow.into();
        v[4] = "0".into();
        v[5] = "0".into();
        v[6] = "0".into();
        v[7] = "0".into();
        v[8] = "0".into();
        v[15] = "1".into();
        v[16] = "2".into();
        v[17] = "2.2360679".into();
        v[18] = "63.4349".into();
        v
    }
    fn text(v: Vec<String>) -> String {
        format!("{}{}\n", header(), v.join(","))
    }
    #[test]
    fn reads_all_workflows() {
        for (measurement, fast, slow) in [
            ("signal_monitor", "", ""),
            ("trkr", "t", ""),
            ("srkr", "x", ""),
            ("strkr", "t", "x"),
            ("srkr_2d", "x", "y"),
        ] {
            let mut r = row(measurement, fast, slow);
            if measurement == "signal_monitor" {
                r[3] = "0".into();
            }
            let f = csv(&text(r));
            let data = load(f.path()).unwrap();
            assert_eq!(data.plan.workflow.key(), measurement);
            assert_eq!(data.samples.len(), 1);
        }
    }
    #[test]
    fn derives_r_theta_from_xy_and_keeps_sparse_position() {
        let mut r = row("trkr", "t", "");
        r[17].clear();
        r[18].clear();
        let f = csv(&text(r));
        let d = load(f.path()).unwrap();
        assert_eq!(d.samples[0].signal.r_v, 5f64.sqrt());
        assert!((d.samples[0].signal.theta_deg - 63.4349).abs() < 1e-3);
        assert_eq!(d.samples[0].position.physical, [None, None, None]);
    }
    #[test]
    fn derives_xy_from_r_theta() {
        let mut r = row("trkr", "t", "");
        r[15].clear();
        r[16].clear();
        r[17] = "2".into();
        r[18] = "90".into();
        let f = csv(&text(r));
        let d = load(f.path()).unwrap();
        assert!(d.samples[0].signal.x_v.abs() < 1e-10);
        assert!((d.samples[0].signal.y_v - 2.).abs() < 1e-10);
    }
    #[test]
    fn restores_uv_grid_theta_and_line() {
        let mut first = row("srkr_2d", "u", "v");
        first[7] = "2".into();
        first[8] = "3".into();
        first[19] = "90".into();
        let mut second = first.clone();
        second[8] = "4".into();
        let f = csv(&format!(
            "{}{}\n{}\n",
            header(),
            first.join(","),
            second.join(",")
        ));
        let d = load(f.path()).unwrap();
        assert_eq!(d.plan.theta_deg, 90.);
        assert!((d.samples[0].target.corrected[1] + 3.).abs() < 1e-9);
        assert!((d.samples[0].target.corrected[2] - 2.).abs() < 1e-9);
        assert_eq!(d.samples[1].target.line, 1);
    }
    #[test]
    fn python_compatible_axis_defaults_cover_implicit_trkr_and_srkr() {
        let mut trkr = row("trkr", "", "");
        trkr[1] = "not-an-axis".into();
        let trkr_file = csv(&text(trkr));
        assert_eq!(load(trkr_file.path()).unwrap().plan.fast, Axis::T);

        let mut srkr = row("srkr", "", "");
        for index in [5, 7, 8, 10, 13, 21, 22, 23, 24] {
            srkr[index].clear();
        }
        srkr[6] = "12".into();
        let srkr_file = csv(&text(srkr));
        assert_eq!(load(srkr_file.path()).unwrap().plan.fast, Axis::Y);
    }
    #[test]
    fn corrected_coordinates_win_and_uv_can_fall_back_to_raw_values() {
        let mut trkr = row("trkr", "t", "");
        trkr[4] = "22".into();
        trkr[9] = "11".into();
        trkr[12] = "33".into();
        let trkr_file = csv(&text(trkr));
        let trkr = load(trkr_file.path()).unwrap();
        assert_eq!(trkr.samples[0].target.fast, 11.);
        assert_eq!(trkr.samples[0].target.corrected[0], 11.);

        let mut srkr = row("srkr", "u", "");
        srkr[7].clear();
        srkr[23] = "5".into();
        let srkr_file = csv(&text(srkr));
        let srkr = load(srkr_file.path()).unwrap();
        assert_eq!(srkr.samples[0].target.fast, 5.);
    }
    #[test]
    fn irregular_readback_coordinates_use_minimum_positive_spacing() {
        let mut first = row("trkr", "", "");
        first[9] = "0".into();
        let mut second = first.clone();
        second[9] = "3".into();
        let mut third = first.clone();
        third[9] = "10".into();
        let file = csv(&format!(
            "{}{}\n{}\n{}\n",
            header(),
            first.join(","),
            second.join(","),
            third.join(",")
        ));
        let data = load(file.path()).unwrap();
        assert_eq!(data.plan.fast_range.min, 0.);
        assert_eq!(data.plan.fast_range.max, 10.);
        assert_eq!(data.plan.fast_range.step, 3.);
    }
    #[test]
    fn rejects_bad_csv() {
        let mut r = row("trkr", "t", "");
        r[16] = "not-a-number".into();
        let f = csv(&text(r));
        assert!(load(f.path()).unwrap_err().contains("Y_V"));
    }
}
