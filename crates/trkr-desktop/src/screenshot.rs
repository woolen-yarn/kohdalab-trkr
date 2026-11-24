//! Native window capture for the demo/smoke validation screenshots.
//!
//! egui 0.37 delivers `ViewportCommand::Screenshot` results to a callback
//! instead of an `Event::Screenshot` in the next frame's input. This keeps
//! the old request-then-poll flow: `request` clears any earlier capture, and
//! `take` returns the image once the integration has produced it.
use eframe::egui;
use std::sync::{Arc, Mutex, PoisonError};

type Slot = Arc<Mutex<Option<Arc<egui::ColorImage>>>>;

fn slot(ctx: &egui::Context) -> Slot {
    ctx.data_mut(|data| data.get_temp_mut_or_default::<Slot>(egui::Id::NULL).clone())
}

pub(crate) fn request(ctx: &egui::Context) {
    let slot = slot(ctx);
    *slot.lock().unwrap_or_else(PoisonError::into_inner) = None;
    let repaint = ctx.clone();
    // `send_viewport_cmd` targets the viewport currently being drawn, so the
    // callback only ever receives that viewport's capture.
    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
        egui::ScreenshotCallback::new(move |image| {
            *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(image);
            // The capture may arrive while the app is idle; wake the poller.
            repaint.request_repaint();
        }),
    ));
}

pub(crate) fn take(ctx: &egui::Context) -> Option<Arc<egui::ColorImage>> {
    slot(ctx)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
}
