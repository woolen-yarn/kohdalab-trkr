//! Test-only wrapper for raw `Context::run_ui` passes without a renderer.
use eframe::egui;

/// Run one headless egui pass and return its complete output.
///
/// epaint 0.37 panics when a `TexturesDelta` (e.g. the font atlas upload) is
/// dropped unapplied. These tests have no renderer to apply it, so only the
/// texture delta is discarded explicitly; shapes, platform output and viewport
/// commands are returned unchanged for assertions. Kittest harnesses render
/// normally and do not use this.
pub(crate) fn run_ui(
    ctx: &egui::Context,
    input: egui::RawInput,
    ui: impl FnMut(&mut egui::Ui),
) -> egui::FullOutput {
    let mut output = ctx.run_ui(input, ui);
    output.textures_delta.clear();
    output
}
