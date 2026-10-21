//! Compact, display-only lock-in monitors shared by the fixed desktop layout.
use crate::{engine::ReadingOrigin, live::History};
use eframe::egui::{self, Align, Color32, Layout, RichText, Stroke};
use egui_plot::{Line, Plot};
use trkr_core::DisplayMode;

const TEAL: Color32 = Color32::from_rgb(63, 215, 194);
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
        ] {
            if value >= factor * (1. - 1e-12) {
                return Self { factor, prefix };
            }
        }
        Self {
            factor: 1e-12,
            prefix: "p",
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
pub fn draw(
    ui: &mut egui::Ui,
    area: egui::Rect,
    history: &History,
    mode: DisplayMode,
    channel: usize,
    active: bool,
) -> egui::Rect {
    let channel = channel.min(1);
    let color = if channel == 0 { TEAL } else { ORANGE };
    let angular = mode == DisplayMode::RTheta && channel == 1;
    let voltage_scale = 1.;
    let newest = history.newest(active);
    let window = newest.clamp(2., 30.);
    let points = history.points_at(mode, channel, voltage_scale, newest);
    let extent = points.iter().fold(0_f64, |m, point| m.max(point[1].abs()));
    let scale = if angular {
        Scale {
            factor: 1.,
            prefix: "",
        }
    } else {
        Scale::for_magnitude(extent)
    };
    let current = history.signal().map(|signal| mode.values(signal)[channel]);
    let state = if history.signal().is_none() {
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
    let unit = if angular { "deg" } else { "V" };

    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(area)
            .layout(Layout::top_down(Align::Min)),
        |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(area));
            ui.spacing_mut().item_spacing.y = 1.;
            ui.set_min_width(area.width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(channel_name(mode, channel))
                        .strong()
                        .color(color),
                );
                ui.label(
                    RichText::new(origin_name(history.origin()))
                        .size(9.)
                        .color(MUTED),
                );
                ui.label(RichText::new(state).size(9.).color(if active {
                    Color32::from_rgb(102, 174, 255)
                } else {
                    MUTED
                }));
            });
            let display = current.map_or_else(
                || format!("— {unit}"),
                |value| {
                    if angular {
                        format!("{value:.3} deg")
                    } else {
                        format!("{:.4} {}{unit}", value / scale.factor, scale.prefix)
                    }
                },
            );
            ui.label(RichText::new(display).monospace().size(22.).color(color));

            let (bar, _) =
                ui.allocate_exact_size(egui::vec2(area.width(), 6.), egui::Sense::hover());
            let fraction = current
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
                color,
            );
            painter.line_segment(
                [
                    egui::pos2(center, bar.top()),
                    egui::pos2(center, bar.bottom()),
                ],
                Stroke::new(1_f32, Color32::WHITE),
            );

            Plot::new(("trkr_iv_live", mode == DisplayMode::RTheta, channel))
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
                        Line::new("trend", points)
                            .color(color)
                            .width(1.5_f32)
                            .fill(0_f32)
                            .fill_alpha(0.75_f32),
                    );
                });
            let age = history
                .age()
                .map_or_else(|| "—".into(), |seconds| format!("{seconds:.1} s"));
            ui.painter().text(
                egui::pos2(area.left(), area.bottom()),
                egui::Align2::LEFT_BOTTOM,
                format!("Time · s ago · {}{unit} · {age} ago", scale.prefix),
                egui::FontId::proportional(9.),
                MUTED,
            );
        },
    )
    .response
    .rect
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
                let _ = context.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(400., 300.),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let area = egui::Rect::from_min_size(
                                ui.max_rect().min,
                                egui::vec2(167., 140.),
                            );
                            let used = draw(ui, area, &history, mode, channel, false);
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
}
