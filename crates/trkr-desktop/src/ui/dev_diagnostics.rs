//! Optional development comparison against the released text chevrons.
use eframe::egui::{self, Color32, Pos2, Stroke, Vec2};

pub(super) fn icons_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("TRKR_OSS_ICONS").as_deref() == Ok("1"))
}

pub(super) fn panel_toggle(ui: &mut egui::Ui, right: bool, size: Vec2) -> egui::Response {
    // Retain the text label in AccessKit and the existing button's interaction/size.
    let response = ui.add(
        egui::Button::new(
            egui::RichText::new(if right { ">" } else { "<" }).color(Color32::TRANSPARENT),
        )
        .min_size(size)
        .corner_radius(4),
    );
    let color = ui.style().interact(&response).text_color();
    let origin = response.rect.center() - Vec2::splat(8.);
    // Lucide chevron paths at 16 logical px, from upstream commit
    // 45aaec4b721c98aa86a2acd0ab7281bcf68b9c5e; ISC/MIT notices are retained
    // in crates/trkr-desktop/licenses/lucide-LICENSE.
    let xs = if right { [9., 15., 9.] } else { [15., 9., 15.] };
    let points: Vec<Pos2> = xs
        .into_iter()
        .zip([6., 12., 18.])
        .map(|(x, y)| origin + Vec2::new(x, y) * (16. / 24.))
        .collect();
    ui.painter().add(egui::Shape::line(
        points,
        Stroke::new(2_f32 * 16. / 24., color),
    ));
    response
}
