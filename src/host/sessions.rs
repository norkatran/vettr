//! Stored agent sessions of a project: listing them and replaying one into Session view state
//! (port of `src/main/sessions.ts` and `src/main/replay.ts`).
//!
//! There is no Node SDK on the host, so the SDK's jsonl transcripts are read directly. The layout
//! is the one Claude Code uses under its config dir (here a project's transcripts folder, see
//! `host/transcripts.rs`): `projects/<encoded cwd>/<sessionId>.jsonl`, where the encoded cwd is
//! the absolute project path with every non-alphanumeric character replaced by `-`. Titles and
//! the message chain mirror the SDK's `listSessions` and `getSessionMessages`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::UNIX_EPOCH;

use regex::Regex;
use serde_json::{json, Value};

use crate::agent::AgentEvent;
use crate::host::translate::Translator;
use crate::session::{
    initial_session, session_reducer, SessionAction, SessionState, TranscriptItem,
};
use crate::sessions::SessionInfo;

const INTERRUPT_MARKER: &str = "[Request interrupted by user";

/// The SDK keeps at most this many characters of the sanitised path before adding a hash.
const MAX_DIR_NAME: usize = 200;

// ---------------------------------------------------------------------------------------------
// Replay (src/main/replay.ts)
// ---------------------------------------------------------------------------------------------

/// The text of a user message typed by the user, or `None` for tool results and non-text content.
fn user_text(message: &Value) -> Option<String> {
    let content = message.get("content")?;
    match content {
        Value::String(s) => Some(s.clone()),
        Value::Array(blocks) => {
            let texts: Vec<String> = blocks
                .iter()
                .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                .filter_map(|b| {
                    b.get("text")
                        .and_then(|t| t.as_str())
                        .map(|t| t.to_string())
                })
                .collect();
            if texts.is_empty() {
                None
            } else {
                Some(texts.join("\n"))
            }
        }
        _ => None,
    }
}

/// Harness-injected user messages (slash command echoes, system reminders) start with a tag.
fn is_injected(text: &str) -> bool {
    text.trim_start().starts_with('<')
}

/// Rebuilds the Session view from a stored transcript by running the messages through the same
/// translator and reducer as a live session. Each message is a JSON object shaped like the SDK's
/// `SessionMessage`: `{"type", "session_id", "message"}`. Errors and other transient notices are
/// not stored, so they do not come back. The session is left waiting for input, with unfinished
/// tools stopped.
pub fn replay_session(messages: &[Value]) -> SessionState {
    let mut translator = Translator::new();
    let mut state = initial_session();
    for message in messages {
        let is_user = message.get("type").and_then(|t| t.as_str()) == Some("user");
        let text = if is_user {
            message.get("message").and_then(user_text)
        } else {
            None
        };
        if let Some(text) = text {
            if text.starts_with(INTERRUPT_MARKER) {
                state.items.push(TranscriptItem::Notice {
                    text: "Interrupted".to_string(),
                });
            } else if !is_injected(&text) {
                state = session_reducer(state, SessionAction::Sent { text });
            }
            continue;
        }
        for event in translator.translate(message) {
            state = session_reducer(state, SessionAction::Event { event });
        }
    }
    session_reducer(
        state,
        SessionAction::Event {
            event: AgentEvent::TurnFinished,
        },
    )
}

// ---------------------------------------------------------------------------------------------
// Locating transcripts
// ---------------------------------------------------------------------------------------------

fn base36(mut n: u32) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let digits: Vec<char> = "0123456789abcdefghijklmnopqrstuvwxyz".chars().collect();
    let mut out: Vec<char> = Vec::new();
    while n > 0 {
        out.push(digits[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    out.into_iter().collect()
}

/// The folder name the SDK uses for a project path: every non-alphanumeric UTF-16 code unit
/// becomes `-`; names over 200 characters are cut and get a hash of the path appended.
pub fn project_dir_name(project: &str) -> String {
    let sanitized: String = project
        .encode_utf16()
        .map(|unit| {
            if unit < 128 && (unit as u8).is_ascii_alphanumeric() {
                (unit as u8) as char
            } else {
                '-'
            }
        })
        .collect();
    if sanitized.len() <= MAX_DIR_NAME {
        return sanitized;
    }
    let mut hash: i32 = 0;
    for unit in project.encode_utf16() {
        hash = hash
            .wrapping_shl(5)
            .wrapping_sub(hash)
            .wrapping_add(unit as i32);
    }
    format!(
        "{}-{}",
        &sanitized[..MAX_DIR_NAME],
        base36(hash.unsigned_abs())
    )
}

/// The folders that can hold the project's transcripts. The transcripts root is private to the
/// project, so when no folder matches the project path (for example it is reached through a
/// symlink) every folder under `projects/` counts.
fn session_dirs(transcripts_root: &Path, project: &str) -> Vec<PathBuf> {
    let projects = transcripts_root.join("projects");
    let mut names: Vec<String> = vec![project_dir_name(project)];
    if let Ok(real) = fs::canonicalize(project) {
        let name = project_dir_name(&real.to_string_lossy());
        if !names.contains(&name) {
            names.push(name);
        }
    }
    let exact: Vec<PathBuf> = names
        .iter()
        .map(|n| projects.join(n))
        .filter(|p| p.is_dir())
        .collect();
    if !exact.is_empty() {
        return exact;
    }
    let mut all: Vec<PathBuf> = match fs::read_dir(&projects) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect(),
        Err(_) => Vec::new(),
    };
    all.sort();
    all
}

fn read_text(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn parse_lines(text: &str) -> Vec<Value> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Listing (src/main/sessions.ts `listProjectSessions`)
// ---------------------------------------------------------------------------------------------

fn command_name_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"<command-name>(.*?)</command-name>").expect("valid regex"))
}

fn bash_input_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"<bash-input>(.*?)</bash-input>").expect("valid regex"))
}

fn skipped_prompt_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(?:\s*<[a-z][\w-]*[\s>]|\[Request interrupted by user[^\]]*\])")
            .expect("valid regex")
    })
}

fn flag(entry: &Value, key: &str) -> bool {
    entry.get(key).and_then(|v| v.as_bool()) == Some(true)
}

/// The prompt a user entry shows as a title, or `None` when it is not a real prompt (tool result,
/// meta message, slash command echo, interrupt marker). A slash command name seen on the way is
/// kept in `fallback`.
fn prompt_of(entry: &Value, fallback: &mut String) -> Option<String> {
    if entry.get("type").and_then(|t| t.as_str()) != Some("user") {
        return None;
    }
    if flag(entry, "isMeta") || flag(entry, "isCompactSummary") {
        return None;
    }
    let content = entry.get("message")?.get("content")?;
    let mut texts: Vec<String> = Vec::new();
    match content {
        Value::String(s) => texts.push(s.clone()),
        Value::Array(blocks) => {
            for block in blocks {
                match block.get("type").and_then(|t| t.as_str()) {
                    Some("tool_result") => return None,
                    Some("text") => {
                        if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                            texts.push(t.to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    for text in texts {
        let mut line = text.replace('\n', " ").trim().to_string();
        if line.is_empty() {
            continue;
        }
        if let Some(caps) = command_name_re().captures(&line) {
            if fallback.is_empty() {
                *fallback = caps[1].to_string();
            }
            continue;
        }
        if let Some(caps) = bash_input_re().captures(&line) {
            return Some(format!("! {}", caps[1].trim()));
        }
        if skipped_prompt_re().is_match(&line) {
            continue;
        }
        if line.chars().count() > 200 {
            let cut: String = line.chars().take(200).collect();
            line = format!("{}…", cut.trim());
        }
        return Some(line);
    }
    None
}

/// The first real prompt, else the first slash command name, else empty.
fn first_prompt(entries: &[Value]) -> String {
    let mut fallback = String::new();
    for entry in entries {
        if let Some(prompt) = prompt_of(entry, &mut fallback) {
            return prompt;
        }
    }
    fallback
}

/// "Image" or "Document" for a session whose first user message has only attachments.
fn media_title(entries: &[Value]) -> String {
    for entry in entries {
        if entry.get("type").and_then(|t| t.as_str()) != Some("user") || flag(entry, "isMeta") {
            continue;
        }
        let blocks = match entry
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_array())
        {
            Some(blocks) => blocks,
            None => continue,
        };
        let types: Vec<&str> = blocks
            .iter()
            .filter_map(|b| b.get("type").and_then(|t| t.as_str()))
            .collect();
        if types.contains(&"tool_result") {
            continue;
        }
        if types.contains(&"image") {
            return "Image".to_string();
        }
        if types.contains(&"document") {
            return "Document".to_string();
        }
    }
    String::new()
}

/// The last non-empty string value of `key` on any entry.
fn last_string(entries: &[Value], key: &str) -> Option<String> {
    entries
        .iter()
        .rev()
        .filter_map(|e| e.get(key).and_then(|v| v.as_str()))
        .find(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// A session's display title like the SDK's `summary`: custom title, AI title, last prompt,
/// summary, first prompt. `None` hides the session (sidechains and sessions with nothing to show).
fn session_title(entries: &[Value]) -> Option<String> {
    if entries
        .first()
        .map(|first| flag(first, "isSidechain"))
        .unwrap_or(true)
    {
        return None;
    }

    last_string(entries, "customTitle")
        .or_else(|| last_string(entries, "aiTitle"))
        .or_else(|| last_string(entries, "lastPrompt"))
        .or_else(|| last_string(entries, "summary"))
        .or_else(|| {
            let prompt = first_prompt(entries);
            if prompt.is_empty() {
                None
            } else {
                Some(prompt)
            }
        })
        .or_else(|| {
            let media = media_title(entries);
            if media.is_empty() {
                None
            } else {
                Some(media)
            }
        })
}

fn modified_ms(path: &Path) -> i64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// The project's stored sessions, newest first. Empty when there are none or they are unreadable.
/// `transcripts_root` is the project's transcripts folder (the Claude config dir).
pub fn list_sessions(transcripts_root: &Path, project: &str) -> Vec<SessionInfo> {
    let mut found: Vec<SessionInfo> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for dir in session_dirs(transcripts_root, project) {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let id = match path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".jsonl"))
            {
                Some(id) if !id.is_empty() => id.to_string(),
                _ => continue,
            };
            if !path.is_file() || seen.contains(&id) {
                continue;
            }
            let text = match read_text(&path) {
                Some(text) => text,
                None => continue,
            };
            let lines = parse_lines(&text);
            if let Some(title) = session_title(&lines) {
                seen.insert(id.clone());
                found.push(SessionInfo {
                    id,
                    title,
                    last_modified: modified_ms(&path),
                });
            }
        }
    }
    found.sort_by(|a, b| {
        b.last_modified
            .cmp(&a.last_modified)
            .then_with(|| a.id.cmp(&b.id))
    });
    found
}

// ---------------------------------------------------------------------------------------------
// Loading (src/main/sessions.ts `loadSession`)
// ---------------------------------------------------------------------------------------------

fn entry_type(entry: &Value) -> &str {
    entry.get("type").and_then(|t| t.as_str()).unwrap_or("")
}

fn uuid_of(entry: &Value) -> Option<&str> {
    entry.get("uuid").and_then(|u| u.as_str())
}

fn parent_of(entry: &Value) -> Option<&str> {
    entry
        .get("parentUuid")
        .and_then(|u| u.as_str())
        .filter(|u| !u.is_empty())
}

/// Ids of the tool calls in an assistant entry.
fn tool_use_ids(entry: &Value) -> Vec<String> {
    block_ids(entry, "tool_use", "id")
}

/// Ids of the tool calls a user entry answers.
fn tool_result_ids(entry: &Value) -> Vec<String> {
    block_ids(entry, "tool_result", "tool_use_id")
}

fn block_ids(entry: &Value, block_type: &str, field: &str) -> Vec<String> {
    entry
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some(block_type))
                .filter_map(|b| b.get(field).and_then(|v| v.as_str()).map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// Whether an entry belongs to the main conversation (not a side chain or team message).
fn is_main(entry: &Value) -> bool {
    if flag(entry, "isSidechain") || entry.get("teamName").map(|t| !t.is_null()).unwrap_or(false) {
        return false;
    }
    if entry_type(entry) == "progress" {
        return false;
    }
    let fork_briefing = entry_type(entry) == "attachment"
        && entry
            .get("attachment")
            .and_then(|a| a.get("type"))
            .and_then(|t| t.as_str())
            == Some("fork_briefing");
    !fork_briefing
}

/// The conversation of a transcript's entries, like the SDK's `getSessionMessages`: the chain of
/// `parentUuid` links ending at the latest leaf, without meta, side chain and system entries.
/// Each message is `{"type", "session_id", "message"}`.
pub fn stored_messages(text: &str) -> Vec<Value> {
    let entries: Vec<Value> = parse_lines(text)
        .into_iter()
        .filter(|e| {
            matches!(
                entry_type(e),
                "user" | "assistant" | "progress" | "system" | "attachment"
            ) && uuid_of(e).is_some()
        })
        .collect();
    if entries.is_empty() {
        return Vec::new();
    }

    let mut index: HashMap<String, usize> = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        if let Some(uuid) = uuid_of(entry) {
            index.insert(uuid.to_string(), i);
        }
    }

    // Leaf: the latest main entry nobody continues from whose ancestry reaches a message.
    let mut has_main_child: HashSet<String> = HashSet::new();
    for entry in &entries {
        if is_main(entry) {
            if let Some(parent) = parent_of(entry) {
                has_main_child.insert(parent.to_string());
            }
        }
    }
    let mut leaf: Option<usize> = None;
    for i in (0..entries.len()).rev() {
        let entry = &entries[i];
        let uuid = uuid_of(entry).unwrap_or("");
        if !is_main(entry) || has_main_child.contains(uuid) {
            continue;
        }
        let mut current = Some(i);
        let mut visited: HashSet<usize> = HashSet::new();
        while let Some(c) = current {
            if !visited.insert(c) {
                break;
            }
            let kind = entry_type(&entries[c]);
            if kind == "user" || kind == "assistant" {
                leaf = Some(c);
                break;
            }
            current = parent_of(&entries[c]).and_then(|p| index.get(p).copied());
        }
        if leaf.is_some() {
            break;
        }
    }
    if leaf.is_none() {
        leaf = (0..entries.len()).rev().find(|i| {
            let e = &entries[*i];
            is_main(e) && matches!(entry_type(e), "user" | "assistant")
        });
    }
    let leaf = match leaf {
        Some(leaf) => leaf,
        None => return Vec::new(),
    };

    // Walk to the root, then flip.
    let mut chain: Vec<usize> = Vec::new();
    let mut visited: HashSet<usize> = HashSet::new();
    let mut current = Some(leaf);
    while let Some(c) = current {
        if !visited.insert(c) {
            break;
        }
        chain.push(c);
        current = parent_of(&entries[c]).and_then(|p| index.get(p).copied());
    }
    chain.reverse();

    // Parallel tool calls branch the chain: a result that hangs off an earlier block of the same
    // assistant message is not on the leaf's path. Put such results after the call they answer.
    let in_chain: HashSet<usize> = chain.iter().copied().collect();
    let mut call_position: HashMap<String, usize> = HashMap::new();
    for (pos, i) in chain.iter().enumerate() {
        if entry_type(&entries[*i]) == "assistant" {
            for id in tool_use_ids(&entries[*i]) {
                call_position.insert(id, pos);
            }
        }
    }
    let answered: HashSet<String> = chain
        .iter()
        .flat_map(|i| tool_result_ids(&entries[*i]))
        .collect();
    let mut extra: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        if in_chain.contains(&i) || entry_type(entry) != "user" || !is_main(entry) {
            continue;
        }
        let ids = tool_result_ids(entry);
        let target = ids
            .iter()
            .filter(|id| !answered.contains(*id))
            .filter_map(|id| call_position.get(id).copied())
            .min();
        if let Some(pos) = target {
            extra.entry(pos).or_default().push(i);
        }
    }
    let mut ordered: Vec<usize> = Vec::new();
    for (pos, i) in chain.iter().enumerate() {
        ordered.push(*i);
        if let Some(more) = extra.get(&pos) {
            ordered.extend(more.iter().copied());
        }
    }

    ordered
        .into_iter()
        .map(|i| &entries[i])
        .filter(|e| matches!(entry_type(e), "user" | "assistant"))
        .filter(|e| !flag(e, "isMeta") && is_main(e))
        .map(|e| {
            json!({
                "type": entry_type(e),
                "session_id": e.get("sessionId").cloned().unwrap_or(Value::Null),
                "message": e.get("message").cloned().unwrap_or(Value::Null),
            })
        })
        .collect()
}

/// A stored session rebuilt as Session view state, or `None` when it cannot be read.
pub fn load_session(transcripts_root: &Path, project: &str, id: &str) -> Option<SessionState> {
    if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains("..") {
        return None;
    }
    let file_name = format!("{}.jsonl", id);
    let path = session_dirs(transcripts_root, project)
        .into_iter()
        .map(|dir| dir.join(&file_name))
        .find(|p| p.is_file())?;
    let text = read_text(&path)?;
    let messages = stored_messages(&text);
    if messages.is_empty() {
        return None;
    }
    let mut state = replay_session(&messages);
    state.session_id = Some(id.to_string());
    Some(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::ToolStatus;
    use std::time::{Duration, SystemTime};

    fn msg(kind: &str, message: Value) -> Value {
        json!({"type": kind, "session_id": "s1", "message": message})
    }
    fn user(content: Value) -> Value {
        msg("user", json!({"role": "user", "content": content}))
    }
    fn assistant(content: Value) -> Value {
        msg(
            "assistant",
            json!({"role": "assistant", "content": content}),
        )
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

    #[test]
    fn replay_rebuilds_prompts_text_tools_edits_and_interrupts() {
        let state = replay_session(&[
            user(json!("fix it")),
            assistant(json!([
                {"type": "text", "text": "On it"},
                {"type": "tool_use", "id": "t1", "name": "Edit", "input": {"file_path": "/p/a.ts"}},
                {"type": "tool_use", "id": "t2", "name": "Bash", "input": {"command": "ls"}}
            ])),
            user(json!([{"type": "tool_result", "tool_use_id": "t1", "content": "ok"}])),
            user(json!([{"type": "text", "text": "[Request interrupted by user]"}])),
            user(json!("<command-name>/clear</command-name>")),
            user(json!([{"type": "image"}])),
            user(json!({"nothing": true})),
            msg("system", Value::Null),
            msg("user", Value::Null),
            user(json!([{"type": "text", "text": "again"}])),
        ]);
        assert_eq!(state.status, crate::session::SessionStatus::Waiting);
        assert_eq!(state.session_id, Some("s1".to_string()));
        assert_eq!(
            kinds(&state),
            vec!["user", "text", "tool", "tool", "edit", "notice", "user"]
        );
        match &state.items[3] {
            TranscriptItem::Tool { id, status, .. } => {
                assert_eq!(id, "t2");
                assert_eq!(*status, ToolStatus::Stopped);
            }
            _ => panic!("expected tool"),
        }
        match &state.items[2] {
            TranscriptItem::Tool {
                id, status, output, ..
            } => {
                assert_eq!(id, "t1");
                assert_eq!(*status, ToolStatus::Done);
                assert_eq!(output, "ok");
            }
            _ => panic!("expected tool"),
        }
    }

    #[test]
    fn replay_keeps_a_sent_review_as_a_user_message_that_can_be_parsed_back() {
        use crate::comments::{format_review, parse_review, ReviewComment, Side};
        let sent = format_review(
            &[ReviewComment {
                id: "c1".into(),
                file: "a.ts".into(),
                staged: false,
                side: Side::New,
                start: 1,
                end: 1,
                snapshot: vec!["x".to_string()],
                text: "Why?".into(),
                round: 1,
                sent: true,
                outdated: false,
            }],
            1,
        );
        let state = replay_session(&[user(json!(sent))]);
        match &state.items[0] {
            TranscriptItem::User { text } => {
                let parsed = parse_review(text).expect("a review");
                assert_eq!(parsed.comments[0].id, "c1");
            }
            _ => panic!("expected a user item"),
        }
    }

    #[test]
    fn replay_returns_an_empty_waiting_session_for_no_messages() {
        let state = replay_session(&[]);
        assert!(state.items.is_empty());
        assert_eq!(state.status, crate::session::SessionStatus::Waiting);
    }

    #[test]
    fn replay_keeps_a_user_message_text_verbatim() {
        let state = replay_session(&[user(json!("line one\nline two"))]);
        assert_eq!(
            state.items,
            vec![TranscriptItem::User {
                text: "line one\nline two".into()
            }]
        );
    }

    // ---- transcripts on disk ----

    const ID: &str = "11111111-1111-4111-8111-111111111111";

    fn line(value: Value) -> String {
        format!("{}\n", value)
    }

    fn base(uuid: &str, parent: Option<&str>, kind: &str, message: Value) -> Value {
        json!({
            "sessionId": ID,
            "cwd": "/work/demo",
            "version": "2.0.0",
            "isSidechain": false,
            "type": kind,
            "uuid": uuid,
            "parentUuid": parent,
            "timestamp": "2026-01-01T00:00:00Z",
            "message": message
        })
    }

    fn simple_transcript() -> String {
        let mut out = String::new();
        out.push_str(&line(base(
            "u1",
            None,
            "user",
            json!({"role": "user", "content": "say hi"}),
        )));
        out.push_str(&line(base(
            "a1",
            Some("u1"),
            "assistant",
            json!({"role": "assistant", "content": [{"type": "text", "text": "hi"}]}),
        )));
        out
    }

    fn write_session(root: &Path, project_dir: &str, id: &str, text: &str) -> PathBuf {
        let dir = root.join("projects").join(project_dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{}.jsonl", id));
        fs::write(&path, text).unwrap();
        path
    }

    fn set_mtime(path: &Path, secs: u64) {
        let file = fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
            .unwrap();
    }

    #[test]
    fn project_dir_name_replaces_every_non_alphanumeric_character() {
        assert_eq!(project_dir_name("/work/demo"), "-work-demo");
        assert_eq!(project_dir_name("/a b/c.d_e"), "-a-b-c-d-e");
    }

    #[test]
    fn project_dir_name_cuts_long_paths_and_appends_a_hash() {
        let long = format!("/{}", "a".repeat(250));
        let name = project_dir_name(&long);
        assert!(name.starts_with(&format!("-{}", "a".repeat(199))));
        assert_eq!(name.as_bytes()[200], b'-');
        assert!(name.len() > 201);
        assert_eq!(name, project_dir_name(&long));
    }

    #[test]
    fn base36_matches_javascript() {
        assert_eq!(base36(0), "0");
        assert_eq!(base36(35), "z");
        assert_eq!(base36(36), "10");
        assert_eq!(base36(123456789), "21i3v9");
    }

    #[test]
    fn load_session_reads_a_real_sdk_transcript_file() {
        let root = tempfile::tempdir().unwrap();
        write_session(root.path(), "-work-demo", ID, &simple_transcript());
        let state = load_session(root.path(), "/work/demo", ID).unwrap();
        assert_eq!(state.session_id, Some(ID.to_string()));
        assert_eq!(
            state.items,
            vec![
                TranscriptItem::User {
                    text: "say hi".into()
                },
                TranscriptItem::Text { text: "hi".into() }
            ]
        );
    }

    #[test]
    fn load_session_is_none_when_the_transcript_is_missing_or_empty() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(load_session(root.path(), "/work/demo", "x"), None);
        write_session(root.path(), "-work-demo", "empty", "");
        assert_eq!(load_session(root.path(), "/work/demo", "empty"), None);
        write_session(root.path(), "-work-demo", "junk", "not json\n");
        assert_eq!(load_session(root.path(), "/work/demo", "junk"), None);
    }

    #[test]
    fn load_session_rejects_ids_that_escape_the_folder() {
        let root = tempfile::tempdir().unwrap();
        write_session(root.path(), "-work-demo", ID, &simple_transcript());
        assert_eq!(load_session(root.path(), "/work/demo", "../x"), None);
        assert_eq!(load_session(root.path(), "/work/demo", "a/b"), None);
    }

    #[test]
    fn load_session_falls_back_to_any_folder_when_the_path_does_not_match() {
        let root = tempfile::tempdir().unwrap();
        write_session(root.path(), "-somewhere-else", ID, &simple_transcript());
        assert!(load_session(root.path(), "/work/demo", ID).is_some());
    }

    #[test]
    fn stored_messages_follows_the_latest_branch_and_drops_meta_and_system_entries() {
        let mut text = String::new();
        text.push_str(&line(base(
            "u1",
            None,
            "user",
            json!({"role": "user", "content": "first"}),
        )));
        text.push_str(&line(base(
            "a1",
            Some("u1"),
            "assistant",
            json!({"role": "assistant", "content": [{"type": "text", "text": "old branch"}]}),
        )));
        // A rewind: the second prompt hangs off u1 again.
        text.push_str(&line(base(
            "u2",
            Some("u1"),
            "user",
            json!({"role": "user", "content": "second"}),
        )));
        let mut meta = base(
            "m1",
            Some("u2"),
            "user",
            json!({"role": "user", "content": "<local-command-caveat>x"}),
        );
        meta["isMeta"] = json!(true);
        text.push_str(&line(meta));
        text.push_str(&line(base(
            "s1",
            Some("m1"),
            "system",
            json!({"content": "sys"}),
        )));
        text.push_str(&line(base(
            "a2",
            Some("s1"),
            "assistant",
            json!({"role": "assistant", "content": [{"type": "text", "text": "new branch"}]}),
        )));
        let state = replay_session(&stored_messages(&text));
        assert_eq!(
            state.items,
            vec![
                TranscriptItem::User {
                    text: "first".into()
                },
                TranscriptItem::User {
                    text: "second".into()
                },
                TranscriptItem::Text {
                    text: "new branch".into()
                }
            ]
        );
    }

    #[test]
    fn stored_messages_keeps_results_of_parallel_tool_calls() {
        let mut text = String::new();
        text.push_str(&line(base(
            "u1",
            None,
            "user",
            json!({"role": "user", "content": "go"}),
        )));
        text.push_str(&line(base(
            "a1",
            Some("u1"),
            "assistant",
            json!({"role": "assistant", "id": "m1", "content": [
                {"type": "tool_use", "id": "A", "name": "Bash", "input": {"command": "a"}}
            ]}),
        )));
        text.push_str(&line(base(
            "a2",
            Some("a1"),
            "assistant",
            json!({"role": "assistant", "id": "m1", "content": [
                {"type": "tool_use", "id": "B", "name": "Bash", "input": {"command": "b"}}
            ]}),
        )));
        text.push_str(&line(base(
            "r1",
            Some("a1"),
            "user",
            json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": "A", "content": "ra"}]}),
        )));
        text.push_str(&line(base(
            "r2",
            Some("a2"),
            "user",
            json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": "B", "content": "rb"}]}),
        )));
        let state = replay_session(&stored_messages(&text));
        let outputs: Vec<(String, ToolStatus, String)> = state
            .items
            .iter()
            .filter_map(|i| match i {
                TranscriptItem::Tool {
                    id, status, output, ..
                } => Some((id.clone(), *status, output.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            outputs,
            vec![
                ("A".to_string(), ToolStatus::Done, "ra".to_string()),
                ("B".to_string(), ToolStatus::Done, "rb".to_string())
            ]
        );
    }

    #[test]
    fn list_sessions_sorts_newest_first_and_titles_them() {
        let root = tempfile::tempdir().unwrap();
        let old = write_session(root.path(), "-work-demo", "old", &simple_transcript());
        let new = write_session(root.path(), "-work-demo", "new", &{
            let mut t = simple_transcript();
            t.push_str(&line(
                json!({"type": "custom-title", "customTitle": "Named", "sessionId": "new"}),
            ));
            t
        });
        set_mtime(&old, 1000);
        set_mtime(&new, 2000);
        let list = list_sessions(root.path(), "/work/demo");
        assert_eq!(
            list,
            vec![
                SessionInfo {
                    id: "new".into(),
                    title: "Named".into(),
                    last_modified: 2_000_000
                },
                SessionInfo {
                    id: "old".into(),
                    title: "say hi".into(),
                    last_modified: 1_000_000
                },
            ]
        );
    }

    #[test]
    fn list_sessions_prefers_summary_and_last_prompt_over_the_first_prompt() {
        let root = tempfile::tempdir().unwrap();
        let mut a = simple_transcript();
        a.push_str(&line(
            json!({"type": "summary", "summary": "A summary", "leafUuid": "a1"}),
        ));
        write_session(root.path(), "-work-demo", "a", &a);
        let mut b = simple_transcript();
        b.push_str(&line(
            json!({"type": "last-prompt", "lastPrompt": "the last one"}),
        ));
        write_session(root.path(), "-work-demo", "b", &b);
        let mut titles: Vec<(String, String)> = list_sessions(root.path(), "/work/demo")
            .into_iter()
            .map(|s| (s.id, s.title))
            .collect();
        titles.sort();
        assert_eq!(
            titles,
            vec![
                ("a".to_string(), "A summary".to_string()),
                ("b".to_string(), "the last one".to_string())
            ]
        );
    }

    #[test]
    fn list_sessions_skips_command_echoes_and_hides_sidechains_and_empty_sessions() {
        let root = tempfile::tempdir().unwrap();
        let mut echo = String::new();
        echo.push_str(&line(base(
            "u0",
            None,
            "user",
            json!({"role": "user", "content": "<command-name>/clear</command-name>"}),
        )));
        echo.push_str(&line(base(
            "u1",
            Some("u0"),
            "user",
            json!({"role": "user", "content": [{"type": "text", "text": "real prompt"}]}),
        )));
        write_session(root.path(), "-work-demo", "echo", &echo);

        let mut side = base(
            "u1",
            None,
            "user",
            json!({"role": "user", "content": "hidden"}),
        );
        side["isSidechain"] = json!(true);
        write_session(root.path(), "-work-demo", "side", &line(side));

        write_session(root.path(), "-work-demo", "empty", "");
        write_session(
            root.path(),
            "-work-demo",
            "tools",
            &line(base(
                "u1",
                None,
                "user",
                json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t", "content": "x"}]}),
            )),
        );
        fs::write(
            root.path()
                .join("projects")
                .join("-work-demo")
                .join("notes.txt"),
            "x",
        )
        .unwrap();

        let list = list_sessions(root.path(), "/work/demo");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "echo");
        assert_eq!(list[0].title, "real prompt");
    }

    #[test]
    fn list_sessions_titles_a_command_only_session_by_its_command_and_truncates_long_prompts() {
        let root = tempfile::tempdir().unwrap();
        write_session(
            root.path(),
            "-work-demo",
            "cmd",
            &line(base(
                "u1",
                None,
                "user",
                json!({"role": "user", "content": "<command-name>/init</command-name>"}),
            )),
        );
        let long = "x".repeat(300);
        write_session(
            root.path(),
            "-work-demo",
            "long",
            &line(base(
                "u1",
                None,
                "user",
                json!({"role": "user", "content": long}),
            )),
        );
        let list = list_sessions(root.path(), "/work/demo");
        let title = |id: &str| list.iter().find(|s| s.id == id).unwrap().title.clone();
        assert_eq!(title("cmd"), "/init");
        assert_eq!(title("long"), format!("{}…", "x".repeat(200)));
    }

    #[test]
    fn list_sessions_is_empty_without_a_transcripts_folder() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            list_sessions(root.path(), "/work/demo"),
            Vec::<SessionInfo>::new()
        );
    }
}
