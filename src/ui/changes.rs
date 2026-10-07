//! The Changes view (port of `Changes.tsx` and `Comments.tsx`, plus the commit box and file list
//! of `Sidebar.tsx`).
//!
//! All files are stacked PR-style in one scroll area. Long code lines wrap, and a row is a whole
//! number of text lines tall (computed from the line length, as the font is monospace), so only the
//! rows inside the viewport are laid out and painted (the others are skipped with one `add_space`);
//! syntax highlighting and split rows are cached per hunk in `ChangesModel` until the changes
//! reload. Rows that carry comments or the comment editor are always drawn.
//!
//! The view state lives in `changes_model.rs`; the mutations the drawing wants (stage, comment,
//! open in editor) are collected as `Action`s and applied after the frame's drawing.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat};
use egui::{
    pos2, vec2, Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Frame, Id, Key, Label,
    Layout, Margin, Modifiers, Rect, RichText, Sense, Stroke, TextEdit,
};

use super::changes_model::{
    can_commit, draft_ends_at, draft_selects, file_key, file_paths, first_line, pick_draft,
    row_visible, since_key, split_index_rows, status_label, status_letter, ChangesModel, DiffMode,
    DraftState, HunkCache, NewComment, ReviewModel, Scope, ViewState,
};
use super::notifications::Notifier;
use super::palette::{primary_button, Palette};
use super::replies::{reply_bubble, resolvable_thread, ResolvedModel};
use crate::backend::Backend;
use crate::comments::{ends_at, snapshot_lines, ReviewComment, Side};
use crate::diff::{ChangeStatus, DiffLine, FileChange, Hunk, LineKind};
use crate::highlight::{HighlightCache, HighlightedLine, HunkHighlight};
use crate::replies::AgentReply;

/// What the Changes view needs from the app besides its models.
pub struct ChangesEnv<'a> {
    pub project: Option<&'a str>,
    pub palette: &'a Palette,
    pub backend: &'a Backend,
    pub notifier: &'a Notifier,
    pub resolved: &'a mut ResolvedModel,
    /// The agent's replies to the comments, by comment id.
    pub replies: &'a HashMap<String, Vec<AgentReply>>,
    /// Why comments cannot be written or saved (the agent is not ready), or `None`.
    pub agent_block: Option<&'a str>,
    /// Why the unsent comments cannot be sent to the agent, or `None`.
    pub send_blocked: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ChangesOutput {
    /// The user pressed "Send N comments to agent".
    pub send_requested: bool,
}

/// What the drawing asks the models to do, applied once drawing is finished.
enum Action {
    Move { stage: bool, paths: Vec<String> },
    OpenInEditor { path: String, line: u32 },
    Add(NewComment),
    Edit { id: String, text: String },
    Remove(String),
}

/// Everything the drawing functions share during one frame.
struct Pass<'a> {
    palette: &'a Palette,
    replies: &'a HashMap<String, Vec<AgentReply>>,
    resolved: &'a mut ResolvedModel,
    comments: &'a [ReviewComment],
    draft: &'a mut DraftState,
    locked: Option<&'a str>,
    mono: FontId,
    row_h: f32,
    char_w: f32,
    actions: Vec<Action>,
    scroll_to: Option<String>,
}

/// The file being drawn.
struct FileCtx<'a> {
    file: &'a FileChange,
    staged: bool,
    base_id: Id,
    can_comment: bool,
}

const NUM_W: f32 = 48.0;

// ----- pure helpers (tested below) -----

/// The lines (as `(is_old_side, line number)`) under which a comment or the draft editor is drawn
/// in this file diff.
fn anchor_set(
    comments: &[ReviewComment],
    draft: &DraftState,
    file: &str,
    staged: bool,
) -> HashSet<(bool, u32)> {
    let mut set: HashSet<(bool, u32)> = HashSet::new();
    for c in comments {
        if !c.outdated && c.file == file && c.staged == staged {
            set.insert((c.side == Side::Old, c.end));
        }
    }
    if let Some(d) = &draft.target {
        if d.file == file && d.staged == staged {
            set.insert((d.side == Side::Old, d.end));
        }
    }
    set
}

fn has_block(anchors: &HashSet<(bool, u32)>, side: Side, no: Option<u32>) -> bool {
    match no {
        Some(n) => anchors.contains(&(side == Side::Old, n)),
        None => false,
    }
}

/// "line 3" or "lines 3-5".
fn range_label(c: &ReviewComment) -> String {
    if c.start == c.end {
        format!("line {}", c.start)
    } else {
        format!("lines {}-{}", c.start, c.end)
    }
}

fn side_label(side: Side) -> &'static str {
    match side {
        Side::Old => "Old",
        Side::New => "New",
    }
}

fn status_color(palette: &Palette, status: ChangeStatus) -> Color32 {
    match status {
        ChangeStatus::Added => palette.green,
        ChangeStatus::Deleted => palette.red,
        ChangeStatus::Modified => palette.amber,
        ChangeStatus::Renamed => Color32::from_rgb(0x58, 0xa6, 0xff),
    }
}

fn title_of(file: &FileChange) -> String {
    match &file.old_path {
        Some(old) if !old.is_empty() => format!("{} -> {}", old, file.path),
        _ => file.path.clone(),
    }
}

fn line_spans(hl: Option<&HunkHighlight>, index: Option<usize>) -> Option<&HighlightedLine> {
    let h = hl?;
    let i = index?;
    match h.get(i) {
        Some(Some(spans)) => Some(spans),
        _ => None,
    }
}

// ----- entry points -----

fn placeholder(ui: &mut egui::Ui, palette: &Palette, text: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(48.0);
        ui.label(RichText::new(text).color(palette.text_muted));
    });
}

/// Draw the whole Changes view.
pub fn show(
    ui: &mut egui::Ui,
    changes: &mut ChangesModel,
    review: &mut ReviewModel,
    env: &mut ChangesEnv<'_>,
) -> ChangesOutput {
    let mut out = ChangesOutput::default();
    let palette: &Palette = env.palette;
    if env.project.is_none() {
        placeholder(ui, palette, "Open a project to see its changes.");
        return out;
    }
    let data = match changes.changes_arc() {
        Some(data) => data,
        None => {
            let text = if changes.loading() {
                "Loading changes..."
            } else {
                "Could not read the changes for this project."
            };
            placeholder(ui, palette, text);
            return out;
        }
    };
    if data.staged.is_empty() && data.unstaged.is_empty() {
        placeholder(ui, palette, "No changes.");
        return out;
    }

    toolbar(ui, changes, review, env.send_blocked, palette, &mut out);
    commit_box(ui, changes, palette);
    ui.add_space(4.0);

    let showing_since = changes.view.scope == Scope::Since && review.baseline().is_some();
    let since = changes.since_arc();
    let scroll_to = changes.scroll_to.take();
    let mut view: ViewState = std::mem::take(&mut changes.view);
    let mono: FontId = egui::TextStyle::Monospace.resolve(ui.style());
    let row_h: f32 = ui.ctx().fonts_mut(|f| f.row_height(&mono)) + 3.0;
    let char_w: f32 = ui
        .painter()
        .layout_no_wrap("M".to_string(), mono.clone(), Color32::WHITE)
        .size()
        .x
        .max(1.0);

    let actions: Vec<Action>;
    {
        let ReviewModel {
            comments, draft, ..
        } = &mut *review;
        let mut pass = Pass {
            palette,
            replies: env.replies,
            resolved: &mut *env.resolved,
            comments: &comments[..],
            draft: &mut *draft,
            locked: env.agent_block,
            mono,
            row_h,
            char_w,
            actions: Vec::new(),
            scroll_to,
        };
        let pass_ref = &mut pass;
        let view_ref = &mut view;
        egui::ScrollArea::vertical()
            .id_salt("changes-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if showing_since {
                    draw_since(ui, pass_ref, view_ref, since.as_deref());
                } else {
                    draw_outdated(ui, pass_ref);
                    draw_group(ui, pass_ref, view_ref, true, &data.staged);
                    draw_group(ui, pass_ref, view_ref, false, &data.unstaged);
                }
                ui.add_space(24.0);
            });
        actions = pass.actions;
    }
    changes.view = view;

    for action in actions {
        match action {
            Action::Move { stage, paths } => {
                if stage {
                    changes.stage(paths);
                } else {
                    changes.unstage(paths);
                }
            }
            Action::OpenInEditor { path, line } => changes.open_in_editor(path, line),
            Action::Add(comment) => review.add(comment),
            Action::Edit { id, text } => review.edit(&id, &text),
            Action::Remove(id) => review.remove(&id),
        }
    }
    out
}

/// The Changes side panel: staged and unstaged files with status letter, path and counts. Clicking
/// a file scrolls the main view to it.
pub fn file_list(ui: &mut egui::Ui, changes: &mut ChangesModel, palette: &Palette) {
    let data = match changes.changes_arc() {
        Some(data) => data,
        None => {
            ui.label(RichText::new("No changes to review.").color(palette.text_muted));
            return;
        }
    };
    if data.staged.is_empty() && data.unstaged.is_empty() {
        ui.label(RichText::new("No changes to review.").color(palette.text_muted));
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("changes-file-list")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            let groups: [(bool, &Vec<FileChange>, &str); 2] = [
                (true, &data.staged, "Staged"),
                (false, &data.unstaged, "Unstaged"),
            ];
            for &(staged, files, title) in groups.iter() {
                if files.is_empty() {
                    continue;
                }
                ui.label(
                    RichText::new(format!("{} ({})", title, files.len()))
                        .strong()
                        .color(palette.text_muted),
                );
                for file in files.iter() {
                    ui.push_id(file_key(staged, &file.path), |ui| {
                        file_list_row(ui, changes, palette, file, staged);
                    });
                }
                ui.add_space(6.0);
            }
        });
}

fn file_list_row(
    ui: &mut egui::Ui,
    changes: &mut ChangesModel,
    palette: &Palette,
    file: &FileChange,
    staged: bool,
) {
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let (label, tip) = if staged {
                ("-", "Unstage")
            } else {
                ("+", "Stage")
            };
            if ui.small_button(label).on_hover_text(tip).clicked() {
                if staged {
                    changes.unstage(file_paths(file));
                } else {
                    changes.stage(file_paths(file));
                }
            }
            ui.label(
                RichText::new(format!("-{}", file.deletions))
                    .monospace()
                    .size(11.0)
                    .color(palette.red),
            );
            ui.label(
                RichText::new(format!("+{}", file.additions))
                    .monospace()
                    .size(11.0)
                    .color(palette.green),
            );
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.label(
                    RichText::new(status_letter(file.status))
                        .monospace()
                        .strong()
                        .color(status_color(palette, file.status)),
                );
                let response = ui
                    .add(Label::new(title_of(file)).truncate().sense(Sense::click()))
                    .on_hover_text(&file.path)
                    .on_hover_cursor(CursorIcon::PointingHand);
                if response.clicked() {
                    changes.reveal(staged, &file.path);
                }
            });
        });
    });
}

// ----- toolbar and commit box -----

fn toolbar(
    ui: &mut egui::Ui,
    changes: &mut ChangesModel,
    review: &ReviewModel,
    send_blocked: Option<&str>,
    palette: &Palette,
    out: &mut ChangesOutput,
) {
    let count = changes.changed_count();
    let pending = review.pending_count();
    let has_baseline = review.baseline().is_some();
    ui.horizontal_wrapped(|ui| {
        ui.label(format!(
            "{} changed {}",
            count,
            if count == 1 { "file" } else { "files" }
        ));
        ui.selectable_value(&mut changes.view.mode, DiffMode::Unified, "Unified");
        ui.selectable_value(&mut changes.view.mode, DiffMode::Split, "Split");
        if has_baseline {
            ui.selectable_value(&mut changes.view.scope, Scope::All, "All changes")
                .on_hover_text("Everything that differs from HEAD");
            ui.selectable_value(&mut changes.view.scope, Scope::Since, "Since last review")
                .on_hover_text("Only what changed after the last review was sent (read-only)");
        }
        let enabled = pending > 0 && send_blocked.is_none();
        let label = format!(
            "Send {} {} to agent",
            pending,
            if pending == 1 { "comment" } else { "comments" }
        );
        let button = ui.add_enabled(enabled, primary_button(palette, label));
        let clicked = button.clicked();
        if enabled {
            button.on_hover_text("Send the unsent comments to the agent");
        } else if let Some(reason) = send_blocked {
            button.on_disabled_hover_text(reason);
        }
        if clicked {
            out.send_requested = true;
        }
    });
}

fn commit_box(ui: &mut egui::Ui, changes: &mut ChangesModel, palette: &Palette) {
    let staged = changes.staged_count();
    let committing = changes.commit.committing;
    let id = Id::new("commit-message");
    let mut message: String = std::mem::take(&mut changes.commit.message);
    let mut submit = false;
    let mut focus = changes.commit.focus;
    ui.horizontal(|ui| {
        let ctx = ui.ctx().clone();
        let has_focus = ctx.memory(|m| m.has_focus(id));
        if has_focus && ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Enter)) {
            submit = true;
        }
        let width = (ui.available_width() - 120.0).max(120.0);
        let edit = TextEdit::multiline(&mut message)
            .id(id)
            .desired_width(width)
            .desired_rows(2)
            .hint_text("Commit message (Ctrl+Enter to commit)");
        let response = ui.add_enabled(!committing, edit);
        if focus {
            response.request_focus();
            focus = false;
        }
        let can = can_commit(true, staged, &message, committing);
        let label = if committing {
            "Committing..."
        } else {
            "Commit"
        };
        let button = ui.add_enabled(can, primary_button(palette, label));
        let clicked = button.clicked();
        if !can && !committing {
            let reason = if staged == 0 {
                "Stage files to commit them"
            } else {
                "Write a commit message"
            };
            button.on_disabled_hover_text(reason);
        }
        if clicked {
            submit = true;
        }
        if committing {
            ui.spinner();
        }
    });
    changes.commit.message = message;
    changes.commit.focus = focus;
    if submit {
        changes.start_commit();
    }
}

// ----- groups and files -----

fn arrow(ui: &mut egui::Ui, palette: &Palette, open: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::click());
    let c = rect.center();
    let points = if open {
        vec![
            pos2(c.x - 4.0, c.y - 2.0),
            pos2(c.x + 4.0, c.y - 2.0),
            pos2(c.x, c.y + 3.0),
        ]
    } else {
        vec![
            pos2(c.x - 2.0, c.y - 4.0),
            pos2(c.x + 3.0, c.y),
            pos2(c.x - 2.0, c.y + 4.0),
        ]
    };
    ui.painter().add(egui::Shape::convex_polygon(
        points,
        palette.text_muted,
        Stroke::NONE,
    ));
    response.on_hover_cursor(CursorIcon::PointingHand)
}

fn draw_group(
    ui: &mut egui::Ui,
    p: &mut Pass<'_>,
    view: &mut ViewState,
    staged: bool,
    files: &[FileChange],
) {
    let palette = p.palette;
    let title = if staged {
        "Staged changes"
    } else {
        "Unstaged changes"
    };
    let collapsed = if staged {
        view.staged_collapsed
    } else {
        view.unstaged_collapsed
    };
    let mut toggle = false;
    let mut move_all = false;
    ui.horizontal(|ui| {
        if arrow(ui, palette, !collapsed).clicked() {
            toggle = true;
        }
        let label = ui.add(
            Label::new(
                RichText::new(format!("{} ({})", title, files.len()))
                    .strong()
                    .color(palette.text_strong),
            )
            .sense(Sense::click()),
        );
        if label.clicked() {
            toggle = true;
        }
        if !files.is_empty() {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let text = if staged { "Unstage all" } else { "Stage all" };
                if ui.small_button(text).clicked() {
                    move_all = true;
                }
            });
        }
    });
    if toggle {
        if staged {
            view.staged_collapsed = !view.staged_collapsed;
        } else {
            view.unstaged_collapsed = !view.unstaged_collapsed;
        }
    }
    if move_all {
        let paths: Vec<String> = files.iter().flat_map(file_paths).collect();
        p.actions.push(Action::Move {
            stage: !staged,
            paths,
        });
    }
    if collapsed {
        return;
    }
    if files.is_empty() {
        ui.label(RichText::new("Nothing here.").color(palette.text_muted));
        ui.add_space(8.0);
        return;
    }
    for file in files.iter() {
        let key = file_key(staged, &file.path);
        draw_file(ui, p, view, file, staged, key, false);
    }
}

/// What changed since the last review was sent: read-only, with no staging or comments.
fn draw_since(
    ui: &mut egui::Ui,
    p: &mut Pass<'_>,
    view: &mut ViewState,
    files: Option<&Vec<FileChange>>,
) {
    let palette = p.palette;
    match files {
        None => {
            ui.label(
                RichText::new("Loading changes since the last review...").color(palette.text_muted),
            );
        }
        Some(list) if list.is_empty() => {
            ui.label(
                RichText::new("Nothing has changed since the last review.")
                    .color(palette.text_muted),
            );
        }
        Some(list) => {
            for file in list.iter() {
                let key = since_key(file);
                draw_file(ui, p, view, file, false, key, true);
            }
        }
    }
}

fn draw_file(
    ui: &mut egui::Ui,
    p: &mut Pass<'_>,
    view: &mut ViewState,
    file: &FileChange,
    staged: bool,
    key: String,
    read_only: bool,
) {
    let palette = p.palette;
    let collapsed = view.collapsed_files.contains(&key);
    let mut toggle = false;
    ui.push_id(("file", key.as_str()), |ui| {
        Frame::new()
            .stroke(Stroke::new(1.0, palette.border))
            .corner_radius(CornerRadius::same(4))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                let width = ui.available_width();
                ui.set_min_width(width);
                let header = Frame::new()
                    .fill(palette.bg_elevated)
                    .inner_margin(Margin::symmetric(12, 6))
                    .show(ui, |ui| {
                        let inner_width = ui.available_width();
                        ui.set_min_width(inner_width);
                        ui.spacing_mut().item_spacing = vec2(8.0, 4.0);
                        ui.horizontal(|ui| {
                            file_header(ui, p, file, staged, read_only, collapsed, &mut toggle);
                        });
                    });
                if p.scroll_to.as_deref() == Some(key.as_str()) {
                    ui.scroll_to_rect(header.response.rect, Some(Align::TOP));
                    p.scroll_to = None;
                }
                if !collapsed {
                    draw_body(ui, p, view, file, staged, &key, read_only);
                }
            });
    });
    if toggle {
        if collapsed {
            view.collapsed_files.remove(&key);
        } else {
            view.collapsed_files.insert(key.clone());
        }
    }
    ui.add_space(12.0);
}

fn file_header(
    ui: &mut egui::Ui,
    p: &mut Pass<'_>,
    file: &FileChange,
    staged: bool,
    read_only: bool,
    collapsed: bool,
    toggle: &mut bool,
) {
    let palette = p.palette;
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if !read_only {
            ui.menu_button("...", |ui| {
                let can = file.status != ChangeStatus::Deleted;
                let item = ui.add_enabled(can, egui::Button::new("Open in editor"));
                if item.clicked() {
                    p.actions.push(Action::OpenInEditor {
                        path: file.path.clone(),
                        line: first_line(file),
                    });
                    ui.close();
                }
            });
            let label = if staged { "Unstage" } else { "Stage" };
            if ui.small_button(label).clicked() {
                p.actions.push(Action::Move {
                    stage: !staged,
                    paths: file_paths(file),
                });
            }
        }
        ui.label(
            RichText::new(format!("-{}", file.deletions))
                .monospace()
                .color(palette.red),
        );
        ui.label(
            RichText::new(format!("+{}", file.additions))
                .monospace()
                .color(palette.green),
        );
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            if arrow(ui, palette, !collapsed).clicked() {
                *toggle = true;
            }
            ui.label(
                RichText::new(status_label(file.status))
                    .size(11.0)
                    .color(status_color(palette, file.status)),
            );
            let title = RichText::new(title_of(file))
                .monospace()
                .color(palette.text_strong);
            let response = ui.add(Label::new(title).truncate().sense(Sense::click()));
            if response.clicked() {
                *toggle = true;
            }
        });
    });
}

fn note(ui: &mut egui::Ui, palette: &Palette, text: &str) {
    Frame::new().inner_margin(Margin::same(12)).show(ui, |ui| {
        ui.label(RichText::new(text).color(palette.text_muted));
    });
}

fn draw_body(
    ui: &mut egui::Ui,
    p: &mut Pass<'_>,
    view: &mut ViewState,
    file: &FileChange,
    staged: bool,
    key: &str,
    read_only: bool,
) {
    let palette = p.palette;
    if file.binary {
        note(ui, palette, "Binary file not shown.");
        return;
    }
    if file.too_large {
        let text = format!(
            "Diff too large to display ({} changed lines).",
            file.additions + file.deletions
        );
        note(ui, palette, &text);
        return;
    }
    if file.hunks.is_empty() {
        note(ui, palette, "No content changes.");
        return;
    }
    let anchors: HashSet<(bool, u32)> = if read_only {
        HashSet::new()
    } else {
        anchor_set(p.comments, &*p.draft, &file.path, staged)
    };
    let fc = FileCtx {
        file,
        staged,
        base_id: Id::new(("diff-file", key)),
        can_comment: !read_only,
    };
    let mode = view.mode;
    let count = file.hunks.len();
    let caches: &mut Vec<HunkCache> = view.hunks.entry(key.to_string()).or_default();
    if caches.len() < count {
        caches.resize_with(count, HunkCache::default);
    }
    let highlighter: &mut HighlightCache = &mut view.highlighter;
    for (hi, hunk) in file.hunks.iter().enumerate() {
        draw_hunk(
            ui,
            p,
            highlighter,
            &mut caches[hi],
            &fc,
            hunk,
            hi,
            mode,
            &anchors,
        );
    }
}

// ----- diff rows -----

/// Reserve the next fixed-height row. Rows outside the viewport are only counted (returns `None`)
/// and are laid out later by a single `add_space`; `force` draws the row regardless.
fn next_row(ui: &mut egui::Ui, skipped: &mut f32, row_h: f32, force: bool) -> Option<Rect> {
    let y = ui.cursor().top() + *skipped;
    let clip = ui.clip_rect();
    if !force && !row_visible(y, row_h, clip.top(), clip.bottom()) {
        *skipped += row_h;
        return None;
    }
    if *skipped > 0.0 {
        ui.add_space(*skipped);
        *skipped = 0.0;
    }
    let width = ui.available_width();
    let (rect, _response) = ui.allocate_exact_size(vec2(width, row_h), Sense::hover());
    Some(rect)
}

fn flush_skipped(ui: &mut egui::Ui, skipped: &mut f32) {
    if *skipped > 0.0 {
        ui.add_space(*skipped);
        *skipped = 0.0;
    }
}

fn hunk_highlight(
    cache: &mut HunkCache,
    highlighter: &mut HighlightCache,
    path: &str,
    lines: &[DiffLine],
) -> Arc<HunkHighlight> {
    if let Some(found) = &cache.highlight {
        return found.clone();
    }
    let made = highlighter.get(path, lines);
    cache.highlight = Some(made.clone());
    made
}

fn hunk_split_rows(
    cache: &mut HunkCache,
    lines: &[DiffLine],
) -> Arc<Vec<(Option<usize>, Option<usize>)>> {
    if let Some(found) = &cache.split {
        return found.clone();
    }
    let made = Arc::new(split_index_rows(lines));
    cache.split = Some(made.clone());
    made
}

#[allow(clippy::too_many_arguments)]
fn draw_hunk(
    ui: &mut egui::Ui,
    p: &mut Pass<'_>,
    highlighter: &mut HighlightCache,
    cache: &mut HunkCache,
    fc: &FileCtx<'_>,
    hunk: &Hunk,
    hi: usize,
    mode: DiffMode,
    anchors: &HashSet<(bool, u32)>,
) {
    let palette = p.palette;
    let row_h = p.row_h;
    let mut skipped = 0.0f32;
    if let Some(rect) = next_row(ui, &mut skipped, row_h, false) {
        ui.painter().rect_filled(rect, 0.0, palette.bg_elevated);
        ui.painter_at(rect).text(
            pos2(rect.left() + 8.0, rect.center().y),
            Align2::LEFT_CENTER,
            &hunk.header,
            p.mono.clone(),
            palette.text_muted,
        );
    }
    let mut hl: Option<Arc<HunkHighlight>> = None;
    match mode {
        DiffMode::Unified => {
            for (li, line) in hunk.lines.iter().enumerate() {
                let code_w = ui.available_width() - 2.0 * NUM_W - 22.0;
                let blocks = has_block(anchors, Side::Old, line.old_no)
                    || has_block(anchors, Side::New, line.new_no);
                let h = wrapped_rows(&line.text, code_w, p.char_w) as f32 * row_h;
                let rect = match next_row(ui, &mut skipped, h, blocks) {
                    Some(rect) => rect,
                    None => continue,
                };
                if hl.is_none() {
                    hl = Some(hunk_highlight(
                        cache,
                        highlighter,
                        &fc.file.path,
                        &hunk.lines,
                    ));
                }
                let spans = line_spans(hl.as_deref(), Some(li));
                let salt: u64 = ((hi as u64) << 32) | (li as u64);
                paint_unified_row(ui, p, fc, rect, line, spans, salt);
                if blocks {
                    draw_blocks(ui, p, fc, Side::Old, line.old_no);
                    draw_blocks(ui, p, fc, Side::New, line.new_no);
                }
            }
        }
        DiffMode::Split => {
            let rows = hunk_split_rows(cache, &hunk.lines);
            let half_code_w = ui.available_width() / 2.0 - NUM_W - 8.0;
            for (ri, pair) in rows.iter().enumerate() {
                let left: Option<&DiffLine> = pair.0.and_then(|i| hunk.lines.get(i));
                let right: Option<&DiffLine> = pair.1.and_then(|i| hunk.lines.get(i));
                let left_no: Option<u32> = left.and_then(|l| l.old_no);
                let right_no: Option<u32> = right.and_then(|l| l.new_no);
                let blocks = has_block(anchors, Side::Old, left_no)
                    || has_block(anchors, Side::New, right_no);
                let n_rows = wrapped_rows(left.map_or("", |l| l.text.as_str()), half_code_w, p.char_w)
                    .max(wrapped_rows(right.map_or("", |l| l.text.as_str()), half_code_w, p.char_w));
                let rect = match next_row(ui, &mut skipped, n_rows as f32 * row_h, blocks) {
                    Some(rect) => rect,
                    None => continue,
                };
                if hl.is_none() {
                    hl = Some(hunk_highlight(
                        cache,
                        highlighter,
                        &fc.file.path,
                        &hunk.lines,
                    ));
                }
                let salt: u64 = ((hi as u64) << 32) | (ri as u64);
                let mid = rect.center().x;
                let left_rect = Rect::from_min_max(rect.min, pos2(mid, rect.max.y));
                let right_rect = Rect::from_min_max(pos2(mid, rect.min.y), rect.max);
                let left_spans = line_spans(hl.as_deref(), pair.0);
                let right_spans = line_spans(hl.as_deref(), pair.1);
                paint_split_half(ui, p, fc, left_rect, left, Side::Old, left_spans, salt);
                paint_split_half(ui, p, fc, right_rect, right, Side::New, right_spans, salt);
                ui.painter().line_segment(
                    [pos2(mid, rect.min.y), pos2(mid, rect.max.y)],
                    Stroke::new(1.0, palette.border),
                );
                if blocks {
                    draw_blocks(ui, p, fc, Side::Old, left_no);
                    draw_blocks(ui, p, fc, Side::New, right_no);
                }
            }
        }
    }
    flush_skipped(ui, &mut skipped);
}

/// How many visual rows a code line takes when wrapped (anywhere) in `width` pixels of
/// monospace text; tabs are drawn as four spaces. Always at least one.
fn wrapped_rows(text: &str, width: f32, char_w: f32) -> usize {
    let cols = ((width / char_w) + 1e-3).floor().max(1.0) as usize;
    let chars: usize = text.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum();
    chars.div_ceil(cols).max(1)
}

fn paint_code(
    ui: &egui::Ui,
    p: &Pass<'_>,
    rect: Rect,
    line: &DiffLine,
    spans: Option<&HighlightedLine>,
) {
    if line.text.is_empty() {
        return;
    }
    let mut job = LayoutJob::default();
    match spans {
        Some(list) if !list.is_empty() => {
            for span in list.iter() {
                let text = span.text.replace('\t', "    ");
                let format = TextFormat::simple(p.mono.clone(), p.palette.token_color(span.kind));
                job.append(&text, 0.0, format);
            }
        }
        _ => {
            let text = line.text.replace('\t', "    ");
            let format = TextFormat::simple(p.mono.clone(), p.palette.text);
            job.append(&text, 0.0, format);
        }
    }
    job.wrap.max_width = rect.width();
    job.wrap.break_anywhere = true;
    let galley = ui.painter().layout_job(job);
    let pos = pos2(rect.left(), rect.top() + 1.5);
    ui.painter_at(rect).galley(pos, galley, p.palette.text);
}

/// A line number; clicking it comments on the line, shift-click extends the range.
fn number_cell(
    ui: &egui::Ui,
    p: &mut Pass<'_>,
    fc: &FileCtx<'_>,
    cell: Rect,
    side: Side,
    no: Option<u32>,
    salt: u64,
) {
    let no = match no {
        Some(n) => n,
        None => return,
    };
    ui.painter().text(
        pos2(cell.right() - 6.0, cell.center().y),
        Align2::RIGHT_CENTER,
        no.to_string(),
        p.mono.clone(),
        p.palette.text_muted,
    );
    if !fc.can_comment {
        return;
    }
    let id = fc.base_id.with((salt, side == Side::Old));
    if let Some(reason) = p.locked {
        ui.interact(cell, id, Sense::hover()).on_hover_text(reason);
        return;
    }
    let response = ui
        .interact(cell, id, Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text("Click to comment (shift-click for a range)");
    if response.clicked() {
        let shift = ui.ctx().input(|i| i.modifiers.shift);
        let next = pick_draft(
            p.draft.target.as_ref(),
            &fc.file.path,
            fc.staged,
            side,
            no,
            shift,
        );
        p.draft.target = Some(next);
        p.draft.focus = true;
    }
}

fn paint_unified_row(
    ui: &egui::Ui,
    p: &mut Pass<'_>,
    fc: &FileCtx<'_>,
    rect: Rect,
    line: &DiffLine,
    spans: Option<&HighlightedLine>,
    salt: u64,
) {
    let palette = p.palette;
    let painter = ui.painter();
    match line.kind {
        LineKind::Add => {
            painter.rect_filled(rect, 0.0, palette.add_bg);
        }
        LineKind::Del => {
            painter.rect_filled(rect, 0.0, palette.del_bg);
        }
        LineKind::Context => {}
    }
    let target = p.draft.target.as_ref();
    let old_selected = match line.old_no {
        Some(n) => draft_selects(target, &fc.file.path, fc.staged, Side::Old, n),
        None => false,
    };
    let new_selected = match line.new_no {
        Some(n) => draft_selects(target, &fc.file.path, fc.staged, Side::New, n),
        None => false,
    };
    if old_selected || new_selected {
        painter.rect_filled(rect, 0.0, palette.select_bg);
    }
    let old_cell = Rect::from_min_size(rect.min, vec2(NUM_W, p.row_h));
    let new_cell = Rect::from_min_size(
        pos2(rect.left() + NUM_W, rect.top()),
        vec2(NUM_W, p.row_h),
    );
    number_cell(ui, p, fc, old_cell, Side::Old, line.old_no, salt);
    number_cell(ui, p, fc, new_cell, Side::New, line.new_no, salt);
    let sign = match line.kind {
        LineKind::Add => "+",
        LineKind::Del => "-",
        LineKind::Context => " ",
    };
    let code_left = rect.left() + 2.0 * NUM_W + 6.0;
    painter.text(
        pos2(code_left + 2.0, rect.top() + p.row_h / 2.0),
        Align2::LEFT_CENTER,
        sign,
        p.mono.clone(),
        palette.text_muted,
    );
    let code_rect = Rect::from_min_max(pos2(code_left + 16.0, rect.top()), rect.max);
    paint_code(ui, p, code_rect, line, spans);
}

#[allow(clippy::too_many_arguments)]
fn paint_split_half(
    ui: &egui::Ui,
    p: &mut Pass<'_>,
    fc: &FileCtx<'_>,
    rect: Rect,
    line: Option<&DiffLine>,
    side: Side,
    spans: Option<&HighlightedLine>,
    salt: u64,
) {
    let palette = p.palette;
    let line = match line {
        Some(line) => line,
        None => {
            ui.painter().rect_filled(rect, 0.0, palette.bg_elevated);
            return;
        }
    };
    let painter = ui.painter();
    match line.kind {
        LineKind::Add => {
            painter.rect_filled(rect, 0.0, palette.add_bg);
        }
        LineKind::Del => {
            painter.rect_filled(rect, 0.0, palette.del_bg);
        }
        LineKind::Context => {}
    }
    let no = match side {
        Side::Old => line.old_no,
        Side::New => line.new_no,
    };
    let selected = match no {
        Some(n) => draft_selects(p.draft.target.as_ref(), &fc.file.path, fc.staged, side, n),
        None => false,
    };
    if selected {
        painter.rect_filled(rect, 0.0, palette.select_bg);
    }
    let cell = Rect::from_min_size(rect.min, vec2(NUM_W, p.row_h));
    number_cell(ui, p, fc, cell, side, no, salt);
    let code_rect = Rect::from_min_max(pos2(rect.left() + NUM_W + 8.0, rect.top()), rect.max);
    paint_code(ui, p, code_rect, line, spans);
}

// ----- comments -----

fn block_frame(ui: &mut egui::Ui, palette: &Palette, add: impl FnOnce(&mut egui::Ui)) {
    Frame::new()
        .fill(palette.bg)
        .inner_margin(Margin {
            left: 60,
            right: 12,
            top: 6,
            bottom: 6,
        })
        .show(ui, |ui| {
            let width = ui.available_width();
            ui.set_min_width(width);
            ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
            add(ui);
        });
}

fn card_frame(ui: &mut egui::Ui, palette: &Palette, add: impl FnOnce(&mut egui::Ui)) {
    Frame::new()
        .fill(palette.bg_elevated)
        .stroke(Stroke::new(1.0, palette.border))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            let width = ui.available_width();
            ui.set_min_width(width);
            add(ui);
        });
}

/// The editor for the current draft and the comments that end at one line, drawn under it.
fn draw_blocks(ui: &mut egui::Ui, p: &mut Pass<'_>, fc: &FileCtx<'_>, side: Side, no: Option<u32>) {
    let no = match no {
        Some(n) => n,
        None => return,
    };
    let comments: &[ReviewComment] = p.comments;
    let here: Vec<&ReviewComment> = comments
        .iter()
        .filter(|c| ends_at(c, &fc.file.path, fc.staged, side, no))
        .collect();
    let draft_here = draft_ends_at(p.draft.target.as_ref(), &fc.file.path, fc.staged, side, no);
    if here.is_empty() && !draft_here {
        return;
    }
    let palette = p.palette;
    let salt = fc.base_id.with(("block", side == Side::Old, no));
    ui.push_id(salt, |ui| {
        block_frame(ui, palette, |ui| {
            for c in here.iter() {
                comment_card(ui, p, c);
            }
            if draft_here {
                new_comment_editor(ui, p, fc);
            }
        });
    });
}

enum EditorEvent {
    Nothing,
    Save(String),
    Cancel,
}

/// A text box with Save and Cancel (Ctrl+Enter saves, Esc cancels).
fn comment_editor(
    ui: &mut egui::Ui,
    palette: &Palette,
    id: Id,
    text: &mut String,
    focus: &mut bool,
    locked: Option<&str>,
) -> EditorEvent {
    let mut event = EditorEvent::Nothing;
    Frame::new()
        .fill(palette.bg_input)
        .stroke(Stroke::new(1.0, palette.border))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::same(8))
        .show(ui, |ui| {
            let ctx = ui.ctx().clone();
            let has_focus = ctx.memory(|m| m.has_focus(id));
            let mut submit = false;
            if has_focus {
                if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Enter)) {
                    submit = true;
                } else if ctx.input(|i| i.key_pressed(Key::Escape)) {
                    event = EditorEvent::Cancel;
                }
            }
            let width = ui.available_width();
            let response = ui.add(
                TextEdit::multiline(&mut *text)
                    .id(id)
                    .desired_width(width)
                    .desired_rows(3)
                    .hint_text("Leave a comment (Ctrl+Enter to save, Esc to cancel)"),
            );
            if *focus {
                response.request_focus();
                *focus = false;
            }
            let can_save = !text.trim().is_empty() && locked.is_none();
            if submit && can_save {
                event = EditorEvent::Save(text.trim().to_string());
            }
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    event = EditorEvent::Cancel;
                }
                let save = ui.add_enabled(can_save, egui::Button::new("Save"));
                let clicked = save.clicked();
                if !can_save {
                    if let Some(reason) = locked {
                        save.on_disabled_hover_text(reason);
                    }
                }
                if clicked {
                    event = EditorEvent::Save(text.trim().to_string());
                }
            });
        });
    event
}

fn new_comment_editor(ui: &mut egui::Ui, p: &mut Pass<'_>, fc: &FileCtx<'_>) {
    let palette = p.palette;
    let locked = p.locked;
    let event = comment_editor(
        ui,
        palette,
        Id::new("new-comment-editor"),
        &mut p.draft.text,
        &mut p.draft.focus,
        locked,
    );
    match event {
        EditorEvent::Save(text) => {
            if let Some(d) = p.draft.target.clone() {
                let snapshot = snapshot_lines(fc.file, d.side, d.start, d.end);
                p.actions.push(Action::Add(NewComment {
                    file: d.file,
                    staged: d.staged,
                    side: d.side,
                    start: d.start,
                    end: d.end,
                    snapshot,
                    text,
                }));
            }
            p.draft.target = None;
            p.draft.text.clear();
        }
        EditorEvent::Cancel => {
            p.draft.target = None;
            p.draft.text.clear();
        }
        EditorEvent::Nothing => {}
    }
}

/// A comment: a card with Edit and Delete while pending, a resolvable thread once sent.
fn comment_card(ui: &mut egui::Ui, p: &mut Pass<'_>, c: &ReviewComment) {
    let palette = p.palette;
    if !c.sent && p.draft.editing.as_deref() == Some(c.id.as_str()) {
        let locked = p.locked;
        let id = Id::new(("edit-comment", c.id.as_str()));
        let event = comment_editor(
            ui,
            palette,
            id,
            &mut p.draft.edit_text,
            &mut p.draft.edit_focus,
            locked,
        );
        match event {
            EditorEvent::Save(text) => {
                p.actions.push(Action::Edit {
                    id: c.id.clone(),
                    text,
                });
                p.draft.editing = None;
            }
            EditorEvent::Cancel => {
                p.draft.editing = None;
            }
            EditorEvent::Nothing => {}
        }
        return;
    }
    let range = range_label(c);
    let meta = if c.sent {
        format!(
            "{} {} - sent in round {}",
            side_label(c.side),
            range,
            c.round
        )
    } else {
        format!("{} {}", side_label(c.side), range)
    };
    if c.sent {
        let summary = format!("{}, {}", c.file, range);
        let replies = p.replies;
        resolvable_thread(ui, palette, &mut *p.resolved, &c.id, &summary, |ui| {
            card_frame(ui, palette, |ui| {
                ui.label(RichText::new(meta).size(11.0).color(palette.text_muted));
                ui.label(&c.text);
                if let Some(list) = replies.get(&c.id) {
                    for reply in list.iter() {
                        reply_bubble(ui, palette, reply, None);
                    }
                }
            });
        });
        return;
    }
    let locked = p.locked;
    card_frame(ui, palette, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(meta).size(11.0).color(palette.text_muted));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let enabled = locked.is_none();
                let delete = ui.add_enabled(enabled, egui::Button::new("Delete").small());
                let delete_clicked = delete.clicked();
                if let Some(reason) = locked {
                    delete.on_disabled_hover_text(reason);
                }
                let edit = ui.add_enabled(enabled, egui::Button::new("Edit").small());
                let edit_clicked = edit.clicked();
                if let Some(reason) = locked {
                    edit.on_disabled_hover_text(reason);
                }
                if delete_clicked {
                    p.actions.push(Action::Remove(c.id.clone()));
                }
                if edit_clicked {
                    p.draft.editing = Some(c.id.clone());
                    p.draft.edit_text = c.text.clone();
                    p.draft.edit_focus = true;
                }
            });
        });
        ui.label(&c.text);
        if let Some(list) = p.replies.get(&c.id) {
            for reply in list.iter() {
                reply_bubble(ui, palette, reply, None);
            }
        }
    });
}

/// Comments whose code is gone from the diff, collapsed by default.
fn draw_outdated(ui: &mut egui::Ui, p: &mut Pass<'_>) {
    let comments: &[ReviewComment] = p.comments;
    let outdated: Vec<&ReviewComment> = comments.iter().filter(|c| c.outdated).collect();
    if outdated.is_empty() {
        return;
    }
    let palette = p.palette;
    let count = outdated.len();
    let title = format!(
        "{} outdated {}",
        count,
        if count == 1 { "comment" } else { "comments" }
    );
    egui::CollapsingHeader::new(RichText::new(title).color(palette.amber))
        .id_salt("outdated-comments")
        .default_open(false)
        .show(ui, |ui| {
            for c in outdated.iter() {
                ui.push_id(("outdated", c.id.as_str()), |ui| {
                    ui.label(
                        RichText::new(c.file.as_str())
                            .size(11.0)
                            .color(palette.text_muted),
                    );
                    if !c.snapshot.is_empty() {
                        ui.label(RichText::new(c.snapshot.join("\n")).monospace());
                    }
                    comment_card(ui, p, c);
                });
                ui.add_space(6.0);
            }
        });
    ui.add_space(8.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comments::Side;
    use crate::diff::LineKind;

    fn comment(file: &str, staged: bool, side: Side, start: u32, end: u32) -> ReviewComment {
        ReviewComment {
            id: format!("{}-{}-{}", file, start, end),
            file: file.to_string(),
            staged,
            side,
            start,
            end,
            snapshot: vec![],
            text: "text".to_string(),
            round: 1,
            sent: false,
            outdated: false,
        }
    }

    #[test]
    fn anchors_are_where_comments_end() {
        let comments = vec![
            comment("a.rs", false, Side::New, 3, 5),
            comment("a.rs", true, Side::New, 9, 9),
            comment("b.rs", false, Side::Old, 1, 1),
        ];
        let set = anchor_set(&comments, &DraftState::default(), "a.rs", false);
        assert_eq!(set.len(), 1);
        assert!(set.contains(&(false, 5)));
        assert!(!set.contains(&(false, 3)));
    }

    #[test]
    fn outdated_comments_have_no_anchor_and_the_draft_has_one() {
        let mut gone = comment("a.rs", false, Side::Old, 2, 2);
        gone.outdated = true;
        let mut draft = DraftState::default();
        let set = anchor_set(&[gone.clone()], &draft, "a.rs", false);
        assert!(set.is_empty());
        draft.target = Some(pick_draft(None, "a.rs", false, Side::Old, 7, false));
        let set = anchor_set(&[gone], &draft, "a.rs", false);
        assert!(set.contains(&(true, 7)));
        assert!(has_block(&set, Side::Old, Some(7)));
        assert!(!has_block(&set, Side::New, Some(7)));
        assert!(!has_block(&set, Side::Old, None));
    }

    #[test]
    fn long_lines_wrap_into_more_rows() {
        assert_eq!(wrapped_rows("", 100.0, 10.0), 1);
        assert_eq!(wrapped_rows("0123456789", 100.0, 10.0), 1);
        assert_eq!(wrapped_rows("01234567890", 100.0, 10.0), 2);
        assert_eq!(wrapped_rows("\t\t", 40.0, 10.0), 2);
    }

    #[test]
    fn range_labels() {
        assert_eq!(range_label(&comment("a", false, Side::New, 4, 4)), "line 4");
        assert_eq!(
            range_label(&comment("a", false, Side::New, 4, 6)),
            "lines 4-6"
        );
    }

    #[test]
    fn spans_are_looked_up_by_line_index() {
        let hl: HunkHighlight = vec![None, Some(vec![])];
        assert!(line_spans(Some(&hl), Some(0)).is_none());
        assert!(line_spans(Some(&hl), Some(1)).is_some());
        assert!(line_spans(Some(&hl), Some(5)).is_none());
        assert!(line_spans(Some(&hl), None).is_none());
        assert!(line_spans(None, Some(0)).is_none());
    }

    #[test]
    fn rename_titles_show_both_paths() {
        let mut file = FileChange {
            path: "b.rs".to_string(),
            old_path: None,
            status: ChangeStatus::Renamed,
            binary: false,
            too_large: false,
            additions: 0,
            deletions: 0,
            hunks: vec![],
        };
        assert_eq!(title_of(&file), "b.rs");
        file.old_path = Some("a.rs".to_string());
        assert_eq!(title_of(&file), "a.rs -> b.rs");
        let _ = LineKind::Add;
    }
}
