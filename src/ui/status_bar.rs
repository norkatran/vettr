//! The footer (port of `StatusBar.tsx`): repository, branch, divergence, changed files, the
//! push / publish area and the theme toggle.
//!
//! No polling: the App calls `StatusBarState::refresh` on `RepoChanged`, when the window gains
//! focus and after palette git commands.

use egui::{Align, Align2, Key, Layout, Margin, RichText, Stroke};

use crate::backend::Backend;
use crate::repo_status::RepoStatus;
use crate::task::Task;
use crate::theme::{other_theme, Theme};

use super::notifications::Notifier;
use super::palette::Palette;

pub struct StatusBarEnv<'a> {
    pub project: Option<&'a str>,
    pub palette: &'a Palette,
    pub backend: &'a Backend,
    pub notifier: &'a Notifier,
    /// The theme currently in effect.
    pub theme: Theme,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StatusBarAction {
    ToggleTheme,
}

pub struct StatusBarState {
    /// The explicit theme choice (`None` follows the OS), for the App to keep with the bar.
    pub theme_choice: Option<Theme>,
    project: Option<String>,
    status: Option<RepoStatus>,
    needs_refresh: bool,
    status_task: Option<Task<(String, Option<RepoStatus>)>>,
    /// A push or publish in flight, with the title for its failure notification.
    op_task: Option<(String, Task<Result<(), String>>)>,
    remotes_task: Option<Task<Vec<String>>>,
    /// The remotes to choose from when publishing with several; `None` when the menu is closed.
    remote_menu: Option<Vec<String>>,
}

impl Default for StatusBarState {
    fn default() -> StatusBarState {
        StatusBarState::new()
    }
}

/// What publishing a branch should do given the repository's remotes.
#[derive(Debug, Clone, PartialEq)]
pub enum PublishPlan {
    NoRemotes,
    Direct(String),
    Choose(Vec<String>),
}

pub fn publish_plan(remotes: &[String]) -> PublishPlan {
    match remotes.len() {
        0 => PublishPlan::NoRemotes,
        1 => PublishPlan::Direct(remotes[0].clone()),
        _ => PublishPlan::Choose(remotes.to_vec()),
    }
}

/// The last path component of the project folder.
pub fn repo_name(project: &str) -> String {
    project
        .split(['/', '\\'])
        .rfind(|part| !part.is_empty())
        .unwrap_or(project)
        .to_string()
}

/// The branch name, or the short SHA of a detached head.
pub fn branch_label(status: &RepoStatus) -> String {
    match &status.branch {
        Some(branch) => branch.clone(),
        None => format!(
            "{} (detached)",
            status
                .sha
                .clone()
                .unwrap_or_else(|| "no commits".to_string())
        ),
    }
}

/// "v2 ^3": commits behind and ahead of the upstream (nothing when level or without upstream).
pub fn divergence_label(status: &RepoStatus) -> Option<String> {
    if status.upstream.is_none() || (status.behind == 0 && status.ahead == 0) {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    if status.behind > 0 {
        parts.push(format!("v{}", status.behind));
    }
    if status.ahead > 0 {
        parts.push(format!("^{}", status.ahead));
    }
    Some(parts.join(" "))
}

pub fn changes_label(changes: u32) -> String {
    if changes == 0 {
        "clean".to_string()
    } else {
        format!("{} changed", changes)
    }
}

/// How the branch segment behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchMode {
    /// A branch without an upstream: click to publish.
    Publish,
    /// Commits to push: click to push.
    Push,
    /// Just information.
    Plain,
}

pub fn branch_mode(status: &RepoStatus) -> BranchMode {
    if status.upstream.is_none() && status.branch.is_some() {
        BranchMode::Publish
    } else if status.upstream.is_some() && status.ahead > 0 {
        BranchMode::Push
    } else {
        BranchMode::Plain
    }
}

fn branch_text(status: &RepoStatus, mode: BranchMode) -> String {
    let mut text = branch_label(status);
    if status.upstream.is_none() {
        text.push_str("  no upstream");
    } else if let Some(divergence) = divergence_label(status) {
        text.push_str("  ");
        text.push_str(&divergence);
    }
    if mode == BranchMode::Publish {
        text.push_str("  Publish Branch");
    }
    text
}

fn take_result<T: Send + 'static>(task: &mut Task<T>) -> (Option<T>, bool) {
    let mut result = task.poll();
    if result.is_none() && !task.is_running() {
        result = task.poll();
    }
    let keep = result.is_none() && task.is_running();
    (result, keep)
}

impl StatusBarState {
    pub fn new() -> StatusBarState {
        StatusBarState {
            theme_choice: None,
            project: None,
            status: None,
            needs_refresh: false,
            status_task: None,
            op_task: None,
            remotes_task: None,
            remote_menu: None,
        }
    }

    /// Reload the repository status (the file watcher, window focus and git commands call this).
    pub fn refresh(&mut self) {
        self.needs_refresh = true;
    }

    fn sync_project(&mut self, project: Option<&str>) {
        if self.project.as_deref() != project {
            self.project = project.map(|p| p.to_string());
            self.status = None;
            self.status_task = None;
            self.remotes_task = None;
            self.remote_menu = None;
            self.needs_refresh = project.is_some();
        }
    }

    fn poll(&mut self, env: &StatusBarEnv, ctx: &egui::Context) {
        if let Some(mut task) = self.status_task.take() {
            let (result, keep) = take_result(&mut task);
            if let Some((project, status)) = result {
                if self.project.as_deref() == Some(project.as_str()) {
                    self.status = status;
                }
            } else if keep {
                self.status_task = Some(task);
            }
        }
        if let Some((title, mut task)) = self.op_task.take() {
            let (result, keep) = take_result(&mut task);
            if let Some(outcome) = result {
                self.needs_refresh = true;
                if let Err(message) = outcome {
                    env.notifier.notify(&format!("{} failed", title), &message);
                }
            } else if keep {
                self.op_task = Some((title, task));
            } else {
                self.needs_refresh = true;
            }
        }
        if let Some(mut task) = self.remotes_task.take() {
            let (result, keep) = take_result(&mut task);
            if let Some(remotes) = result {
                match publish_plan(&remotes) {
                    PublishPlan::NoRemotes => env.notifier.notify(
                        "Cannot publish branch",
                        "This repository has no remotes. Add one with `git remote add`.",
                    ),
                    PublishPlan::Direct(remote) => self.start_op(ctx, env, "Publish", Some(remote)),
                    PublishPlan::Choose(names) => self.remote_menu = Some(names),
                }
            } else if keep {
                self.remotes_task = Some(task);
            }
        }
        if self.needs_refresh && self.status_task.is_none() {
            if let Some(project) = self.project.clone() {
                self.needs_refresh = false;
                let backend = env.backend.clone();
                self.status_task = Some(Task::spawn(ctx, move || {
                    let status = backend.repo_status(&project);
                    (project, status)
                }));
            } else {
                self.needs_refresh = false;
            }
        }
    }

    /// Push (`remote` is `None`) or publish to `remote`.
    fn start_op(
        &mut self,
        ctx: &egui::Context,
        env: &StatusBarEnv,
        title: &str,
        remote: Option<String>,
    ) {
        if self.op_task.is_some() {
            return;
        }
        let project = match self.project.clone() {
            Some(project) => project,
            None => return,
        };
        let backend = env.backend.clone();
        let task: Task<Result<(), String>> = Task::spawn(ctx, move || match remote {
            Some(remote) => backend.publish(&project, &remote),
            None => backend.push(&project),
        });
        self.op_task = Some((title.to_string(), task));
    }
}

/// Draw the footer as the bottom panel. Call before the central panel.
pub fn show(
    ui: &mut egui::Ui,
    state: &mut StatusBarState,
    env: &StatusBarEnv,
) -> Vec<StatusBarAction> {
    let ctx: egui::Context = ui.ctx().clone();
    state.sync_project(env.project);
    state.poll(env, &ctx);

    let palette: Palette = *env.palette;
    let status: Option<RepoStatus> = state.status.clone();
    let pushing = state.op_task.is_some();
    let menu: Option<Vec<String>> = state.remote_menu.clone();
    let mut actions: Vec<StatusBarAction> = Vec::new();
    let mut push_clicked = false;
    let mut publish_clicked = false;
    let mut publish_rect: Option<egui::Rect> = None;

    egui::Panel::bottom("vettr-statusbar")
        .exact_size(24.0)
        .frame(
            egui::Frame::new()
                .fill(palette.bg_titlebar)
                .inner_margin(Margin::symmetric(12, 0)),
        )
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                if let Some(project) = env.project {
                    let label = ui.label(RichText::new(repo_name(project)).size(12.0));
                    let _ = label.on_hover_text(project);
                }
                if let Some(status) = &status {
                    let mode = branch_mode(status);
                    let text = RichText::new(branch_text(status, mode)).size(12.0);
                    match mode {
                        BranchMode::Plain => {
                            let tip = match &status.upstream {
                                Some(upstream) => format!("Compared with {}", upstream),
                                None => "Current branch".to_string(),
                            };
                            let _ = ui.label(text).on_hover_text(tip);
                        }
                        BranchMode::Publish => {
                            let response = ui.add_enabled(
                                !pushing,
                                egui::Button::new(text).frame_when_inactive(false),
                            );
                            publish_rect = Some(response.rect);
                            if response.clicked() {
                                publish_clicked = true;
                            }
                            let _ = response.on_hover_text(
                                "Publish this branch to a remote and set its upstream",
                            );
                        }
                        BranchMode::Push => {
                            let response = ui.add_enabled(
                                !pushing,
                                egui::Button::new(text).frame_when_inactive(false),
                            );
                            if response.clicked() {
                                push_clicked = true;
                            }
                            let upstream = status.upstream.clone().unwrap_or_default();
                            let _ =
                                response.on_hover_text(format!("Click to push to {}", upstream));
                        }
                    }
                    if pushing {
                        ui.add(egui::Spinner::new().size(12.0));
                    }
                    let _ = ui
                        .label(RichText::new(changes_label(status.changes)).size(12.0))
                        .on_hover_text("Changed files, including untracked");
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (name, target) = match env.theme {
                        Theme::Dark => ("Dark", other_theme(env.theme)),
                        Theme::Light => ("Light", other_theme(env.theme)),
                    };
                    let response = ui.add(
                        egui::Button::new(RichText::new(name).size(12.0))
                            .frame_when_inactive(false),
                    );
                    if response.clicked() {
                        actions.push(StatusBarAction::ToggleTheme);
                    }
                    let _ = response.on_hover_text(format!("Switch to {} theme", target.as_str()));
                });
            });
        });

    if publish_clicked && !pushing {
        if state.remote_menu.is_some() {
            state.remote_menu = None;
        } else if state.remotes_task.is_none() {
            if let Some(project) = state.project.clone() {
                let backend = env.backend.clone();
                state.remotes_task = Some(Task::spawn(&ctx, move || backend.remotes(&project)));
            }
        }
    }
    if push_clicked {
        state.start_op(&ctx, env, "Push", None);
    }

    if let (Some(names), Some(anchor)) = (menu, publish_rect) {
        let mut picked: Option<String> = None;
        let area = egui::Area::new(egui::Id::new("vettr-remote-menu"))
            .order(egui::Order::Foreground)
            .pivot(Align2::LEFT_BOTTOM)
            .fixed_pos(anchor.left_top())
            .show(&ctx, |ui| {
                egui::Frame::new()
                    .fill(palette.bg_input)
                    .stroke(Stroke::new(1.0, palette.border))
                    .corner_radius(egui::CornerRadius::same(4))
                    .inner_margin(Margin::symmetric(4, 4))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("Publish to remote")
                                .size(11.0)
                                .color(palette.text_muted),
                        );
                        for name in names.iter() {
                            if ui.button(name.as_str()).clicked() {
                                picked = Some(name.clone());
                            }
                        }
                    });
            });
        let menu_rect = area.response.rect;
        let dismissed = ctx.input(|i| {
            let escape = i.key_pressed(Key::Escape);
            let outside = i.pointer.any_pressed()
                && match i.pointer.interact_pos() {
                    Some(pos) => !menu_rect.contains(pos) && !anchor.contains(pos),
                    None => false,
                };
            escape || outside
        });
        if let Some(remote) = picked {
            state.remote_menu = None;
            state.start_op(&ctx, env, "Publish", Some(remote));
        } else if dismissed {
            state.remote_menu = None;
        }
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(branch: Option<&str>, upstream: Option<&str>, ahead: u32, behind: u32) -> RepoStatus {
        RepoStatus {
            branch: branch.map(|b| b.to_string()),
            sha: Some("abc1234".to_string()),
            upstream: upstream.map(|u| u.to_string()),
            ahead,
            behind,
            changes: 0,
        }
    }

    #[test]
    fn repo_name_is_the_last_path_component() {
        assert_eq!(repo_name("/home/me/vettr"), "vettr");
        assert_eq!(repo_name("C:\\code\\vettr\\"), "vettr");
        assert_eq!(repo_name("/"), "/");
    }

    #[test]
    fn detached_heads_show_the_short_sha() {
        assert_eq!(
            branch_label(&status(None, None, 0, 0)),
            "abc1234 (detached)"
        );
        let mut s = status(None, None, 0, 0);
        s.sha = None;
        assert_eq!(branch_label(&s), "no commits (detached)");
        assert_eq!(branch_label(&status(Some("main"), None, 0, 0)), "main");
    }

    #[test]
    fn divergence_lists_behind_then_ahead() {
        assert_eq!(
            divergence_label(&status(Some("m"), Some("o/m"), 3, 2)),
            Some("v2 ^3".to_string())
        );
        assert_eq!(
            divergence_label(&status(Some("m"), Some("o/m"), 3, 0)),
            Some("^3".to_string())
        );
        assert_eq!(
            divergence_label(&status(Some("m"), Some("o/m"), 0, 0)),
            None
        );
        assert_eq!(divergence_label(&status(Some("m"), None, 3, 2)), None);
    }

    #[test]
    fn the_branch_segment_mode_follows_the_upstream() {
        assert_eq!(
            branch_mode(&status(Some("m"), None, 0, 0)),
            BranchMode::Publish
        );
        assert_eq!(branch_mode(&status(None, None, 0, 0)), BranchMode::Plain);
        assert_eq!(
            branch_mode(&status(Some("m"), Some("o/m"), 1, 0)),
            BranchMode::Push
        );
        assert_eq!(
            branch_mode(&status(Some("m"), Some("o/m"), 0, 4)),
            BranchMode::Plain
        );
    }

    #[test]
    fn branch_text_mentions_the_missing_upstream_and_publish() {
        let s = status(Some("topic"), None, 0, 0);
        assert_eq!(
            branch_text(&s, BranchMode::Publish),
            "topic  no upstream  Publish Branch"
        );
    }

    #[test]
    fn changes_label_says_clean_or_counts() {
        assert_eq!(changes_label(0), "clean");
        assert_eq!(changes_label(3), "3 changed");
    }

    #[test]
    fn publish_plan_follows_the_number_of_remotes() {
        assert_eq!(publish_plan(&[]), PublishPlan::NoRemotes);
        assert_eq!(
            publish_plan(&["origin".to_string()]),
            PublishPlan::Direct("origin".to_string())
        );
        assert!(matches!(
            publish_plan(&["a".to_string(), "b".to_string()]),
            PublishPlan::Choose(_)
        ));
    }
}
