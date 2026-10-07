//! Turns the Claude Agent SDK's messages into app events (port of `src/runner/translate.ts`).
//!
//! The TypeScript runner keeps its own copy; the host uses this one to replay stored transcripts.
//! Messages are plain JSON values so no SDK types are needed.

use std::collections::HashMap;

use serde_json::Value;

use crate::agent::{AgentEvent, SlashCommandInfo};

/// Keep only the fields the app uses, so the protocol does not depend on SDK additions.
pub fn to_command_info(commands: &[SlashCommandInfo]) -> Vec<SlashCommandInfo> {
    commands
        .iter()
        .map(|c| SlashCommandInfo {
            name: c.name.clone(),
            description: c.description.clone(),
            argument_hint: c.argument_hint.clone(),
            aliases: match &c.aliases {
                Some(a) if !a.is_empty() => Some(a.clone()),
                _ => None,
            },
        })
        .collect()
}

fn str_field(value: &Value, field: &str) -> String {
    value
        .get(field)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// Read the SDK's `commands` array; missing hints become empty and empty alias lists are dropped.
fn commands_from_value(value: Option<&Value>) -> Vec<SlashCommandInfo> {
    let list = match value.and_then(|v| v.as_array()) {
        Some(list) => list,
        None => return Vec::new(),
    };
    list.iter()
        .map(|c| {
            let aliases: Vec<String> = c
                .get("aliases")
                .and_then(|a| a.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            SlashCommandInfo {
                name: str_field(c, "name"),
                description: str_field(c, "description"),
                argument_hint: str_field(c, "argumentHint"),
                aliases: if aliases.is_empty() {
                    None
                } else {
                    Some(aliases)
                },
            }
        })
        .collect()
}

/// Tools whose successful completion means a file changed; the value is the input field with its path.
fn edit_field(tool: &str) -> Option<&'static str> {
    match tool {
        "Edit" | "MultiEdit" | "Write" => Some("file_path"),
        "NotebookEdit" => Some("notebook_path"),
        _ => None,
    }
}

/// Tool result content is a string or an array of blocks; keep the text.
fn result_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(blocks)) => {
            let texts: Vec<String> = blocks
                .iter()
                .filter_map(|block| {
                    if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                        block
                            .get("text")
                            .and_then(|t| t.as_str())
                            .filter(|t| !t.is_empty())
                            .map(|t| t.to_string())
                    } else {
                        None
                    }
                })
                .collect();
            texts.join("\n")
        }
        _ => String::new(),
    }
}

fn blocks_of(message: &Value) -> Vec<Value> {
    message
        .get("message")
        .and_then(|inner| inner.get("content"))
        .and_then(|content| content.as_array())
        .cloned()
        .unwrap_or_default()
}

/// Turns the SDK's message stream into app events. Stateful because a file-edited event is only
/// emitted once the matching tool call finishes without an error.
#[derive(Debug, Default)]
pub struct Translator {
    edit_paths: HashMap<String, String>,
    session_id: Option<String>,
}

impl Translator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn translate(&mut self, message: &Value) -> Vec<AgentEvent> {
        let mut events = self.translate_message(message);
        // Every SDK message carries the session ID; report it once, ahead of the first events
        if let Some(id) = message.get("session_id").and_then(|v| v.as_str()) {
            if !id.is_empty() && self.session_id.as_deref() != Some(id) {
                self.session_id = Some(id.to_string());
                events.insert(
                    0,
                    AgentEvent::SessionStarted {
                        session_id: id.to_string(),
                    },
                );
            }
        }
        events
    }

    fn translate_message(&mut self, message: &Value) -> Vec<AgentEvent> {
        let kind = message.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let subtype = message.get("subtype").and_then(|v| v.as_str());
        match kind {
            "assistant" => self.assistant(message),
            "user" => self.tool_results(message),
            "result" => Self::result(message),
            // Skills found mid-session push the whole list, which replaces the earlier one
            "system" if subtype == Some("commands_changed") => vec![AgentEvent::Commands {
                commands: commands_from_value(message.get("commands")),
            }],
            _ => Vec::new(),
        }
    }

    fn assistant(&mut self, message: &Value) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        for block in blocks_of(message) {
            let kind = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if kind == "text" {
                let text = str_field(&block, "text");
                if !text.is_empty() {
                    events.push(AgentEvent::Text { text });
                }
            } else if kind == "tool_use" {
                let id = str_field(&block, "id");
                let name = str_field(&block, "name");
                let input = block.get("input").cloned().unwrap_or(Value::Null);
                events.push(AgentEvent::ToolStarted {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                });
                if let Some(field) = edit_field(&name) {
                    if let Some(path) = input.get(field).and_then(|p| p.as_str()) {
                        self.edit_paths.insert(id, path.to_string());
                    }
                }
            }
        }
        events
    }

    fn tool_results(&mut self, message: &Value) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        for block in blocks_of(message) {
            if block.get("type").and_then(|v| v.as_str()) != Some("tool_result") {
                continue;
            }
            let id = str_field(&block, "tool_use_id");
            let is_error = block.get("is_error").and_then(|v| v.as_bool()) == Some(true);
            events.push(AgentEvent::ToolFinished {
                id: id.clone(),
                output: result_text(block.get("content")),
                is_error,
            });
            let path = self.edit_paths.remove(&id);
            if let Some(path) = path {
                if !path.is_empty() && !is_error {
                    events.push(AgentEvent::FileEdited { path });
                }
            }
        }
        events
    }

    fn result(message: &Value) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        let subtype = message.get("subtype").and_then(|v| v.as_str());
        let is_error = message.get("is_error").and_then(|v| v.as_bool()) == Some(true);
        if subtype != Some("success") || is_error {
            let errors: String = message
                .get("errors")
                .and_then(|e| e.as_array())
                .map(|list| {
                    list.iter()
                        .filter_map(|e| e.as_str())
                        .collect::<Vec<&str>>()
                        .join("; ")
                })
                .unwrap_or_default();
            let result = str_field(message, "result");
            let detail = if !errors.is_empty() {
                errors
            } else if !result.is_empty() {
                result
            } else {
                subtype.unwrap_or("undefined").to_string()
            };
            events.push(AgentEvent::Error {
                message: format!("The agent stopped: {}", detail),
            });
        }
        events.push(AgentEvent::TurnFinished);
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn assistant(content: Value) -> Value {
        json!({"type": "assistant", "message": {"content": content}})
    }

    fn user(content: Value) -> Value {
        json!({"type": "user", "message": {"content": content}})
    }

    fn kinds(events: &[AgentEvent]) -> Vec<&'static str> {
        events
            .iter()
            .map(|e| match e {
                AgentEvent::SessionStarted { .. } => "session-started",
                AgentEvent::Commands { .. } => "commands",
                AgentEvent::Text { .. } => "text",
                AgentEvent::ToolStarted { .. } => "tool-started",
                AgentEvent::ToolFinished { .. } => "tool-finished",
                AgentEvent::FileEdited { .. } => "file-edited",
                AgentEvent::TurnFinished => "turn-finished",
                AgentEvent::Error { .. } => "error",
                AgentEvent::Exited { .. } => "exited",
            })
            .collect()
    }

    fn finished(id: &str, output: &str, is_error: bool) -> AgentEvent {
        AgentEvent::ToolFinished {
            id: id.into(),
            output: output.into(),
            is_error,
        }
    }

    #[test]
    fn emits_text_blocks_and_skips_empty_or_thinking_blocks() {
        let events = Translator::new().translate(&assistant(json!([
            {"type": "text", "text": "hi"},
            {"type": "text", "text": ""},
            {"type": "thinking"}
        ])));
        assert_eq!(events, vec![AgentEvent::Text { text: "hi".into() }]);
    }

    #[test]
    fn emits_tool_started_for_tool_use_blocks() {
        let events = Translator::new().translate(&assistant(json!([
            {"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "ls"}}
        ])));
        assert_eq!(
            events,
            vec![AgentEvent::ToolStarted {
                id: "t1".into(),
                name: "Bash".into(),
                input: json!({"command": "ls"})
            }]
        );
    }

    #[test]
    fn emits_tool_finished_with_string_output() {
        let events = Translator::new().translate(&user(json!([
            {"type": "tool_result", "tool_use_id": "t1", "content": "done"}
        ])));
        assert_eq!(events, vec![finished("t1", "done", false)]);
    }

    #[test]
    fn joins_text_blocks_of_array_output_and_ignores_other_blocks() {
        let events = Translator::new().translate(&user(json!([{
            "type": "tool_result",
            "tool_use_id": "t1",
            "is_error": true,
            "content": [{"type": "text", "text": "a"}, {"type": "image"}, {"type": "text", "text": "b"}]
        }])));
        assert_eq!(events, vec![finished("t1", "a\nb", true)]);
    }

    #[test]
    fn uses_empty_output_for_missing_content() {
        let events = Translator::new()
            .translate(&user(json!([{"type": "tool_result", "tool_use_id": "t1"}])));
        assert_eq!(events, vec![finished("t1", "", false)]);
    }

    #[test]
    fn ignores_non_tool_result_blocks_in_user_messages() {
        let events = Translator::new().translate(&user(json!([{"type": "text", "text": "x"}])));
        assert_eq!(events, Vec::<AgentEvent>::new());
    }

    #[test]
    fn emits_file_edited_after_a_successful_edit_tool_finishes() {
        let mut t = Translator::new();
        t.translate(&assistant(json!([
            {"type": "tool_use", "id": "e1", "name": "Edit", "input": {"file_path": "/p/a.ts"}},
            {"type": "tool_use", "id": "n1", "name": "NotebookEdit", "input": {"notebook_path": "/p/n.ipynb"}}
        ])));
        assert_eq!(
            t.translate(&user(json!([
                {"type": "tool_result", "tool_use_id": "e1", "content": "ok"}
            ]))),
            vec![
                finished("e1", "ok", false),
                AgentEvent::FileEdited {
                    path: "/p/a.ts".into()
                }
            ]
        );
        assert_eq!(
            t.translate(&user(json!([
                {"type": "tool_result", "tool_use_id": "n1", "content": "ok"}
            ]))),
            vec![
                finished("n1", "ok", false),
                AgentEvent::FileEdited {
                    path: "/p/n.ipynb".into()
                }
            ]
        );
    }

    #[test]
    fn emits_file_edited_only_once_per_call() {
        let mut t = Translator::new();
        t.translate(&assistant(json!([
            {"type": "tool_use", "id": "e1", "name": "Write", "input": {"file_path": "a"}}
        ])));
        let result = user(json!([{"type": "tool_result", "tool_use_id": "e1", "content": ""}]));
        assert_eq!(t.translate(&result).len(), 2);
        assert_eq!(t.translate(&result).len(), 1);
    }

    #[test]
    fn does_not_emit_file_edited_for_a_failed_edit() {
        let mut t = Translator::new();
        t.translate(&assistant(json!([
            {"type": "tool_use", "id": "e1", "name": "Edit", "input": {"file_path": "a"}}
        ])));
        let events = t.translate(&user(json!([
            {"type": "tool_result", "tool_use_id": "e1", "content": "no match", "is_error": true}
        ])));
        assert_eq!(kinds(&events), vec!["tool-finished"]);
    }

    #[test]
    fn does_not_track_an_edit_tool_call_without_a_usable_path() {
        let mut t = Translator::new();
        t.translate(&assistant(json!([
            {"type": "tool_use", "id": "a", "name": "Edit", "input": {"file_path": 7}},
            {"type": "tool_use", "id": "b", "name": "Write"}
        ])));
        for id in ["a", "b"] {
            let events = t.translate(&user(json!([
                {"type": "tool_result", "tool_use_id": id, "content": ""}
            ])));
            assert_eq!(kinds(&events), vec!["tool-finished"]);
        }
    }

    #[test]
    fn does_not_treat_other_tools_as_edits() {
        let mut t = Translator::new();
        t.translate(&assistant(json!([
            {"type": "tool_use", "id": "r", "name": "Read", "input": {"file_path": "a"}}
        ])));
        let events = t.translate(&user(json!([
            {"type": "tool_result", "tool_use_id": "r", "content": "x"}
        ])));
        assert_eq!(kinds(&events), vec!["tool-finished"]);
    }

    #[test]
    fn emits_turn_finished_for_a_successful_result() {
        let events = Translator::new().translate(&json!({"type": "result", "subtype": "success"}));
        assert_eq!(events, vec![AgentEvent::TurnFinished]);
    }

    #[test]
    fn emits_an_error_before_turn_finished_for_a_failed_result() {
        let mut t = Translator::new();
        assert_eq!(
            t.translate(
                &json!({"type": "result", "subtype": "error_max_turns", "errors": ["a", "b"]})
            ),
            vec![
                AgentEvent::Error {
                    message: "The agent stopped: a; b".into()
                },
                AgentEvent::TurnFinished
            ]
        );
        assert_eq!(
            t.translate(&json!({
                "type": "result", "subtype": "success", "is_error": true, "result": "bad key"
            })),
            vec![
                AgentEvent::Error {
                    message: "The agent stopped: bad key".into()
                },
                AgentEvent::TurnFinished
            ]
        );
        assert_eq!(
            t.translate(&json!({"type": "result", "subtype": "error_during_execution"})),
            vec![
                AgentEvent::Error {
                    message: "The agent stopped: error_during_execution".into()
                },
                AgentEvent::TurnFinished
            ]
        );
    }

    #[test]
    fn handles_messages_without_content_and_ignores_other_message_types() {
        let mut t = Translator::new();
        assert_eq!(
            t.translate(&json!({"type": "assistant"})),
            Vec::<AgentEvent>::new()
        );
        assert_eq!(
            t.translate(&json!({"type": "user", "message": {"content": "plain string"}})),
            Vec::<AgentEvent>::new()
        );
        assert_eq!(
            t.translate(&json!({"type": "system", "subtype": "init"})),
            Vec::<AgentEvent>::new()
        );
    }

    #[test]
    fn reports_the_session_id_once_before_the_message_events() {
        let mut translator = Translator::new();
        let mut message = assistant(json!([{"type": "text", "text": "hi"}]));
        message["session_id"] = json!("s1");
        assert_eq!(
            translator.translate(&message),
            vec![
                AgentEvent::SessionStarted {
                    session_id: "s1".into()
                },
                AgentEvent::Text { text: "hi".into() }
            ]
        );
        assert_eq!(
            translator.translate(&message),
            vec![AgentEvent::Text { text: "hi".into() }]
        );
    }

    #[test]
    fn reports_a_new_session_id_if_it_changes() {
        let mut translator = Translator::new();
        translator.translate(&json!({"type": "system", "session_id": "s1"}));
        assert_eq!(
            translator.translate(&json!({"type": "system", "session_id": "s2"})),
            vec![AgentEvent::SessionStarted {
                session_id: "s2".into()
            }]
        );
    }

    #[test]
    fn turns_commands_changed_into_a_commands_event_with_only_the_app_fields() {
        let events = Translator::new().translate(&json!({
            "type": "system",
            "subtype": "commands_changed",
            "commands": [
                {"name": "init", "description": "Set up", "argumentHint": "", "builtin": true},
                {"name": "usage", "description": "Cost", "argumentHint": "<x>", "aliases": ["cost"]}
            ]
        }));
        assert_eq!(
            events,
            vec![AgentEvent::Commands {
                commands: vec![
                    SlashCommandInfo {
                        name: "init".into(),
                        description: "Set up".into(),
                        argument_hint: "".into(),
                        aliases: None
                    },
                    SlashCommandInfo {
                        name: "usage".into(),
                        description: "Cost".into(),
                        argument_hint: "<x>".into(),
                        aliases: Some(vec!["cost".to_string()])
                    }
                ]
            }]
        );
    }

    #[test]
    fn fills_a_missing_argument_hint_and_tolerates_a_missing_list() {
        let mut translator = Translator::new();
        assert_eq!(
            translator.translate(&json!({
                "type": "system",
                "subtype": "commands_changed",
                "commands": [{"name": "a", "description": "d"}]
            })),
            vec![AgentEvent::Commands {
                commands: vec![SlashCommandInfo {
                    name: "a".into(),
                    description: "d".into(),
                    argument_hint: "".into(),
                    aliases: None
                }]
            }]
        );
        assert_eq!(
            translator.translate(&json!({"type": "system", "subtype": "commands_changed"})),
            vec![AgentEvent::Commands { commands: vec![] }]
        );
    }

    #[test]
    fn to_command_info_drops_empty_alias_lists() {
        let list = vec![SlashCommandInfo {
            name: "a".into(),
            description: "d".into(),
            argument_hint: "".into(),
            aliases: Some(vec![]),
        }];
        assert_eq!(to_command_info(&list)[0].aliases, None);
    }
}
