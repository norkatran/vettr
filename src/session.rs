//! Session view state and its pure reducer over agent events (port of `src/shared/session.ts`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::AgentEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolStatus {
    Running,
    Done,
    Error,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TranscriptItem {
    User {
        text: String,
    },
    Text {
        text: String,
    },
    Tool {
        id: String,
        name: String,
        input: Value,
        status: ToolStatus,
        output: String,
    },
    Edit {
        path: String,
    },
    Error {
        message: String,
    },
    Notice {
        text: String,
    },
}

/// - idle: no session yet, the prompt is shown
/// - running: a turn is in progress
/// - waiting: the turn finished and a follow-up can be sent
/// - ended: the agent process is gone
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Idle,
    Running,
    Waiting,
    Ended,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionState {
    pub status: SessionStatus,
    pub items: Vec<TranscriptItem>,
    /// An interrupt was requested and the turn has not finished yet.
    pub interrupting: bool,
    /// The prompt text to put back in the input after a failed start.
    pub draft: String,
    pub start_error: Option<String>,
    /// The SDK session ID once the agent reports it; used to resume the session later.
    pub session_id: Option<String>,
}

impl Default for SessionState {
    fn default() -> Self {
        initial_session()
    }
}

/// The state before any session exists (`initialSession` in the TypeScript).
pub fn initial_session() -> SessionState {
    SessionState {
        status: SessionStatus::Idle,
        items: Vec::new(),
        interrupting: false,
        draft: String::new(),
        start_error: None,
        session_id: None,
    }
}

impl SessionState {
    /// Apply `action` in place (convenience over `session_reducer`).
    pub fn apply(&mut self, action: SessionAction) {
        let current = std::mem::take(self);
        *self = session_reducer(current, action);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum SessionAction {
    Sent { text: String },
    Resumed { text: String },
    StartFailed { message: String, prompt: String },
    SendFailed { message: String },
    InterruptRequested,
    Event { event: AgentEvent },
    Reset,
    Load { state: SessionState },
}

/// Tools still marked running when a turn ends or the agent exits will never report back.
fn stop_running(items: Vec<TranscriptItem>) -> Vec<TranscriptItem> {
    items
        .into_iter()
        .map(|item| match item {
            TranscriptItem::Tool {
                id,
                name,
                input,
                status: ToolStatus::Running,
                output,
            } => TranscriptItem::Tool {
                id,
                name,
                input,
                status: ToolStatus::Stopped,
                output,
            },
            other => other,
        })
        .collect()
}

fn apply_event(mut state: SessionState, event: AgentEvent) -> SessionState {
    match event {
        AgentEvent::SessionStarted { session_id } => {
            state.session_id = Some(session_id);
            state
        }
        AgentEvent::Commands { .. } => state,
        AgentEvent::Text { text } => {
            state.items.push(TranscriptItem::Text { text });
            state
        }
        AgentEvent::ToolStarted { id, name, input } => {
            state.items.push(TranscriptItem::Tool {
                id,
                name,
                input,
                status: ToolStatus::Running,
                output: String::new(),
            });
            state
        }
        AgentEvent::ToolFinished {
            id,
            output,
            is_error,
        } => {
            for item in state.items.iter_mut() {
                if let TranscriptItem::Tool {
                    id: tool_id,
                    status,
                    output: tool_output,
                    ..
                } = item
                {
                    if *tool_id == id {
                        *status = if is_error {
                            ToolStatus::Error
                        } else {
                            ToolStatus::Done
                        };
                        *tool_output = output.clone();
                    }
                }
            }
            state
        }
        AgentEvent::FileEdited { path } => {
            state.items.push(TranscriptItem::Edit { path });
            state
        }
        AgentEvent::TurnFinished => {
            let interrupted = state.interrupting;
            let items = std::mem::take(&mut state.items);
            let mut stopped = stop_running(items);
            if interrupted {
                stopped.push(TranscriptItem::Notice {
                    text: "Interrupted".to_string(),
                });
            }
            state.status = SessionStatus::Waiting;
            state.interrupting = false;
            state.items = stopped;
            state
        }
        AgentEvent::Error { message } => {
            // Stopping a turn makes the SDK report it as an error; the user asked for that
            if !state.interrupting {
                state.items.push(TranscriptItem::Error { message });
            }
            state
        }
        AgentEvent::Exited { .. } => {
            let items = std::mem::take(&mut state.items);
            state.status = SessionStatus::Ended;
            state.interrupting = false;
            state.items = stop_running(items);
            state
        }
    }
}

/// The pure reducer: the next state after `action`.
pub fn session_reducer(state: SessionState, action: SessionAction) -> SessionState {
    let mut state = state;
    match action {
        SessionAction::Sent { text } => {
            state.status = SessionStatus::Running;
            state.draft = String::new();
            state.start_error = None;
            state.items.push(TranscriptItem::User { text });
            state
        }
        SessionAction::Resumed { text } => {
            state.status = SessionStatus::Running;
            state.draft = String::new();
            state.start_error = None;
            state.items.push(TranscriptItem::Notice {
                text: "Session resumed".to_string(),
            });
            state.items.push(TranscriptItem::User { text });
            state
        }
        SessionAction::StartFailed { message, prompt } => {
            let mut next = initial_session();
            next.draft = prompt;
            next.start_error = Some(message);
            next
        }
        SessionAction::SendFailed { message } => {
            state.status = SessionStatus::Waiting;
            state.items.push(TranscriptItem::Error { message });
            state
        }
        SessionAction::InterruptRequested => {
            state.interrupting = true;
            state
        }
        SessionAction::Event { event } => apply_event(state, event),
        SessionAction::Reset => initial_session(),
        SessionAction::Load { state: loaded } => loaded,
    }
}

const SUMMARY_FIELDS: [&str; 8] = [
    "command",
    "file_path",
    "notebook_path",
    "pattern",
    "path",
    "url",
    "query",
    "description",
];

fn first_line(text: &str, max: usize) -> String {
    let line = text.split('\n').next().unwrap_or("");
    if line.chars().count() > max {
        let cut: String = line.chars().take(max).collect();
        format!("{}…", cut)
    } else {
        line.to_string()
    }
}

/// A one-line summary of a tool call's input, for the collapsed transcript row.
pub fn describe_tool(input: &Value) -> String {
    match input {
        Value::Object(map) => {
            for field in SUMMARY_FIELDS.iter() {
                if let Some(Value::String(value)) = map.get(*field) {
                    return first_line(value, 100);
                }
            }
            first_line(&input.to_string(), 100)
        }
        Value::Array(_) => first_line(&input.to_string(), 100),
        Value::String(text) => first_line(text, 100),
        other => first_line(&other.to_string(), 100),
    }
}

/// `path` relative to `project` when it is inside it (the container uses host paths).
pub fn relative_path(project: &str, path: &str) -> String {
    if path == project {
        return ".".to_string();
    }
    let prefix = format!("{}/", project);
    match path.strip_prefix(prefix.as_str()) {
        Some(rest) => rest.to_string(),
        None => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(state: SessionState, actions: Vec<SessionAction>) -> SessionState {
        actions.into_iter().fold(state, session_reducer)
    }

    fn events(state: SessionState, list: Vec<AgentEvent>) -> SessionState {
        run(
            state,
            list.into_iter()
                .map(|event| SessionAction::Event { event })
                .collect(),
        )
    }

    fn event(e: AgentEvent) -> SessionAction {
        SessionAction::Event { event: e }
    }

    fn running() -> SessionState {
        run(
            initial_session(),
            vec![SessionAction::Sent { text: "go".into() }],
        )
    }

    fn started() -> AgentEvent {
        AgentEvent::ToolStarted {
            id: "t1".into(),
            name: "Bash".into(),
            input: json!({"command": "ls"}),
        }
    }

    fn kinds(state: &SessionState) -> Vec<&'static str> {
        state
            .items
            .iter()
            .map(|i| match i {
                TranscriptItem::User { .. } => "user",
                TranscriptItem::Text { .. } => "text",
                TranscriptItem::Tool { .. } => "tool",
                TranscriptItem::Edit { .. } => "edit",
                TranscriptItem::Error { .. } => "error",
                TranscriptItem::Notice { .. } => "notice",
            })
            .collect()
    }

    fn tool_status(item: &TranscriptItem) -> ToolStatus {
        match item {
            TranscriptItem::Tool { status, .. } => *status,
            _ => panic!("not a tool item"),
        }
    }

    #[test]
    fn starts_idle_and_empty() {
        let s = initial_session();
        assert_eq!(s.status, SessionStatus::Idle);
        assert!(s.items.is_empty());
        assert_eq!(s.start_error, None);
    }

    #[test]
    fn adds_the_user_message_and_runs_when_something_is_sent() {
        let state = running();
        assert_eq!(state.status, SessionStatus::Running);
        assert_eq!(
            state.items,
            vec![TranscriptItem::User { text: "go".into() }]
        );
    }

    #[test]
    fn appends_assistant_text() {
        let state = events(
            running(),
            vec![AgentEvent::Text {
                text: "hello".into(),
            }],
        );
        assert_eq!(
            state.items.last(),
            Some(&TranscriptItem::Text {
                text: "hello".into()
            })
        );
    }

    #[test]
    fn tracks_a_tool_call_from_started_to_finished() {
        let open = events(running(), vec![started()]);
        match open.items.last().unwrap() {
            TranscriptItem::Tool { status, output, .. } => {
                assert_eq!(*status, ToolStatus::Running);
                assert_eq!(output, "");
            }
            _ => panic!("expected tool"),
        }
        let done = events(
            open,
            vec![AgentEvent::ToolFinished {
                id: "t1".into(),
                output: "a b".into(),
                is_error: false,
            }],
        );
        match done.items.last().unwrap() {
            TranscriptItem::Tool { status, output, .. } => {
                assert_eq!(*status, ToolStatus::Done);
                assert_eq!(output, "a b");
            }
            _ => panic!("expected tool"),
        }
    }

    #[test]
    fn marks_a_failed_tool_call_as_an_error_and_leaves_other_items_alone() {
        let state = events(
            running(),
            vec![
                started(),
                AgentEvent::Text {
                    text: "between".into(),
                },
                AgentEvent::ToolFinished {
                    id: "t1".into(),
                    output: "nope".into(),
                    is_error: true,
                },
            ],
        );
        assert_eq!(kinds(&state), vec!["user", "tool", "text"]);
        match &state.items[1] {
            TranscriptItem::Tool { status, output, .. } => {
                assert_eq!(*status, ToolStatus::Error);
                assert_eq!(output, "nope");
            }
            _ => panic!("expected tool"),
        }
    }

    #[test]
    fn ignores_a_result_for_an_unknown_tool() {
        let before = running();
        let state = events(
            before.clone(),
            vec![AgentEvent::ToolFinished {
                id: "zzz".into(),
                output: String::new(),
                is_error: false,
            }],
        );
        assert_eq!(state.items, before.items);
    }

    #[test]
    fn records_file_edits() {
        let state = events(
            running(),
            vec![AgentEvent::FileEdited {
                path: "/p/a.ts".into(),
            }],
        );
        assert_eq!(
            state.items.last(),
            Some(&TranscriptItem::Edit {
                path: "/p/a.ts".into()
            })
        );
    }

    #[test]
    fn waits_for_a_follow_up_when_the_turn_finishes_and_stops_tools_still_running() {
        let state = events(running(), vec![started(), AgentEvent::TurnFinished]);
        assert_eq!(state.status, SessionStatus::Waiting);
        assert_eq!(
            tool_status(state.items.last().unwrap()),
            ToolStatus::Stopped
        );
    }

    #[test]
    fn shows_errors() {
        let state = events(
            running(),
            vec![AgentEvent::Error {
                message: "bad key".into(),
            }],
        );
        assert_eq!(
            state.items.last(),
            Some(&TranscriptItem::Error {
                message: "bad key".into()
            })
        );
    }

    #[test]
    fn treats_the_error_from_an_interrupted_turn_as_expected_and_says_so_instead() {
        let state = run(
            running(),
            vec![
                SessionAction::InterruptRequested,
                event(AgentEvent::Error {
                    message: "error_during_execution".into(),
                }),
                event(AgentEvent::TurnFinished),
            ],
        );
        assert!(!state.interrupting);
        assert_eq!(state.status, SessionStatus::Waiting);
        assert_eq!(
            state.items,
            vec![
                TranscriptItem::User { text: "go".into() },
                TranscriptItem::Notice {
                    text: "Interrupted".into()
                }
            ]
        );
    }

    #[test]
    fn ends_the_session_when_the_agent_exits_and_clears_a_pending_interrupt() {
        let state = run(
            running(),
            vec![
                SessionAction::InterruptRequested,
                event(started()),
                event(AgentEvent::Exited { code: Some(1) }),
            ],
        );
        assert_eq!(state.status, SessionStatus::Ended);
        assert!(!state.interrupting);
        assert_eq!(
            tool_status(state.items.last().unwrap()),
            ToolStatus::Stopped
        );
    }

    #[test]
    fn returns_to_the_prompt_with_the_draft_and_error_when_a_start_fails() {
        let state = run(
            running(),
            vec![SessionAction::StartFailed {
                message: "No Docker".into(),
                prompt: "go".into(),
            }],
        );
        let mut expected = initial_session();
        expected.draft = "go".into();
        expected.start_error = Some("No Docker".into());
        assert_eq!(state, expected);
    }

    #[test]
    fn clears_the_start_error_and_draft_on_the_next_send() {
        let failed = run(
            running(),
            vec![SessionAction::StartFailed {
                message: "x".into(),
                prompt: "go".into(),
            }],
        );
        let state = run(failed, vec![SessionAction::Sent { text: "go".into() }]);
        assert_eq!(state.draft, "");
        assert_eq!(state.start_error, None);
    }

    #[test]
    fn shows_a_failed_follow_up_as_an_error_and_lets_the_user_try_again() {
        let state = run(
            running(),
            vec![SessionAction::SendFailed {
                message: "No session is running".into(),
            }],
        );
        assert_eq!(state.status, SessionStatus::Waiting);
        assert_eq!(
            state.items.last(),
            Some(&TranscriptItem::Error {
                message: "No session is running".into()
            })
        );
    }

    #[test]
    fn resets_to_the_initial_state() {
        assert_eq!(
            run(running(), vec![SessionAction::Reset]),
            initial_session()
        );
    }

    #[test]
    fn apply_updates_in_place() {
        let mut state = initial_session();
        state.apply(SessionAction::Sent { text: "go".into() });
        assert_eq!(state, running());
    }

    #[test]
    fn resuming_adds_a_notice_and_the_message_and_keeps_the_session_id() {
        let mut ended = initial_session();
        ended.status = SessionStatus::Ended;
        ended.session_id = Some("s1".into());
        let state = session_reducer(
            ended,
            SessionAction::Resumed {
                text: "go on".into(),
            },
        );
        assert_eq!(state.status, SessionStatus::Running);
        assert_eq!(state.session_id, Some("s1".to_string()));
        assert_eq!(
            state.items,
            vec![
                TranscriptItem::Notice {
                    text: "Session resumed".into()
                },
                TranscriptItem::User {
                    text: "go on".into()
                }
            ]
        );
    }

    #[test]
    fn describe_tool_summarises_by_the_most_telling_input_field() {
        assert_eq!(
            describe_tool(&json!({"command": "npm test", "description": "run"})),
            "npm test"
        );
        assert_eq!(
            describe_tool(&json!({"file_path": "/p/a.ts", "old_string": "x"})),
            "/p/a.ts"
        );
        assert_eq!(describe_tool(&json!({"pattern": "TODO"})), "TODO");
    }

    #[test]
    fn describe_tool_uses_only_the_first_line_and_truncates_long_ones() {
        assert_eq!(describe_tool(&json!({"command": "a\nb"})), "a");
        let long = "x".repeat(150);
        assert_eq!(
            describe_tool(&json!({ "command": long })),
            format!("{}…", "x".repeat(100))
        );
    }

    #[test]
    fn describe_tool_falls_back_to_the_json_of_an_input_with_no_known_field() {
        // serde_json sorts object keys (no `preserve_order`), unlike JSON.stringify in the TS
        assert_eq!(
            describe_tool(&json!({"foo": 1, "command": 5})),
            "{\"command\":5,\"foo\":1}"
        );
    }

    #[test]
    fn describe_tool_handles_inputs_that_are_not_objects() {
        assert_eq!(describe_tool(&Value::Null), "null");
        assert_eq!(describe_tool(&json!("raw")), "raw");
    }

    #[test]
    fn relative_path_strips_the_project_prefix() {
        assert_eq!(relative_path("/p", "/p/src/a.ts"), "src/a.ts");
    }

    #[test]
    fn relative_path_names_the_project_itself() {
        assert_eq!(relative_path("/p", "/p"), ".");
    }

    #[test]
    fn relative_path_leaves_paths_outside_the_project_including_look_alike_prefixes() {
        assert_eq!(relative_path("/p", "/other/a.ts"), "/other/a.ts");
        assert_eq!(relative_path("/p", "/p2/a.ts"), "/p2/a.ts");
    }

    #[test]
    fn session_started_stores_the_sdk_session_id() {
        let state = session_reducer(
            initial_session(),
            event(AgentEvent::SessionStarted {
                session_id: "s1".into(),
            }),
        );
        assert_eq!(state.session_id, Some("s1".to_string()));
    }

    #[test]
    fn load_replaces_the_whole_state() {
        let mut loaded = initial_session();
        loaded.draft = "restored".into();
        assert_eq!(
            run(
                initial_session(),
                vec![SessionAction::Load {
                    state: loaded.clone()
                }]
            ),
            loaded
        );
    }

    #[test]
    fn commands_leave_the_session_unchanged() {
        let state = initial_session();
        let next = session_reducer(
            state.clone(),
            event(AgentEvent::Commands { commands: vec![] }),
        );
        assert_eq!(next, state);
    }

    #[test]
    fn state_serialises_with_camel_case_fields() {
        let v = serde_json::to_value(running()).unwrap();
        assert_eq!(v["status"], "running");
        assert_eq!(v["interrupting"], false);
        assert!(v["startError"].is_null());
        assert_eq!(v["items"][0], json!({"kind": "user", "text": "go"}));
        let back: SessionState = serde_json::from_value(v).unwrap();
        assert_eq!(back, running());
    }
}
