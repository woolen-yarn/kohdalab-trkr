//! Measurement tab strip and scan/output/run form for the 1D and 2D
//! measurement workflows, plus the per-workflow scan hints it displays.
use super::*;

/// Measurement tabs. Signal Monitor is shown inside Alignment instead.
pub(super) const MEASUREMENT_TABS: [Workflow; 4] = [
    Workflow::Trkr,
    Workflow::Srkr,
    Workflow::Strkr,
    Workflow::Srkr2d,
];
pub(super) const MEASUREMENT_PAGE_TOP: f32 = 30.;
const MEASUREMENT_TAB_WIDTHS: [f32; 4] = [51., 51., 57., 68.];
struct ScanColumn<'a> {
    fast: bool,
    x: f32,
    width: f32,
    tc: Option<f64>,
    replay_wait: bool,
    hints: &'a [String; 3],
}
pub(super) fn show_scan_theta(workflow: Workflow, axis: Axis) -> bool {
    if workflow == Workflow::Srkr2d {
        axis == Axis::U
    } else {
        matches!(axis, Axis::U | Axis::V)
    }
}
pub(super) fn delay_step_hint(
    config: &trkr_motion::DelayConfig,
    cached: Option<&trkr_motion::DelayCalibration>,
) -> String {
    let Ok(normalized) = config.normalized() else {
        return "— ps".into();
    };
    let actual = cached
        .filter(|c| c.configuration.as_ref() == Some(&normalized))
        .and_then(|c| c.effective_ps_per_pulse);
    let hint = normalized.calibration_hint().ok();
    if let Some(step) = actual.or_else(|| hint.as_ref().and_then(|h| h.effective_ps_per_pulse)) {
        format!("≥ {} ps", App::significant(step, 5))
    } else if let Some(full) = hint.and_then(|h| h.full_step_ps) {
        format!("full {} ps", App::significant(full, 5))
    } else {
        "Connect for step".into()
    }
}
pub(super) fn scan_unit(axis: Axis) -> &'static str {
    if axis == Axis::T { "ps" } else { "µm" }
}

impl App {
    pub(super) fn measure(&mut self, ui: &mut egui::Ui, area: egui::Rect) {
        use precision::{Form, rect};
        if !self.demo_viewer {
            self.sync();
        }
        let mut plan = self.plan().clone();
        let running = self.snapshot.running;
        // The workflow is the source of truth: Signal Monitor is the
        // Alignment page, every other workflow is a measurement tab.
        let mut selected = self.workflow;
        let mut x = area.min.x;
        for (i, workflow) in MEASUREMENT_TABS.into_iter().enumerate() {
            let r = rect(x, area.min.y, MEASUREMENT_TAB_WIDTHS[i], 24.);
            precision::record(&mut self.geometry, &format!("tab_{}", workflow.key()), r);
            if ui
                .scope(|ui| {
                    if self.demo_viewer && running {
                        ui.disable();
                    }
                    ui.put(
                        r,
                        egui::Button::new(workflow.name())
                            .selected(workflow == selected)
                            .min_size(r.size())
                            .corner_radius(6),
                    )
                    .clicked()
                })
                .inner
            {
                selected = workflow;
            }
            x += MEASUREMENT_TAB_WIDTHS[i];
        }
        let alignment_rect = rect(x + 3., area.min.y, 82., 24.);
        precision::record(&mut self.geometry, "tab_alignment", alignment_rect);
        // Signal Monitor has no tab of its own; its tab key names the page
        // that hosts it so per-workflow geometry checks still find it.
        precision::record(
            &mut self.geometry,
            &format!("tab_{}", Workflow::SignalMonitor.key()),
            alignment_rect,
        );
        if ui
            .scope(|ui| {
                if self.demo_viewer && running {
                    ui.disable();
                }
                ui.put(
                    alignment_rect,
                    egui::Button::new("Alignment").selected(selected == Workflow::SignalMonitor),
                )
                .clicked()
            })
            .inner
        {
            selected = Workflow::SignalMonitor;
        }
        self.switch_workflow(selected);
        if self.workflow != plan.workflow {
            plan = self.plan().clone();
        }
        if self.alignment_tab {
            let tabs = egui::Rect::from_min_size(area.min, Vec2::new(area.width(), 24.));
            precision::record(&mut self.geometry, "center_top_widget", tabs);
            precision::record(&mut self.geometry, "measurement_tabs", tabs);
            return;
        }
        precision::record(&mut self.geometry, "measurement_tabs", area);
        let page = rect(
            area.min.x,
            area.min.y + MEASUREMENT_PAGE_TOP,
            area.width(),
            area.height() - MEASUREMENT_PAGE_TOP - 4.,
        );
        precision::record(&mut self.geometry, "measurement_page", page);
        ui.painter().rect(
            page,
            10.,
            PANEL,
            Stroke::new(1_f32, BORDER),
            egui::StrokeKind::Inside,
        );
        // The measurement panel deliberately stays useful in the fixed-width
        // centre column.  Keep a readable output field, but reserve the rest
        // for the scan controls instead of stretching labels and inputs.
        let side = 250_f32.min((page.width() - 8. - 216.).max(180.));
        let settings = page.width() - 24. - side;
        let side_x = settings + 12.;
        let tc = self.workflow_time_constant(plan.workflow);

        let hint_config = self.workflow_hardware(plan.workflow);
        let hints_fast = self.scan_hints(plan.fast, &plan, hint_config.as_ref());
        let hints_slow = self.scan_hints(plan.slow, &plan, hint_config.as_ref());
        let scan_column_width = ((settings - 24.) * 0.5).round();
        let availability = self.start_availability(&plan);
        let (start, stop, save, browse, settings_clicked);
        {
            let mut form = Form::new(ui, &mut self.geometry, page.min);
            form.enabled = !running && !self.demo_viewer;
            if !plan.workflow.is_2d() {
                let prefix = plan.workflow.key();
                let label = 66.;
                let field = 9. + label + 6.;
                let input = 88.;
                form.label(
                    &format!("{prefix}_axis_label"),
                    rect(9., 9., label, 26.),
                    "Axis",
                );
                let options = if plan.workflow == Workflow::Trkr {
                    vec![(Axis::T, "t")]
                } else {
                    vec![
                        (Axis::X, "x"),
                        (Axis::Y, "y"),
                        (Axis::U, "u"),
                        (Axis::V, "v"),
                    ]
                };
                form.combo(
                    &format!("{prefix}_axis_combo"),
                    rect(field, 9., 60., 26.),
                    &mut plan.fast,
                    &options,
                );
                if matches!(plan.fast, Axis::U | Axis::V) {
                    let theta_x = field + 60. + 4.;
                    form.label("spatial_theta_label", rect(theta_x, 9., 32., 26.), "θ (°)");
                    form.number(
                        "spatial_theta_spin",
                        rect(theta_x + 36., 9., 68., 26.),
                        &mut plan.theta_deg,
                        1.,
                        2,
                        true,
                    );
                }
                let unit = scan_unit(plan.fast);
                let labels = [
                    format!("Min ({unit})"),
                    format!("Max ({unit})"),
                    format!("Step ({unit})"),
                ];
                for (row, (key, value)) in [
                    ("min", &mut plan.fast_range.min),
                    ("max", &mut plan.fast_range.max),
                    ("step", &mut plan.fast_range.step),
                ]
                .into_iter()
                .enumerate()
                {
                    let y = 41. + row as f32 * 32.;
                    form.label(
                        &format!("{prefix}_{key}_label"),
                        rect(9., y, label, 26.),
                        &labels[row],
                    );
                    form.number(
                        &format!("{prefix}_{key}_spin"),
                        rect(field, y, input, 26.),
                        value,
                        1.,
                        3,
                        true,
                    );
                    form.value(
                        &format!("{prefix}_{key}_hint"),
                        rect(
                            field + input + 6.,
                            y,
                            scan_column_width - label - input - 12.,
                            26.,
                        ),
                        &hints_fast[row],
                        Color32::GRAY,
                    );
                }
                form.label(
                    &format!("{prefix}_wait_label"),
                    rect(9., 137., label, 26.),
                    "Wait (s)",
                );
                form.enabled = !running;
                form.number(
                    &format!("{prefix}_wait_spin"),
                    rect(field, 137., 72., 26.),
                    &mut plan.interval_s,
                    1.,
                    2,
                    true,
                );
                form.enabled = !running && !self.demo_viewer;
                if form.button(
                    &format!("{prefix}_tc_button"),
                    rect(field + 78., 137., 76., 26.),
                    "TC × 4",
                    tc.is_some(),
                ) && let Some(wait) = tc
                {
                    plan.interval_s = wait;
                }
            } else {
                let column = ((settings - 18. - 6.) * 0.5).round();
                for fast in [true, false] {
                    let origin = if fast { 9. } else { 9. + column + 6. };
                    let width = if fast {
                        column
                    } else {
                        settings - 18. - 6. - column
                    };
                    Self::precise_scan_column(
                        &mut form,
                        &mut plan,
                        ScanColumn {
                            fast,
                            x: origin,
                            width,
                            tc,
                            replay_wait: self.demo_viewer && !running,
                            hints: if fast { &hints_fast } else { &hints_slow },
                        },
                    );
                }
                if let Some(warning) =
                    axis_choices::pair_warning(plan.workflow, plan.fast, plan.slow)
                {
                    form.value(
                        "invalid_axis_pair",
                        rect(112., 172., settings - 121., 16.),
                        warning,
                        Color32::from_rgb(245, 183, 82),
                    );
                }
            }
            form.enabled = !running && !self.demo_viewer;
            form.group(
                "output_group",
                rect(side_x, 12., side, 96.),
                if self.demo_viewer {
                    "Replay CSV"
                } else {
                    "Output"
                },
            );
            let output = &mut self.outputs[Self::ix(self.workflow)];
            form.label(
                "output_directory_label",
                rect(side_x + 11., 39., 24., 26.),
                "Dir",
            );
            form.path(
                "output_dir_edit",
                rect(side_x + 41., 39., side - 112., 26.),
                &mut output.dir,
            );
            form.enabled = !running;
            browse = form.button(
                "output_browse_button",
                rect(side_x + side - 65., 39., 54., 26.),
                "…",
                self.dialog.is_none() && self.snapshot_copy.is_none(),
            );
            form.enabled = !running && !self.demo_viewer;
            form.label(
                "output_file_label",
                rect(side_x + 11., 71., 24., 26.),
                "File",
            );
            form.text(
                "output_name_edit",
                rect(side_x + 41., 71., side - 112., 26.),
                &mut output.filename,
            );
            form.check(
                "auto_suffix_check",
                rect(side_x + side - 65., 72., 54., 24.),
                &mut output.auto_timestamp_suffix,
                "Auto",
            );
            form.group("run_group", rect(side_x, 120., side, 73.), "Run");
            let usable = side - 22. - 12.;
            let first = (usable / 3.).round();
            let second = (usable * 2. / 3.).round() - first;
            let third = usable - first - second;
            form.enabled = true;
            start = form.button(
                "start_button",
                rect(side_x + 11., 147., first, 26.),
                "Start",
                availability.allows(&plan),
            );
            stop = form.button(
                "stop_button",
                rect(side_x + 17. + first, 147., second, 26.),
                "Stop",
                running,
            );
            save = form.button(
                "save_rows_button",
                rect(side_x + 23. + first + second, 147., third, 26.),
                if self.demo_viewer {
                    "Load"
                } else if self.snapshot_copy.is_some() {
                    "Saving…"
                } else {
                    "Save Now"
                },
                if self.demo_viewer {
                    !running
                        && self.demo.pending.is_none()
                        && !self.demo.paths[Self::ix(self.workflow)]
                            .as_os_str()
                            .is_empty()
                } else {
                    self.output_paths[Self::ix(self.workflow)].is_some()
                        && self.dialog.is_none()
                        && self.snapshot_copy.is_none()
                },
            );
            settings_clicked = form.button(
                "measurement_settings_button",
                rect(9., 169., 96., 24.),
                "Settings…",
                !self.controller.has_pending() && self.pending_device_selection.is_none(),
            );
            precision::record(
                form.geometry,
                "output_run_widget",
                rect(side_x, 0., side, page.height()).translate(page.min.to_vec2()),
            );
        }
        self.plans[Self::ix(self.workflow)] = plan;
        self.select_plot_axes();
        if settings_clicked {
            self.open_measurement_settings();
        }
        if browse {
            self.begin_dialog(
                if self.demo_viewer {
                    DialogKind::ReplayCsv
                } else {
                    DialogKind::Directory
                },
                if self.demo_viewer {
                    self.demo.paths[Self::ix(self.workflow)]
                        .display()
                        .to_string()
                } else {
                    self.outputs[Self::ix(self.workflow)].dir.clone()
                },
            );
        }
        if start && self.start_availability(self.plan()).allows(self.plan()) {
            self.start();
        }
        if stop {
            if self.demo_viewer {
                self.demo_stop();
            } else {
                self.controller.stop();
                self.note("Stopping…");
            }
        }
        if save && self.demo_viewer {
            self.demo_load(
                self.demo.paths[Self::ix(self.workflow)].clone(),
                Some(self.workflow),
            );
        }
        if save && !self.demo_viewer {
            let out = &self.outputs[Self::ix(self.workflow)];
            self.begin_dialog(
                DialogKind::SaveCsv,
                PathBuf::from(&out.dir)
                    .join(format!("{}_snapshot.csv", out.filename))
                    .display()
                    .to_string(),
            );
        }
    }
    fn precise_scan_column(form: &mut precision::Form<'_>, p: &mut Plan, column: ScanColumn<'_>) {
        let ScanColumn {
            fast,
            x,
            width,
            tc,
            replay_wait,
            hints,
        } = column;
        use precision::rect;
        let prefix = p.workflow.key();
        let role = if fast { "fast" } else { "slow" };
        // All tabs reserve the same two-column scan area. Units and limits
        // stay visible beside each numeric field without shifting Output/Run.
        let label = 66.;
        let field = x + label + 6.;
        let input = 88.;
        form.label(
            &format!("{prefix}_{role}_label"),
            rect(x, 9., label, 26.),
            if fast { "Fast" } else { "Slow" },
        );
        let choices = if fast {
            axis_choices::fast_options(p.workflow)
        } else {
            axis_choices::slow_options(p.workflow, p.fast)
        };
        let options: Vec<_> = choices
            .iter()
            .copied()
            .map(|axis| (axis, axis.name()))
            .collect();
        let previous = if fast { p.fast } else { p.slow };
        let axis_input = 60.;
        if fast {
            form.combo(
                &format!("{prefix}_{role}_axis_combo"),
                rect(field, 9., axis_input, 26.),
                &mut p.fast,
                &options,
            );
            if p.fast != previous {
                let chosen = p.fast;
                p.fast = previous;
                axis_choices::apply_fast_choice(p, chosen);
            }
        } else {
            form.combo(
                &format!("{prefix}_{role}_axis_combo"),
                rect(field, 9., axis_input, 26.),
                &mut p.slow,
                &options,
            );
        }
        let selected = if fast { p.fast } else { p.slow };
        if show_scan_theta(p.workflow, selected) {
            let theta_x = field + axis_input + 4.;
            form.label(
                &format!("{prefix}_{role}_theta_label"),
                rect(theta_x, 9., 32., 26.),
                "θ (°)",
            );
            form.number(
                &format!("{prefix}_{role}_theta"),
                rect(theta_x + 36., 9., 68., 26.),
                &mut p.theta_deg,
                1.,
                2,
                true,
            );
        }
        let unit = scan_unit(selected);
        let r = if fast {
            &mut p.fast_range
        } else {
            &mut p.slow_range
        };
        for (row, (key, value)) in [
            ("min", &mut r.min),
            ("max", &mut r.max),
            ("step", &mut r.step),
        ]
        .into_iter()
        .enumerate()
        {
            let y = 41. + row as f32 * 32.;
            form.label(
                &format!("{prefix}_{role}_{key}_label"),
                rect(x, y, label, 26.),
                format!(
                    "{} ({unit})",
                    match key {
                        "min" => "Min",
                        "max" => "Max",
                        _ => "Step",
                    }
                ),
            );
            form.number(
                &format!("{prefix}_{role}_{key}"),
                rect(field, y, input, 26.),
                value,
                1.,
                3,
                true,
            );
            form.value(
                &format!("{prefix}_{role}_{key}_hint"),
                rect(field + input + 6., y, width - label - input - 12., 26.),
                &hints[row],
                Color32::GRAY,
            );
        }
        if fast {
            form.label(
                &format!("{prefix}_wait_label"),
                rect(x, 137., label, 26.),
                "Wait (s)",
            );
            let enabled = form.enabled;
            form.enabled |= replay_wait;
            form.number(
                &format!("{prefix}_wait_spin"),
                rect(field, 137., 72., 26.),
                &mut p.interval_s,
                1.,
                2,
                true,
            );
            form.enabled = enabled;
            if form.button(
                &format!("{prefix}_tc_button"),
                rect(field + 78., 137., 76., 26.),
                "TC × 4",
                tc.is_some(),
            ) && let Some(wait) = tc
            {
                p.interval_s = wait;
            }
        }
    }
    pub(super) fn workflow_time_constant(&self, workflow: Workflow) -> Option<f64> {
        let ids = self.settings.workflow_device_ids(workflow).ok()?;
        (ids.lockin == self.inventory.selected.lockin)
            .then(|| {
                self.snapshot
                    .lockin
                    .get("Time Constant")
                    .and_then(Value::as_f64)
                    .filter(|v| v.is_finite() && *v >= 0.)
                    .map(|v| v * 4.)
            })
            .flatten()
    }

    pub(super) fn workflow_hardware(&self, workflow: Workflow) -> Option<engine::HardwareConfig> {
        let mut settings = self.settings.clone();
        settings.select_workflow(workflow).ok()?;
        settings.hardware().ok()
    }

    fn workflow_delay_calibration(
        &self,
        config: &engine::HardwareConfig,
    ) -> Option<&trkr_motion::DelayCalibration> {
        self.snapshot
            .registered_devices
            .iter()
            .find(|device| device.kind == "delay" && device.id == config.device_ids.delay)
            .and_then(|device| device.delay_calibration.as_ref())
            .or_else(|| {
                (config.device_ids.delay == self.inventory.selected.delay)
                    .then_some(self.snapshot.delay_calibration.as_ref())
                    .flatten()
            })
    }

    pub(super) fn scan_hints(
        &self,
        axis: Axis,
        plan: &Plan,
        config: Option<&engine::HardwareConfig>,
    ) -> [String; 3] {
        let unit = scan_unit(axis);
        if self.demo_viewer {
            let r = if axis == plan.fast {
                plan.fast_range
            } else {
                plan.slow_range
            };
            return [
                format!("≥ {} {unit}", Self::significant(r.min, 5)),
                format!("≤ {} {unit}", Self::significant(r.max, 5)),
                format!("≥ {} {unit}", Self::significant(r.step, 5)),
            ];
        }
        if matches!(axis, Axis::U | Axis::V) {
            return std::array::from_fn(|_| "checks x/y".into());
        }
        let calibration = config.and_then(|config| self.workflow_delay_calibration(config));
        let t_step_hint = config
            .and_then(|c| c.motion.delay.as_ref())
            .map(|d| delay_step_hint(d, calibration));
        let limits = config.and_then(|c| {
            if axis == Axis::T {
                let mut d = c.motion.delay.as_ref()?.normalized().ok()?;
                if let Some(cached) = calibration.filter(|c| c.configuration.as_ref() == Some(&d)) {
                    d.pos_um_per_pulse = cached.effective_pos_um_per_pulse.or(d.pos_um_per_pulse);
                }
                let low_mm = d
                    .min_pulse
                    .zip(d.pos_um_per_pulse)
                    .map(|(p, u)| p as f64 * u / 1000.)
                    .unwrap_or(0.);
                let high_mm = d
                    .max_pulse
                    .zip(d.pos_um_per_pulse)
                    .map(|(p, u)| p as f64 * u / 1000.)
                    .or(d.travel_mm)?;
                let a = d.measurement_ps_from_mm(low_mm).ok()? - plan.zero[0];
                let b = d.measurement_ps_from_mm(high_mm).ok()? - plan.zero[0];
                let step = d.pos_um_per_pulse.map(|u| 2. * u / 1000. / 0.299792458);
                Some((a.min(b), a.max(b), step))
            } else if matches!(axis, Axis::X | Axis::Y) {
                let i = if axis == Axis::X { 1 } else { 2 };
                let s = if i == 1 {
                    c.motion.x.as_ref()?
                } else {
                    c.motion.y.as_ref()?
                }
                .normalized()
                .ok()?;
                let a = s.sample_um_from_raw(s.min_pos?) - plan.zero[i];
                let b = s.sample_um_from_raw(s.max_pos?) - plan.zero[i];
                Some((
                    a.min(b),
                    a.max(b),
                    s.resolution
                        .or(s.min_step)
                        .map(|r| (r * s.sample_um_per_unit).abs()),
                ))
            } else {
                None
            }
        });
        let fmt = |v: Option<f64>| {
            v.map(|v| Self::significant(v, 5))
                .unwrap_or_else(|| "—".into())
        };
        if let Some((min, max, step)) = limits {
            [
                format!("≥ {} {unit}", fmt(Some(min))),
                format!("≤ {} {unit}", fmt(Some(max))),
                if axis == Axis::T {
                    t_step_hint.unwrap_or_else(|| format!("≥ {} {unit}", fmt(step)))
                } else {
                    format!("≥ {} {unit}", fmt(step))
                },
            ]
        } else {
            std::array::from_fn(|_| format!("— {unit}"))
        }
    }
}
