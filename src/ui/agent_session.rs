//! The agent session of the open project (port of `useAgentSession.ts`, `useSlashCommands.ts` and
//! `useSessionList.ts`).
//!
//! The model lives above the views, so switching to Changes and back keeps the transcript. All
//! blocking backend calls run on worker threads; they report back over a channel and the results
//! are applied on the UI thread in `update`, so the reducer is only ever touched by the UI thread.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use super::notifications::Notifier;
use crate::agent::{AgentEvent, SlashCommandInfo};
use crate::backend::Backend;
use crate::comments::{parse_review, ParsedReview, SentComment};
use crate::readiness::{ReadinessReason, ReadinessStatus};
use crate::replies::{replies_by_comment, AgentReply};
use crate::session::{SessionAction, SessionState, SessionStatus, TranscriptItem};
use crate::sessions::SessionInfo;
use crate::slash_commands::{complete_command, filter_commands, slash_query};
use crate::task::{fire_and_forget, Task};

/// Tool output longer than this is cut in the transcript.
pub const MAX_OUTPUT: usize = 5000;

// ----- pure helpers -----

/// The tool output as shown in the transcript: cut at `max` characters with a note saying how
/// much was left out.
pub fn truncate_output(output: &str, max: usize) -> String {
    let total = output.chars().count();
    if total <= max {
        return output.to_string();
    }
    let kept: String = output.chars().take(max).collect();
    format!("{}\n... ({} more characters)", kept, total - max)
}

/// `file:line` or `file:start-end`, where a sent comment points.
pub fn place_label(comment: &SentComment) -> String {
    if comment.start == comment.end {
        format!("{}:{}", comment.file, comment.start)
    } else {
        format!("{}:{}-{}", comment.file, comment.start, comment.end)
    }
}

/// What the transcript needs besides the items themselves; rebuilt only when the items change.
#[derive(Debug, Default, Clone)]
pub struct TranscriptCache {
    /// The agent's replies grouped by comment id.
    pub threads: HashMap<String, Vec<AgentReply>>,
    /// Where each comment sent so far points, so a reply in the flow says what it answers.
    pub places: HashMap<String, String>,
    /// For each user item (by index) that is a review with at least one comment, the review.
    pub reviews: HashMap<usize, ParsedReview>,
}

/// Build the [`TranscriptCache`] of a transcript.
pub fn build_cache(items: &[TranscriptItem]) -> TranscriptCache {
    let mut cache = TranscriptCache {
        threads: replies_by_comment(items),
        places: HashMap::new(),
        reviews: HashMap::new(),
    };
    for (index, item) in items.iter().enumerate() {
        if let TranscriptItem::User { text } = item {
            if let Some(review) = parse_review(text) {
                for c in review.comments.iter() {
                    cache.places.insert(c.id.clone(), place_label(c));
                }
                if !review.comments.is_empty() {
                    cache.reviews.insert(index, review);
                }
            }
        }
    }
    cache
}

/// When this key changes the stored session list is read again (the live session's status or ID).
pub fn list_key(state: &SessionState) -> String {
    format!(
        "{:?}:{}",
        state.status,
        state.session_id.clone().unwrap_or_default()
    )
}

/// How a message reaches the agent, by the state of the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    /// No session yet: the message is the first prompt.
    Start,
    /// The session ended without ever reporting an ID: begin a new one, then send.
    NewSessionThenSend,
    /// The shown session has no live agent: restart it, resuming this session ID.
    Resume(String),
    /// A live session: send a follow-up.
    Send,
}

/// Decide how to deliver a message (the logic of `sendReview`'s deliver step and `send`).
pub fn plan_delivery(
    status: SessionStatus,
    session_id: Option<&str>,
    resume_id: Option<&str>,
) -> Delivery {
    if status == SessionStatus::Idle {
        return Delivery::Start;
    }
    if status == SessionStatus::Ended && session_id.is_none() && resume_id.is_none() {
        return Delivery::NewSessionThenSend;
    }
    match resume_id {
        Some(id) => Delivery::Resume(id.to_string()),
        None => Delivery::Send,
    }
}

/// A text input with a slash command menu: the draft and the menu's navigation state.
#[derive(Debug, Clone)]
pub struct ComposerState {
    pub text: String,
    /// The highlighted menu entry (clamped to the number of matches when read).
    pub selected: usize,
    /// Escape closed the menu; typing opens it again.
    pub dismissed: bool,
    /// Put the keyboard focus in the input on the next frame.
    pub focus_pending: bool,
}

impl Default for ComposerState {
    fn default() -> ComposerState {
        ComposerState::new()
    }
}

impl ComposerState {
    pub fn new() -> ComposerState {
        ComposerState {
            text: String::new(),
            selected: 0,
            dismissed: false,
            focus_pending: true,
        }
    }

    /// The commands the menu offers for the current text (empty when it is closed).
    pub fn matches(&self, commands: &[SlashCommandInfo]) -> Vec<SlashCommandInfo> {
        if self.dismissed {
            return Vec::new();
        }
        match slash_query(&self.text) {
            Some(query) => filter_commands(commands, &query),
            None => Vec::new(),
        }
    }

    /// The highlighted entry among `len` matches.
    pub fn active(&self, len: usize) -> usize {
        if len == 0 {
            0
        } else {
            self.selected.min(len - 1)
        }
    }

    /// Move the highlight by `delta` (wrapping around) among `len` matches.
    pub fn step(&mut self, len: usize, delta: i32) {
        if len == 0 {
            return;
        }
        let current = self.active(len) as i64;
        let n = len as i64;
        self.selected = (((current + delta as i64) % n + n) % n) as usize;
    }

    /// Complete the text with `command`, ready for its arguments.
    pub fn choose(&mut self, command: &SlashCommandInfo) {
        self.text = complete_command(command);
        self.selected = 0;
    }

    /// Close the menu until the text changes.
    pub fn dismiss(&mut self) {
        self.dismissed = true;
    }

    /// The text was edited by the user.
    pub fn edited(&mut self) {
        self.selected = 0;
        self.dismissed = false;
    }

    /// The trimmed text to send, clearing the input; `None` (and the text kept) when sending is
    /// blocked or there is nothing to send.
    pub fn take_submit(&mut self, disabled: bool) -> Option<String> {
        if disabled || self.text.trim().is_empty() {
            return None;
        }
        let message = self.text.trim().to_string();
        self.text.clear();
        self.selected = 0;
        self.dismissed = false;
        Some(message)
    }

    /// Empty the input and close the menu.
    pub fn clear(&mut self) {
        self.text.clear();
        self.selected = 0;
        self.dismissed = false;
    }
}

// ----- the model -----

/// What worker threads report back to the UI thread.
enum Outcome {
    Sessions {
        seq: u64,
        list: Vec<SessionInfo>,
    },
    Commands {
        epoch: u64,
        list: Vec<SlashCommandInfo>,
    },
    Loaded {
        generation: u64,
        id: String,
        state: Option<SessionState>,
    },
    Action {
        generation: u64,
        action: SessionAction,
    },
    StartResult {
        generation: u64,
        error: Option<String>,
        restore: String,
    },
    SendResult {
        generation: u64,
        error: Option<String>,
        resumed: Option<String>,
    },
}

pub struct AgentSessionModel {
    ctx: egui::Context,
    backend: Backend,
    notifier: Notifier,
    tx: Sender<Outcome>,
    rx: Receiver<Outcome>,
    project: Option<String>,
    /// Bumped when the project changes, so late answers for an old project are dropped.
    generation: u64,
    state: SessionState,
    /// Bumped on every change of `state`, to know when the cache is stale.
    revision: u64,
    cache: RefCell<Option<(u64, Arc<TranscriptCache>)>>,
    sessions: Vec<SessionInfo>,
    list_seq: u64,
    list_key: String,
    commands: Vec<SlashCommandInfo>,
    /// Bumped when the agent reports or drops its commands, so older reads do not overwrite them.
    commands_epoch: u64,
    last_readiness: Option<(ReadinessStatus, Option<ReadinessReason>)>,
    /// How many stored sessions have been opened; changes when one replaces the transcript.
    loads: u64,
    loaded: bool,
    /// Set while the shown session has no live agent: a stored session or one whose agent exited.
    resume_id: Option<String>,
    /// The prompt input of the empty state.
    pub prompt: ComposerState,
    /// The follow-up input under the transcript.
    pub follow_up: ComposerState,
}

impl AgentSessionModel {
    pub fn new(ctx: &egui::Context, backend: &Backend, notifier: &Notifier) -> AgentSessionModel {
        let (tx, rx) = channel::<Outcome>();
        let mut model = AgentSessionModel {
            ctx: ctx.clone(),
            backend: backend.clone(),
            notifier: notifier.clone(),
            tx,
            rx,
            project: None,
            generation: 0,
            state: SessionState::default(),
            revision: 0,
            cache: RefCell::new(None),
            sessions: Vec::new(),
            list_seq: 0,
            list_key: String::new(),
            commands: Vec::new(),
            commands_epoch: 0,
            last_readiness: None,
            loads: 0,
            loaded: false,
            resume_id: None,
            prompt: ComposerState::new(),
            follow_up: ComposerState::new(),
        };
        model.list_key = list_key(&model.state);
        model.refresh_commands();
        model
    }

    pub fn state(&self) -> &SessionState {
        &self.state
    }

    pub fn sessions(&self) -> &[SessionInfo] {
        &self.sessions
    }

    pub fn slash_commands(&self) -> &[SlashCommandInfo] {
        &self.commands
    }

    /// How many stored sessions have been opened.
    pub fn loads(&self) -> u64 {
        self.loads
    }

    /// True once after a stored session was loaded, so the App can restore its sent comments.
    pub fn take_loaded(&mut self) -> bool {
        std::mem::replace(&mut self.loaded, false)
    }

    /// Transcript data derived from the items (rebuilt only when they changed).
    pub fn cache(&self) -> Arc<TranscriptCache> {
        let mut slot = self.cache.borrow_mut();
        if let Some((revision, cached)) = slot.as_ref() {
            if *revision == self.revision {
                return cached.clone();
            }
        }
        let built = Arc::new(build_cache(&self.state.items));
        *slot = Some((self.revision, built.clone()));
        built
    }

    /// The agent's replies by comment id for the current transcript.
    pub fn replies(&self) -> HashMap<String, Vec<AgentReply>> {
        self.cache().threads.clone()
    }

    /// Call when the open project changes (also fine to call every frame with the same value).
    pub fn set_project(&mut self, project: Option<&str>) {
        if self.project.as_deref() == project {
            return;
        }
        self.project = project.map(|p| p.to_string());
        self.generation += 1;
        self.apply_action(SessionAction::Reset);
        self.list_key = list_key(&self.state);
        self.refresh_sessions();
    }

    /// Pick up finished work and refresh what depends on the session. Call once per frame.
    pub fn update(&mut self) {
        loop {
            match self.rx.try_recv() {
                Ok(outcome) => self.apply_outcome(outcome),
                Err(_) => break,
            }
        }
        self.sync_resume();
        let key = list_key(&self.state);
        if key != self.list_key {
            self.list_key = key;
            self.refresh_sessions();
        }
        let readiness = self.backend.readiness();
        let seen = (readiness.status, readiness.reason);
        if self.last_readiness != Some(seen) {
            self.last_readiness = Some(seen);
            self.refresh_commands();
        }
    }

    /// Feed an agent event to the transcript (the App forwards `BackendEvent::Agent`).
    pub fn handle_agent_event(&mut self, event: &AgentEvent) {
        match event {
            AgentEvent::Commands { commands } => {
                self.commands = commands.clone();
                self.commands_epoch += 1;
            }
            AgentEvent::Exited { .. } => {
                self.commands.clear();
                self.commands_epoch += 1;
            }
            _ => {}
        }
        self.apply_action(SessionAction::Event {
            event: event.clone(),
        });
        self.sync_resume();
    }

    /// Stop the running session, if any, and return to the prompt.
    pub fn new_session(&mut self) {
        let backend = self.backend.clone();
        let tx = self.tx.clone();
        let generation = self.generation;
        fire_and_forget(&self.ctx, move || {
            let _ = backend.new_session();
            let _ = tx.send(Outcome::Action {
                generation,
                action: SessionAction::Reset,
            });
        });
    }

    /// Replace the current session with a stored one and rewrite the transcript. Blocked while
    /// the agent is working.
    pub fn open_session(&mut self, id: &str) {
        if self.state.status == SessionStatus::Running {
            return;
        }
        let backend = self.backend.clone();
        let tx = self.tx.clone();
        let generation = self.generation;
        let id = id.to_string();
        fire_and_forget(&self.ctx, move || {
            let state = backend.load_session(&id);
            let _ = tx.send(Outcome::Loaded {
                generation,
                id,
                state,
            });
        });
    }

    /// Stop the current turn without ending the session.
    pub fn interrupt(&mut self) {
        self.apply_action(SessionAction::InterruptRequested);
        let backend = self.backend.clone();
        fire_and_forget(&self.ctx, move || {
            let _ = backend.agent_interrupt();
        });
    }

    /// Start a session with `prompt` (the empty state's input). If it fails the prompt is put
    /// back in the input.
    pub fn start_prompt(&mut self, prompt: String) {
        let _ = self.deliver(prompt, true);
    }

    /// Send a follow-up (resuming the session first when it has no live agent).
    pub fn send_follow_up(&mut self, message: String) {
        let _ = self.deliver(message, false);
    }

    /// Hand a formatted review to the agent: the deliver step of the review round. The returned
    /// task reports the outcome (`Err` carries the message for the user); the transcript is
    /// updated by `update` when it finishes.
    pub fn submit_review(&mut self, message: String) -> Task<Result<(), String>> {
        self.deliver(message, false)
    }

    fn deliver(&mut self, text: String, restore_draft: bool) -> Task<Result<(), String>> {
        let plan = plan_delivery(
            self.state.status,
            self.state.session_id.as_deref(),
            self.resume_id.as_deref(),
        );
        let tx = self.tx.clone();
        let backend = self.backend.clone();
        let generation = self.generation;
        match plan {
            Delivery::Start => {
                let restore = if restore_draft {
                    text.clone()
                } else {
                    String::new()
                };
                self.apply_action(SessionAction::Sent { text: text.clone() });
                Task::spawn(&self.ctx, move || {
                    let result = backend.agent_start(&text, None);
                    let _ = tx.send(Outcome::StartResult {
                        generation,
                        error: result.clone().err(),
                        restore,
                    });
                    result
                })
            }
            Delivery::NewSessionThenSend => Task::spawn(&self.ctx, move || {
                let _ = backend.new_session();
                let _ = tx.send(Outcome::Action {
                    generation,
                    action: SessionAction::Reset,
                });
                let _ = tx.send(Outcome::Action {
                    generation,
                    action: SessionAction::Sent { text: text.clone() },
                });
                let result = backend.agent_send(&text);
                let _ = tx.send(Outcome::SendResult {
                    generation,
                    error: result.clone().err(),
                    resumed: None,
                });
                result
            }),
            Delivery::Resume(id) => {
                self.apply_action(SessionAction::Resumed { text: text.clone() });
                Task::spawn(&self.ctx, move || {
                    // The backend restarts the agent to resume, so no stop is needed first
                    let result = backend.agent_start(&text, Some(id.as_str()));
                    let _ = tx.send(Outcome::SendResult {
                        generation,
                        error: result.clone().err(),
                        resumed: Some(id),
                    });
                    result
                })
            }
            Delivery::Send => {
                self.apply_action(SessionAction::Sent { text: text.clone() });
                Task::spawn(&self.ctx, move || {
                    let result = backend.agent_send(&text);
                    let _ = tx.send(Outcome::SendResult {
                        generation,
                        error: result.clone().err(),
                        resumed: None,
                    });
                    result
                })
            }
        }
    }

    /// Apply a reducer action and the side effects the views depend on.
    fn apply_action(&mut self, action: SessionAction) {
        let is_reset = matches!(action, SessionAction::Reset);
        let is_load = matches!(action, SessionAction::Load { .. });
        let is_start_failed = matches!(action, SessionAction::StartFailed { .. });
        self.state.apply(action);
        self.revision += 1;
        if is_reset {
            self.resume_id = None;
            self.prompt.clear();
            self.prompt.focus_pending = true;
        }
        if is_reset || is_load {
            self.follow_up.clear();
            self.follow_up.focus_pending = true;
        }
        if is_start_failed {
            self.prompt.text = self.state.draft.clone();
            self.prompt.focus_pending = true;
        }
    }

    /// Remember the session to resume once its agent is gone.
    fn sync_resume(&mut self) {
        if self.state.status == SessionStatus::Ended {
            if let Some(id) = self.state.session_id.clone() {
                self.resume_id = Some(id);
            }
        }
    }

    fn apply_outcome(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Sessions { seq, list } => {
                if seq == self.list_seq {
                    self.sessions = list;
                }
            }
            Outcome::Commands { epoch, list } => {
                if epoch == self.commands_epoch {
                    self.commands = list;
                }
            }
            Outcome::Loaded {
                generation,
                id,
                state,
            } => {
                if generation != self.generation {
                    return;
                }
                match state {
                    Some(loaded) => {
                        self.resume_id = Some(loaded.session_id.clone().unwrap_or(id));
                        self.apply_action(SessionAction::Load { state: loaded });
                        self.loads += 1;
                        self.loaded = true;
                    }
                    None => {
                        self.notifier.notify(
                            "Could not open the session",
                            "Its transcript could not be read.",
                        );
                    }
                }
            }
            Outcome::Action { generation, action } => {
                if generation == self.generation {
                    self.apply_action(action);
                }
            }
            Outcome::StartResult {
                generation,
                error,
                restore,
            } => {
                if generation != self.generation {
                    return;
                }
                if let Some(message) = error {
                    self.apply_action(SessionAction::StartFailed {
                        message,
                        prompt: restore,
                    });
                }
            }
            Outcome::SendResult {
                generation,
                error,
                resumed,
            } => {
                if generation != self.generation {
                    return;
                }
                match error {
                    Some(message) => self.apply_action(SessionAction::SendFailed { message }),
                    None => {
                        if let Some(id) = resumed {
                            if self.resume_id.as_deref() == Some(id.as_str()) {
                                self.resume_id = None;
                            }
                        }
                    }
                }
            }
        }
    }

    fn refresh_sessions(&mut self) {
        self.list_seq += 1;
        if self.project.is_none() {
            self.sessions.clear();
            return;
        }
        let seq = self.list_seq;
        let backend = self.backend.clone();
        let tx = self.tx.clone();
        fire_and_forget(&self.ctx, move || {
            let list = backend.list_sessions();
            let _ = tx.send(Outcome::Sessions { seq, list });
        });
    }

    fn refresh_commands(&mut self) {
        let epoch = self.commands_epoch;
        let backend = self.backend.clone();
        let tx = self.tx.clone();
        fire_and_forget(&self.ctx, move || {
            let list = backend.slash_commands();
            let _ = tx.send(Outcome::Commands { epoch, list });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comments::{format_review, ReviewComment, Side};
    use serde_json::json;

    fn cmd(name: &str) -> SlashCommandInfo {
        SlashCommandInfo {
            name: name.to_string(),
            description: String::new(),
            argument_hint: String::new(),
            aliases: None,
        }
    }

    fn review_text(id: &str, start: u32, end: u32) -> String {
        let comment = ReviewComment {
            id: id.to_string(),
            file: "a.rs".to_string(),
            staged: false,
            side: Side::New,
            start,
            end,
            snapshot: vec![],
            text: "fix".to_string(),
            round: 1,
            sent: true,
            outdated: false,
        };
        format_review(&[comment], 1)
    }

    #[test]
    fn truncates_long_output_and_says_how_much_is_missing() {
        assert_eq!(truncate_output("short", 5000), "short");
        let long = "x".repeat(5003);
        let cut = truncate_output(&long, 5000);
        assert!(cut.starts_with(&"x".repeat(5000)));
        assert!(cut.ends_with("(3 more characters)"));
        assert_eq!(truncate_output("abc", 3), "abc");
    }

    #[test]
    fn truncation_counts_characters_not_bytes() {
        let text = "\u{e9}".repeat(10);
        assert_eq!(truncate_output(&text, 10), text);
        assert!(truncate_output(&text, 4).contains("(6 more characters)"));
    }

    #[test]
    fn the_cache_knows_reviews_places_and_replies() {
        let items = vec![
            TranscriptItem::User {
                text: "hello".into(),
            },
            TranscriptItem::User {
                text: review_text("c1", 3, 5),
            },
            TranscriptItem::Tool {
                id: "t".into(),
                name: crate::comments::REPLY_TOOL_NAME.into(),
                input: json!({"comment_id": "c1", "message": "done"}),
                status: crate::session::ToolStatus::Done,
                output: String::new(),
            },
        ];
        let cache = build_cache(&items);
        assert_eq!(cache.reviews.len(), 1);
        assert!(cache.reviews.contains_key(&1));
        assert_eq!(cache.places.get("c1").map(|s| s.as_str()), Some("a.rs:3-5"));
        assert_eq!(cache.threads["c1"].len(), 1);
    }

    #[test]
    fn a_review_block_without_comments_is_shown_as_a_plain_message() {
        let items = vec![TranscriptItem::User {
            text: "<vettr-review round=\"1\"></vettr-review>".into(),
        }];
        assert!(build_cache(&items).reviews.is_empty());
    }

    #[test]
    fn place_label_collapses_single_lines() {
        let found = crate::comments::parse_review(&review_text("c", 7, 7)).unwrap();
        assert_eq!(place_label(&found.comments[0]), "a.rs:7");
    }

    #[test]
    fn the_list_key_changes_with_status_and_session_id() {
        let mut state = SessionState::default();
        let idle = list_key(&state);
        state.status = SessionStatus::Running;
        let running = list_key(&state);
        state.session_id = Some("s1".into());
        let with_id = list_key(&state);
        assert_ne!(idle, running);
        assert_ne!(running, with_id);
    }

    #[test]
    fn delivery_follows_the_state_of_the_session() {
        assert_eq!(
            plan_delivery(SessionStatus::Idle, None, None),
            Delivery::Start
        );
        assert_eq!(
            plan_delivery(SessionStatus::Ended, None, None),
            Delivery::NewSessionThenSend
        );
        assert_eq!(
            plan_delivery(SessionStatus::Ended, Some("s"), Some("s")),
            Delivery::Resume("s".to_string())
        );
        assert_eq!(
            plan_delivery(SessionStatus::Waiting, Some("s"), None),
            Delivery::Send
        );
        assert_eq!(
            plan_delivery(SessionStatus::Waiting, Some("s"), Some("s")),
            Delivery::Resume("s".to_string())
        );
    }

    #[test]
    fn submit_trims_clears_and_respects_the_block() {
        let mut c = ComposerState::new();
        c.text = "  hello \n".into();
        assert_eq!(c.take_submit(true), None);
        assert_eq!(c.text, "  hello \n");
        assert_eq!(c.take_submit(false), Some("hello".to_string()));
        assert_eq!(c.text, "");
        c.text = "   ".into();
        assert_eq!(c.take_submit(false), None);
    }

    #[test]
    fn the_menu_opens_for_a_slash_word_and_closes_when_dismissed() {
        let commands = vec![cmd("review"), cmd("init")];
        let mut c = ComposerState::new();
        c.text = "/re".into();
        let found = c.matches(&commands);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "review");
        c.dismiss();
        assert!(c.matches(&commands).is_empty());
        c.edited();
        assert_eq!(c.matches(&commands).len(), 1);
        c.text = "/review now".into();
        assert!(c.matches(&commands).is_empty());
        c.text = "plain".into();
        assert!(c.matches(&commands).is_empty());
    }

    #[test]
    fn arrow_navigation_wraps_in_both_directions() {
        let mut c = ComposerState::new();
        c.step(3, 1);
        assert_eq!(c.selected, 1);
        c.step(3, 1);
        c.step(3, 1);
        assert_eq!(c.selected, 0);
        c.step(3, -1);
        assert_eq!(c.selected, 2);
        c.step(0, 1);
        assert_eq!(c.selected, 2);
        assert_eq!(c.active(2), 1);
        assert_eq!(c.active(0), 0);
    }

    #[test]
    fn choosing_a_command_completes_it_for_arguments() {
        let mut c = ComposerState::new();
        c.text = "/re".into();
        c.selected = 1;
        c.choose(&cmd("review"));
        assert_eq!(c.text, "/review ");
        assert_eq!(c.selected, 0);
        assert!(c.matches(&[cmd("review")]).is_empty());
    }
}
