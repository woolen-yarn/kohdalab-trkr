use super::*;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum MonitorTab {
    #[default]
    Signal,
    Power,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SettingsTab {
    Acquisition,
    Output,
    Device,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum SignalChannel {
    #[default]
    X,
    Y,
    R,
    Theta,
}

impl SignalChannel {
    const CHOICES: [(Self, &'static str); 4] = [
        (Self::X, "X"),
        (Self::Y, "Y"),
        (Self::R, "R"),
        (Self::Theta, "θ"),
    ];

    fn display(self) -> (DisplayMode, usize, &'static str) {
        match self {
            Self::X => (DisplayMode::Xy, 0, "X"),
            Self::Y => (DisplayMode::Xy, 1, "Y"),
            Self::R => (DisplayMode::RTheta, 0, "R"),
            Self::Theta => (DisplayMode::RTheta, 1, "θ"),
        }
    }
}

pub(super) struct PowerTrend {
    pub id: String,
    pub started: Instant,
    pub running: bool,
    pub points: VecDeque<[f64; 2]>,
}

#[derive(Default)]
pub(super) struct Workspace {
    pub monitor: MonitorTab,
    pub(super) power_trend: Option<PowerTrend>,
    pub(super) power_meter: Option<String>,
    pub(super) power_settings: bool,
    signal: SignalChannel,
    pub motion_only: bool,
    pub settings: Option<SettingsTab>,
    device_draft: Option<super::measurement_settings::Draft>,
}

impl App {
    fn signal_recording(&self) -> bool {
        (self.snapshot.running || self.measurement_pending)
            && self.active_workflow == Workflow::SignalMonitor
    }

    fn alignment_recording_status(&self) -> String {
        if self.snapshot.phase.starts_with("Stopping") {
            return "Stopping…".into();
        }
        if self.showing_until_stop_run() {
            return format!("{} rows · Continuous", self.snapshot.rows);
        }
        let total = self.snapshot.total;
        if total == 0 {
            return "Recording".into();
        }
        format!("{}/{} · Recording", self.snapshot.rows.min(total), total)
    }

    pub(super) fn alignment_workspace_view(
        &mut self,
        ui: &mut egui::Ui,
        tabs: egui::Rect,
        content: egui::Rect,
    ) {
        let toggle = precision::rect(tabs.right() - 128., tabs.top(), 128., 24.);
        if ui
            .put(
                toggle,
                egui::Button::new(if self.alignment_workspace.motion_only {
                    "Show Monitors"
                } else {
                    "Motion Only"
                })
                .min_size(toggle.size()),
            )
            .clicked()
        {
            self.alignment_cancel_hold();
            self.alignment_workspace.motion_only = !self.alignment_workspace.motion_only;
        }
        precision::record(&mut self.geometry, "alignment_view_toggle", toggle);
        let recording = self.signal_recording();
        let signal_visible = !self.alignment_workspace.motion_only
            && self.alignment_workspace.monitor == MonitorTab::Signal;
        if recording && !signal_visible {
            let status = self.alignment_recording_status();
            ui.put(
                precision::rect(toggle.left() - 244., tabs.top(), 108., 24.),
                egui::Label::new(&status).truncate(),
            )
            .on_hover_text(status);
            self.recording_stop_button(
                ui,
                "Stop Recording",
                precision::rect(toggle.left() - 132., tabs.top(), 124., 24.),
            );
        }
        if self.alignment_workspace.motion_only {
            self.alignment_view(ui, content);
            return;
        }
        let monitor_height = (content.height() * 0.34).clamp(212., 300.);
        let monitor = precision::rect(
            content.left(),
            content.top(),
            content.width(),
            monitor_height,
        );
        precision::record(&mut self.geometry, "alignment_monitor_panel", monitor);
        let motion = egui::Rect::from_min_max(
            egui::pos2(content.left(), monitor.bottom() + 8.),
            content.max,
        );
        ui.painter().rect(
            monitor,
            10.,
            PANEL,
            Stroke::new(1_f32, BORDER),
            egui::StrokeKind::Inside,
        );
        for (tab, text, x) in [
            (MonitorTab::Signal, "Signal Monitor", 10.),
            (MonitorTab::Power, "Power Monitor", 134.),
        ] {
            let rect = precision::rect(monitor.left() + x, monitor.top() + 8., 118., 26.);
            if ui
                .put(
                    rect,
                    egui::Button::new(text)
                        .min_size(rect.size())
                        .selected(self.alignment_workspace.monitor == tab),
                )
                .clicked()
                && self.alignment_workspace.monitor != tab
            {
                self.alignment_cancel_hold();
                self.alignment_workspace.monitor = tab;
            }
        }
        let panel = egui::Rect::from_min_max(
            monitor.min + Vec2::new(10., 40.),
            monitor.max - Vec2::splat(10.),
        );
        match self.alignment_workspace.monitor {
            MonitorTab::Signal => self.alignment_signal_monitor(ui, panel),
            MonitorTab::Power => self.alignment_power_monitor(ui, panel),
        }
        self.alignment_view(ui, motion);
    }

    fn recording_stop_button(&mut self, ui: &mut egui::Ui, label: &str, rect: egui::Rect) {
        precision::record(&mut self.geometry, "stop_button", rect);
        if ui
            .put(
                rect,
                egui::Button::new(label)
                    .fill(Color32::from_rgb(173, 69, 76))
                    .min_size(rect.size()),
            )
            .clicked()
        {
            self.controller.stop();
            self.note("Stopping recording…");
        }
    }

    fn show_monitor_settings(&mut self, tab: SettingsTab) {
        self.alignment_cancel_hold();
        self.alignment_workspace.settings = Some(tab);
    }

    fn alignment_signal_monitor(&mut self, ui: &mut egui::Ui, panel: egui::Rect) {
        let recording = self.signal_recording();
        let mut form = precision::Form::new(ui, &mut self.geometry, panel.min);
        let settings = form.button(
            "alignment_settings_button",
            precision::rect(panel.width() - 88., 0., 88., 26.),
            "Settings…",
            !self.demo_viewer,
        );
        let available = !recording
            && self.start_availability(self.plan()).allows(self.plan())
            && !self.demo_viewer;
        let (start, stop) = self.alignment_monitor_buttons(ui, panel, available, recording);
        if start {
            self.alignment_cancel_hold();
            self.start();
        }
        if stop {
            self.controller.stop();
            self.note("Stopping recording…");
        }
        if settings {
            self.show_monitor_settings(SettingsTab::Acquisition);
        }
        let plots = self.alignment_monitor_plot_area(panel);
        precision::record(&mut self.geometry, "standard_plot_widget", plots);
        self.alignment_signal_plot(ui, plots);
    }

    pub(super) fn alignment_monitor_buttons(
        &mut self,
        ui: &mut egui::Ui,
        panel: egui::Rect,
        can_start: bool,
        can_stop: bool,
    ) -> (bool, bool) {
        let mut button = |label, key, offset, fill, enabled| {
            let rect = precision::rect(panel.right() - offset, panel.top(), 76., 26.);
            precision::record(&mut self.geometry, key, rect);
            ui.add_enabled_ui(enabled, |ui| {
                ui.put(
                    rect,
                    egui::Button::new(label).fill(fill).min_size(rect.size()),
                )
            })
            .inner
            .clicked()
        };
        let start = button(
            "Start",
            "start_button",
            256.,
            Color32::from_rgb(48, 107, 214),
            can_start,
        );
        let stop = button(
            "Stop",
            "stop_button",
            172.,
            Color32::from_rgb(173, 69, 76),
            can_stop,
        );
        (start, stop)
    }

    pub(super) fn alignment_monitor_plot_area(&mut self, panel: egui::Rect) -> egui::Rect {
        let plots = egui::Rect::from_min_max(panel.min + Vec2::new(0., 32.), panel.max);
        precision::record(&mut self.geometry, "alignment_monitor_plots", plots);
        plots
    }

    fn alignment_recording_name(&self) -> String {
        let index = Self::ix(Workflow::SignalMonitor);
        let profile = &self.outputs[index];
        let base = output_filename(profile, self.source_demo);
        let suffix = if profile.auto_timestamp_suffix {
            "_[timestamp]"
        } else {
            ""
        };
        format!("{base}{suffix}.csv")
    }

    fn alignment_signal_plot(&self, ui: &mut egui::Ui, area: egui::Rect) {
        let (mode, index, name) = self.alignment_workspace.signal.display();
        let latest = self.trace.last();
        let base_unit = latest.map_or_else(
            || self.display_signal_unit(),
            |sample| presentation::sample_signal_unit(sample).into(),
        );
        let (scale, unit) = if matches!(base_unit.as_str(), "A" | "V") {
            let magnitude = presentation::trace_signal_magnitude(&self.trace, mode, false);
            let (scale, label) = magnitude
                .or_else(|| qualified_lockin_sensitivity(&self.snapshot, self.model))
                .map(presentation::voltage_scale_from_sensitivity)
                .unwrap_or_else(|| {
                    if self.source_demo && !self.demo_viewer {
                        (1e3, "mV")
                    } else {
                        self.demo_voltage_scale()
                    }
                });
            presentation::signal_display_scale(scale, label, &base_unit)
        } else {
            (1., base_unit.clone())
        };
        let recording_status = self
            .signal_recording()
            .then(|| self.alignment_recording_status());
        let source = if let Some(status) = &recording_status {
            status
        } else if latest.is_some() {
            if self.source_demo {
                "Simulated"
            } else {
                "Last recording"
            }
        } else {
            "No recording"
        };
        let angle = mode == DisplayMode::RTheta && index == 1;
        let factor = if angle { 1. } else { scale };
        let unit = if angle { "°" } else { &unit };
        let value = latest
            .map(|sample| format!("{:.4} {unit}", mode.values(sample.signal)[index] * factor))
            .unwrap_or_else(|| format!("— {unit}"));
        let points: Vec<_> = self
            .trace
            .iter()
            .filter_map(|s| {
                let value = mode.values(s.signal)[index] * factor;
                ((angle
                    || latest.is_some_and(|last| {
                        presentation::sample_signal_unit(s)
                            == presentation::sample_signal_unit(last)
                    }))
                    && value.is_finite())
                .then_some([s.elapsed, value])
            })
            .collect();
        let y_label = if angle {
            "Theta (deg)".into()
        } else {
            format!("{name} ({unit})")
        };
        let id = egui::Id::new(("alignment-signal", name, self.plot_epoch, &y_label));
        draw_monitor_plot(
            ui,
            area,
            id,
            (&format!("{name}  {value}"), source, &y_label),
            points,
            index,
        );
    }

    pub(super) fn alignment_monitor_settings_dialog(&mut self, ctx: &egui::Context) {
        let Some(tab) = self.alignment_workspace.settings else {
            return;
        };
        let index = Self::ix(Workflow::SignalMonitor);
        let recording = self.snapshot.running || self.measurement_pending || self.python_locked();
        let mut browse = false;
        let mut save = false;
        let mut device_draft = self.alignment_workspace.device_draft.take();
        if tab == SettingsTab::Device && device_draft.is_none() {
            match self.measurement_device_draft() {
                Ok(draft) => device_draft = Some(draft),
                Err(error) => {
                    self.note(error);
                    self.alignment_workspace.settings = Some(SettingsTab::Acquisition);
                    return;
                }
            }
        }
        let mut apply_device = false;
        let mut close = false;
        let mut next = tab;
        let dialog =
            egui::Modal::new(egui::Id::new("alignment-monitor-settings")).show(ctx, |ui| {
                ui.set_width(390.);
                ui.heading("Signal Monitor Settings");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut next, SettingsTab::Acquisition, "Acquisition");
                    ui.selectable_value(&mut next, SettingsTab::Output, "Output");
                    ui.selectable_value(&mut next, SettingsTab::Device, "Device");
                });
                ui.add_space(10.);
                match next {
                    SettingsTab::Acquisition => {
                        let plan = &mut self.plans[index];
                        ui.add_enabled_ui(!recording, |ui| {
                            egui::Grid::new("alignment-acquisition-settings")
                                .spacing([18., 10.])
                                .show(ui, |ui| {
                                    ui.label("Channel");
                                    egui::ComboBox::from_id_salt("alignment_signal_mode")
                                        .selected_text(self.alignment_workspace.signal.display().2)
                                        .show_ui(ui, |ui| {
                                            for (value, label) in SignalChannel::CHOICES {
                                                ui.selectable_value(
                                                    &mut self.alignment_workspace.signal,
                                                    value,
                                                    label,
                                                );
                                            }
                                        });
                                    ui.end_row();
                                    ui.label("Interval (s)");
                                    ui.add(
                                        egui::DragValue::new(&mut plan.interval_s)
                                            .speed(0.01)
                                            .range(0.0..=3600.0)
                                            .max_decimals(3),
                                    );
                                    ui.end_row();
                                    ui.label("Point");
                                    ui.horizontal(|ui| {
                                        ui.add_enabled(
                                            !self.monitor_until_stop,
                                            egui::DragValue::new(&mut plan.n_points)
                                                .range(1..=trkr_core::MAX_TOTAL_POINTS)
                                                .min_decimals(0),
                                        );
                                        ui.checkbox(&mut self.monitor_until_stop, "Continuous");
                                    });
                                    ui.end_row();
                                });
                        });
                    }
                    SettingsTab::Output => {
                        let profile = &mut self.outputs[index];
                        ui.add_enabled_ui(!recording && self.dialog.is_none(), |ui| {
                            egui::Grid::new("alignment-recording-settings")
                                .spacing([18., 10.])
                                .show(ui, |ui| {
                                    ui.label("Folder");
                                    ui.horizontal(|ui| {
                                        ui.add(
                                            egui::TextEdit::singleline(&mut profile.dir)
                                                .desired_width(240.)
                                                .hint_text("Recording folder"),
                                        );
                                        browse = ui.button("Browse…").clicked();
                                    });
                                    ui.end_row();
                                    ui.label("File name");
                                    ui.add(
                                        egui::TextEdit::singleline(&mut profile.filename)
                                            .desired_width(240.)
                                            .hint_text("File name"),
                                    );
                                    ui.end_row();
                                    ui.label("");
                                    ui.checkbox(
                                        &mut profile.auto_timestamp_suffix,
                                        "Add timestamp",
                                    );
                                    ui.end_row();
                                });
                        });
                        ui.add_space(6.);
                        ui.label(egui::RichText::new(self.alignment_recording_name()).monospace());
                        save = ui
                            .add_enabled(
                                self.output_paths[index].is_some()
                                    && self.dialog.is_none()
                                    && self.snapshot_copy.is_none(),
                                egui::Button::new("Save Snapshot…"),
                            )
                            .clicked();
                    }
                    SettingsTab::Device => {
                        if let Some(draft) = &mut device_draft {
                            let locked = recording || self.controller.has_pending();
                            ui.add_enabled_ui(!locked, |ui| {
                                self.measurement_device_fields(ui, draft);
                                ui.add_space(6.);
                                apply_device = ui.button("Apply").clicked();
                            });
                            if let Some(error) = &draft.error {
                                ui.colored_label(Color32::LIGHT_RED, error);
                            }
                        }
                    }
                }
                ui.add_space(10.);
                close = ui.button("Done").clicked();
            });
        self.sync_output(Workflow::SignalMonitor);
        if browse {
            self.begin_dialog(DialogKind::Directory, self.outputs[index].dir.clone());
        }
        if save {
            let output = &self.outputs[index];
            self.begin_dialog(
                DialogKind::SaveCsv,
                PathBuf::from(&output.dir)
                    .join(format!("{}_snapshot.csv", output.filename))
                    .display()
                    .to_string(),
            );
        }
        if apply_device && let Some(draft) = &mut device_draft {
            match self.apply_measurement_settings(draft) {
                Ok(()) => device_draft = None,
                Err(error) => draft.error = Some(error),
            }
        }
        self.alignment_workspace.settings = (!(close || dialog.should_close())).then_some(next);
        if self.alignment_workspace.settings.is_some() {
            self.alignment_workspace.device_draft = device_draft;
        }
    }
}

pub(super) fn draw_monitor_plot(
    ui: &mut egui::Ui,
    area: egui::Rect,
    id: egui::Id,
    (value, source, y_label): (&str, &str, &str),
    points: Vec<[f64; 2]>,
    color_index: usize,
) {
    let header = precision::rect(area.left(), area.top(), area.width(), 28.);
    ui.scope_builder(egui::UiBuilder::new().max_rect(header), |ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(value)
                    .size(18.)
                    .strong()
                    .color(presentation::color(color_index)),
            );
            ui.weak(source);
        });
    });
    let chart = egui::Rect::from_min_max(area.min + Vec2::new(0., 28.), area.max);
    let x_label = "Elapsed (s)";
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(chart)
            .layout(egui::Layout::top_down(egui::Align::Min)),
        |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(chart));
            presentation::measurement_plot(id, chart, x_label, y_label)
                .allow_zoom(false)
                .allow_scroll(false)
                .allow_drag(false)
                .include_x(0.)
                .label_formatter(|name, point| {
                    presentation::point_label(name, x_label, point.x, point.y)
                })
                .show(ui, |plot| {
                    plot.line(presentation::measurement_series(
                        y_label,
                        points,
                        color_index,
                    ))
                });
        },
    );
}

#[cfg(test)]
#[test]
fn signal_monitor_plot_includes_start_and_samples_older_than_thirty_seconds() {
    let ctx = egui::Context::default();
    let mut app = App::with_context(&ctx, false);
    app.enable_alignment_monitor_preview();
    for sample in &mut app.trace {
        sample.elapsed *= 3.;
    }
    let end = app.trace.last().unwrap().elapsed;
    assert!(end > 60.);
    let _ = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(800., 300.),
            )),
            ..Default::default()
        },
        |ui| app.alignment_signal_plot(ui, ui.max_rect()),
    );
    let id = egui::Id::new(("alignment-signal", "X", app.plot_epoch, "X (µV)"));
    let bounds = egui_plot::PlotMemory::load(&ctx, id).unwrap();
    assert!(bounds.bounds().min()[0] <= 0.);
    assert!(bounds.bounds().max()[0] >= end);
}

#[cfg(all(test, feature = "gui-interaction-tests"))]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    #[test]
    fn settings_output_changes_the_next_recording_folder() {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1566., 700.))
            .build_eframe(|cc| App::new_alignment_jog_trial(cc).unwrap());
        h.run_steps(3);
        h.get_by_label("Settings…").click();
        h.run_steps(3);
        h.get_by_label("Output").click();
        h.run_steps(3);
        assert_eq!(
            h.state().alignment_workspace.settings,
            Some(SettingsTab::Output)
        );
        let folder = h.state().outputs[App::ix(Workflow::SignalMonitor)]
            .dir
            .clone();
        h.query_all_by_value(&folder).next().unwrap().click();
        h.run_steps(2);
        h.event(egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        });
        let updated = PathBuf::from(&folder).join("alignment");
        std::fs::create_dir(&updated).unwrap();
        h.event(egui::Event::Text(updated.display().to_string()));
        h.run_steps(2);
        h.get_by_label("Done").click();
        h.run_steps(3);
        assert_eq!(h.state().alignment_workspace.settings, None);
        let path = h
            .state()
            .measurement_output_for(Workflow::SignalMonitor)
            .unwrap();
        assert_eq!(path.parent().unwrap(), updated);
        assert_eq!(
            h.state().settings.raw["measurements"]["signal_monitor"]["output"]["dir"],
            updated.display().to_string()
        );
        h.state_mut().plans[App::ix(Workflow::SignalMonitor)].n_points = 2;
        h.get_by_label("Start").click();
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            h.run_steps(1);
            if !h.state().snapshot.running && h.state().trace.len() == 2 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "recording did not finish: {:?}",
                h.state().log
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let recorded = h.state().output_paths[App::ix(Workflow::SignalMonitor)]
            .as_ref()
            .unwrap();
        assert_eq!(recorded.parent().unwrap(), updated);
        assert!(recorded.is_file());
    }

    #[test]
    fn finite_recording_shows_points_without_an_eta() {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1566., 700.))
            .build_eframe(|cc| App::new_alignment_jog_trial(cc).unwrap());
        h.run_steps(3);
        h.get_by_label("Start").click();
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            h.run_steps(1);
            if h.state().snapshot.running && h.state().snapshot.rows > 0 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "finite monitor did not acquire a point"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let status = h.state().alignment_recording_status();
        assert!(status.contains('/') && status.ends_with("Recording"));
        assert!(!status.contains("ETA"));
        h.query_all_by_label("Stop")
            .min_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
            .unwrap()
            .click();
        while h.state().snapshot.running {
            assert!(Instant::now() < deadline, "finite monitor did not stop");
            std::thread::sleep(Duration::from_millis(10));
            h.run_steps(1);
        }
    }

    #[test]
    fn power_and_motion_only_can_be_toggled_without_changing_measurement_workflow() {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1566., 700.))
            .build_eframe(|cc| App::new_alignment_jog_trial(cc).unwrap());
        h.run_steps(3);
        assert!(h.query_all_by_label("Show Live").next().is_none());
        let plot = h.state().geometry["alignment_monitor_plots"];
        let motion = h.state().geometry["alignment_widget"];
        h.state_mut().enable_power_meter_preview();
        h.get_by_label("Power Monitor").click();
        h.run_steps(3);
        assert!(h.query_all_by_label("Show Live").next().is_none());
        assert!(h.state().geometry.contains_key("alignment_power_monitor"));
        assert_eq!(h.state().geometry["alignment_monitor_plots"], plot);
        assert_eq!(h.state().geometry["alignment_widget"], motion);
        assert!(h.state().alignment_workspace.power_trend.is_none());
        h.get_by_label("Start").click();
        h.run_steps(3);
        assert!(
            h.state()
                .alignment_workspace
                .power_trend
                .as_ref()
                .unwrap()
                .running
        );
        let mut reading = h.state().snapshot.power_meters[0].clone();
        reading.revision += 1;
        reading
            .reading
            .as_mut()
            .unwrap()
            .host_completed_at_utc
            .push_str(" sample");
        h.state_mut().observe_power_meter(reading.clone());
        h.query_all_by_label("Stop")
            .min_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
            .unwrap()
            .click();
        h.run_steps(3);
        reading.revision += 1;
        reading
            .reading
            .as_mut()
            .unwrap()
            .host_completed_at_utc
            .push_str(" after stop");
        h.state_mut().observe_power_meter(reading);
        let trend = h.state().alignment_workspace.power_trend.as_ref().unwrap();
        assert!(!trend.running);
        assert_eq!(trend.points.len(), 1);
        let motion_height = |app: &App| app.geometry["alignment_widget"][3];
        let height = motion_height(h.state());
        h.get_by_label("Scanners").click();
        h.run_steps(3);
        h.get_by_label("Motion Only").click();
        h.run_steps(3);
        assert!(!h.state().geometry.contains_key("alignment_power_monitor"));
        assert!(motion_height(h.state()) > height);
        let geometry = &h.state().geometry;
        let tab = geometry["tab_alignment"];
        let toggle = geometry["alignment_view_toggle"];
        let motion = geometry["alignment_widget"];
        assert_eq!(toggle[1], tab[1]);
        assert!((0. ..=8.).contains(&(motion[1] - (tab[1] + tab[3]))));
        assert!(h.query_all_by_label("X+").next().is_some());
        h.get_by_label("Show Monitors").click();
        h.run_steps(3);
        assert!(h.state().geometry.contains_key("alignment_power_monitor"));
        assert_eq!(h.state().workflow, Workflow::SignalMonitor);
        assert!(!h.state().controller.has_pending());
    }
}
