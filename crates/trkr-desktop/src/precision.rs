//! Measured Qt-compatible geometry. Positions are logical pixels, independent of DPI.
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use std::collections::BTreeMap;

pub type Geometry = BTreeMap<String, [f32; 4]>;
pub fn record(geometry: &mut Geometry, name: &str, rect: Rect) {
    geometry.insert(
        name.into(),
        [rect.min.x, rect.min.y, rect.width(), rect.height()],
    );
}
pub fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::from_min_size(Pos2::new(x, y), Vec2::new(w.max(0.), h.max(0.)))
}

fn paint_arrow(ui: &egui::Ui, r: Rect, up: bool) {
    let c = r.center();
    let sign = if up { -1. } else { 1. };
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            c + Vec2::new(-3., -2. * sign),
            c + Vec2::new(3., -2. * sign),
            c + Vec2::new(0., 2. * sign),
        ],
        ui.visuals().text_color(),
        egui::Stroke::NONE,
    ));
}

pub struct Form<'a> {
    pub ui: &'a mut egui::Ui,
    pub geometry: &'a mut Geometry,
    pub origin: Pos2,
    pub enabled: bool,
}

#[derive(Clone, Copy)]
enum PathEdit {
    Opening,
    Editing,
}

impl<'a> Form<'a> {
    pub fn new(ui: &'a mut egui::Ui, geometry: &'a mut Geometry, origin: Pos2) -> Self {
        Self {
            ui,
            geometry,
            origin,
            enabled: true,
        }
    }
    pub fn at(&mut self, name: &str, local: Rect) -> Rect {
        let r = local.translate(self.origin.to_vec2());
        record(self.geometry, name, r);
        r
    }
    pub fn label(&mut self, name: &str, local: Rect, text: impl Into<String>) {
        self.value(name, local, text, Color32::from_rgb(235, 236, 241));
    }
    pub fn value(&mut self, name: &str, local: Rect, text: impl Into<String>, color: Color32) {
        let r = self.at(name, local);
        let galley =
            self.ui
                .painter()
                .layout_no_wrap(text.into(), egui::FontId::proportional(12.), color);
        let position = Pos2::new(r.min.x, r.center().y - galley.size().y * 0.5);
        self.ui
            .painter()
            .with_clip_rect(r.intersect(self.ui.clip_rect()))
            .galley(position, galley, color);
    }
    pub fn button(&mut self, name: &str, local: Rect, text: &str, enabled: bool) -> bool {
        let r = self.at(name, local);
        // Keep the Qt-era fixed rectangles, but use the same restrained controls as
        // the IV desktop: neutral controls recede, while the two measurement
        // actions are immediately recognisable.
        let fill = match (text, enabled && self.enabled) {
            ("Start", true) => Color32::from_rgb(48, 107, 214),
            ("Stop", true) => Color32::from_rgb(173, 69, 76),
            _ => Color32::from_rgb(45, 46, 53),
        };
        self.ui
            .scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
                if !enabled || !self.enabled {
                    ui.disable();
                }
                let response = ui.put(
                    r,
                    egui::Button::new(text)
                        .fill(fill)
                        .min_size(r.size())
                        .corner_radius(6)
                        .truncate(),
                );
                if name == "start_button" {
                    response.on_hover_text("Start · Ctrl+Enter")
                } else {
                    response
                }
            })
            .inner
            .clicked()
    }
    pub fn text(&mut self, name: &str, local: Rect, value: &mut String) -> egui::Response {
        let r = self.at(name, local);
        self.ui
            .scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
                if !self.enabled {
                    ui.disable();
                }
                ui.put(
                    r,
                    egui::TextEdit::singleline(value)
                        .id(egui::Id::new(name))
                        .margin(Vec2::new(4., 2.))
                        .min_size(r.size())
                        .vertical_align(egui::Align::Center)
                        .desired_width(r.width()),
                )
            })
            .inner
    }
    pub fn path(&mut self, name: &str, local: Rect, value: &mut String) {
        let r = self.at(name, local);
        let id = egui::Id::new((name, "path_edit"));
        let editing = self.ui.ctx().data_mut(|data| data.get_temp::<PathEdit>(id));
        self.ui
            .scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
                if !self.enabled {
                    ui.disable();
                }
                let editing = if ui.is_enabled() {
                    editing
                } else {
                    ui.ctx().data_mut(|data| data.remove::<PathEdit>(id));
                    ui.memory_mut(|memory| memory.surrender_focus(egui::Id::new(name)));
                    None
                };
                if let Some(editing) = editing {
                    let response = ui.put(
                        r,
                        egui::TextEdit::singleline(value)
                            .id(egui::Id::new(name))
                            .margin(Vec2::new(4., 2.))
                            .min_size(r.size())
                            .vertical_align(egui::Align::Center)
                            .desired_width(r.width()),
                    );
                    if matches!(editing, PathEdit::Opening) {
                        // Focus the TextEdit only once it exists, not the
                        // preceding summary button or every repaint.
                        response.request_focus();
                        ui.ctx()
                            .data_mut(|data| data.insert_temp(id, PathEdit::Editing));
                    }
                    let close = response.lost_focus()
                        || ui.input(|input| {
                            input.key_pressed(egui::Key::Enter)
                                || input.key_pressed(egui::Key::Escape)
                        });
                    if close {
                        response.surrender_focus();
                        ui.ctx().data_mut(|data| data.remove::<PathEdit>(id));
                    }
                    response.on_hover_text(value.as_str());
                } else {
                    let summary = path_summary(value);
                    let response = ui.put(
                        r,
                        egui::Button::new(summary)
                            .fill(Color32::from_rgb(20, 21, 25))
                            .min_size(r.size())
                            .corner_radius(6)
                            .truncate(),
                    );
                    if response.clicked() {
                        ui.ctx()
                            .data_mut(|data| data.insert_temp(id, PathEdit::Opening));
                        ui.ctx().request_repaint();
                    }
                    response.on_hover_text(format!("{}\nClick to edit the full value.", value));
                }
            });
    }
    pub fn editable_combo(
        &mut self,
        name: &str,
        local: Rect,
        value: &mut String,
        options: &[String],
    ) {
        let r = self.at(name, local);
        self.ui
            .scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
                if !self.enabled {
                    ui.disable();
                }
                let arrow_width = 18.;
                let text_rect =
                    Rect::from_min_max(r.min, Pos2::new(r.max.x - arrow_width, r.max.y));
                ui.put(
                    text_rect,
                    egui::TextEdit::singleline(value)
                        .id(egui::Id::new((name, "text")))
                        .margin(Vec2::new(4., 2.))
                        .min_size(text_rect.size())
                        .vertical_align(egui::Align::Center)
                        .desired_width(text_rect.width()),
                );
                let arrow_rect = Rect::from_min_max(Pos2::new(text_rect.max.x, r.min.y), r.max);
                let arrow = ui.put(
                    arrow_rect,
                    egui::Button::new("")
                        .min_size(arrow_rect.size())
                        .corner_radius(6),
                );
                paint_arrow(ui, arrow_rect, false);
                egui::Popup::menu(&arrow).show(|ui| {
                    if options.is_empty() {
                        ui.label("No ports found");
                    }
                    for option in options {
                        if ui.selectable_label(*value == *option, option).clicked() {
                            *value = option.clone();
                            ui.close();
                        }
                    }
                });
            });
    }
    pub fn check(&mut self, name: &str, local: Rect, value: &mut bool, label: &str) {
        let r = self.at(name, local);
        self.ui
            .scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
                if !self.enabled {
                    ui.disable();
                }
                ui.put(r, egui::Checkbox::new(value, label));
            });
    }

    pub fn number(
        &mut self,
        name: &str,
        local: Rect,
        value: &mut f64,
        step: f64,
        decimals: usize,
        enabled: bool,
    ) {
        let r = self.at(name, local);
        let id = egui::Id::new(name);
        self.ui
            .scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
                if !enabled || !self.enabled {
                    ui.disable();
                }
                let focused = ui.memory(|m| m.has_focus(id));
                let mut text = if focused {
                    ui.data(|d| d.get_temp::<String>(id))
                        .unwrap_or_else(|| format!("{value:.decimals$}"))
                } else {
                    format!("{value:.decimals$}")
                };
                let arrows = name == "signal_points_spin";
                let field = if arrows {
                    Rect::from_min_max(r.min, Pos2::new(r.max.x - 15., r.max.y))
                } else {
                    r
                };
                let response = ui.put(
                    field,
                    egui::TextEdit::singleline(&mut text)
                        .id(id)
                        .margin(Vec2::new(4., 2.))
                        .min_size(field.size())
                        .vertical_align(egui::Align::Center)
                        .desired_width(field.width()),
                );
                numeric_feedback(ui, field, &response, &text);
                if response.changed()
                    && let Ok(parsed) = text.parse::<f64>()
                    && parsed.is_finite()
                {
                    *value = parsed;
                }
                ui.data_mut(|d| d.insert_temp(id, text));
                let mut clicked = 0.;
                if arrows {
                    let up = rect(r.max.x - 15., r.min.y, 15., r.height() * 0.5);
                    let down = rect(r.max.x - 15., r.center().y, 15., r.height() * 0.5);
                    for (r, is_up, change) in [(up, true, 1.), (down, false, -1.)] {
                        let response = ui.interact(r, id.with(is_up), egui::Sense::click());
                        let fill = ui.style().interact(&response).bg_fill;
                        ui.painter().rect_filled(r, 0., fill);
                        paint_arrow(ui, r, is_up);
                        if response.clicked() {
                            clicked = change;
                        }
                    }
                }
                let key = clicked
                    + if focused && enabled && self.enabled && ui.is_enabled() {
                        ui.input(|i| {
                            if i.key_pressed(egui::Key::ArrowUp) {
                                1.
                            } else if i.key_pressed(egui::Key::ArrowDown) {
                                -1.
                            } else {
                                0.
                            }
                        })
                    } else {
                        0.
                    };
                let incremented = *value + step * key;
                if key != 0. && incremented.is_finite() {
                    *value = incremented;
                    ui.data_mut(|d| d.insert_temp(id, format!("{value:.decimals$}")));
                }
            });
    }
    pub fn combo<T: PartialEq + Clone>(
        &mut self,
        name: &str,
        local: Rect,
        value: &mut T,
        options: &[(T, &str)],
    ) {
        let r = self.at(name, local);
        self.ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(r)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                if !self.enabled {
                    ui.disable();
                }
                ui.spacing_mut().combo_width = r.width();
                ui.spacing_mut().interact_size.y = r.height();
                let label = options
                    .iter()
                    .find(|(v, _)| v == value)
                    .map(|(_, s)| *s)
                    .unwrap_or("—");
                egui::ComboBox::from_id_salt(name)
                    .width(r.width())
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        for (v, label) in options {
                            ui.selectable_value(value, v.clone(), *label);
                        }
                    });
            },
        );
    }
    pub fn segments<T: PartialEq + Clone>(
        &mut self,
        name: &str,
        local: Rect,
        value: &mut T,
        options: &[(T, &str)],
    ) {
        let r = self.at(name, local);
        let width = (r.width() - 3. * (options.len().saturating_sub(1)) as f32)
            / options.len().max(1) as f32;
        for (i, (v, label)) in options.iter().enumerate() {
            let segment = Rect::from_min_size(
                r.min + Vec2::new(i as f32 * (width + 3.), 0.),
                Vec2::new(width, r.height()),
            );
            let clicked = self
                .ui
                .scope(|ui| {
                    if !self.enabled {
                        ui.disable();
                    }
                    ui.put(
                        segment,
                        egui::Button::new(*label)
                            .selected(value == v)
                            .min_size(segment.size()),
                    )
                    .clicked()
                })
                .inner;
            if clicked {
                *value = v.clone();
            }
        }
    }
    pub fn group(&mut self, name: &str, local: Rect, title: &str) {
        let r = self.at(name, local);
        self.ui.painter().rect(
            r,
            10.,
            Color32::from_rgb(26, 27, 32),
            egui::Stroke::new(1_f32, Color32::from_rgb(56, 58, 67)),
            egui::StrokeKind::Inside,
        );
        let font = egui::FontId::proportional(12.);
        let galley =
            self.ui
                .painter()
                .layout_no_wrap(title.into(), font, Color32::from_rgb(235, 236, 241));
        let textrect = Rect::from_min_size(r.min + Vec2::new(10., 6.), galley.size());
        self.ui
            .painter()
            .galley(textrect.min, galley, Color32::from_rgb(235, 236, 241));
    }
    pub fn toggle(&mut self, name: &str, local: Rect, expanded: &mut bool, label: &str) {
        if self.button(name, local, "", true) {
            *expanded = !*expanded;
        }
        let r = local.translate(self.origin.to_vec2());
        let center = r.min + Vec2::new(11., 12.);
        let triangle = if *expanded {
            vec![
                center + Vec2::new(-3., -2.),
                center + Vec2::new(3., -2.),
                center + Vec2::new(0., 2.),
            ]
        } else {
            vec![
                center + Vec2::new(-2., -3.),
                center + Vec2::new(-2., 3.),
                center + Vec2::new(2., 0.),
            ]
        };
        self.ui.painter().add(egui::Shape::convex_polygon(
            triangle,
            Color32::LIGHT_GRAY,
            egui::Stroke::NONE,
        ));
        self.value(
            &format!("{name}_caption"),
            rect(
                local.min.x + 25.,
                local.min.y,
                local.width() - 28.,
                local.height(),
            ),
            label,
            Color32::from_rgb(235, 236, 241),
        );
    }
}

fn path_summary(value: &str) -> String {
    let trimmed = value.trim_end_matches(['/', '\\']);
    match trimmed.rsplit_once(['/', '\\']) {
        Some((_, tail)) if !tail.is_empty() => format!("…/{tail}"),
        _ => value.to_owned(),
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Columns {
    pub left: Rect,
    pub center: Rect,
    pub right: Rect,
    pub measurement: Rect,
    pub log: Rect,
    pub snapshot: Rect,
    pub plots: Rect,
    pub toolbar: Rect,
    pub toggle: Rect,
    pub left_toggle: Rect,
}
impl Columns {
    pub const WIDTH: f32 = 850.;
    pub const RIGHT_WIDTH: f32 = 300.;
    pub fn minimum_width(collapsed: bool, left_collapsed: bool) -> f32 {
        (if left_collapsed { 40. } else { 384. })
            + Self::WIDTH
            + 32.
            + if collapsed { 27. } else { Self::RIGHT_WIDTH }
    }
    pub fn new(viewport: Rect, collapsed: bool, left_collapsed: bool) -> Self {
        let w = viewport.width();
        let h = viewport.height();
        let left = if left_collapsed { 40. } else { 384. };
        let right = if collapsed { 27. } else { Self::RIGHT_WIDTH };
        let top = 235.;
        let center_width = (w - left - right - 32.).max(Self::WIDTH);
        let c = rect(left + 16., 8., center_width, h - 16.);
        let measurement = rect(c.min.x + 9., 17., c.width() - 18., top);
        let plot_y = measurement.max.y + 40.;
        let right_x = c.max.x + 8.;
        let origin = viewport.min.to_vec2();
        let mov = |r: Rect| r.translate(origin);
        Self {
            left: mov(rect(8., 8., left, h - 16.)),
            left_toggle: mov(rect(left - 24., 8., 24., 28.)),
            center: mov(c),
            right: mov(rect(right_x, 8., right, h - 16.)),
            measurement: mov(measurement),
            log: mov(rect(right_x + 37., 17., right - 46., top)),
            snapshot: mov(rect(
                right_x + 37.,
                17. + top + 8.,
                right - 46.,
                h - 34. - top - 8.,
            )),
            plots: mov(rect(
                c.min.x + 9.,
                plot_y,
                c.width() - 18.,
                h - 17. - plot_y,
            )),
            toolbar: mov(rect(
                c.min.x + 9.,
                measurement.max.y + 8.,
                c.width() - 18.,
                24.,
            )),
            toggle: mov(rect(
                right_x + if collapsed { 1. } else { 0. },
                8.,
                24.,
                28.,
            )),
        }
    }
    pub fn record(&self, g: &mut Geometry) {
        for (name, r) in [
            ("left_panel", self.left),
            ("left_panel_toggle", self.left_toggle),
            ("center_panel", self.center),
            ("center_top_widget", self.measurement),
            ("right_panel", self.right),
            (
                "right_content",
                rect(
                    self.right.min.x + 28.,
                    self.right.min.y,
                    self.right.width() - 28.,
                    self.right.height(),
                ),
            ),
            ("log", self.log),
            ("snapshot_table", self.snapshot),
            ("plot_stack", self.plots),
            ("right_panel_toggle", self.toggle),
        ] {
            record(g, name, r);
        }
    }
}

pub fn configure(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    if cfg!(windows) {
        let windows = std::env::var_os("WINDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "C:\\Windows".into());
        // Segoe UI has the lighter, compact Windows control metrics used by the
        // IV app. Yu Gothic UI remains the Japanese fallback when Segoe is not
        // available, and supplies Japanese glyphs after Segoe UI.
        for (order, file) in ["segoeui.ttf", "YuGothR.ttc"].into_iter().enumerate() {
            if let Ok(bytes) = std::fs::read(windows.join("Fonts").join(file)) {
                let index = crate::font_selection::windows_ui_face_index(&bytes);
                let mut data = egui::FontData::from_owned(bytes);
                data.index = index;
                fonts
                    .font_data
                    .insert(format!("windows-ui-{order}"), std::sync::Arc::new(data));
                let family = fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default();
                family.insert(order.min(family.len()), format!("windows-ui-{order}"));
            }
        }
    }
    ctx.set_fonts(fonts);
    ctx.global_style_mut(|style| {
        for size in style.text_styles.values_mut() {
            *size = egui::FontId::proportional(12.);
        }
        style.spacing.interact_size = Vec2::new(80., 26.);
        style.spacing.button_padding = Vec2::new(6., 3.);
        style.spacing.item_spacing = Vec2::new(6., 6.);
        style.spacing.scroll = egui::style::ScrollStyle::solid();
        style.spacing.scroll.bar_width = 12.;
        style.spacing.scroll.bar_inner_margin = 0.;
        style.spacing.scroll.bar_outer_margin = 0.;
        // IV's dark, low-contrast palette. This belongs here rather than in
        // individual layouts so cards retain their measured TRKR geometry.
        style.visuals.panel_fill = Color32::from_rgb(18, 19, 23);
        style.visuals.window_fill = Color32::from_rgb(26, 27, 32);
        style.visuals.window_corner_radius = 12.into();
        style.visuals.window_stroke = egui::Stroke::new(1_f32, Color32::from_rgb(56, 58, 67));
        style.visuals.extreme_bg_color = Color32::from_rgb(18, 19, 23);
        style.visuals.text_edit_bg_color = Some(Color32::from_rgb(20, 21, 25));
        style.visuals.faint_bg_color = Color32::from_rgb(26, 27, 32);
        style.visuals.override_text_color = Some(Color32::from_rgb(235, 236, 241));
        style.visuals.selection.bg_fill = Color32::from_rgb(48, 91, 158);
        style.visuals.selection.stroke = egui::Stroke::new(1_f32, Color32::from_rgb(235, 236, 241));
        for widget in [
            &mut style.visuals.widgets.inactive,
            &mut style.visuals.widgets.hovered,
            &mut style.visuals.widgets.active,
        ] {
            widget.bg_stroke = egui::Stroke::new(1_f32, Color32::from_rgb(56, 58, 67));
            widget.corner_radius = 6.into();
            widget.fg_stroke = egui::Stroke::new(1_f32, Color32::from_rgb(235, 236, 241));
        }
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(45, 46, 53);
        style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(20, 21, 25);
        style.visuals.widgets.inactive.bg_stroke =
            egui::Stroke::new(0.7_f32, Color32::from_rgb(56, 58, 67));
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(62, 64, 74);
        style.visuals.widgets.active.bg_fill = Color32::from_rgb(48, 91, 158);
        style.visuals.widgets.noninteractive.bg_stroke =
            egui::Stroke::new(1_f32, Color32::from_rgb(56, 58, 67));
    });
}

/// Called after editing so feedback reflects this frame's text. Drawing an
/// outline and tooltip does not reserve layout space or replace the editor ID.
fn numeric_feedback(ui: &egui::Ui, field: egui::Rect, response: &egui::Response, text: &str) {
    if !text.parse::<f64>().is_ok_and(f64::is_finite) {
        ui.painter().rect_stroke(
            field,
            6.,
            egui::Stroke::new(1_f32, ui.visuals().error_fg_color),
            egui::StrokeKind::Inside,
        );
        response.clone().on_hover_text(
            "Input not applied: enter a finite number. Start uses the last accepted value.",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn focused_number_cannot_change_with_arrow_keys_when_disabled() {
        let ctx = egui::Context::default();
        let mut value = 5.;
        let id = egui::Id::new("locked-number");
        ctx.memory_mut(|m| m.request_focus(id));
        let _ = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::ArrowUp,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                }],
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show_inside(ui, |ui| {
                    let mut geometry = Geometry::new();
                    Form::new(ui, &mut geometry, Pos2::ZERO).number(
                        "locked-number",
                        rect(10., 10., 100., 26.),
                        &mut value,
                        1.,
                        2,
                        false,
                    );
                });
            },
        );
        assert_eq!(value, 5.);
    }
    #[test]
    fn number_increment_never_commits_infinity() {
        let ctx = egui::Context::default();
        let mut value = f64::MAX;
        ctx.memory_mut(|m| m.request_focus(egui::Id::new("overflow-number")));
        let _ = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::ArrowUp,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                }],
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show_inside(ui, |ui| {
                    let mut geometry = Geometry::new();
                    Form::new(ui, &mut geometry, Pos2::ZERO).number(
                        "overflow-number",
                        rect(10., 10., 100., 26.),
                        &mut value,
                        f64::MAX,
                        2,
                        true,
                    );
                });
            },
        );
        assert_eq!(value, f64::MAX);
    }
    fn draw_paths(
        context: &egui::Context,
        input: egui::RawInput,
        first: &mut String,
        second: &mut String,
    ) {
        draw_enabled_paths(context, input, first, second, true);
    }
    fn draw_enabled_paths(
        context: &egui::Context,
        input: egui::RawInput,
        first: &mut String,
        second: &mut String,
        enabled: bool,
    ) {
        let _ = context.run_ui(input, |ui| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show_inside(ui, |ui| {
                    if !enabled {
                        ui.disable();
                    }
                    let mut geometry = Geometry::new();
                    let mut form = Form::new(ui, &mut geometry, Pos2::ZERO);
                    form.path("first_path", rect(10., 10., 180., 26.), first);
                    form.path("second_path", rect(10., 44., 180., 26.), second);
                });
        });
    }
    fn click(pos: Pos2) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(rect(0., 0., 320., 120.)),
            events: vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        }
    }
    #[test]
    fn path_summary_uses_the_tail_for_platform_paths_and_keeps_roots() {
        assert_eq!(path_summary("/runs/2026/sample"), "…/sample");
        assert_eq!(path_summary("C:\\runs\\2026\\sample\\"), "…/sample");
        assert_eq!(path_summary("/"), "/");
        assert_eq!(path_summary("C:\\"), "C:\\");
        assert_eq!(path_summary("profile.json"), "profile.json");
    }
    #[test]
    fn path_editor_moves_focus_between_fields_and_does_not_recapture_it() {
        let context = egui::Context::default();
        let mut first = "/long/configs/experiment.json".to_owned();
        let mut second = "/long/output/2026-11-09".to_owned();
        draw_paths(&context, Default::default(), &mut first, &mut second);
        draw_paths(
            &context,
            click(egui::pos2(40., 20.)),
            &mut first,
            &mut second,
        );
        draw_paths(&context, Default::default(), &mut first, &mut second);
        assert!(context.memory(|memory| memory.has_focus(egui::Id::new("first_path"))));
        assert_eq!(first, "/long/configs/experiment.json");
        draw_paths(
            &context,
            egui::RawInput {
                screen_rect: Some(rect(0., 0., 320., 120.)),
                events: vec![egui::Event::Text("x".into())],
                ..Default::default()
            },
            &mut first,
            &mut second,
        );
        assert!(first.contains("/long/configs/experiment.json"));

        draw_paths(
            &context,
            click(egui::pos2(40., 54.)),
            &mut first,
            &mut second,
        );
        draw_paths(&context, Default::default(), &mut first, &mut second);
        assert!(!context.memory(|memory| memory.has_focus(egui::Id::new("first_path"))));
        assert!(context.memory(|memory| memory.has_focus(egui::Id::new("second_path"))));

        draw_paths(
            &context,
            egui::RawInput {
                screen_rect: Some(rect(0., 0., 320., 120.)),
                events: vec![egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            &mut first,
            &mut second,
        );
        assert!(!context.memory(|memory| memory.has_focus(egui::Id::new("second_path"))));
        draw_paths(&context, Default::default(), &mut first, &mut second);
        assert!(!context.memory(|memory| memory.has_focus(egui::Id::new("second_path"))));
    }
    #[test]
    fn path_editor_releases_focus_on_escape_and_parent_disable() {
        for disable in [false, true] {
            let context = egui::Context::default();
            let mut first = "C:\\Profiles\\measurement.json".to_owned();
            let mut second = "/output/run".to_owned();
            draw_paths(&context, Default::default(), &mut first, &mut second);
            draw_paths(
                &context,
                click(egui::pos2(40., 20.)),
                &mut first,
                &mut second,
            );
            draw_paths(&context, Default::default(), &mut first, &mut second);
            assert!(context.memory(|memory| memory.has_focus(egui::Id::new("first_path"))));
            let mut input = egui::RawInput::default();
            if !disable {
                input.events.push(egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            draw_enabled_paths(&context, input, &mut first, &mut second, !disable);
            draw_paths(&context, Default::default(), &mut first, &mut second);
            assert!(!context.memory(|memory| memory.has_focus(egui::Id::new("first_path"))));
            assert_eq!(first, "C:\\Profiles\\measurement.json");
        }
    }

    #[test]
    fn only_center_panel_grows_with_window_width() {
        for width in [1566., 1920., 2560.] {
            for collapsed in [false, true] {
                let c = Columns::new(rect(0., 0., width, 820.), collapsed, false);
                let right = if collapsed { 27. } else { Columns::RIGHT_WIDTH };
                assert_eq!(c.right.width(), right);
                assert_eq!(c.center.width(), width - 384. - right - 32.);
                assert_eq!(c.measurement.width(), c.center.width() - 18.);
                assert_eq!(c.plots.width(), c.center.width() - 18.);
                assert_eq!(c.left.width(), 384.);
                assert!(c.left.max.x < c.center.min.x && c.center.max.x < c.right.min.x);
                assert!(c.right.max.x <= width - 8.);
            }
        }
    }
    #[test]
    fn sidebar_window_resize_preserves_center_width() {
        for width in [1566., 1920., 2560.] {
            let expanded = Columns::new(rect(0., 0., width, 900.), false, false);
            let collapsed = Columns::new(
                rect(0., 0., width - (Columns::RIGHT_WIDTH - 27.), 900.),
                true,
                false,
            );
            assert_eq!(expanded.center, collapsed.center);
        }
    }
    #[test]
    fn hiding_left_panel_gives_its_width_to_center() {
        let viewport = rect(0., 0., 1920., 900.);
        for right_collapsed in [false, true] {
            let expanded = Columns::new(viewport, right_collapsed, false);
            let collapsed = Columns::new(viewport, right_collapsed, true);
            assert_eq!(collapsed.left.width(), 40.);
            assert_eq!(collapsed.center.width() - expanded.center.width(), 344.);
            assert_eq!(collapsed.right, expanded.right);
            assert_eq!(collapsed.left_toggle.width(), expanded.left_toggle.width());
        }
    }

    #[test]
    fn fixed_text_edit_has_exact_requested_outer_rectangle() {
        let context = egui::Context::default();
        let mut measured = None;
        let _ = context.run_ui(
            egui::RawInput {
                screen_rect: Some(rect(0., 0., 1440., 820.)),
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show_inside(ui, |ui| {
                        let r = rect(100., 100., 209., 26.);
                        let mut text = "0.000".to_owned();
                        measured = Some(
                            ui.put(
                                r,
                                egui::TextEdit::singleline(&mut text)
                                    .margin(Vec2::new(4., 2.))
                                    .min_size(r.size())
                                    .vertical_align(egui::Align::Center)
                                    .desired_width(r.width()),
                            )
                            .rect,
                        );
                    });
            },
        );
        assert_eq!(measured.unwrap(), rect(100., 100., 209., 26.));
    }
}
