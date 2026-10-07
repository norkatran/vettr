//! Dismissable popups in the top-right (port of `Notifications.tsx`).
//!
//! Errors stay until dismissed so the user can read git's output; information fades after 6 s.
//! `Notifier` is cheap to clone and can be used from any thread.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::{Align2, Color32, Frame, Margin, RichText, Stroke};

use super::palette::Palette;

const INFO_LIFETIME: Duration = Duration::from_secs(6);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyLevel {
    Error,
    Info,
}

#[derive(Debug, Clone)]
struct Notification {
    id: u64,
    title: String,
    detail: String,
    level: NotifyLevel,
    created: Instant,
}

#[derive(Default)]
struct Shared {
    next_id: u64,
    items: Vec<Notification>,
}

#[derive(Clone, Default)]
pub struct Notifier {
    shared: Arc<Mutex<Shared>>,
}

impl Notifier {
    pub fn new() -> Notifier {
        Notifier::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        match self.shared.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn push(&self, title: &str, detail: &str, level: NotifyLevel) {
        let mut shared = self.lock();
        shared.next_id += 1;
        let id = shared.next_id;
        shared.items.push(Notification {
            id,
            title: title.to_string(),
            detail: detail.to_string(),
            level,
            created: Instant::now(),
        });
    }

    /// Report a failure: stays until dismissed.
    pub fn notify(&self, title: &str, detail: &str) {
        self.push(title, detail, NotifyLevel::Error);
    }

    /// Report a piece of information: fades after a few seconds.
    pub fn info(&self, title: &str, detail: &str) {
        self.push(title, detail, NotifyLevel::Info);
    }

    /// Report with an explicit level.
    pub fn notify_level(&self, title: &str, detail: &str, level: NotifyLevel) {
        self.push(title, detail, level);
    }

    /// How many notifications are currently showing (for tests).
    pub fn len(&self) -> usize {
        self.lock().items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Drop info notifications older than their lifetime; returns the time until the next expiry.
    fn expire(&self, now: Instant) -> Option<Duration> {
        let mut shared = self.lock();
        shared.items.retain(|n| {
            n.level == NotifyLevel::Error || now.duration_since(n.created) < INFO_LIFETIME
        });
        shared
            .items
            .iter()
            .filter(|n| n.level == NotifyLevel::Info)
            .map(|n| INFO_LIFETIME.saturating_sub(now.duration_since(n.created)))
            .min()
    }

    fn dismiss(&self, id: u64) {
        self.lock().items.retain(|n| n.id != id);
    }

    /// Draw the popups. Call once per frame, after the main UI.
    pub fn show(&self, ctx: &egui::Context, palette: &Palette) {
        if let Some(next) = self.expire(Instant::now()) {
            ctx.request_repaint_after(next + Duration::from_millis(50));
        }
        let items: Vec<Notification> = self.lock().items.clone();
        if items.is_empty() {
            return;
        }
        egui::Area::new(egui::Id::new("vettr-notifications"))
            .order(egui::Order::Foreground)
            .anchor(Align2::RIGHT_TOP, egui::vec2(-12.0, 44.0))
            .show(ctx, |ui| {
                ui.set_max_width(420.0);
                for n in &items {
                    let accent: Color32 = match n.level {
                        NotifyLevel::Error => palette.red,
                        NotifyLevel::Info => palette.accent,
                    };
                    Frame::new()
                        .fill(palette.bg_input)
                        .stroke(Stroke::new(1.0, palette.border))
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(Margin::symmetric(12, 10))
                        .show(ui, |ui| {
                            ui.horizontal_top(|ui| {
                                let (bar, _) = ui.allocate_exact_size(
                                    egui::vec2(3.0, 18.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(bar, 1.0, accent);
                                ui.vertical(|ui| {
                                    ui.set_max_width(340.0);
                                    ui.label(
                                        RichText::new(&n.title).strong().color(palette.text_strong),
                                    );
                                    if !n.detail.is_empty() {
                                        ui.label(RichText::new(&n.detail).size(12.0));
                                    }
                                });
                                if ui.small_button("×").on_hover_text("Dismiss").clicked() {
                                    self.dismiss(n.id);
                                }
                            });
                        });
                    ui.add_space(8.0);
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_stay_and_info_expires() {
        let notifier = Notifier::new();
        notifier.notify("Failed", "details");
        notifier.info("Saved", "");
        assert_eq!(notifier.len(), 2);
        let later = Instant::now() + INFO_LIFETIME + Duration::from_secs(1);
        let next = notifier.expire(later);
        assert_eq!(notifier.len(), 1);
        assert!(next.is_none());
    }

    #[test]
    fn dismiss_removes_only_that_notification() {
        let notifier = Notifier::new();
        notifier.notify("a", "");
        notifier.notify("b", "");
        let first = notifier.lock().items[0].id;
        notifier.dismiss(first);
        assert_eq!(notifier.len(), 1);
        assert_eq!(notifier.lock().items[0].title, "b");
    }
}
