//! The application shell (port of `App.tsx`): title bar, menu, layout, global shortcuts, and the
//! glue between the backend events and the view models.

use egui::{Key, Modifiers, RichText};

use super::agent_session::AgentSessionModel;
use super::api_key_form::ApiKeyForm;
use super::changes::{self, ChangesEnv};
use super::changes_model::{ChangesModel, ReviewModel};
use super::command_palette::{AppRequest, CommandPalette, PaletteEnv};
use super::notifications::Notifier;
use super::palette::{self, Palette};
use super::readiness_notices::notify_transition;
use super::replies::ResolvedModel;
use super::session::{self, SessionEnv};
use super::settings_view::{self, SettingsEnv, SettingsViewState};
use super::sidebar::{self, SidebarAction, SidebarEnv, SidebarState, View};
use super::status_bar::{self, StatusBarAction, StatusBarEnv, StatusBarState};
use crate::backend::{Backend, BackendEvent};
use crate::comments::format_review;
use crate::profiles::ProfilesState;
use crate::readiness::{readiness_block_reason, Readiness};
use crate::replies::sent_comments;
use crate::session::SessionStatus;
use crate::task::{fire_and_forget, Task};
use crate::theme::{other_theme, resolve_theme, Theme};

/// Sending a review round: record the tree first (so the next round can show what the agent
/// changed), then deliver the message, and only mark the comments sent once the agent took it.
enum SendStage {
    Snapshot {
        task: Task<Option<String>>,
        message: String,
    },
    Deliver {
        task: Task<Result<(), String>>,
        baseline: Option<String>,
    },
}

struct SendingReview {
    ids: Vec<String>,
    stage: SendStage,
}

pub struct VettrApp {
    ctx: egui::Context,
    backend: Backend,
    notifier: Notifier,
    project: Option<String>,
    sidebar: SidebarState,
    changes: ChangesModel,
    review: ReviewModel,
    resolved: ResolvedModel,
    session: AgentSessionModel,
    readiness: Readiness,
    building: bool,
    profiles: ProfilesState,
    api_key_form: ApiKeyForm,
    settings_view: SettingsViewState,
    status: StatusBarState,
    command_palette: CommandPalette,
    theme_choice: Option<Theme>,
    applied_theme: Option<Theme>,
    was_focused: bool,
    sending: Option<SendingReview>,
}

impl VettrApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> VettrApp {
        VettrApp::with_context(cc.egui_ctx.clone())
    }

    /// Build the app on an egui context (tests use a headless one).
    pub fn with_context(ctx: egui::Context) -> VettrApp {
        let backend = Backend::new(&ctx);
        let notifier = Notifier::new();
        let mut app = VettrApp {
            changes: ChangesModel::new(&ctx, &backend, &notifier),
            review: ReviewModel::new(),
            resolved: ResolvedModel::new(&ctx, &backend),
            session: AgentSessionModel::new(&ctx, &backend, &notifier),
            readiness: backend.readiness(),
            building: false,
            profiles: backend.profiles_state(),
            api_key_form: ApiKeyForm::new(),
            settings_view: SettingsViewState::default(),
            status: StatusBarState::new(),
            command_palette: CommandPalette::new(),
            theme_choice: backend.load_theme(),
            applied_theme: None,
            was_focused: true,
            sending: None,
            sidebar: SidebarState::default(),
            project: None,
            ctx,
            backend,
            notifier,
        };
        let launch_project = app.backend.current_project();
        app.set_project(launch_project);
        app
    }

    fn set_project(&mut self, project: Option<String>) {
        if self.project == project {
            return;
        }
        self.project = project;
        let path = self.project.clone();
        self.session.set_project(path.as_deref());
        self.changes.set_project(path.as_deref());
        self.review.set_project(path.as_deref());
        self.resolved.set_project(path.as_deref());
        self.status.refresh();
        self.sending = None;
    }

    fn current_theme(&self, ctx: &egui::Context) -> Theme {
        let system_dark = matches!(ctx.system_theme(), Some(egui::Theme::Dark) | None);
        resolve_theme(self.theme_choice, system_dark)
    }

    fn pump_events(&mut self) {
        for event in self.backend.poll_events() {
            match event {
                BackendEvent::ProjectOpened(path) => self.set_project(Some(path)),
                BackendEvent::RepoChanged => {
                    self.changes.reload();
                    self.status.refresh();
                    // The launch project may have been dropped as not a repository, and the
                    // recent list may have changed
                    let current = self.backend.current_project();
                    if current != self.project {
                        self.set_project(current);
                    }
                }
                BackendEvent::Agent(agent_event) => self.session.handle_agent_event(&agent_event),
                BackendEvent::Readiness(next) => {
                    notify_transition(&self.notifier, &self.readiness, &next, &mut self.building);
                    self.readiness = next;
                }
                BackendEvent::ProfilesChanged(state) => self.profiles = state,
            }
        }
    }

    fn advance_send(&mut self) {
        let sending = match self.sending.take() {
            Some(sending) => sending,
            None => return,
        };
        let SendingReview { ids, stage } = sending;
        match stage {
            SendStage::Snapshot { mut task, message } => match task.poll() {
                Some(baseline) => {
                    let deliver = self.session.submit_review(message);
                    self.sending = Some(SendingReview {
                        ids,
                        stage: SendStage::Deliver {
                            task: deliver,
                            baseline,
                        },
                    });
                }
                None => {
                    if task.is_running() {
                        self.sending = Some(SendingReview {
                            ids,
                            stage: SendStage::Snapshot { task, message },
                        });
                    } else {
                        self.notifier
                            .notify("Comments not sent", "Could not record the working tree.");
                    }
                }
            },
            SendStage::Deliver { mut task, baseline } => match task.poll() {
                Some(Ok(())) => {
                    self.review.mark_sent(&ids, baseline);
                    self.sidebar.view = View::Session;
                }
                Some(Err(error)) => self.notifier.notify("Comments not sent", &error),
                None => {
                    if task.is_running() {
                        self.sending = Some(SendingReview {
                            ids,
                            stage: SendStage::Deliver { task, baseline },
                        });
                    } else {
                        self.notifier
                            .notify("Comments not sent", "The agent did not respond.");
                    }
                }
            },
        }
    }

    fn start_send(&mut self) {
        if self.sending.is_some() {
            return;
        }
        let project = match &self.project {
            Some(project) => project.clone(),
            None => return,
        };
        let pending = self.review.pending();
        if pending.is_empty() {
            return;
        }
        let message = format_review(&pending, self.review.round());
        let ids: Vec<String> = pending.iter().map(|c| c.id.clone()).collect();
        let backend = self.backend.clone();
        let task = Task::spawn(&self.ctx, move || backend.snapshot_tree(&project));
        self.sending = Some(SendingReview {
            ids,
            stage: SendStage::Snapshot { task, message },
        });
    }

    fn handle_shortcuts(&mut self, ui: &mut egui::Ui) {
        let open_palette =
            ui.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::P));
        if open_palette {
            self.command_palette.open();
        }
        if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::B)) {
            self.sidebar.expanded = !self.sidebar.expanded;
        }
        if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::O)) {
            let backend = self.backend.clone();
            fire_and_forget(&self.ctx, move || backend.open_project_dialog());
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let recent = self.backend.recent_projects();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Open Project...").clicked() {
                    let backend = self.backend.clone();
                    fire_and_forget(&self.ctx, move || backend.open_project_dialog());
                    ui.close();
                }
                ui.add_enabled_ui(!recent.is_empty(), |ui| {
                    ui.menu_button("Recent Projects", |ui| {
                        for path in &recent {
                            if ui.button(path).clicked() {
                                let backend = self.backend.clone();
                                let path = path.clone();
                                fire_and_forget(&self.ctx, move || backend.open_project_at(&path));
                                ui.close();
                            }
                        }
                    });
                });
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });
    }

    fn title_bar(&mut self, ui: &mut egui::Ui, palette: &Palette) {
        ui.horizontal(|ui| {
            let active_name: Option<String> = self.profiles.active_id.as_ref().and_then(|id| {
                self.profiles
                    .profiles
                    .iter()
                    .find(|p| &p.id == id)
                    .map(|p| p.name.clone())
            });
            if !self.profiles.profiles.is_empty() || active_name.is_some() {
                let label = active_name.clone().unwrap_or_else(|| "No profile".to_string());
                let chip = egui::Button::new(RichText::new(label).size(11.0)).small();
                if ui
                    .add(chip)
                    .on_hover_text(
                        "Claude profile in use by this window. Click to manage profiles in Settings",
                    )
                    .clicked()
                {
                    self.sidebar.view = View::Settings;
                }
            }
            let project_text = self
                .project
                .clone()
                .unwrap_or_else(|| "No project open".to_string());
            ui.label(RichText::new(project_text).monospace().size(12.0).color(palette.text_muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.project.is_some() {
                    let count = self.changes.changed_count();
                    let label = if count > 0 {
                        format!("Changes ({})", count)
                    } else {
                        "Changes".to_string()
                    };
                    if ui.button(label).clicked() {
                        self.sidebar.view = View::Changes;
                        self.sidebar.expanded = true;
                    }
                }
                if ui
                    .button("Commands")
                    .on_hover_text("Command palette (Ctrl+Shift+P)")
                    .clicked()
                {
                    self.command_palette.open();
                }
            });
        });
    }

    fn apply_requests(&mut self, requests: Vec<AppRequest>) {
        for request in requests {
            match request {
                AppRequest::ShowView(view) => {
                    self.sidebar.view = view;
                    self.sidebar.expanded = true;
                }
                AppRequest::NewSession => {
                    self.session.new_session();
                    self.sidebar.view = View::Session;
                    self.sidebar.expanded = true;
                }
                AppRequest::OpenSession(id) => {
                    self.session.open_session(&id);
                    self.sidebar.view = View::Session;
                    self.sidebar.expanded = true;
                }
                AppRequest::FocusCommit => {
                    self.sidebar.view = View::Changes;
                    self.sidebar.expanded = true;
                    self.changes.focus_commit();
                }
            }
        }
    }
}

impl eframe::App for VettrApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

impl VettrApp {
    /// Draw one frame of the whole app.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();

        // Theme
        let theme = self.current_theme(&ctx);
        let palette = Palette::new(theme);
        if self.applied_theme != Some(theme) {
            palette::apply(&ctx, &palette);
            self.applied_theme = Some(theme);
        }

        // State that changed outside the UI
        self.pump_events();
        let focused = ctx.input(|i| i.focused);
        if focused && !self.was_focused {
            self.changes.reload();
            self.status.refresh();
        }
        self.was_focused = focused;
        self.resolved.update();
        self.session.update();
        self.changes.update(&mut self.review);
        self.advance_send();
        if self.session.take_loaded() {
            // Opening a stored session brings back the comments it sent, from its transcript
            let sent = sent_comments(&self.session.state().items);
            self.review.restore(sent);
        }

        self.handle_shortcuts(ui);

        // Layout: menu and title bar, status bar, sidebar, then the main view
        egui::Panel::top("menubar").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::top("titlebar")
            .frame(
                egui::Frame::new()
                    .fill(palette.bg_titlebar)
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ui, |ui| self.title_bar(ui, &palette));

        // `status_bar::show` creates its own bottom panel; wrapping it in another one stacked a
        // second, default-framed panel (with its own top separator line) around it.
        let status_actions = {
            let env = StatusBarEnv {
                project: self.project.as_deref(),
                palette: &palette,
                backend: &self.backend,
                notifier: &self.notifier,
                theme,
            };
            status_bar::show(ui, &mut self.status, &env)
        };
        for action in status_actions {
            match action {
                StatusBarAction::ToggleTheme => {
                    let next = other_theme(theme);
                    self.theme_choice = Some(next);
                    self.backend.save_theme(Some(next));
                }
            }
        }

        let sidebar_actions = {
            let changed_count = self.changes.changed_count();
            let session_started = self.session.state().status != SessionStatus::Idle;
            let mut env = SidebarEnv {
                project: self.project.as_deref(),
                palette: &palette,
                sessions: self.session.sessions(),
                session_started,
                changes: &mut self.changes,
                changed_count,
            };
            sidebar::show(ui, &mut self.sidebar, &mut env)
        };
        for action in sidebar_actions {
            match action {
                SidebarAction::SelectView(_) => {}
                SidebarAction::NewSession => {
                    self.session.new_session();
                    self.sidebar.view = View::Session;
                }
                SidebarAction::OpenSession(id) => {
                    self.session.open_session(&id);
                    self.sidebar.view = View::Session;
                }
            }
        }

        let agent_block = readiness_block_reason(&self.readiness);
        let mut send_requested = false;
        egui::CentralPanel::default_margins().show(ui, |ui| match self.sidebar.view {
            View::Settings => {
                let mut env = SettingsEnv {
                    palette: &palette,
                    backend: &self.backend,
                    notifier: &self.notifier,
                    ctx: &ctx,
                    profiles: &self.profiles,
                    api_key_form: &mut self.api_key_form,
                };
                settings_view::show(ui, &mut self.settings_view, &mut env);
            }
            View::Session => {
                let mut env = SessionEnv {
                    project: self.project.as_deref(),
                    readiness: &self.readiness,
                    profiles: &self.profiles,
                    palette: &palette,
                    resolved: &mut self.resolved,
                    api_key_form: &mut self.api_key_form,
                    backend: &self.backend,
                    notifier: &self.notifier,
                };
                session::show(ui, &mut self.session, &mut env);
            }
            View::Changes => {
                let replies = self.session.replies();
                let running = self.session.state().status == SessionStatus::Running;
                let send_blocked: Option<String> = match &agent_block {
                    Some(reason) => Some(reason.clone()),
                    None if running => {
                        Some("Wait for the agent to finish its current turn".to_string())
                    }
                    None => None,
                };
                let mut env = ChangesEnv {
                    project: self.project.as_deref(),
                    palette: &palette,
                    backend: &self.backend,
                    notifier: &self.notifier,
                    resolved: &mut self.resolved,
                    replies: &replies,
                    agent_block: agent_block.as_deref(),
                    send_blocked: send_blocked.as_deref(),
                };
                let output = changes::show(ui, &mut self.changes, &mut self.review, &mut env);
                if output.send_requested {
                    send_requested = true;
                }
            }
        });
        if send_requested {
            self.start_send();
        }

        // Overlays
        let requests = {
            let env = PaletteEnv {
                project: self.project.as_deref(),
                palette: &palette,
                backend: &self.backend,
                notifier: &self.notifier,
                staged_count: self.changes.staged_count(),
                sessions: self.session.sessions(),
            };
            self.command_palette.show(&ctx, &env)
        };
        if self.command_palette.refresh_requested() {
            self.status.refresh();
            self.changes.reload();
        }
        self.apply_requests(requests);
        self.notifier.show(&ctx, &palette);
    }
}

impl Drop for VettrApp {
    fn drop(&mut self) {
        self.backend.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn frame(app: &mut VettrApp, ctx: &egui::Context, time: f64) {
        let mut input = egui::RawInput::default();
        input.screen_rect = Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1200.0, 800.0),
        ));
        input.time = Some(time);
        let output = ctx.run_ui(input, |ui| app.show(ui));
        output.drop_without_applying_deltas();
    }

    fn git(dir: &std::path::Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(status.success());
    }

    /// A repo with a commit, then staged, unstaged and untracked changes (and a long file).
    fn make_repo(dir: &std::path::Path) -> String {
        git(dir, &["init", "-q"]);
        let long: String = (0..400)
            .map(|i| format!("let value_{i} = {i}; // line\n"))
            .collect();
        std::fs::write(dir.join("a.rs"), "fn main() {\n    println!(\"hi\");\n}\n").unwrap();
        std::fs::write(dir.join("long.rs"), &long).unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "init"]);
        std::fs::write(
            dir.join("a.rs"),
            "fn main() {\n    println!(\"hello\");\n    let x = 1;\n}\n",
        )
        .unwrap();
        git(dir, &["add", "a.rs"]);
        std::fs::write(dir.join("long.rs"), long.replace("value_7 ", "renamed ")).unwrap();
        std::fs::write(dir.join("new.txt"), "untracked\n").unwrap();
        std::fs::canonicalize(dir)
            .unwrap()
            .to_string_lossy()
            .to_string()
    }

    /// Draw every view headlessly against a repo with every kind of change.
    #[test]
    fn renders_every_view_without_panicking() {
        let data = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_DATA_HOME", data.path());
        std::env::set_var("HOME", data.path());
        let ctx = egui::Context::default();
        let mut app = VettrApp::with_context(ctx.clone());
        let repo = tempfile::tempdir().unwrap();
        let project = make_repo(repo.path());
        let backend = app.backend.clone();
        let p = project.clone();
        std::thread::spawn(move || backend.open_project_at(&p));

        // A populated transcript: text, tool calls, an edit, an error and a review message
        app.session.set_project(Some(&project));
        for event in [
            crate::agent::AgentEvent::SessionStarted {
                session_id: "s1".into(),
            },
            crate::agent::AgentEvent::Text {
                text: "Working on it".into(),
            },
            crate::agent::AgentEvent::ToolStarted {
                id: "t1".into(),
                name: "Bash".into(),
                input: serde_json::json!({"command": "ls -la"}),
            },
            crate::agent::AgentEvent::ToolFinished {
                id: "t1".into(),
                output: "x".repeat(6000),
                is_error: false,
            },
            crate::agent::AgentEvent::FileEdited {
                path: format!("{}/a.rs", project),
            },
            crate::agent::AgentEvent::Error {
                message: "boom".into(),
            },
            crate::agent::AgentEvent::TurnFinished,
        ] {
            app.session.handle_agent_event(&event);
        }

        let start = Instant::now();
        let mut time = 0.0;
        let views = [View::Session, View::Changes, View::Settings, View::Changes];
        let mut view_index = 0;
        while start.elapsed() < Duration::from_secs(8) {
            time += 0.05;
            frame(&mut app, &ctx, time);
            std::thread::sleep(Duration::from_millis(20));
            if app.project.is_some() && app.changes.changes().is_some() {
                app.sidebar.view = views[view_index % views.len()];
                view_index += 1;
                if view_index == 2 {
                    app.command_palette.open();
                }
                if view_index > 40 {
                    break;
                }
            }
        }
        assert_eq!(
            app.project.as_deref().map(|p| p.trim_end_matches('/')),
            Some(project.trim_end_matches('/'))
        );
        assert!(app.changes.changes().is_some());
        assert!(view_index > 4, "the project never finished loading");
    }
}
