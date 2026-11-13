//! Bounded transient notifications. Logs remain authoritative; this module
//! never drives measurement state or changes input commit rules.
use eframe::egui;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

const MAX_NOTIFICATIONS: usize = 3;
const MAX_RECENT_MESSAGES: usize = 16;
const MAX_CAPTION_CHARS: usize = 240;
const REPEAT_INTERVAL: Duration = Duration::from_secs(4);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Info,
    Success,
    Error,
}

pub struct Notifications {
    toasts: egui_notify::Toasts,
    recent: VecDeque<(NoticeKind, String, Instant)>,
}
impl Default for Notifications {
    fn default() -> Self {
        Self {
            toasts: egui_notify::Toasts::default().with_anchor(egui_notify::Anchor::BottomRight),
            recent: VecDeque::new(),
        }
    }
}
impl Notifications {
    pub fn push(&mut self, kind: NoticeKind, message: &str) {
        self.push_at(kind, message, Instant::now());
    }
    fn push_at(&mut self, kind: NoticeKind, message: &str, now: Instant) {
        self.recent
            .retain(|(_, _, at)| now.duration_since(*at) < REPEAT_INTERVAL);
        if self
            .recent
            .iter()
            .any(|(k, text, _)| *k == kind && text == message)
        {
            return;
        }
        // A burst cannot create an unbounded overlay or allocation queue. Every
        // message still reaches the log independently of this visual cap.
        if self.toasts.len() >= MAX_NOTIFICATIONS {
            if kind != NoticeKind::Error {
                return;
            }
            // Errors take precedence over transient completion notices.
            self.toasts =
                egui_notify::Toasts::default().with_anchor(egui_notify::Anchor::BottomRight);
        }
        self.recent.push_back((kind, message.to_owned(), now));
        if self.recent.len() > MAX_RECENT_MESSAGES {
            self.recent.pop_front();
        }
        let caption = notification_caption(message);
        let toast = match kind {
            NoticeKind::Info => self.toasts.info(&caption),
            NoticeKind::Success => self.toasts.success(&caption),
            NoticeKind::Error => self.toasts.error(&caption),
        };
        toast.duration(Some(if kind == NoticeKind::Error {
            Duration::from_secs(4)
        } else {
            Duration::from_secs(3)
        }));
    }
    pub fn show(&mut self, ctx: &egui::Context) {
        self.toasts.show(ctx);
    }
}

/// Keep exceptional device/file-system messages from covering the controls.
/// The complete text remains in the existing log; only this overlay is shortened.
fn notification_caption(message: &str) -> String {
    let mut caption = String::new();
    let mut lines = 1;
    for (index, ch) in message.chars().enumerate() {
        if index >= MAX_CAPTION_CHARS || (ch == '\n' && lines == 3) {
            caption.push_str("… See log for details.");
            break;
        }
        if ch == '\n' {
            lines += 1;
        }
        caption.push(ch);
    }
    caption
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_unicode_and_multiline_errors_keep_the_overlay_compact() {
        assert_eq!(notification_caption("Saved"), "Saved");
        let long = "測定".repeat(200);
        let caption = notification_caption(&long);
        assert!(caption.starts_with("測定"));
        assert!(caption.ends_with("See log for details."));
        assert!(caption.chars().count() < 270);
        let lines = notification_caption("one\ntwo\nthree\nfour");
        assert_eq!(lines.lines().count(), 3);
        assert!(!lines.contains("four"));
    }
    #[test]
    fn repeated_errors_and_event_bursts_are_bounded() {
        let mut notices = Notifications::default();
        let now = Instant::now();
        for _ in 0..100 {
            notices.push_at(NoticeKind::Error, "Connection failed", now);
        }
        assert_eq!(notices.toasts.len(), 1);
        for n in 0..100 {
            notices.push_at(NoticeKind::Info, &format!("Event {n}"), now);
        }
        assert_eq!(notices.toasts.len(), MAX_NOTIFICATIONS);
        assert_eq!(notices.recent.len(), MAX_NOTIFICATIONS);
        notices.push_at(NoticeKind::Error, "A different failure", now);
        assert_eq!(notices.toasts.len(), 1);
    }
}
