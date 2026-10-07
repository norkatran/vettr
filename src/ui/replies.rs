//! Agent replies and resolvable comment threads (port of `Replies.tsx` and `useResolvedComments.ts`).

use std::collections::HashSet;

use egui::{Frame, Margin, RichText, Stroke};

use super::palette::Palette;
use crate::backend::Backend;
use crate::comments::ReplyKind;
use crate::replies::{reply_kind_label, AgentReply};
use crate::resolution::with_resolved;
use crate::task::Task;

/// The comments the user resolved in the open project, kept by the backend across restarts.
pub struct ResolvedModel {
    ctx: egui::Context,
    backend: Backend,
    project: Option<String>,
    ids: HashSet<String>,
    /// Bumped whenever the project changes, so late answers for an old project are dropped.
    generation: u64,
    loads: Vec<Task<(u64, Vec<String>)>>,
}

impl ResolvedModel {
    pub fn new(ctx: &egui::Context, backend: &Backend) -> ResolvedModel {
        ResolvedModel {
            ctx: ctx.clone(),
            backend: backend.clone(),
            project: None,
            ids: HashSet::new(),
            generation: 0,
            loads: Vec::new(),
        }
    }

    /// Call when the open project changes (also fine to call every frame with the same value).
    pub fn set_project(&mut self, project: Option<&str>) {
        if self.project.as_deref() == project {
            return;
        }
        self.project = project.map(|p| p.to_string());
        self.generation += 1;
        self.ids.clear();
        self.loads.clear();
        if let Some(path) = &self.project {
            let backend = self.backend.clone();
            let path = path.clone();
            let generation = self.generation;
            self.loads.push(Task::spawn(&self.ctx, move || {
                (generation, backend.resolved_comments(&path))
            }));
        }
    }

    /// Pick up finished loads and saves. Call once per frame.
    pub fn update(&mut self) {
        let mut finished: Vec<(u64, Vec<String>)> = Vec::new();
        for task in self.loads.iter_mut() {
            if let Some(result) = task.poll() {
                finished.push(result);
            }
        }
        self.loads.retain(|t| t.is_running());
        for (generation, ids) in finished {
            if generation == self.generation {
                self.ids = ids.into_iter().collect();
            }
        }
    }

    pub fn is_resolved(&self, id: &str) -> bool {
        self.ids.contains(id)
    }

    /// Resolve or reopen a comment; only the user does this, never the agent.
    pub fn set(&mut self, id: &str, resolved: bool) {
        let path = match &self.project {
            Some(path) => path.clone(),
            None => return,
        };
        let current: Vec<String> = self.ids.iter().cloned().collect();
        self.ids = with_resolved(&current, id, resolved).into_iter().collect();
        let backend = self.backend.clone();
        let id = id.to_string();
        let generation = self.generation;
        self.loads.push(Task::spawn(&self.ctx, move || {
            (
                generation,
                backend.set_comment_resolved(&path, &id, resolved),
            )
        }));
    }
}

/// The agent's answer to a comment. A `resolved` reply is only its belief: the reviewer decides.
pub fn reply_bubble(ui: &mut egui::Ui, palette: &Palette, reply: &AgentReply, place: Option<&str>) {
    let accent = match reply.kind {
        Some(ReplyKind::Question) => palette.amber,
        Some(ReplyKind::Resolved) => palette.green,
        None => palette.border,
    };
    Frame::new()
        .fill(palette.bg_elevated)
        .stroke(Stroke::new(1.0, accent))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            let mut head = match reply.kind {
                Some(kind) => format!("Agent {}", reply_kind_label(kind)),
                None => "Agent replied".to_string(),
            };
            if let Some(place) = place {
                if !place.is_empty() {
                    head.push_str(&format!(" on {}", place));
                }
            }
            ui.label(RichText::new(head).size(11.0).color(palette.text_muted));
            ui.label(&reply.message);
        });
}

/// A sent comment and its replies as one thread the user can resolve or reopen. Resolved threads
/// collapse to their summary line. `body` draws the thread's content.
pub fn resolvable_thread(
    ui: &mut egui::Ui,
    palette: &Palette,
    resolved: &mut ResolvedModel,
    id: &str,
    summary: &str,
    body: impl FnOnce(&mut egui::Ui),
) {
    if resolved.is_resolved(id) {
        let mut reopen = false;
        egui::CollapsingHeader::new(
            RichText::new(format!("Resolved: {}", summary)).color(palette.text_muted),
        )
        .id_salt(format!("resolved-thread-{}", id))
        .default_open(false)
        .show(ui, |ui| {
            body(ui);
            if ui.button("Reopen").clicked() {
                reopen = true;
            }
        });
        if reopen {
            resolved.set(id, false);
        }
    } else {
        let mut resolve = false;
        body(ui);
        if ui.button("Resolve").clicked() {
            resolve = true;
        }
        if resolve {
            resolved.set(id, true);
        }
    }
}
