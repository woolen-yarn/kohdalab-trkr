use super::*;

pub(super) struct PositionDisplay {
    pub(super) current: Option<f64>,
    pub(super) limits: Option<[f64; 2]>,
    pub(super) label: String,
    pub(super) native_increases: bool,
    pub(super) native_readback: Option<String>,
}

pub(super) fn delay_display(panel: &DelayPanel, unit: AlignmentUnit) -> PositionDisplay {
    let micro = panel
        .state
        .as_ref()
        .and_then(|d| d.delay_calibration.as_ref())
        .and_then(|c| c.microstep_division)
        .unwrap_or(1);
    let convert = |native| match unit {
        AlignmentUnit::Native => Some(native),
        _ => panel
            .entry
            .config
            .measurement_ps_from_mm_with_micro(native, micro)
            .ok()
            .map(|v| {
                if unit == AlignmentUnit::Corrected {
                    v - panel.entry.zero_ps
                } else {
                    v
                }
            }),
    };
    let endpoints = panel
        .entry
        .config
        .normalized()
        .and_then(|c| c.native_limits_mm(micro))
        .ok()
        .and_then(|(low, high)| Some([convert(low)?, convert(high)?]));
    PositionDisplay {
        current: panel
            .state
            .as_ref()
            .and_then(|d| d.position.delay_stage_mm)
            .and_then(convert),
        limits: endpoints.map(|[a, b]| [a.min(b), a.max(b)]),
        label: match unit {
            AlignmentUnit::Native => "Position (mm)",
            AlignmentUnit::Physical => "t (ps)",
            AlignmentUnit::Corrected => "t_cor (ps)",
        }
        .into(),
        native_increases: endpoints.is_none_or(|[a, b]| b > a),
        native_readback: None,
    }
}

pub(super) fn scanner_display(
    panel: &ScannerPanel,
    unit: AlignmentUnit,
    axis: usize,
) -> PositionDisplay {
    let config = if axis == 0 {
        &panel.entry.x
    } else {
        &panel.entry.y
    };
    let config = config.as_ref().and_then(|c| c.clone().normalized().ok());
    let position = panel.state.as_ref().map(|d| &d.position);
    let native = position.and_then(|p| {
        if axis == 0 {
            p.x_scanner_mm.or(p.x_scanner_deg)
        } else {
            p.y_scanner_mm.or(p.y_scanner_deg)
        }
    });
    let convert = |raw| {
        if unit == AlignmentUnit::Native {
            Some(raw)
        } else {
            config.as_ref().map(|c| c.sample_um_from_raw(raw)).map(|v| {
                if unit == AlignmentUnit::Corrected {
                    v - panel.entry.zero_um[axis]
                } else {
                    v
                }
            })
        }
    };
    let endpoints = config
        .as_ref()
        .and_then(|c| Some([convert(c.min_pos?)?, convert(c.max_pos?)?]));
    let name = if axis == 0 { "X" } else { "Y" };
    let label = match unit {
        AlignmentUnit::Native => format!(
            "{name} ({})",
            if config.as_ref().is_some_and(|c| c.controller == "CONEXAGAP") {
                "°"
            } else {
                "mm"
            }
        ),
        AlignmentUnit::Physical => format!("{name} (µm)"),
        AlignmentUnit::Corrected => format!("{name}_cor (µm)"),
    };
    let current = if unit == AlignmentUnit::Native {
        native
    } else {
        position.and_then(|p| p.physical[axis + 1]).map(|v| {
            if unit == AlignmentUnit::Corrected {
                v - panel.entry.zero_um[axis]
            } else {
                v
            }
        })
    };
    PositionDisplay {
        current,
        limits: endpoints.map(|[a, b]| [a.min(b), a.max(b)]),
        label,
        native_increases: endpoints.is_none_or(|[a, b]| b > a),
        native_readback: config.as_ref().and_then(|config| {
            let p = position?;
            if config.controller == "CONEXAGAP" {
                let degrees = if axis == 0 {
                    p.x_scanner_deg
                } else {
                    p.y_scanner_deg
                }?;
                Some(format!(
                    "Controller {} · measured angle: {degrees}°",
                    config.axis
                ))
            } else {
                let mm = if axis == 0 {
                    p.x_scanner_mm
                } else {
                    p.y_scanner_mm
                }?;
                Some(format!("Controller TP: {mm} mm"))
            }
        }),
    }
}

pub(super) fn position_rail(ui: &mut egui::Ui, rect: egui::Rect, display: &PositionDisplay) {
    let y = rect.top() + 9.;
    ui.painter().rect_filled(
        egui::Rect::from_min_max(egui::pos2(rect.left(), y), egui::pos2(rect.right(), y + 6.)),
        3.,
        BORDER,
    );
    let Some([low, high]) = display
        .limits
        .filter(|[a, b]| a.is_finite() && b.is_finite() && b > a)
    else {
        label(
            ui,
            rect.left_bottom() - Vec2::new(0., 12.),
            "Travel unavailable",
            MUTED,
        );
        return;
    };
    if let Some(value) = display.current {
        let x = rect.left() + ((value - low) / (high - low)).clamp(0., 1.) as f32 * rect.width();
        ui.painter().rect_filled(
            egui::Rect::from_center_size(egui::pos2(x, y + 3.), Vec2::new(24., 16.)),
            4.,
            ACCENT,
        );
    }
    for (x, align, value) in [
        (rect.left(), egui::Align2::LEFT_TOP, low),
        (rect.right(), egui::Align2::RIGHT_TOP, high),
    ] {
        ui.painter().text(
            egui::pos2(x, y + 18.),
            align,
            compact(Some(value)),
            egui::FontId::proportional(11.),
            MUTED,
        );
    }
}

pub(super) fn position_graph(ui: &mut egui::Ui, rect: egui::Rect, display: &[PositionDisplay; 2]) {
    ui.painter().rect(
        rect,
        5.,
        Color32::from_rgb(21, 22, 27),
        Stroke::new(1_f32, BORDER),
        egui::StrokeKind::Inside,
    );
    let area = rect.shrink(16.);
    let limits: [[f64; 2]; 2] = std::array::from_fn(|axis| {
        display[axis]
            .limits
            .filter(|[a, b]| a.is_finite() && b.is_finite() && b > a)
            .unwrap_or_else(|| {
                let span = display[axis].current.unwrap_or(0.).abs().max(1.) * 1.2;
                [-span, span]
            })
    });
    let map = |axis: usize, value: f64| {
        ((value - limits[axis][0]) / (limits[axis][1] - limits[axis][0])).clamp(0., 1.) as f32
    };
    for fraction in [0., 0.5, 1.] {
        let x = area.left() + fraction * area.width();
        let y = area.top() + fraction * area.height();
        ui.painter().line_segment(
            [egui::pos2(x, area.top()), egui::pos2(x, area.bottom())],
            Stroke::new(0.5_f32, BORDER),
        );
        ui.painter().line_segment(
            [egui::pos2(area.left(), y), egui::pos2(area.right(), y)],
            Stroke::new(0.5_f32, BORDER),
        );
    }
    for (axis, bounds) in limits.iter().enumerate() {
        if bounds[0] <= 0. && bounds[1] >= 0. {
            let fraction = map(axis, 0.);
            let points = if axis == 0 {
                let x = area.left() + fraction * area.width();
                [egui::pos2(x, area.top()), egui::pos2(x, area.bottom())]
            } else {
                let y = area.bottom() - fraction * area.height();
                [egui::pos2(area.left(), y), egui::pos2(area.right(), y)]
            };
            ui.painter().line_segment(points, Stroke::new(1_f32, MUTED));
        }
    }
    label(ui, rect.right_center() - Vec2::new(18., 14.), "X", MUTED);
    label(ui, rect.center_top() + Vec2::new(3., 1.), "Y", MUTED);
    if let [Some(x), Some(y)] = display.each_ref().map(|d| d.current) {
        ui.painter().circle_filled(
            egui::pos2(
                area.left() + map(0, x) * area.width(),
                area.bottom() - map(1, y) * area.height(),
            ),
            4.,
            ACCENT,
        );
    }
    let help = format!(
        "{}: {} to {}\n{}: {} to {}",
        display[0].label,
        compact(Some(limits[0][0])),
        compact(Some(limits[0][1])),
        display[1].label,
        compact(Some(limits[1][0])),
        compact(Some(limits[1][1]))
    );
    ui.interact(
        rect,
        ui.scope_id().with((
            "alignment-position",
            rect.left().to_bits(),
            rect.top().to_bits(),
        )),
        egui::Sense::hover(),
    )
    .on_hover_text(help);
}

pub(super) fn compact(value: Option<f64>) -> String {
    value
        .map(|value| format!("{:.3}", if value.abs() < 0.0005 { 0. } else { value }))
        .unwrap_or_else(|| "—".into())
}
