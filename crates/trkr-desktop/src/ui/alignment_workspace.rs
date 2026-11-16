use super::*;
use crate::engine::ReadingOrigin;
use egui_plot::{Line, Plot};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum MonitorTab {
    #[default]
    Signal,
    Power,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SettingsTab {
    Acquisition,
    Recording,
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

#[derive(Default)]
pub(super) struct Workspace {
    pub monitor: MonitorTab,
    signal: SignalChannel,
    pub motion_only: bool,
    pub settings: Option<SettingsTab>,
}

impl App {
    fn signal_recording(&self) -> bool {
        (self.snapshot.running || self.measurement_pending)
            && self.active_workflow == Workflow::SignalMonitor
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
            ui.put(
                precision::rect(toggle.left() - 244., tabs.top(), 108., 24.),
                egui::Label::new("● Recording"),
            );
            self.recording_stop_button(
                ui,
                precision::rect(toggle.left() - 132., tabs.top(), 124., 24.),
            );
        }
        if self.alignment_workspace.motion_only {
            self.alignment_view(ui, content);
            return;
        }
        let monitor_height = (content.height() * 0.40).clamp(212., 350.);
        let monitor = precision::rect(
            content.left(),
            content.top(),
            content.width(),
            monitor_height,
        );
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

    fn recording_stop_button(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        precision::record(&mut self.geometry, "stop_button", rect);
        if ui
            .put(
                rect,
                egui::Button::new("Stop Recording")
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
        form.combo(
            "alignment_signal_mode",
            precision::rect(0., 0., 88., 26.),
            &mut self.alignment_workspace.signal,
            &SignalChannel::CHOICES,
        );
        let settings = form.button(
            "alignment_settings_button",
            precision::rect(panel.width() - 88., 0., 88., 26.),
            "Settings…",
            !self.demo_viewer,
        );
        let action = precision::rect(panel.right() - 222., panel.top(), 126., 26.);
        let filename = self.alignment_recording_name(recording);
        let filename_rect =
            precision::rect(panel.left() + 102., panel.top(), panel.width() - 334., 26.);
        precision::record(
            &mut self.geometry,
            "alignment_recording_name",
            filename_rect,
        );
        let file = ui
            .add_enabled_ui(!self.demo_viewer && !recording, |ui| {
                ui.put(filename_rect, egui::Button::new(&filename).truncate())
            })
            .inner;
        let file = if recording {
            file.on_disabled_hover_text(
                self.output_paths[Self::ix(Workflow::SignalMonitor)]
                    .as_ref()
                    .map_or_else(|| filename.clone(), |path| path.display().to_string()),
            )
        } else {
            file.on_hover_text("Change the recording folder or file name")
        };
        if recording {
            self.recording_stop_button(ui, action);
        } else {
            precision::record(&mut self.geometry, "start_button", action);
            let available =
                self.start_availability(self.plan()).allows(self.plan()) && !self.demo_viewer;
            if ui
                .add_enabled_ui(available, |ui| {
                    ui.put(
                        action,
                        egui::Button::new("Record")
                            .fill(Color32::from_rgb(48, 107, 214))
                            .min_size(action.size()),
                    )
                })
                .inner
                .on_disabled_hover_text("Connect a lock-in and wait for other operations to finish")
                .clicked()
            {
                self.alignment_cancel_hold();
                self.start();
            }
        }
        if file.clicked() {
            self.show_monitor_settings(SettingsTab::Recording);
        } else if settings {
            self.show_monitor_settings(SettingsTab::Acquisition);
        }
        let plots = egui::Rect::from_min_max(panel.min + Vec2::new(0., 32.), panel.max);
        precision::record(&mut self.geometry, "alignment_monitor_plots", plots);
        precision::record(&mut self.geometry, "standard_plot_widget", plots);
        self.alignment_signal_plot(ui, plots);
    }

    fn alignment_recording_name(&self, recording: bool) -> String {
        let index = Self::ix(Workflow::SignalMonitor);
        if recording {
            return self.output_paths[index]
                .as_ref()
                .and_then(|path| path.file_name())
                .map_or_else(
                    || "Recording".into(),
                    |name| name.to_string_lossy().into_owned(),
                );
        }
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
        let hardware_live = !self.source_demo
            && !self.demo_viewer
            && self.snapshot.devices[0]
            && self.live.origin() == Some(ReadingOrigin::Hardware);
        let use_live = hardware_live && !self.signal_recording();
        let fresh = use_live && self.live.recent();
        let newest = if use_live {
            self.live.newest(fresh)
        } else {
            latest.map_or(0., |sample| sample.elapsed)
        };
        let base_unit = if use_live {
            self.display_signal_unit()
        } else {
            latest
                .map(|s| s.recording.signal_unit.as_str())
                .filter(|u| !u.is_empty())
                .map_or_else(|| self.display_signal_unit(), str::to_owned)
        };
        let (scale, unit) = if matches!(base_unit.as_str(), "A" | "V") {
            let (scale, label) = qualified_lockin_sensitivity(&self.snapshot, self.model)
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
        let source = if use_live {
            if fresh { "Live" } else { "Stale" }
        } else if self.signal_recording() {
            "Recording"
        } else if latest.is_some() {
            if self.source_demo {
                "Simulated"
            } else {
                "Last recording"
            }
        } else {
            "Waiting for signal"
        };
        let angle = mode == DisplayMode::RTheta && index == 1;
        let factor = if angle { 1. } else { scale };
        let unit = if angle { "°" } else { &unit };
        let value = if use_live {
            self.live.signal()
        } else {
            latest.map(|s| s.signal)
        };
        let value = value
            .map(|signal| format!("{:.4} {unit}", mode.values(signal)[index] * factor))
            .unwrap_or_else(|| format!("— {unit}"));
        let points = if use_live {
            self.live.points_at(mode, index, scale, newest)
        } else {
            self.trace
                .iter()
                .rev()
                .take_while(|s| newest - s.elapsed <= 30.)
                .filter_map(|s| {
                    let value = mode.values(s.signal)[index] * factor;
                    ((angle
                        || latest.is_some_and(|last| {
                            s.recording.signal_unit == last.recording.signal_unit
                        }))
                        && value.is_finite())
                    .then_some([s.elapsed - newest, value])
                })
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect()
        };
        let color = if index == 0 {
            crate::live_view::TEAL
        } else {
            Color32::from_rgb(245, 183, 82)
        };
        let header = precision::rect(area.left(), area.top(), area.width(), 28.);
        ui.scope_builder(egui::UiBuilder::new().max_rect(header), |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{name}  {value}"))
                        .size(18.)
                        .strong()
                        .color(color),
                );
                ui.weak(source);
            });
        });
        let chart = egui::Rect::from_min_max(area.min + Vec2::new(0., 28.), area.max);
        draw_monitor_trend(
            ui,
            chart,
            egui::Id::new(("alignment-signal", name)),
            (name, points, color),
            true,
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
        let mut devices = false;
        let mut close = false;
        let mut next = tab;
        let dialog =
            egui::Modal::new(egui::Id::new("alignment-monitor-settings")).show(ctx, |ui| {
                ui.set_width(460.);
                ui.heading("Signal Monitor Settings");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut next, SettingsTab::Acquisition, "Acquisition");
                    ui.selectable_value(&mut next, SettingsTab::Recording, "Recording");
                });
                ui.add_space(10.);
                match next {
                    SettingsTab::Acquisition => {
                        let plan = &mut self.plans[index];
                        ui.add_enabled_ui(!recording, |ui| {
                            egui::Grid::new("alignment-acquisition-settings")
                                .spacing([18., 10.])
                                .show(ui, |ui| {
                                    ui.label("Interval (s)");
                                    ui.add(
                                        egui::DragValue::new(&mut plan.interval_s)
                                            .speed(0.01)
                                            .range(0.0..=3600.0)
                                            .max_decimals(3),
                                    );
                                    ui.end_row();
                                    ui.label("Length");
                                    ui.horizontal(|ui| {
                                        ui.add_enabled(
                                            !self.monitor_until_stop,
                                            egui::DragValue::new(&mut plan.n_points)
                                                .range(1..=trkr_core::MAX_TOTAL_POINTS)
                                                .suffix(" points"),
                                        );
                                        ui.checkbox(&mut self.monitor_until_stop, "Continuous");
                                    });
                                    ui.end_row();
                                });
                            devices = ui.button("Measurement Devices…").clicked();
                        });
                    }
                    SettingsTab::Recording => {
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
                        ui.label(
                            egui::RichText::new(self.alignment_recording_name(false)).monospace(),
                        );
                        save = ui
                            .add_enabled(
                                self.output_paths[index].is_some()
                                    && self.dialog.is_none()
                                    && self.snapshot_copy.is_none(),
                                egui::Button::new("Save Snapshot…"),
                            )
                            .clicked();
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
        self.alignment_workspace.settings =
            (!(close || devices || dialog.should_close())).then_some(next);
        if devices {
            self.open_measurement_settings();
        }
    }
}

pub(super) fn draw_monitor_trend(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: egui::Id,
    (name, points, color): (&str, Vec<[f64; 2]>, Color32),
    show_time: bool,
) {
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
        |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(rect));
            Plot::new(id)
                .width(rect.width())
                .height(rect.height() - 2.)
                .show_background(false)
                .show_axes([show_time, true])
                .show_grid([true, true])
                .allow_zoom(false)
                .allow_scroll(false)
                .allow_drag(false)
                .include_x(-30.)
                .include_x(0.)
                .x_axis_formatter(|tick, _| format!("{} s", tick.value))
                .y_axis_min_width(62.)
                .show(ui, |plot| {
                    plot.line(Line::new(name, points).color(color).width(1.5_f32))
                });
        },
    );
}

#[cfg(all(test, feature = "gui-interaction-tests"))]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    #[test]
    fn planned_filename_opens_recording_settings_and_changes_next_output() {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1566., 700.))
            .build_eframe(|cc| App::new_alignment_jog_trial(cc).unwrap());
        h.run_steps(3);
        let original = h.state().alignment_recording_name(false);
        h.get_by_label(&original).click();
        h.run_steps(3);
        assert_eq!(
            h.state().alignment_workspace.settings,
            Some(SettingsTab::Recording)
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
        h.get_by_label(&original);
        h.state_mut().plans[App::ix(Workflow::SignalMonitor)].n_points = 2;
        h.get_by_label("Record").click();
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
    fn power_and_motion_only_can_be_toggled_without_changing_measurement_workflow() {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1566., 700.))
            .build_eframe(|cc| App::new_alignment_jog_trial(cc).unwrap());
        h.run_steps(3);
        h.get_by_label("Power Monitor").click();
        h.run_steps(3);
        assert!(h.state().geometry.contains_key("alignment_power_monitor"));
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
