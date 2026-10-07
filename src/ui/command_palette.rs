//! The command palette (port of `CommandPalette.tsx`): a modal text box with a fuzzy-filtered
//! list. The promise-based prompts of the TypeScript become a small state machine: the palette is
//! in one `Step` (command list, a pick, or a text input), and each answer produces an `Effect`
//! (see `commands.rs`) that either changes the step, runs a git job in the background or asks the
//! App to do something (`AppRequest`).
//!
//! Call `show` every frame (also while closed): it collects the results of background jobs.

use egui::{Align2, Key, Margin, Modifiers, RichText};

use crate::backend::Backend;
use crate::fuzzy::fuzzy_filter;
use crate::git_actions::Branch;
use crate::sessions::SessionInfo;
use crate::task::Task;

use super::commands::{
    branch_effect, branch_items, command_items, discard_effect, discard_items, find_command,
    new_branch_effect, pick_branch, remote_effect, remote_items, start_command, BranchOp, Effect,
    GitJob, PaletteItem, DISCARD_PROMPT,
};
use super::notifications::Notifier;
use super::palette::Palette;
use super::sidebar::View;

/// What the palette asks the App to do (the TS `CommandContext` callbacks).
#[derive(Debug, Clone, PartialEq)]
pub enum AppRequest {
    /// Switch to a view, expanding the side panel.
    ShowView(View),
    /// Start a fresh agent session and show the Session view.
    NewSession,
    /// Replace the current session with a stored one and show the Session view.
    OpenSession(String),
    /// Show the Changes view and focus the commit message input.
    FocusCommit,
}

pub struct PaletteEnv<'a> {
    pub project: Option<&'a str>,
    pub palette: &'a Palette,
    pub backend: &'a Backend,
    pub notifier: &'a Notifier,
    /// Number of staged files, to check before asking for a commit message.
    pub staged_count: usize,
    pub sessions: &'a [SessionInfo],
}

#[derive(Debug, Clone)]
enum PickAction {
    Command,
    Branch(BranchOp, Vec<Branch>),
    Remote,
    Discard,
    Session,
}

enum Step {
    Commands,
    Pick {
        placeholder: String,
        items: Vec<PaletteItem>,
        wide: bool,
        action: PickAction,
    },
    Input {
        placeholder: String,
    },
}

enum Job {
    Branches(BranchOp, Task<Vec<Branch>>),
    Remotes(Task<Vec<String>>),
    Git {
        title: String,
        task: Task<Result<(), String>>,
    },
}

enum Done {
    Branches(BranchOp, Vec<Branch>),
    Remotes(Vec<String>),
    Git(String, Result<(), String>),
}

/// Take the result of a task if it has one; the flag says whether to keep waiting.
fn take_result<T: Send + 'static>(task: &mut Task<T>) -> (Option<T>, bool) {
    let mut result = task.poll();
    if result.is_none() && !task.is_running() {
        // Finished between the two calls
        result = task.poll();
    }
    let keep = result.is_none() && task.is_running();
    (result, keep)
}

/// Run a git job (blocking; call on a worker thread).
fn run_git_job(backend: &Backend, project: &str, job: &GitJob) -> Result<(), String> {
    match job {
        GitJob::Action(action) => backend.run_git_action(project, action),
        GitJob::Push => backend.push(project),
        GitJob::Publish(remote) => backend.publish(project, remote),
    }
}

pub struct CommandPalette {
    open: bool,
    step: Step,
    query: String,
    index: usize,
    want_focus: bool,
    refresh: bool,
    jobs: Vec<Job>,
}

impl Default for CommandPalette {
    fn default() -> CommandPalette {
        CommandPalette::new()
    }
}

impl CommandPalette {
    pub fn new() -> CommandPalette {
        CommandPalette {
            open: false,
            step: Step::Commands,
            query: String::new(),
            index: 0,
            want_focus: false,
            refresh: false,
            jobs: Vec::new(),
        }
    }

    /// Show the command list (Ctrl+Shift+P).
    pub fn open(&mut self) {
        self.open = true;
        self.step = Step::Commands;
        self.reset_query();
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Whether a git command finished since the last call (so the status bar should reload).
    pub fn refresh_requested(&mut self) -> bool {
        let requested = self.refresh;
        self.refresh = false;
        requested
    }

    fn close(&mut self) {
        self.open = false;
        self.step = Step::Commands;
        self.reset_query();
    }

    fn reset_query(&mut self) {
        self.query.clear();
        self.index = 0;
        self.want_focus = true;
    }

    fn items(&self) -> Vec<PaletteItem> {
        let label = |i: &PaletteItem| -> String { i.label.clone() };
        match &self.step {
            Step::Commands => fuzzy_filter(&command_items(), &self.query, label),
            Step::Pick { items, .. } => fuzzy_filter(items, &self.query, label),
            Step::Input { .. } => Vec::new(),
        }
    }

    fn open_pick(
        &mut self,
        placeholder: &str,
        items: Vec<PaletteItem>,
        wide: bool,
        action: PickAction,
    ) {
        self.open = true;
        self.step = Step::Pick {
            placeholder: placeholder.to_string(),
            items,
            wide,
            action,
        };
        self.reset_query();
    }

    fn apply(
        &mut self,
        effect: Effect,
        ctx: &egui::Context,
        env: &PaletteEnv,
        requests: &mut Vec<AppRequest>,
    ) {
        let project: Option<String> = env.project.map(|p| p.to_string());
        match effect {
            Effect::Request(request) => {
                requests.push(request);
                self.close();
            }
            Effect::Notify { title, detail } => {
                env.notifier.notify(&title, &detail);
                self.close();
            }
            Effect::Git { title, job } => {
                self.close();
                if let Some(project) = project {
                    let backend = env.backend.clone();
                    let task: Task<Result<(), String>> =
                        Task::spawn(ctx, move || run_git_job(&backend, &project, &job));
                    self.jobs.push(Job::Git { title, task });
                }
            }
            Effect::ListBranches(op) => {
                self.close();
                if let Some(project) = project {
                    let backend = env.backend.clone();
                    let task: Task<Vec<Branch>> =
                        Task::spawn(ctx, move || backend.branches(&project));
                    self.jobs.push(Job::Branches(op, task));
                }
            }
            Effect::ListRemotes => {
                self.close();
                if let Some(project) = project {
                    let backend = env.backend.clone();
                    let task: Task<Vec<String>> =
                        Task::spawn(ctx, move || backend.remotes(&project));
                    self.jobs.push(Job::Remotes(task));
                }
            }
            Effect::PickBranch(op, choices) => {
                let items = branch_items(&choices);
                self.open_pick(
                    op.placeholder(),
                    items,
                    false,
                    PickAction::Branch(op, choices),
                );
            }
            Effect::PickRemote(remotes) => {
                let items = remote_items(&remotes);
                self.open_pick("Publish to which remote?", items, false, PickAction::Remote);
            }
            Effect::AskBranchName => {
                self.open = true;
                self.step = Step::Input {
                    placeholder: "New branch name".to_string(),
                };
                self.reset_query();
            }
            Effect::AskDiscard => {
                self.open_pick(DISCARD_PROMPT, discard_items(), false, PickAction::Discard);
            }
            Effect::PickSession(items) => {
                self.open_pick("Search sessions", items, true, PickAction::Session);
            }
        }
    }

    fn poll_jobs(&mut self, ctx: &egui::Context, env: &PaletteEnv, requests: &mut Vec<AppRequest>) {
        if self.jobs.is_empty() {
            return;
        }
        let jobs = std::mem::take(&mut self.jobs);
        let mut still: Vec<Job> = Vec::new();
        let mut done: Vec<Done> = Vec::new();
        for job in jobs {
            match job {
                Job::Branches(op, mut task) => {
                    let (result, keep) = take_result(&mut task);
                    if let Some(branches) = result {
                        done.push(Done::Branches(op, branches));
                    } else if keep {
                        still.push(Job::Branches(op, task));
                    }
                }
                Job::Remotes(mut task) => {
                    let (result, keep) = take_result(&mut task);
                    if let Some(remotes) = result {
                        done.push(Done::Remotes(remotes));
                    } else if keep {
                        still.push(Job::Remotes(task));
                    }
                }
                Job::Git { title, mut task } => {
                    let (result, keep) = take_result(&mut task);
                    if let Some(outcome) = result {
                        done.push(Done::Git(title, outcome));
                    } else if keep {
                        still.push(Job::Git { title, task });
                    }
                }
            }
        }
        self.jobs = still;
        for item in done {
            match item {
                Done::Branches(op, branches) => {
                    let effect = branch_effect(op, &branches);
                    self.apply(effect, ctx, env, requests);
                }
                Done::Remotes(remotes) => {
                    let effect = remote_effect(&remotes);
                    self.apply(effect, ctx, env, requests);
                }
                Done::Git(title, outcome) => {
                    if let Err(message) = outcome {
                        env.notifier.notify(&format!("{} failed", title), &message);
                    }
                    self.refresh = true;
                }
            }
        }
    }

    /// The user chose the row `id` (or submitted `typed` for an input step).
    fn choose(
        &mut self,
        id: &str,
        ctx: &egui::Context,
        env: &PaletteEnv,
        requests: &mut Vec<AppRequest>,
    ) {
        let action: Option<PickAction> = match &self.step {
            Step::Commands => Some(PickAction::Command),
            Step::Pick { action, .. } => Some(action.clone()),
            Step::Input { .. } => None,
        };
        let action = match action {
            Some(action) => action,
            None => return,
        };
        let effect: Option<Effect> = match action {
            PickAction::Command => find_command(id)
                .map(|command| start_command(command, env.project, env.staged_count, env.sessions)),
            PickAction::Branch(op, choices) => pick_branch(op, &choices, id),
            PickAction::Remote => Some(Effect::Git {
                title: "Publish".to_string(),
                job: GitJob::Publish(id.to_string()),
            }),
            PickAction::Discard => discard_effect(id),
            PickAction::Session => Some(Effect::Request(AppRequest::OpenSession(id.to_string()))),
        };
        match effect {
            Some(effect) => self.apply(effect, ctx, env, requests),
            None => self.close(),
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, env: &PaletteEnv) -> Vec<AppRequest> {
        let mut requests: Vec<AppRequest> = Vec::new();
        self.poll_jobs(ctx, env, &mut requests);
        if !self.open {
            return requests;
        }
        let palette = env.palette;
        let screen = ctx.content_rect();
        let wide = matches!(&self.step, Step::Pick { wide: true, .. });
        let is_input = matches!(&self.step, Step::Input { .. });
        let placeholder: String = match &self.step {
            Step::Commands => "Type a command".to_string(),
            Step::Pick { placeholder, .. } => placeholder.clone(),
            Step::Input { placeholder } => placeholder.clone(),
        };
        let width = if wide {
            (screen.width() * 0.95).min(1100.0)
        } else {
            (screen.width() * 0.9).min(560.0)
        };
        let max_list = if wide { screen.height() * 0.6 } else { 320.0 };

        let (escape, enter, down, up) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Escape),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
            )
        });
        if escape {
            self.close();
            return requests;
        }

        let items = self.items();
        let count = items.len();
        let mut selected: usize = if count == 0 {
            0
        } else {
            self.index.min(count - 1)
        };
        let mut key_moved = false;
        if count > 0 && down {
            selected = (selected + 1) % count;
            key_moved = true;
        }
        if count > 0 && up {
            selected = (selected + count - 1) % count;
            key_moved = true;
        }
        self.index = selected;

        let pointer_moved = ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
        let mut query = self.query.clone();
        let mut want_focus = self.want_focus;
        let mut hovered: Option<usize> = None;
        let mut clicked: Option<String> = None;

        let area = egui::Area::new(egui::Id::new("vettr-palette"))
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_TOP, egui::vec2(0.0, 80.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(palette.bg_input)
                    .stroke(egui::Stroke::new(1.0, palette.border))
                    .corner_radius(egui::CornerRadius::same(6))
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let edit = egui::TextEdit::singleline(&mut query)
                            .hint_text(placeholder.clone())
                            .desired_width(f32::INFINITY)
                            .margin(Margin::symmetric(12, 10))
                            .frame(egui::Frame::NONE)
                            .text_color(palette.text_strong);
                        let response = ui.add(edit);
                        if want_focus {
                            response.request_focus();
                            want_focus = false;
                        }
                        let (line, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 1.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(line, 0.0, palette.border);
                        if is_input {
                            return;
                        }
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical()
                            .max_height(max_list)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                if items.is_empty() {
                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        ui.add_space(12.0);
                                        ui.label(
                                            RichText::new("No matching items")
                                                .color(palette.text_muted),
                                        );
                                    });
                                }
                                let font = egui::FontId::proportional(14.0);
                                for (i, item) in items.iter().enumerate() {
                                    let (rect, resp) = ui.allocate_exact_size(
                                        egui::vec2(ui.available_width(), 24.0),
                                        egui::Sense::click(),
                                    );
                                    let is_selected = i == selected;
                                    if is_selected {
                                        ui.painter().rect_filled(rect, 0.0, palette.accent);
                                    }
                                    let fg = if is_selected {
                                        palette.text_on_accent
                                    } else {
                                        palette.text
                                    };
                                    let muted = if is_selected {
                                        palette.text_on_accent
                                    } else {
                                        palette.text_muted
                                    };
                                    let mut label_right = rect.right() - 12.0;
                                    if let Some(detail) = &item.detail {
                                        let used = ui.painter().text(
                                            egui::pos2(rect.right() - 12.0, rect.center().y),
                                            Align2::RIGHT_CENTER,
                                            detail,
                                            font.clone(),
                                            muted,
                                        );
                                        label_right = used.left() - 12.0;
                                    }
                                    let clip = egui::Rect::from_min_max(
                                        rect.min,
                                        egui::pos2(label_right.max(rect.left()), rect.max.y),
                                    )
                                    .intersect(ui.clip_rect());
                                    ui.painter().with_clip_rect(clip).text(
                                        egui::pos2(rect.left() + 12.0, rect.center().y),
                                        Align2::LEFT_CENTER,
                                        &item.label,
                                        font.clone(),
                                        fg,
                                    );
                                    if key_moved && is_selected {
                                        resp.scroll_to_me(None);
                                    }
                                    if resp.hovered() && pointer_moved {
                                        hovered = Some(i);
                                    }
                                    if resp.clicked() {
                                        clicked = Some(item.id.clone());
                                    }
                                }
                            });
                        ui.add_space(4.0);
                    });
            });

        if query != self.query {
            self.query = query.clone();
            self.index = 0;
        }
        self.want_focus = want_focus;
        if let Some(i) = hovered {
            self.index = i;
        }

        let panel_rect = area.response.rect;
        let outside_click = ctx.input(|i| {
            i.pointer.any_pressed()
                && match i.pointer.interact_pos() {
                    Some(pos) => !panel_rect.contains(pos),
                    None => false,
                }
        });
        if outside_click {
            self.close();
            return requests;
        }

        if let Some(id) = clicked {
            self.choose(&id, ctx, env, &mut requests);
        } else if enter {
            if is_input {
                let effect = new_branch_effect(&query);
                self.apply(effect, ctx, env, &mut requests);
            } else if count > 0 {
                let id = items[selected].id.clone();
                self.choose(&id, ctx, env, &mut requests);
            }
        }
        requests
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_palette_is_closed_and_has_nothing_to_refresh() {
        let mut palette = CommandPalette::new();
        assert!(!palette.is_open());
        assert!(!palette.refresh_requested());
        palette.open();
        assert!(palette.is_open());
        assert_eq!(
            palette.items().len(),
            super::super::commands::COMMANDS.len()
        );
    }

    #[test]
    fn typing_filters_the_command_list() {
        let mut palette = CommandPalette::new();
        palette.open();
        palette.query = "stash".to_string();
        let items = palette.items();
        assert!(!items.is_empty());
        assert!(items[0].label.contains("Stash"));
    }

    #[test]
    fn close_resets_the_prompt() {
        let mut palette = CommandPalette::new();
        palette.open();
        palette.query = "x".to_string();
        palette.close();
        assert!(!palette.is_open());
        assert!(palette.query.is_empty());
    }
}
