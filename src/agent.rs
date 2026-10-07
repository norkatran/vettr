//! Agent events, the runner protocol and the adapter trait (port of `src/shared/agent.ts`).
//!
//! The JSON produced and accepted here must match the TypeScript runner (`runner/`) exactly.

use serde::{Deserialize, Serialize};

/// A slash command the agent offers (a built-in, skill, project or plugin command).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlashCommandInfo {
    /// Without the leading slash.
    pub name: String,
    pub description: String,
    /// Hint for the arguments, for example `<file>`; empty when it takes none.
    #[serde(default)]
    pub argument_hint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aliases: Option<Vec<String>>,
}

/// Events an agent session emits; the UI depends only on these, never on a specific agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum AgentEvent {
    /// The SDK session ID, reported once per session; it is what `resume` takes later.
    #[serde(rename_all = "camelCase")]
    SessionStarted {
        session_id: String,
    },
    /// The full list of slash commands now available; replaces any earlier list.
    Commands {
        commands: Vec<SlashCommandInfo>,
    },
    Text {
        text: String,
    },
    ToolStarted {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename_all = "camelCase")]
    ToolFinished {
        id: String,
        output: String,
        is_error: bool,
    },
    FileEdited {
        path: String,
    },
    TurnFinished,
    Error {
        message: String,
    },
    /// The agent process went away; `code` is `None` when it was killed by a signal.
    Exited {
        code: Option<i32>,
    },
}

/// Messages the host writes to the runner's stdin, one JSON object per line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum RunnerCommand {
    /// Always first. The credential (an API key or an OAuth token) travels over stdin so it never
    /// shows up in `docker inspect`.
    Init {
        credential: String,
        cwd: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resume: Option<String>,
    },
    Prompt {
        text: String,
    },
    Interrupt,
}

/// Serialise a runner command as one protocol line (JSON followed by a newline).
pub fn encode_command(command: &RunnerCommand) -> String {
    // Serialising these plain types cannot fail.
    format!("{}\n", serde_json::to_string(command).unwrap_or_default())
}

/// Serialise an event as one protocol line (JSON followed by a newline).
pub fn encode_event(event: &AgentEvent) -> String {
    format!("{}\n", serde_json::to_string(event).unwrap_or_default())
}

/// Parse one line from the runner into an event, or `None` if it is not a well-formed event.
pub fn parse_event_line(line: &str) -> Option<AgentEvent> {
    serde_json::from_str::<AgentEvent>(line).ok()
}

/// Parse one line from the host into a command, or `None` if it is not a well-formed command.
/// An empty `resume` counts as absent.
pub fn parse_command_line(line: &str) -> Option<RunnerCommand> {
    match serde_json::from_str::<RunnerCommand>(line).ok()? {
        RunnerCommand::Init {
            credential,
            cwd,
            resume,
        } => Some(RunnerCommand::Init {
            credential,
            cwd,
            resume: resume.filter(|r| !r.is_empty()),
        }),
        other => Some(other),
    }
}

/// Reassembles protocol lines from stream chunks, which can split or merge lines anywhere.
/// Blank lines are skipped; a trailing partial line waits for its newline.
#[derive(Debug, Default)]
pub struct LineBuffer {
    pending: String,
}

impl LineBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, chunk: &str) -> Vec<String> {
        self.pending.push_str(chunk);
        let mut lines: Vec<String> = self.pending.split('\n').map(|s| s.to_string()).collect();
        // split always yields at least one part
        self.pending = lines.pop().unwrap_or_default();
        lines.into_iter().filter(|l| !l.trim().is_empty()).collect()
    }
}

/// Listener for agent events. Called from whatever thread produced the event.
pub type EventListener = Box<dyn Fn(AgentEvent) + Send + Sync>;

/// Adapter between the app and a concrete agent (the first one wraps the Claude Agent SDK).
/// All methods block; callers run them on worker threads.
pub trait AgentAdapter: Send + Sync {
    /// Prewarm an idle agent for `cwd` (optionally resuming `resume`) so `start` is fast.
    fn warm(&self, cwd: &str, resume: Option<&str>) -> Result<(), String>;
    /// Begin a session in `cwd` with the first prompt (using the warm agent when one matches),
    /// optionally resuming SDK session `resume`.
    fn start(&self, prompt: &str, cwd: &str, resume: Option<&str>) -> Result<(), String>;
    /// Send a follow-up in the same session, including batched review comments.
    fn send(&self, message: &str) -> Result<(), String>;
    /// Stop the current turn without ending the session.
    fn interrupt(&self) -> Result<(), String>;
    /// Subscribe to events; returns a subscription id for `off_event`.
    fn on_event(&self, listener: EventListener) -> u64;
    /// Remove a subscription made with `on_event`.
    fn off_event(&self, subscription: u64);
    /// Kept for later: the MVP sandbox grants full permissions, so nothing requests approval yet.
    fn respond_to_approval(&self, id: &str, allow: bool) -> Result<(), String>;
    /// End the session and release its resources (the container).
    fn stop(&self) -> Result<(), String>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn encodes_events_with_the_typescript_field_names() {
        let line = encode_event(&AgentEvent::ToolFinished {
            id: "t1".into(),
            output: "ok".into(),
            is_error: false,
        });
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&line).unwrap(),
            json!({"type":"tool-finished","id":"t1","output":"ok","isError":false})
        );
        assert!(line.ends_with('\n'));
        let line = encode_event(&AgentEvent::SessionStarted {
            session_id: "s".into(),
        });
        assert!(line.contains("\"sessionId\":\"s\""));
        assert!(line.contains("\"type\":\"session-started\""));
    }

    #[test]
    fn parses_events_and_drops_malformed_lines() {
        assert_eq!(
            parse_event_line(r#"{"type":"text","text":"hi"}"#),
            Some(AgentEvent::Text { text: "hi".into() })
        );
        assert_eq!(
            parse_event_line(r#"{"type":"exited","code":null}"#),
            Some(AgentEvent::Exited { code: None })
        );
        assert_eq!(
            parse_event_line(r#"{"type":"turn-finished"}"#),
            Some(AgentEvent::TurnFinished)
        );
        assert_eq!(parse_event_line("not json"), None);
        assert_eq!(parse_event_line(r#"{"type":"bogus"}"#), None);
        assert_eq!(parse_event_line("[1]"), None);
    }

    #[test]
    fn parses_commands() {
        assert_eq!(
            parse_command_line(r#"{"type":"interrupt"}"#),
            Some(RunnerCommand::Interrupt)
        );
        assert_eq!(
            parse_command_line(r#"{"type":"prompt","text":"go"}"#),
            Some(RunnerCommand::Prompt { text: "go".into() })
        );
        assert_eq!(
            parse_command_line(r#"{"type":"init","credential":"k","cwd":"/p","resume":""}"#),
            Some(RunnerCommand::Init {
                credential: "k".into(),
                cwd: "/p".into(),
                resume: None
            })
        );
        assert_eq!(
            parse_command_line(r#"{"type":"init","credential":"k","cwd":"/p","resume":"r1"}"#),
            Some(RunnerCommand::Init {
                credential: "k".into(),
                cwd: "/p".into(),
                resume: Some("r1".into())
            })
        );
        assert_eq!(parse_command_line(r#"{"type":"prompt"}"#), None);
        assert_eq!(parse_command_line("nope"), None);
    }

    #[test]
    fn init_omits_absent_resume() {
        let line = encode_command(&RunnerCommand::Init {
            credential: "k".into(),
            cwd: "/p".into(),
            resume: None,
        });
        assert!(!line.contains("resume"));
    }

    #[test]
    fn line_buffer_reassembles_split_and_merged_lines() {
        let mut b = LineBuffer::new();
        assert_eq!(b.push("{\"a\":1}\n{\"b\""), vec!["{\"a\":1}".to_string()]);
        assert_eq!(b.push(":2}\n\n  \n"), vec!["{\"b\":2}".to_string()]);
        assert_eq!(b.push("partial"), Vec::<String>::new());
        assert_eq!(b.push("\n"), vec!["partial".to_string()]);
    }
}
