//! The agent's replies to review comments, read from the transcript (port of
//! `src/shared/replies.ts`).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::comments::{parse_review, ReplyKind, SentComment, REPLY_TOOL_NAME};
use crate::session::{ToolStatus, TranscriptItem};

/// The agent's answer to one review comment, read from a call to the reply tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentReply {
    pub comment_id: String,
    pub message: String,
    pub kind: Option<ReplyKind>,
}

/// The reply a transcript item carries, or `None` if it is not a reply tool call or its input is
/// malformed. A call that failed (for example an unknown comment id) is not a reply.
pub fn reply_of(item: &TranscriptItem) -> Option<AgentReply> {
    match item {
        TranscriptItem::Tool {
            name,
            input,
            status,
            ..
        } => {
            if name != REPLY_TOOL_NAME || *status == ToolStatus::Error {
                return None;
            }
            let comment_id = input.get("comment_id")?.as_str()?.to_string();
            let message = input.get("message")?.as_str()?.to_string();
            let kind = match input.get("kind").and_then(|k| k.as_str()) {
                Some("question") => Some(ReplyKind::Question),
                Some("resolved") => Some(ReplyKind::Resolved),
                _ => None,
            };
            Some(AgentReply {
                comment_id,
                message,
                kind,
            })
        }
        _ => None,
    }
}

/// Every reply in the transcript grouped by comment id, in the order the agent made them.
pub fn replies_by_comment(items: &[TranscriptItem]) -> HashMap<String, Vec<AgentReply>> {
    let mut threads: HashMap<String, Vec<AgentReply>> = HashMap::new();
    for item in items {
        if let Some(reply) = reply_of(item) {
            threads
                .entry(reply.comment_id.clone())
                .or_default()
                .push(reply);
        }
    }
    threads
}

/// How a reply's kind reads in the UI.
pub fn reply_kind_label(kind: ReplyKind) -> &'static str {
    match kind {
        ReplyKind::Question => "asks a question",
        ReplyKind::Resolved => "believes this is fixed",
    }
}

/// Every comment the transcript shows as sent to the agent, in order (see `parse_review`).
pub fn sent_comments(items: &[TranscriptItem]) -> Vec<SentComment> {
    let mut found: Vec<SentComment> = Vec::new();
    for item in items {
        if let TranscriptItem::User { text } = item {
            if let Some(review) = parse_review(text) {
                found.extend(review.comments);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comments::{format_review, ReviewComment, Side};
    use serde_json::{json, Value};

    fn call_named(input: Value, status: ToolStatus, name: &str) -> TranscriptItem {
        TranscriptItem::Tool {
            id: "t".into(),
            name: name.into(),
            input,
            status,
            output: String::new(),
        }
    }

    fn call(input: Value) -> TranscriptItem {
        call_named(input, ToolStatus::Done, REPLY_TOOL_NAME)
    }

    fn text(t: &str) -> TranscriptItem {
        TranscriptItem::Text { text: t.into() }
    }

    #[test]
    fn reply_of_reads_a_reply_and_its_kind() {
        assert_eq!(
            reply_of(&call(
                json!({"comment_id": "c1", "message": "Done", "kind": "resolved"})
            )),
            Some(AgentReply {
                comment_id: "c1".into(),
                message: "Done".into(),
                kind: Some(ReplyKind::Resolved)
            })
        );
    }

    #[test]
    fn reply_of_treats_a_missing_or_unknown_kind_as_a_plain_reply() {
        assert_eq!(
            reply_of(&call(json!({"comment_id": "c1", "message": "x"})))
                .unwrap()
                .kind,
            None
        );
        assert_eq!(
            reply_of(&call(
                json!({"comment_id": "c1", "message": "x", "kind": "wat"})
            ))
            .unwrap()
            .kind,
            None
        );
    }

    #[test]
    fn reply_of_ignores_other_tools_failed_calls_and_malformed_input() {
        assert_eq!(reply_of(&text("hi")), None);
        assert_eq!(
            reply_of(&call_named(
                json!({"comment_id": "c1", "message": "x"}),
                ToolStatus::Done,
                "Bash"
            )),
            None
        );
        assert_eq!(
            reply_of(&call_named(
                json!({"comment_id": "c1", "message": "x"}),
                ToolStatus::Error,
                REPLY_TOOL_NAME
            )),
            None
        );
        assert_eq!(
            reply_of(&call(json!({"comment_id": 1, "message": "x"}))),
            None
        );
        assert_eq!(reply_of(&call(Value::Null)), None);
    }

    #[test]
    fn replies_by_comment_groups_replies_by_comment_in_order() {
        let threads = replies_by_comment(&[
            call(json!({"comment_id": "a", "message": "1"})),
            text("between"),
            call(json!({"comment_id": "b", "message": "2"})),
            call(json!({"comment_id": "a", "message": "3", "kind": "question"})),
        ]);
        let a: Vec<String> = threads["a"].iter().map(|r| r.message.clone()).collect();
        assert_eq!(a, vec!["1".to_string(), "3".to_string()]);
        assert_eq!(threads["b"].len(), 1);
    }

    #[test]
    fn reply_kind_label_reads_naturally() {
        assert_eq!(reply_kind_label(ReplyKind::Question), "asks a question");
        assert_eq!(
            reply_kind_label(ReplyKind::Resolved),
            "believes this is fixed"
        );
    }

    #[test]
    fn sent_comments_collects_the_comments_of_every_review_the_user_sent() {
        let review = |id: &str, round: u32| -> String {
            let comment = ReviewComment {
                id: id.to_string(),
                file: "a.ts".to_string(),
                staged: false,
                side: Side::New,
                start: 1,
                end: 1,
                snapshot: vec![],
                text: id.to_string(),
                round,
                sent: true,
                outdated: false,
            };
            format_review(&[comment], round)
        };
        let found = sent_comments(&[
            TranscriptItem::User {
                text: "hello".into(),
            },
            TranscriptItem::User {
                text: review("c1", 1),
            },
            TranscriptItem::Text {
                text: review("nope", 1),
            },
            TranscriptItem::User {
                text: review("c2", 2),
            },
        ]);
        let pairs: Vec<(String, u32)> = found.iter().map(|c| (c.id.clone(), c.round)).collect();
        assert_eq!(pairs, vec![("c1".to_string(), 1), ("c2".to_string(), 2)]);
    }
}
