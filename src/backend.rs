//! Glue between the host services and the UI (replaces `src/main/index.ts` and the IPC layer).
//!
//! `Backend` is cheap to clone and is created once with the `egui::Context`. It owns the stores,
//! the agent manager and the project watcher. Events reach the UI through one channel that the UI
//! drains every frame with `poll_events`; every send is followed by `request_repaint`. Methods
//! block (git, docker, HTTP, dialogs), so the UI calls them inside `Task::spawn`.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::agent::{AgentAdapter, AgentEvent, SlashCommandInfo};
use crate::diff::{FileChange, RepoChanges};
use crate::editor::build_editor_command;
use crate::git_actions::{Branch, GitAction};
use crate::profiles::{validate_profile_name, ProfilesState};
use crate::readiness::Readiness;
use crate::repo_status::RepoStatus;
use crate::sandbox::SANDBOX_IMAGE;
use crate::session::SessionState;
use crate::sessions::SessionInfo;
use crate::settings::Settings;
use crate::theme::{parse_theme_choice, Theme};

use crate::host::agent_manager::{AgentManager, AgentManagerDeps, BuildOutcome};
use crate::host::api_key_check::{ureq_fetch, validate_api_key};
use crate::host::busy_guard::confirm_if_busy;
use crate::host::claude_adapter::{ClaudeAdapter, ClaudeAdapterDeps};
use crate::host::git;
use crate::host::profile_store::{ProfileChanges, ProfileStore};
use crate::host::project_store::ProjectStore;
use crate::host::resolved_store;
use crate::host::sandbox_image::{build_image_from_app, EnsureImageOptions};
use crate::host::sandbox_runtime::{
    check_docker, check_docker_detailed, current_uid_gid, default_spawn, exec_command,
    is_process_alive, start_sandbox, stop_sandbox, sweep_orphans, SandboxProcess, StartOptions,
};
use crate::host::secret::{write_file_atomic, KeyringStore, SecretStore};
use crate::host::sessions as stored_sessions;
use crate::host::settings_store::SettingsStore;
use crate::host::transcripts::{project_data_dir, transcripts_dir};
use crate::host::watcher::{ProjectWatcher, Watcher};

/// What the backend tells the UI. Drain with `Backend::poll_events`.
#[derive(Debug, Clone)]
pub enum BackendEvent {
    /// A project was opened (File > Open, a recent project or the launch project is not sent).
    ProjectOpened(String),
    /// The working tree or git state of the open project changed.
    RepoChanged,
    /// An event from the running agent session.
    Agent(AgentEvent),
    /// The agent's readiness changed.
    Readiness(Readiness),
    /// The profiles, or the active one, changed.
    ProfilesChanged(ProfilesState),
}

/// Where the app keeps its configuration: `dirs::data_dir()/vettr`.
pub fn data_dir() -> PathBuf {
    let base: PathBuf = match dirs::data_dir() {
        Some(dir) => dir,
        None => PathBuf::from("."),
    };
    base.join("vettr")
}

// ---------------------------------------------------------------------------------------------
// Locating the Dockerfile and the runner directory
// ---------------------------------------------------------------------------------------------

/// The files the sandbox image is built from.
#[derive(Debug, Clone, PartialEq)]
pub struct SandboxPaths {
    /// `sandbox/Dockerfile`.
    pub dockerfile: PathBuf,
    /// `runner/`, the build context.
    pub runner_dir: PathBuf,
}

/// Directories that may hold `sandbox/` and `runner/`, in the order to try them: next to the
/// executable, the repo root above a `target/<profile>` build directory, a `share/vettr` install
/// directory, the current directory and the crate directory (development).
pub fn resource_roots(
    exe_dir: Option<&Path>,
    cwd: Option<&Path>,
    manifest_dir: &Path,
) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(exe) = exe_dir {
        roots.push(exe.to_path_buf());
        roots.push(exe.join(".."));
        roots.push(exe.join("..").join(".."));
        roots.push(exe.join("..").join("share").join("vettr"));
    }
    if let Some(dir) = cwd {
        roots.push(dir.to_path_buf());
    }
    roots.push(manifest_dir.to_path_buf());
    roots
}

/// The first root that has both `sandbox/Dockerfile` and a `runner` directory.
pub fn find_sandbox_paths(
    roots: &[PathBuf],
    is_file: &dyn Fn(&Path) -> bool,
    is_dir: &dyn Fn(&Path) -> bool,
) -> Option<SandboxPaths> {
    for root in roots {
        let dockerfile = root.join("sandbox").join("Dockerfile");
        let runner_dir = root.join("runner");
        if is_file(&dockerfile) && is_dir(&runner_dir) {
            return Some(SandboxPaths {
                dockerfile,
                runner_dir,
            });
        }
    }
    None
}

/// `find_sandbox_paths` against the real file system and the real executable and working dirs.
pub fn locate_sandbox_paths() -> Option<SandboxPaths> {
    let exe_dir: Option<PathBuf> = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.to_path_buf()));
    let cwd: Option<PathBuf> = std::env::current_dir().ok();
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let roots = resource_roots(exe_dir.as_deref(), cwd.as_deref(), &manifest);
    let is_file = |p: &Path| -> bool { p.is_file() };
    let is_dir = |p: &Path| -> bool { p.is_dir() };
    find_sandbox_paths(&roots, &is_file, &is_dir)
}

// ---------------------------------------------------------------------------------------------
// Small pure helpers
// ---------------------------------------------------------------------------------------------

/// Resolve `.` and `..` lexically (the file may not exist).
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The absolute file for `path` inside `project`, or `None` when it points outside the project.
pub fn resolve_in_project(project: &str, path: &str) -> Option<PathBuf> {
    let root = normalize(Path::new(project));
    let file = normalize(&Path::new(project).join(path));
    if file.starts_with(&root) {
        Some(file)
    } else {
        None
    }
}

/// The theme saved in `ui_state.json` text, if any.
pub fn parse_ui_state_theme(text: &str) -> Option<Theme> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    parse_theme_choice(value.get("theme").and_then(|t| t.as_str()))
}

/// The `ui_state.json` text for a theme choice (`None` means follow the OS).
pub fn ui_state_text(theme: Option<Theme>) -> String {
    let value = match theme {
        Some(t) => serde_json::json!({ "theme": t.as_str() }),
        None => serde_json::json!({ "theme": serde_json::Value::Null }),
    };
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Ask before stopping an agent that is working, since its work in progress is lost.
fn confirm_stop(detail: &str, confirm_label: &str) -> bool {
    let result = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Warning)
        .set_title("The agent is still working.")
        .set_description(detail)
        .set_buttons(rfd::MessageButtons::OkCancelCustom(
            confirm_label.to_string(),
            "Cancel".to_string(),
        ))
        .show();
    match result {
        rfd::MessageDialogResult::Ok | rfd::MessageDialogResult::Yes => true,
        rfd::MessageDialogResult::Custom(label) => label == confirm_label,
        _ => false,
    }
}

fn show_error(message: &str, detail: &str) {
    let _ = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title(message)
        .set_description(detail)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

// ---------------------------------------------------------------------------------------------
// The backend
// ---------------------------------------------------------------------------------------------

/// Sends events to the UI and wakes it up.
struct Emitter {
    ctx: egui::Context,
    tx: Mutex<Sender<BackendEvent>>,
}

impl Emitter {
    fn emit(&self, event: BackendEvent) {
        {
            let tx = lock(&self.tx);
            let _ = tx.send(event);
        }
        self.ctx.request_repaint();
    }
}

struct Inner {
    emitter: Arc<Emitter>,
    rx: Mutex<Receiver<BackendEvent>>,
    data_dir: PathBuf,
    projects: Mutex<ProjectStore>,
    settings: Mutex<SettingsStore>,
    profiles: Arc<ProfileStore>,
    manager: AgentManager,
    watcher: ProjectWatcher<Watcher>,
}

/// The app's services, shared by the UI and its worker threads. Clone freely.
#[derive(Clone)]
pub struct Backend {
    inner: Arc<Inner>,
}

impl Backend {
    /// Build the services, load the persisted state and start working on the launch project.
    pub fn new(ctx: &egui::Context) -> Backend {
        let dir = data_dir();
        let _ = fs::create_dir_all(&dir);

        let (tx, rx) = channel::<BackendEvent>();
        let emitter = Arc::new(Emitter {
            ctx: ctx.clone(),
            tx: Mutex::new(tx),
        });

        let mut project_store = ProjectStore::new(&dir);
        project_store.load();
        let mut settings_store = SettingsStore::new(&dir);
        settings_store.load();

        let secrets: Arc<dyn SecretStore> = Arc::new(KeyringStore::new());
        let profiles = Arc::new(ProfileStore::new(&dir, secrets));
        profiles.init_active();

        // The adapter talks to the container.
        let check_docker_dep: Box<dyn Fn() -> Option<String> + Send + Sync> =
            Box::new(|| check_docker(&exec_command));
        let start_dir = dir.clone();
        let start_sandbox_dep: Box<
            dyn Fn(&str) -> Result<Arc<dyn SandboxProcess>, String> + Send + Sync,
        > = Box::new(
            move |project: &str| -> Result<Arc<dyn SandboxProcess>, String> {
                let spawn = default_spawn();
                let (uid, gid) = current_uid_gid();
                start_sandbox(StartOptions {
                    project: project.to_string(),
                    owner_pid: std::process::id(),
                    transcripts_dir: transcripts_dir(&start_dir, project)
                        .to_string_lossy()
                        .to_string(),
                    spawn: &spawn,
                    uid,
                    gid,
                })
            },
        );
        let key_profiles = profiles.clone();
        let get_key_dep: Box<dyn Fn() -> Option<String> + Send + Sync> =
            Box::new(move || key_profiles.active_credential());
        let stop_dep: Box<dyn Fn(&dyn SandboxProcess) + Send + Sync> =
            Box::new(|container: &dyn SandboxProcess| stop_sandbox(container));
        let adapter = ClaudeAdapter::new(ClaudeAdapterDeps {
            check_docker: check_docker_dep,
            start_sandbox: start_sandbox_dep,
            get_api_key: get_key_dep,
            stop_sandbox: stop_dep,
        });
        let agent: Arc<dyn AgentAdapter> = Arc::new(adapter);

        // The manager owns the lifecycle and readiness.
        let build_image_dep: Box<dyn Fn(&dyn Fn(&str)) -> BuildOutcome + Send + Sync> =
            Box::new(|on_progress: &dyn Fn(&str)| -> BuildOutcome {
                let paths = match locate_sandbox_paths() {
                    Some(paths) => paths,
                    None => return BuildOutcome::Unsupported,
                };
                let spawn = default_spawn();
                let exists = |p: &str| -> bool { Path::new(p).exists() };
                let read_file = |p: &str| -> Result<String, String> {
                    fs::read_to_string(p).map_err(|e| e.to_string())
                };
                let outcome = build_image_from_app(EnsureImageOptions {
                    spawn: &spawn,
                    dockerfile: paths.dockerfile.to_string_lossy().to_string(),
                    runner_dir: paths.runner_dir.to_string_lossy().to_string(),
                    image: SANDBOX_IMAGE.to_string(),
                    exists: &exists,
                    read_file: &read_file,
                    on_progress,
                });
                match outcome {
                    None => BuildOutcome::Unsupported,
                    Some(Ok(())) => BuildOutcome::Built,
                    Some(Err(message)) => BuildOutcome::Failed(message),
                }
            });
        let has_key_profiles = profiles.clone();
        let manager = AgentManager::new(AgentManagerDeps {
            agent,
            check_docker: Box::new(|| check_docker_detailed(&exec_command)),
            build_image: Some(build_image_dep),
            has_key: Box::new(move || has_key_profiles.active_credential().is_some()),
        });
        let event_emitter = emitter.clone();
        manager.on_event(Box::new(move |event: AgentEvent| {
            event_emitter.emit(BackendEvent::Agent(event));
        }));
        let readiness_emitter = emitter.clone();
        manager.on_readiness(Box::new(move |readiness: Readiness| {
            readiness_emitter.emit(BackendEvent::Readiness(readiness));
        }));

        // Tells the UI when the open project's working tree or git state changes.
        let watch_emitter = emitter.clone();
        let watcher: ProjectWatcher<Watcher> = ProjectWatcher::new(Arc::new(move || {
            watch_emitter.emit(BackendEvent::RepoChanged);
        }));

        let backend = Backend {
            inner: Arc::new(Inner {
                emitter,
                rx: Mutex::new(rx),
                data_dir: dir,
                projects: Mutex::new(project_store),
                settings: Mutex::new(settings_store),
                profiles,
                manager,
                watcher,
            }),
        };
        backend.start_up();
        backend
    }

    /// Background start-up work: orphan sweep and the launch project.
    fn start_up(&self) {
        // Containers left by an earlier run that died are removed in the background; ours carry
        // this process's pid, so this is safe to run alongside the first prewarm.
        std::thread::spawn(|| {
            let _ = sweep_orphans(&exec_command, &is_process_alive);
        });
        let me = self.clone();
        std::thread::spawn(move || {
            let current = lock(&me.inner.projects).state().current;
            let project = match current {
                Some(project) => project,
                None => return,
            };
            if git::find_repo_root(&project).is_none() {
                lock(&me.inner.projects).forget_project(&project);
                me.inner.emitter.emit(BackendEvent::RepoChanged);
                return;
            }
            let _ = me.inner.manager.set_project_async(Some(&project));
            me.inner.watcher.watch(&project);
        });
    }

    /// Take the events that arrived since the last call (call once per frame).
    pub fn poll_events(&self) -> Vec<BackendEvent> {
        let rx = lock(&self.inner.rx);
        let mut events: Vec<BackendEvent> = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        events
    }

    /// The directory holding the app's configuration.
    pub fn data_dir(&self) -> PathBuf {
        self.inner.data_dir.clone()
    }

    // ----- projects -----

    /// The persisted current project, or `None`. The launch project is not announced with an
    /// event, so the UI asks for it at start-up (it may be dropped just after if it is not a
    /// repository any more; ask again on `RepoChanged`).
    pub fn current_project(&self) -> Option<String> {
        lock(&self.inner.projects).state().current
    }

    /// Recently opened projects, most recent first.
    pub fn recent_projects(&self) -> Vec<String> {
        lock(&self.inner.projects).state().recent
    }

    /// Ask for a folder and open it. Returns without doing anything when the dialog is cancelled.
    pub fn open_project_dialog(&self) {
        let picked = rfd::FileDialog::new()
            .set_title("Open Project")
            .pick_folder();
        if let Some(path) = picked {
            self.open_project_at(&path.to_string_lossy());
        }
    }

    /// Open `path` as the project, resolving it to its git repo root. If it is not in a git
    /// repository, tell the user and drop it from the recent list.
    pub fn open_project_at(&self, path: &str) {
        match git::find_repo_root(path) {
            Some(root) => {
                let current = lock(&self.inner.projects).state().current;
                let differs = current.as_deref() != Some(root.as_str());
                if differs
                    && self.inner.manager.is_busy()
                    && !confirm_stop(
                        "Opening another project stops it and the work in progress is lost.",
                        "Open project",
                    )
                {
                    return;
                }
                self.activate_project(&root);
            }
            None => {
                lock(&self.inner.projects).forget_project(path);
                // Wake the UI so its recent list refreshes
                self.inner.emitter.emit(BackendEvent::RepoChanged);
                show_error(
                    &format!("{} is not a git repository.", path),
                    "Choose a folder that is inside a git repository.",
                );
            }
        }
    }

    fn activate_project(&self, root: &str) {
        // The manager tears the old project's agent down and prewarms one for this project
        let _ = self.inner.manager.set_project_async(Some(root));
        lock(&self.inner.projects).set_current_project(root);
        let me = self.clone();
        let watched = root.to_string();
        std::thread::spawn(move || {
            me.inner.watcher.watch(&watched);
        });
        self.inner
            .emitter
            .emit(BackendEvent::ProjectOpened(root.to_string()));
    }

    // ----- repository -----

    pub fn repo_status(&self, project: &str) -> Option<RepoStatus> {
        git::get_repo_status(project)
    }

    pub fn changes(&self, project: &str) -> Option<RepoChanges> {
        git::get_changes(project)
    }

    pub fn snapshot_tree(&self, project: &str) -> Option<String> {
        git::snapshot_tree(project)
    }

    pub fn changes_since(&self, project: &str, tree: &str) -> Option<Vec<FileChange>> {
        git::get_changes_since(project, tree)
    }

    pub fn stage(&self, project: &str, paths: &[String]) -> Result<(), String> {
        git::stage_files(project, paths)
    }

    pub fn unstage(&self, project: &str, paths: &[String]) -> Result<(), String> {
        git::unstage_files(project, paths)
    }

    pub fn commit(&self, project: &str, message: &str) -> Result<(), String> {
        git::commit_staged(project, message)
    }

    pub fn push(&self, project: &str) -> Result<(), String> {
        git::push_current(project)
    }

    pub fn remotes(&self, project: &str) -> Vec<String> {
        git::list_remotes(project)
    }

    pub fn publish(&self, project: &str, remote: &str) -> Result<(), String> {
        git::publish_branch(project, remote)
    }

    pub fn branches(&self, project: &str) -> Vec<Branch> {
        git::list_branches(project)
    }

    pub fn run_git_action(&self, project: &str, action: &GitAction) -> Result<(), String> {
        git::run_git_action(project, action)
    }

    // ----- agent -----

    /// Start a session in the current project with a first prompt, optionally resuming a stored
    /// session. The error is a message for the user (Docker missing, no API key, ...).
    pub fn agent_start(&self, prompt: &str, resume: Option<&str>) -> Result<(), String> {
        if self.current_project().is_none() {
            return Err("Open a project first".to_string());
        }
        self.inner.manager.start(prompt, resume)
    }

    /// Send a follow-up in the running session.
    pub fn agent_send(&self, message: &str) -> Result<(), String> {
        self.inner.manager.send(message)
    }

    /// Stop the current turn without ending the session.
    pub fn agent_interrupt(&self) -> Result<(), String> {
        self.inner.manager.interrupt()
    }

    /// End the session and warm a fresh agent (`agentStop` in the TS).
    pub fn new_session(&self) -> Result<(), String> {
        self.inner.manager.new_session()
    }

    /// Whether the agent is working on a turn.
    pub fn is_busy(&self) -> bool {
        self.inner.manager.is_busy()
    }

    pub fn readiness(&self) -> Readiness {
        self.inner.manager.readiness()
    }

    pub fn slash_commands(&self) -> Vec<SlashCommandInfo> {
        self.inner.manager.slash_commands()
    }

    /// The open project's stored sessions, newest first (empty when none or no project).
    pub fn list_sessions(&self) -> Vec<SessionInfo> {
        match self.current_project() {
            Some(project) => {
                let root = transcripts_dir(&self.inner.data_dir, &project);
                stored_sessions::list_sessions(&root, &project)
            }
            None => Vec::new(),
        }
    }

    /// A stored session of the open project rebuilt as Session view state.
    pub fn load_session(&self, id: &str) -> Option<SessionState> {
        let project = self.current_project()?;
        let root = transcripts_dir(&self.inner.data_dir, &project);
        stored_sessions::load_session(&root, &project, id)
    }

    // ----- comments -----

    /// The ids of the comments the user resolved in `project`.
    pub fn resolved_comments(&self, project: &str) -> Vec<String> {
        resolved_store::read_resolved(&project_data_dir(&self.inner.data_dir, project))
    }

    /// Resolve or reopen a comment; returns the ids now resolved.
    pub fn set_comment_resolved(&self, project: &str, id: &str, resolved: bool) -> Vec<String> {
        resolved_store::set_resolved(
            &project_data_dir(&self.inner.data_dir, project),
            id,
            resolved,
        )
    }

    // ----- profiles -----

    pub fn profiles_state(&self) -> ProfilesState {
        self.inner.profiles.state()
    }

    fn broadcast_profiles(&self) {
        let state = self.inner.profiles.state();
        self.inner
            .emitter
            .emit(BackendEvent::ProfilesChanged(state));
    }

    /// Use `id` (or nothing) in this instance, remember it for the next launch and restart the
    /// agent.
    fn activate_profile(&self, id: Option<&str>) {
        if let Err(message) = self.inner.profiles.activate(id) {
            eprintln!("vettr: could not remember the active profile: {}", message);
        }
        let _ = self.inner.manager.key_changed_async();
        self.broadcast_profiles();
    }

    /// Save a named credential (validated first); the first profile becomes active.
    pub fn add_profile(&self, name: &str, credential: &str) -> Result<(), String> {
        let existing = self.inner.profiles.list();
        let checked = validate_profile_name(name, &existing, None)?;
        let trimmed = validate_api_key(credential, &ureq_fetch)?;
        let id = self.inner.profiles.add(&checked, &trimmed)?;
        // The first profile (or one added while none is usable) is used straight away
        if self.inner.profiles.state().active_id.is_none() {
            self.activate_profile(Some(&id));
        } else {
            self.broadcast_profiles();
        }
        Ok(())
    }

    /// Rename and/or replace the credential (`None` keeps the old one).
    pub fn update_profile(
        &self,
        id: &str,
        name: Option<&str>,
        credential: Option<&str>,
    ) -> Result<(), String> {
        let credential: Option<String> = credential
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty());
        if let Some(new_name) = name {
            let existing = self.inner.profiles.list();
            validate_profile_name(new_name, &existing, Some(id))?;
        }
        let validated: Option<String> = match credential {
            Some(raw) => Some(validate_api_key(&raw, &ureq_fetch)?),
            None => None,
        };
        let is_active = self.inner.profiles.active_id().as_deref() == Some(id);
        let restart = validated.is_some() && is_active;
        if restart {
            confirm_if_busy(self.inner.manager.is_busy(), || {
                confirm_stop(
                    "Saving a new key restarts the agent, and the work in progress is lost.",
                    "Save key",
                )
            })?;
        }
        self.inner.profiles.update(
            id,
            ProfileChanges {
                name: name.map(|n| n.to_string()),
                credential: validated,
            },
        )?;
        if restart {
            let _ = self.inner.manager.key_changed_async();
        }
        self.broadcast_profiles();
        Ok(())
    }

    /// Delete a profile; if it is active the agent switches to another or stops.
    pub fn remove_profile(&self, id: &str) -> Result<(), String> {
        let was_active = self.inner.profiles.active_id().as_deref() == Some(id);
        if was_active {
            confirm_if_busy(self.inner.manager.is_busy(), || {
                confirm_stop(
                    "Removing the profile in use restarts or stops the agent, and the work in progress is lost.",
                    "Remove profile",
                )
            })?;
        }
        self.inner.profiles.remove(id)?;
        if was_active {
            let next: Option<String> = self.inner.profiles.list().first().map(|p| p.id.clone());
            self.activate_profile(next.as_deref());
        } else {
            self.broadcast_profiles();
        }
        Ok(())
    }

    /// Use a profile in this app instance only, restarting the agent.
    pub fn set_active_profile(&self, id: &str) -> Result<(), String> {
        if self.inner.profiles.active_id().as_deref() == Some(id) {
            return Ok(());
        }
        if !self.inner.profiles.list().iter().any(|p| p.id == id) {
            return Err("That profile no longer exists.".to_string());
        }
        confirm_if_busy(self.inner.manager.is_busy(), || {
            confirm_stop(
                "Switching profile restarts the agent, and the work in progress is lost.",
                "Switch profile",
            )
        })?;
        self.activate_profile(Some(id));
        Ok(())
    }

    // ----- editor and settings -----

    /// Open a project file in the user's editor at `line` (the editor is started detached).
    pub fn open_in_editor(&self, project: &str, path: &str, line: u32) -> Result<(), String> {
        let no_editor = "No editor is set. Choose one in Settings.".to_string();
        let template = lock(&self.inner.settings).settings().editor_command;
        if template.trim().is_empty() {
            return Err(no_editor);
        }
        let file = match resolve_in_project(project, path) {
            Some(file) => file,
            None => return Err("That file is outside the project.".to_string()),
        };
        let line = if line > 0 { line } else { 1 };
        let built = match build_editor_command(&template, &file.to_string_lossy(), line, project) {
            Some(built) => built,
            None => return Err(no_editor),
        };
        let mut command = Command::new(&built.command);
        command
            .args(&built.args)
            .current_dir(project)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        match command.spawn() {
            Ok(mut child) => {
                // Reap it when it exits so it does not linger as a zombie
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                Ok(())
            }
            Err(err) => Err(format!("Could not run \"{}\": {}", built.command, err)),
        }
    }

    pub fn settings(&self) -> Settings {
        lock(&self.inner.settings).settings()
    }

    /// Validate and persist settings; returns the settings now in effect.
    pub fn set_settings(&self, settings: &Settings) -> Settings {
        let value = serde_json::to_value(settings).unwrap_or(serde_json::Value::Null);
        lock(&self.inner.settings).update(&value)
    }

    // ----- UI state -----

    fn ui_state_file(&self) -> PathBuf {
        self.inner.data_dir.join("ui_state.json")
    }

    /// The saved theme choice (`None` follows the OS).
    pub fn load_theme(&self) -> Option<Theme> {
        let text = fs::read_to_string(self.ui_state_file()).ok()?;
        parse_ui_state_theme(&text)
    }

    /// Remember the theme choice (`None` follows the OS).
    pub fn save_theme(&self, theme: Option<Theme>) {
        let text = ui_state_text(theme);
        if let Err(message) = write_file_atomic(&self.ui_state_file(), &text, false) {
            eprintln!("vettr: could not save the theme: {}", message);
        }
    }

    // ----- shutdown -----

    /// Stop watching and stop the agent's container (call when the app quits; it blocks).
    pub fn shutdown(&self) {
        self.inner.watcher.close();
        let _ = self.inner.manager.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn resource_roots_try_the_executable_then_the_repo_root_then_cwd_then_the_manifest() {
        let roots = resource_roots(
            Some(Path::new("/app/target/debug")),
            Some(Path::new("/work")),
            Path::new("/src/vettr"),
        );
        assert_eq!(roots[0], p("/app/target/debug"));
        assert_eq!(roots[2], p("/app/target/debug/../.."));
        assert_eq!(roots[roots.len() - 2], p("/work"));
        assert_eq!(roots[roots.len() - 1], p("/src/vettr"));
    }

    #[test]
    fn resource_roots_without_an_executable_dir_or_cwd_still_has_the_manifest_dir() {
        let roots = resource_roots(None, None, Path::new("/src/vettr"));
        assert_eq!(roots, vec![p("/src/vettr")]);
    }

    #[test]
    fn find_sandbox_paths_takes_the_first_root_with_both_the_dockerfile_and_runner() {
        let roots = vec![p("/a"), p("/b"), p("/c")];
        let is_file = |path: &Path| -> bool {
            path == Path::new("/a/sandbox/Dockerfile") || path == Path::new("/b/sandbox/Dockerfile")
        };
        let is_dir = |path: &Path| -> bool { path == Path::new("/b/runner") };
        let found = find_sandbox_paths(&roots, &is_file, &is_dir);
        assert_eq!(
            found,
            Some(SandboxPaths {
                dockerfile: p("/b/sandbox/Dockerfile"),
                runner_dir: p("/b/runner"),
            })
        );
    }

    #[test]
    fn find_sandbox_paths_is_none_when_nothing_matches() {
        let roots = vec![p("/a")];
        let never = |_: &Path| -> bool { false };
        assert_eq!(find_sandbox_paths(&roots, &never, &never), None);
    }

    #[test]
    fn resolve_in_project_keeps_files_inside_and_rejects_escapes() {
        assert_eq!(
            resolve_in_project("/repo", "src/a.rs"),
            Some(p("/repo/src/a.rs"))
        );
        assert_eq!(
            resolve_in_project("/repo", "src/../b.rs"),
            Some(p("/repo/b.rs"))
        );
        assert_eq!(resolve_in_project("/repo", "../other/a.rs"), None);
        assert_eq!(resolve_in_project("/repo", "/etc/passwd"), None);
        assert_eq!(
            resolve_in_project("/repo", "/repo/a.rs"),
            Some(p("/repo/a.rs"))
        );
    }

    #[test]
    fn the_theme_round_trips_through_ui_state_text() {
        assert_eq!(
            parse_ui_state_theme(&ui_state_text(Some(Theme::Dark))),
            Some(Theme::Dark)
        );
        assert_eq!(
            parse_ui_state_theme(&ui_state_text(Some(Theme::Light))),
            Some(Theme::Light)
        );
        assert_eq!(parse_ui_state_theme(&ui_state_text(None)), None);
        assert_eq!(parse_ui_state_theme("not json"), None);
        assert_eq!(parse_ui_state_theme("{\"theme\":\"blue\"}"), None);
    }

    #[test]
    fn data_dir_ends_with_vettr() {
        assert!(data_dir().ends_with("vettr"));
    }
}
