//! Compact, display-only lock-in monitors shared by the fixed desktop layout.
use crate::{engine::ReadingOrigin, live::History};
use eframe::egui::{self, Align, Color32, Layout, RichText, Stroke};
use egui_plot::{Line, Plot};
use trkr_core::DisplayMode;

pub(crate) const TEAL: Color32 = Color32::from_rgb(63, 215, 194);
const ORANGE: Color32 = Color32::from_rgb(245, 183, 82);
const MUTED: Color32 = Color32::from_rgb(146, 150, 164);
const BORDER: Color32 = Color32::from_rgb(56, 58, 67);

#[derive(Clone, Copy)]
struct Scale {
    factor: f64,
    prefix: &'static str,
}

impl Scale {
    fn for_magnitude(value: f64) -> Self {
        if !value.is_finite() || value <= 0. {
            return Self {
                factor: 1.,
                prefix: "",
            };
        }
        for (factor, prefix) in [
            (1e12, "T"),
            (1e9, "G"),
            (1e6, "M"),
            (1e3, "k"),
            (1., ""),
            (1e-3, "m"),
            (1e-6, "µ"),
            (1e-9, "n"),
            (1e-12, "p"),
            (1e-15, "f"),
            (1e-18, "a"),
        ] {
            if value >= factor * (1. - 1e-12) {
                return Self { factor, prefix };
            }
        }
        Self {
            factor: 1e-18,
            prefix: "a",
        }
    }
    fn tick(self, value: f64, spacing: f64) -> String {
        let step = (spacing.abs() / self.factor).max(f64::MIN_POSITIVE);
        let decimals = (-step.log10()).ceil().clamp(0., 6.) as usize;
        format!("{:.prec$}", value / self.factor, prec = decimals)
    }
}

fn channel_name(mode: DisplayMode, channel: usize) -> &'static str {
    match (mode, channel) {
        (DisplayMode::Xy, 0) => "X",
        (DisplayMode::Xy, _) => "Y",
        (DisplayMode::RTheta, 0) => "R",
        (DisplayMode::RTheta, _) => "θ",
    }
}

fn origin_name(origin: Option<ReadingOrigin>) -> &'static str {
    match origin {
        Some(ReadingOrigin::Demo) => "SIM",
        Some(ReadingOrigin::Replay) => "REPLAY",
        _ => "MEASURED",
    }
}

/// Draw one IV-style, fixed-size live channel. `active` means that new samples
/// are expected; it also determines whether the time axis scrolls or is held.
pub struct LiveChannel {
    pub id: egui::Id,
    pub name: String,
    pub origin: String,
    pub state: String,
    pub active: bool,
    pub unit: String,
    pub prefix: &'static str,
    pub display: String,
    pub current: Option<f64>,
    pub points: Vec<[f64; 2]>,
    pub age_seconds: Option<f64>,
    pub window_seconds: f64,
    pub scale: f64,
    pub color: Color32,
}

pub fn draw_channel(ui: &mut egui::Ui, area: egui::Rect, channel: LiveChannel) -> egui::Rect {
    let extent = channel
        .points
        .iter()
        .fold(0_f64, |m, point| m.max(point[1].abs()));
    let window = channel.window_seconds.clamp(2., 30.);
    let scale = Scale {
        factor: channel.scale,
        prefix: channel.prefix,
    };
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(area)
            .layout(Layout::top_down(Align::Min)),
        |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(area));
            ui.spacing_mut().item_spacing.y = 1.;
            ui.set_min_width(area.width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(&channel.name).strong().color(channel.color));
                ui.label(RichText::new(&channel.origin).size(9.).color(MUTED));
                ui.label(
                    RichText::new(&channel.state)
                        .size(9.)
                        .color(if channel.active {
                            Color32::from_rgb(102, 174, 255)
                        } else {
                            MUTED
                        }),
                );
            });
            ui.label(
                RichText::new(&channel.display)
                    .monospace()
                    .size(22.)
                    .color(channel.color),
            );
            let (bar, _) =
                ui.allocate_exact_size(egui::vec2(area.width(), 6.), egui::Sense::hover());
            let fraction = channel
                .current
                .filter(|_| extent > 0.)
                .map_or(0., |value| (value / extent).clamp(-1., 1.))
                as f32;
            let center = bar.center().x;
            let end = center + fraction * bar.width() * 0.5;
            let painter = ui.painter();
            painter.rect_filled(bar, 2., BORDER);
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(center.min(end), bar.top()),
                    egui::pos2(center.max(end), bar.bottom()),
                ),
                0.,
                channel.color,
            );
            painter.line_segment(
                [
                    egui::pos2(center, bar.top()),
                    egui::pos2(center, bar.bottom()),
                ],
                Stroke::new(1_f32, Color32::WHITE),
            );
            Plot::new(channel.id)
                .width(area.width())
                .height(55.)
                .show_background(false)
                .show_axes([true, true])
                .show_grid([true, true])
                .allow_drag(false)
                .allow_zoom(false)
                .allow_scroll(false)
                .include_x(-window)
                .include_x(0.)
                .include_y(0.)
                .y_axis_formatter(move |mark, _| scale.tick(mark.value, mark.step_size))
                .label_formatter(|_, _| String::new())
                .show(ui, |plot| {
                    plot.line(
                        Line::new("trend", channel.points)
                            .color(channel.color)
                            .width(1.5_f32)
                            .fill(0_f32)
                            .fill_alpha(0.75_f32),
                    );
                });
            let age = channel
                .age_seconds
                .map_or_else(|| "—".into(), |seconds| format!("{seconds:.1} s"));
            ui.painter().text(
                egui::pos2(area.left(), area.bottom()),
                egui::Align2::LEFT_BOTTOM,
                format!(
                    "Time · s ago · {}{} · {age} ago",
                    channel.prefix, channel.unit
                ),
                egui::FontId::proportional(9.),
                MUTED,
            );
        },
    )
    .response
    .rect
}

pub(crate) enum SignalFormat {
    Default,
    Resolution(f64),
    Li5640,
}

pub(crate) struct SignalDisplay<'a> {
    pub unit: &'a str,
    pub format: SignalFormat,
}

pub fn draw(
    ui: &mut egui::Ui,
    area: egui::Rect,
    history: &History,
    mode: DisplayMode,
    channel: usize,
    active: bool,
    SignalDisplay {
        unit: signal_unit,
        format,
    }: SignalDisplay<'_>,
) -> egui::Rect {
    let resolution = match format {
        SignalFormat::Resolution(step) => Some(step),
        _ => None,
    };
    let channel = channel.min(1);
    let color = if channel == 0 { TEAL } else { ORANGE };
    let angular = mode == DisplayMode::RTheta && channel == 1;
    let voltage_scale = 1.;
    let newest = history.newest(active);
    let window = newest.clamp(2., 30.);
    let points = history.points_at(mode, channel, voltage_scale, newest);
    let extent = points.iter().fold(0_f64, |m, point| m.max(point[1].abs()));
    let scale = if angular || !matches!(signal_unit, "V" | "A") {
        Scale {
            factor: 1.,
            prefix: "",
        }
    } else {
        Scale::for_magnitude(extent.max(resolution.unwrap_or(0.)))
    };
    let current = history
        .signal()
        .map(|signal| mode.values(signal)[channel])
        .filter(|value| value.is_finite());
    let state = if current.is_none() {
        "WAITING"
    } else if active && history.recent() {
        "LIVE"
    } else if active {
        "STALE"
    } else if history.signal().is_some() {
        "HELD"
    } else {
        "WAITING"
    };
    let unit = if angular { "deg" } else { signal_unit };

    let display = current.map_or_else(
        || format!("— {unit}"),
        |value| {
            if matches!(format, SignalFormat::Li5640) {
                li5640_live_text(value / scale.factor, angular, scale.prefix, unit)
            } else if angular {
                format!("{value:.3} deg")
            } else {
                let digits = resolution
                    .map(|step| (scale.factor / step).log10().ceil().clamp(0., 6.) as usize)
                    .unwrap_or(4);
                format!("{:.*} {}{unit}", digits, value / scale.factor, scale.prefix)
            }
        },
    );
    draw_channel(
        ui,
        area,
        LiveChannel {
            id: egui::Id::new(("trkr_iv_live", mode == DisplayMode::RTheta, channel)),
            name: channel_name(mode, channel).into(),
            origin: origin_name(history.origin()).into(),
            state: state.into(),
            active,
            unit: unit.into(),
            prefix: scale.prefix,
            display,
            current,
            points,
            age_seconds: history.age(),
            window_seconds: window,
            scale: scale.factor,
            color,
        },
    )
}

// LI5640 DOUT reports the displayed amplitude with up to four significant
// digits and phase in hundredths of a degree. This only formats the live label.
fn li5640_live_text(value: f64, angular: bool, prefix: &str, unit: &str) -> String {
    let text = if angular {
        format!("{value:.2}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned()
    } else {
        crate::ui::App::significant(value, 4)
    };
    format!("{text} {prefix}{unit}")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn iv_channel_contents_fit_the_fixed_panel_in_both_modes() {
        let context = egui::Context::default();
        crate::precision::configure(&context);
        let mut history = History::default();
        history.push(
            trkr_core::Signal::new(-999.123e-6, 999.987e-6, 0.002, -179.999).unwrap(),
            ReadingOrigin::Replay,
        );
        for mode in [DisplayMode::Xy, DisplayMode::RTheta] {
            for channel in 0..2 {
                let _ = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(400., 300.),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        egui::CentralPanel::default().show_inside(ui, |ui| {
                            let area = egui::Rect::from_min_size(
                                ui.max_rect().min,
                                egui::vec2(167., 140.),
                            );
                            let used = draw(
                                ui,
                                area,
                                &history,
                                mode,
                                channel,
                                false,
                                SignalDisplay {
                                    unit: "V",
                                    format: SignalFormat::Default,
                                },
                            );
                            assert!(
                                used.width() <= area.width() + 0.1,
                                "live panel overflow: {used:?}"
                            );
                            assert!(
                                used.height() <= area.height() + 0.1,
                                "live panel overflow: {used:?}"
                            );
                        });
                    },
                );
            }
        }
    }
    #[test]
    fn labels_and_sources_match_lockin_modes() {
        assert_eq!(channel_name(DisplayMode::Xy, 0), "X");
        assert_eq!(channel_name(DisplayMode::RTheta, 1), "θ");
        assert_eq!(origin_name(Some(ReadingOrigin::Replay)), "REPLAY");
        assert_eq!(Scale::for_magnitude(1e-6).prefix, "µ");
    }
    #[test]
    fn li5640_live_values_preserve_reported_digits_without_padding() {
        assert_eq!(li5640_live_text(143.0, false, "n", "V"), "143 nV");
        assert_eq!(li5640_live_text(21.8, false, "n", "V"), "21.8 nV");
        assert_eq!(li5640_live_text(-131.2, false, "p", "A"), "-131.2 pA");
        assert_eq!(li5640_live_text(-145.48, true, "", "deg"), "-145.48 deg");
        assert_eq!(li5640_live_text(90.0, true, "", "deg"), "90 deg");
    }
}
