//! Recorded-data replay through the worker's sample and Stop paths.
use super::*;

impl Session {
    pub(super) fn replay(
        &mut self,
        path: &Path,
        interval: f64,
        cancel: &AtomicBool,
        send: &SyncSender<Event>,
    ) -> Result<(), String> {
        if !crate::replay_data::valid_replay_interval(interval) {
            return Err("Invalid replay interval".into());
        }
        // Documented recordings (with `measurement`) use the same parser as
        // the GUI replay, preserving scan axes, line index, readback
        // positions, and run_id/point_index provenance. Only pre-schema
        // files without that column use the legacy signal-only reader below.
        if crate::replay_data::has_measurement_column(path)? {
            // Parsing a large recording is polled for Stop (per row and before
            // plan inference); a cancelled load ends as a clean Stop.
            let Some(data) =
                crate::replay_data::load_cancellable(path, &|| cancel.load(Ordering::Acquire))?
            else {
                return Ok(());
            };
            for sample in data.samples {
                if cancel.load(Ordering::Acquire)
                    || !self.emit_replay_sample(sample, interval, cancel, send)
                {
                    break;
                }
            }
            return Ok(());
        }
        let mut reader = csv::Reader::from_path(path).map_err(|e| e.to_string())?;
        let headers = reader.headers().map_err(|e| e.to_string())?.clone();
        let modern = headers.iter().any(|value| value == "X");
        for key in if modern {
            ["X", "Y", "R", "Theta_deg"]
        } else {
            ["X_V", "Y_V", "R_V", "Theta_deg"]
        } {
            if !headers.iter().any(|v| v == key) {
                return Err(format!("Replay missing {key}"));
            }
        }
        for record in reader.records() {
            if cancel.load(Ordering::Acquire) {
                break;
            }
            let r = record.map_err(|e| e.to_string())?;
            let get = |key: &str| -> Option<f64> {
                headers
                    .iter()
                    .position(|s| s == key)
                    .and_then(|i| r.get(i))
                    .and_then(|s| s.parse().ok())
            };
            let required = |key| get(key).ok_or_else(|| format!("Replay invalid {key}"));
            let text = |key: &str| {
                headers
                    .iter()
                    .position(|h| h == key)
                    .and_then(|i| r.get(i))
                    .filter(|s| !s.trim().is_empty())
            };
            let flags = text("quality_flags").unwrap_or("legacy_status_unknown");
            let signal = if flags.split('|').any(|f| f == "signal_xy_only") {
                Signal::from_pair(
                    DisplayMode::Xy,
                    required(if modern { "X" } else { "X_V" })?,
                    required(if modern { "Y" } else { "Y_V" })?,
                )?
            } else if flags.split('|').any(|f| f == "signal_rtheta_only") {
                Signal::from_pair(
                    DisplayMode::RTheta,
                    required(if modern { "R" } else { "R_V" })?,
                    required("Theta_deg")?,
                )?
            } else {
                Signal::new(
                    required(if modern { "X" } else { "X_V" })?,
                    required(if modern { "Y" } else { "Y_V" })?,
                    required(if modern { "R" } else { "R_V" })?,
                    required("Theta_deg")?,
                )?
            };
            let target = Target {
                fast: get("target_t_cor_ps")
                    .or_else(|| get("target_x_cor_um"))
                    .or_else(|| get("target_elapsed_s"))
                    .unwrap_or(self.state.rows as f64),
                slow: get("target_y_cor_um"),
                corrected: [
                    get("t_cor_ps").unwrap_or(0.),
                    get("x_cor_um").unwrap_or(0.),
                    get("y_cor_um").unwrap_or(0.),
                ],
                physical: [
                    get("t_ps").unwrap_or(0.),
                    get("x_um").unwrap_or(0.),
                    get("y_um").unwrap_or(0.),
                ],
                line: 0,
            };
            let position = Position {
                physical: [get("t_ps"), get("x_um"), get("y_um")],
                ..Default::default()
            };
            let sample = Sample {
                recording: Box::new(SampleRecording {
                    power_w: get("sample_power_W").or_else(|| get("power_W")),
                    signal_unit: text("signal_unit")
                        .unwrap_or(if modern { "unknown" } else { "V" })
                        .into(),
                    quality_flags: flags.into(),
                    ..Default::default()
                }),
                timing: Default::default(),
                captured_at: None,
                elapsed: get("elapsed_s")
                    .or_else(|| get("acquisition_elapsed_s"))
                    .unwrap_or(0.),
                target,
                signal,
                position,
            };
            if !self.emit_replay_sample(sample, interval, cancel, send) {
                break;
            }
        }
        Ok(())
    }
    /// Publish one replayed row and wait the replay interval; false on Stop.
    fn emit_replay_sample(
        &mut self,
        sample: Sample,
        interval: f64,
        cancel: &AtomicBool,
        send: &SyncSender<Event>,
    ) -> bool {
        self.state.signal = Some(sample.signal);
        self.state.signal_origin = Some(ReadingOrigin::Replay);
        self.state.position = sample.position.clone();
        self.state.rows += 1;
        self.publish_sample(sample, send);
        wait_until(Instant::now() + Duration::from_secs_f64(interval), cancel)
    }
}
fn wait_until(deadline: Instant, cancel: &AtomicBool) -> bool {
    while !cancel.load(Ordering::Acquire) {
        let now = Instant::now();
        if now >= deadline {
            return true;
        }
        thread::sleep((deadline - now).min(Duration::from_millis(20)));
    }
    false
}
