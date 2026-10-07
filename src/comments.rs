//! Review comments: anchoring, re-anchoring against a changing diff, and the message format sent
//! to the agent (port of `src/shared/comments.ts`).

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::diff::{DiffLine, FileChange, RepoChanges};

/// Which side of the diff a comment is on: the old file (deleted lines) or the new one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Old,
    New,
}

impl Side {
    pub fn as_str(self) -> &'static str {
        match self {
            Side::Old => "old",
            Side::New => "new",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewComment {
    /// Unique for the comment's whole life, including across review rounds (a UUID).
    pub id: String,
    /// Path in the working tree, as in `FileChange::path`.
    pub file: String,
    /// Whether it was made on the staged or the unstaged diff; line numbers differ between them.
    pub staged: bool,
    pub side: Side,
    /// First and last line of the range (inclusive, `start <= end`) in that side's numbering.
    pub start: u32,
    pub end: u32,
    /// The text of the commented lines when the comment was made, used later for re-anchoring.
    pub snapshot: Vec<String>,
    pub text: String,
    /// The review round it was made in (the first round is 1).
    pub round: u32,
    /// True once it has been sent to the agent.
    pub sent: bool,
    /// True when the snapshot can no longer be found in the diff (see [`reanchor`]).
    pub outdated: bool,
}

/// The lines of a file's diff that exist on `side`, in order.
fn side_lines(file: &FileChange, side: Side) -> Vec<&DiffLine> {
    let mut out: Vec<&DiffLine> = Vec::new();
    for hunk in &file.hunks {
        for line in &hunk.lines {
            let no = match side {
                Side::Old => line.old_no,
                Side::New => line.new_no,
            };
            if no.is_some() {
                out.push(line);
            }
        }
    }
    out
}

/// The number of `line` on `side`; 0 for a line that has none on that side (callers only ask for
/// lines known to have one).
pub fn line_no(line: &DiffLine, side: Side) -> u32 {
    match side {
        Side::Old => line.old_no.unwrap_or(0),
        Side::New => line.new_no.unwrap_or(0),
    }
}

/// Order two clicked line numbers into a range.
pub fn range_of(a: u32, b: u32) -> (u32, u32) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// The text of lines `start` to `end` on `side` that are visible in the diff.
pub fn snapshot_lines(file: &FileChange, side: Side, start: u32, end: u32) -> Vec<String> {
    side_lines(file, side)
        .into_iter()
        .filter(|line| {
            let n = line_no(line, side);
            n >= start && n <= end
        })
        .map(|line| line.text.clone())
        .collect()
}

/// Whether a comment is anchored at (ends on) this line, which is where it is drawn. Outdated
/// comments are anchored nowhere; they are listed separately.
pub fn ends_at(c: &ReviewComment, file: &str, staged: bool, side: Side, no: u32) -> bool {
    !c.outdated && c.file == file && c.staged == staged && c.side == side && c.end == no
}

/// Whether a line lies inside the range `start..=end` being selected or commented.
pub fn in_range(start: u32, end: u32, no: u32) -> bool {
    no >= start && no <= end
}

/// Where a snapshot sits in `file` on `side`: the start line of the run of consecutive lines whose
/// text equals the snapshot, nearest to `near` when there are several. `None` when there is none
/// (or the snapshot is empty, which proves nothing).
fn find_snapshot(file: &FileChange, side: Side, snapshot: &[String], near: u32) -> Option<u32> {
    if snapshot.is_empty() {
        return None;
    }
    let lines = side_lines(file, side);
    let mut best: Option<u32> = None;
    let mut i = 0;
    while i + snapshot.len() <= lines.len() {
        let first = line_no(lines[i], side);
        let mut matches = true;
        for (n, expected) in snapshot.iter().enumerate() {
            let l = lines[i + n];
            if l.text != *expected || line_no(l, side) != first + n as u32 {
                matches = false;
                break;
            }
        }
        if matches {
            let better = match best {
                None => true,
                Some(b) => first.abs_diff(near) < b.abs_diff(near),
            };
            if better {
                best = Some(first);
            }
        }
        i += 1;
    }
    best
}

fn reanchored(c: &ReviewComment, changes: &RepoChanges) -> ReviewComment {
    let order: [bool; 2] = if c.staged {
        [true, false]
    } else {
        [false, true]
    };
    for staged in order {
        let files = if staged {
            &changes.staged
        } else {
            &changes.unstaged
        };
        let file = files.iter().find(|f| f.path == c.file);
        let start = match file {
            Some(f) => find_snapshot(f, c.side, &c.snapshot, c.start),
            None => None,
        };
        if let Some(start) = start {
            let mut moved = c.clone();
            moved.staged = staged;
            moved.start = start;
            moved.end = start + c.snapshot.len() as u32 - 1;
            moved.outdated = false;
            return moved;
        }
    }
    let mut outdated = c.clone();
    outdated.outdated = true;
    outdated
}

/// Re-anchor comments against the current diff by matching their snapshot text, on the same
/// staged/unstaged diff first and then the other (staging a file moves it between them). A match
/// moves the comment to the new line numbers; no match marks it outdated, keeping its last position.
/// Outdated is not sticky: if the text comes back the comment is anchored again.
pub fn reanchor(comments: &[ReviewComment], changes: &RepoChanges) -> Vec<ReviewComment> {
    comments.iter().map(|c| reanchored(c, changes)).collect()
}

/// Like [`reanchor`], but updates the comments in place and returns whether any changed.
pub fn reanchor_in_place(comments: &mut [ReviewComment], changes: &RepoChanges) -> bool {
    let mut changed = false;
    for c in comments.iter_mut() {
        let moved = reanchored(c, changes);
        if moved != *c {
            *c = moved;
            changed = true;
        }
    }
    changed
}

/// Comments not yet sent to the agent.
pub fn pending_comments(comments: &[ReviewComment]) -> Vec<ReviewComment> {
    comments.iter().filter(|c| !c.sent).cloned().collect()
}

/// Escape text for use in XML content or a double-quoted attribute (lossless, see `unescape_xml`).
fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\r', "&#13;")
}

fn unescape_xml(text: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"&(amp|lt|gt|quot|#13);").expect("valid regex"));
    re.replace_all(text, |caps: &regex::Captures| -> String {
        let name = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        match name {
            "amp" => "&".to_string(),
            "lt" => "<".to_string(),
            "gt" => ">".to_string(),
            "quot" => "\"".to_string(),
            _ => "\r".to_string(),
        }
    })
    .into_owned()
}

/// What the agent says about a comment when it replies: a question for the reviewer, or "fixed".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReplyKind {
    Question,
    Resolved,
}

/// The name the agent calls the reply tool by (server `vettr`, tool `respond_to_comment`).
pub const REPLY_TOOL_NAME: &str = "mcp__vettr__respond_to_comment";

fn review_intro() -> String {
    format!(
        "Please address these review comments on your changes. They are in the <vettr-review> block \
below. Each <comment> has an id, the file, the side (new is the file as it is now, old is the \
code before your change) and the line range, followed by the commented <code> and the \
reviewer's <note>. If a comment is marked outdated, the code has since changed, so its line \
numbers may be stale. Reply to individual comments with the {} tool (comment_id, message, and \
optionally kind: question or resolved).",
        REPLY_TOOL_NAME
    )
}

/// The message sent to the agent: a short introduction and a `<vettr-review>` block with one
/// `<comment>` per comment (id, file, side, line range, the quoted code and the note). Everything
/// the reader needs to rebuild the comments is in the block (see [`parse_review`]).
pub fn format_review(comments: &[ReviewComment], round: u32) -> String {
    let entries: Vec<String> = comments
        .iter()
        .map(|c| {
            let lines = if c.start == c.end {
                format!("{}", c.start)
            } else {
                format!("{}-{}", c.start, c.end)
            };
            let outdated = if c.outdated { " outdated=\"true\"" } else { "" };
            let code = if c.snapshot.is_empty() {
                String::new()
            } else {
                format!("\n{}\n", escape_xml(&c.snapshot.join("\n")))
            };
            [
                format!(
                    "  <comment id=\"{}\" file=\"{}\" side=\"{}\" lines=\"{}\"{}>",
                    escape_xml(&c.id),
                    escape_xml(&c.file),
                    c.side.as_str(),
                    lines,
                    outdated
                ),
                format!("    <code>{}</code>", code),
                format!("    <note>{}</note>", escape_xml(&c.text)),
                "  </comment>".to_string(),
            ]
            .join("\n")
        })
        .collect();
    format!(
        "{}\n\n<vettr-review round=\"{}\">\n{}\n</vettr-review>",
        review_intro(),
        round,
        entries.join("\n")
    )
}

/// A comment as read back from a sent review: what the message carries, no staging or sent state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SentComment {
    pub id: String,
    pub file: String,
    pub side: Side,
    pub start: u32,
    pub end: u32,
    pub snapshot: Vec<String>,
    pub text: String,
    pub round: u32,
    pub outdated: bool,
}

/// The result of [`parse_review`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedReview {
    pub round: u32,
    pub comments: Vec<SentComment>,
}

fn attributes(source: &str) -> HashMap<String, String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#"([\w-]+)="([^"]*)""#).expect("valid regex"));
    let mut found: HashMap<String, String> = HashMap::new();
    for m in re.captures_iter(source) {
        let key = m.get(1).map(|x| x.as_str()).unwrap_or("");
        let value = m.get(2).map(|x| x.as_str()).unwrap_or("");
        found.insert(key.to_string(), unescape_xml(value));
    }
    found
}

/// Read the comments back out of a message made by [`format_review`]; the inverse of it. Returns
/// `None` when the text holds no `<vettr-review>` block. Malformed comments inside a block are
/// skipped.
pub fn parse_review(message: &str) -> Option<ParsedReview> {
    static BLOCK: OnceLock<Regex> = OnceLock::new();
    static COMMENT: OnceLock<Regex> = OnceLock::new();
    static RANGE: OnceLock<Regex> = OnceLock::new();
    let block_re = BLOCK.get_or_init(|| {
        Regex::new(r#"<vettr-review round="(\d+)">([\s\S]*?)</vettr-review>"#).expect("valid regex")
    });
    let comment_re = COMMENT.get_or_init(|| {
        Regex::new(
            r"<comment ([^>]*)>\s*<code>([\s\S]*?)</code>\s*<note>([\s\S]*?)</note>\s*</comment>",
        )
        .expect("valid regex")
    });
    let range_re = RANGE.get_or_init(|| Regex::new(r"^(\d+)(?:-(\d+))?$").expect("valid regex"));

    let block = block_re.captures(message)?;
    let round: u32 = block
        .get(1)
        .and_then(|m| m.as_str().parse::<u32>().ok())
        .unwrap_or(0);
    let body = block.get(2).map(|m| m.as_str()).unwrap_or("");
    let mut comments: Vec<SentComment> = Vec::new();
    for m in comment_re.captures_iter(body) {
        let attrs_src = m.get(1).map(|x| x.as_str()).unwrap_or("");
        let a = attributes(attrs_src);
        let lines_attr = a.get("lines").map(|s| s.as_str()).unwrap_or("");
        let range = match range_re.captures(lines_attr) {
            Some(r) => r,
            None => continue,
        };
        let id = match a.get("id") {
            Some(id) if !id.is_empty() => id.clone(),
            _ => continue,
        };
        let file = match a.get("file") {
            Some(f) => f.clone(),
            None => continue,
        };
        let side = match a.get("side").map(|s| s.as_str()) {
            Some("old") => Side::Old,
            Some("new") => Side::New,
            _ => continue,
        };
        let start: u32 = match range.get(1).and_then(|x| x.as_str().parse::<u32>().ok()) {
            Some(n) => n,
            None => continue,
        };
        let end: u32 = match range.get(2) {
            Some(x) => match x.as_str().parse::<u32>() {
                Ok(n) => n,
                Err(_) => continue,
            },
            None => start,
        };
        let code = unescape_xml(m.get(2).map(|x| x.as_str()).unwrap_or(""));
        let snapshot: Vec<String> = if code.is_empty() {
            Vec::new()
        } else {
            let mut chars = code.chars();
            chars.next();
            chars.next_back();
            chars.as_str().split('\n').map(|s| s.to_string()).collect()
        };
        comments.push(SentComment {
            id,
            file,
            side,
            start,
            end,
            snapshot,
            text: unescape_xml(m.get(3).map(|x| x.as_str()).unwrap_or("")),
            round,
            outdated: a.get("outdated").map(|s| s.as_str()) == Some("true"),
        });
    }
    Some(ParsedReview { round, comments })
}

/// The result of [`rehydrate`].
#[derive(Debug, Clone, PartialEq)]
pub struct Rehydrated {
    pub comments: Vec<ReviewComment>,
    /// The round to continue from.
    pub round: u32,
}

/// Rebuild the sent comments from what the transcript says was sent (a stored session being opened).
/// The sent comments in `current` are replaced; pending ones are kept, moved to the next round. The
/// message does not say whether a comment was on the staged or unstaged diff, so each starts on the
/// unstaged one and [`reanchor`] finds it on either. A comment sent in several rounds keeps its last
/// text.
pub fn rehydrate(
    current: &[ReviewComment],
    sent: &[SentComment],
    changes: Option<&RepoChanges>,
) -> Rehydrated {
    // Latest version of each id, in order of first appearance.
    let mut latest: Vec<SentComment> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for c in sent {
        let existing: Option<usize> = index.get(&c.id).copied();
        match existing {
            Some(i) => latest[i] = c.clone(),
            None => {
                index.insert(c.id.clone(), latest.len());
                latest.push(c.clone());
            }
        }
    }
    let round = sent.iter().map(|c| c.round).max().unwrap_or(0) + 1;
    let mut comments: Vec<ReviewComment> = latest
        .into_iter()
        .map(|c| ReviewComment {
            id: c.id,
            file: c.file,
            staged: false,
            side: c.side,
            start: c.start,
            end: c.end,
            snapshot: c.snapshot,
            text: c.text,
            round: c.round,
            sent: true,
            outdated: c.outdated,
        })
        .collect();
    for c in pending_comments(current) {
        let mut moved = c;
        moved.round = round;
        comments.push(moved);
    }
    if let Some(changes) = changes {
        comments = reanchor(&comments, changes);
    }
    Rehydrated { comments, round }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{ChangeStatus, Hunk, LineKind};

    fn line(kind: LineKind, old_no: Option<u32>, new_no: Option<u32>, text: &str) -> DiffLine {
        DiffLine {
            kind,
            old_no,
            new_no,
            text: text.to_string(),
        }
    }

    fn file() -> FileChange {
        FileChange {
            path: "a.ts".to_string(),
            old_path: None,
            status: ChangeStatus::Modified,
            binary: false,
            too_large: false,
            additions: 2,
            deletions: 1,
            hunks: vec![Hunk {
                header: "@@".to_string(),
                lines: vec![
                    line(LineKind::Context, Some(1), Some(1), "one"),
                    line(LineKind::Del, Some(2), None, "two"),
                    line(LineKind::Add, None, Some(2), "TWO"),
                    line(LineKind::Add, None, Some(3), "three"),
                ],
            }],
        }
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn comment() -> ReviewComment {
        ReviewComment {
            id: "c1".to_string(),
            file: "a.ts".to_string(),
            staged: false,
            side: Side::New,
            start: 2,
            end: 3,
            snapshot: strings(&["TWO", "three"]),
            text: "Why?".to_string(),
            round: 1,
            sent: false,
            outdated: false,
        }
    }

    #[test]
    fn snapshot_lines_quotes_the_lines_of_a_side_within_the_range() {
        assert_eq!(
            snapshot_lines(&file(), Side::New, 2, 3),
            strings(&["TWO", "three"])
        );
        assert_eq!(
            snapshot_lines(&file(), Side::Old, 1, 2),
            strings(&["one", "two"])
        );
    }

    #[test]
    fn snapshot_lines_skips_lines_outside_the_range() {
        assert_eq!(
            snapshot_lines(&file(), Side::New, 3, 3),
            strings(&["three"])
        );
        assert_eq!(
            snapshot_lines(&file(), Side::New, 9, 9),
            Vec::<String>::new()
        );
    }

    #[test]
    fn line_no_reads_the_number_for_a_side() {
        assert_eq!(
            line_no(&line(LineKind::Del, Some(2), None, "x"), Side::Old),
            2
        );
        assert_eq!(
            line_no(&line(LineKind::Add, None, Some(5), "x"), Side::New),
            5
        );
    }

    #[test]
    fn range_of_orders_a_range_whichever_way_it_was_selected() {
        assert_eq!(range_of(3, 5), (3, 5));
        assert_eq!(range_of(5, 3), (3, 5));
    }

    #[test]
    fn in_range_tests_membership_inclusively() {
        assert!(in_range(2, 4, 2));
        assert!(in_range(2, 4, 4));
        assert!(!in_range(2, 4, 5));
        assert!(!in_range(2, 4, 1));
    }

    #[test]
    fn ends_at_matches_only_the_exact_file_group_side_and_end_line() {
        let c = comment();
        assert!(ends_at(&c, "a.ts", false, Side::New, 3));
        assert!(!ends_at(&c, "b.ts", false, Side::New, 3));
        assert!(!ends_at(&c, "a.ts", true, Side::New, 3));
        assert!(!ends_at(&c, "a.ts", false, Side::Old, 3));
        assert!(!ends_at(&c, "a.ts", false, Side::New, 2));
    }

    #[test]
    fn pending_comments_drops_comments_already_sent() {
        let pending = comment();
        let sent = ReviewComment {
            id: "c2".to_string(),
            sent: true,
            ..comment()
        };
        assert_eq!(pending_comments(&[pending.clone(), sent]), vec![pending]);
    }

    #[test]
    fn format_wraps_the_comments_in_a_vettr_review_block_with_the_round() {
        let second = ReviewComment {
            id: "c2".to_string(),
            side: Side::Old,
            start: 2,
            end: 2,
            snapshot: strings(&["two"]),
            text: "Keep this".to_string(),
            ..comment()
        };
        let message = format_review(&[comment(), second], 2);
        let expected = [
            "<vettr-review round=\"2\">",
            "  <comment id=\"c1\" file=\"a.ts\" side=\"new\" lines=\"2-3\">",
            "    <code>\nTWO\nthree\n</code>",
            "    <note>Why?</note>",
            "  </comment>",
            "  <comment id=\"c2\" file=\"a.ts\" side=\"old\" lines=\"2\">",
            "    <code>\ntwo\n</code>",
            "    <note>Keep this</note>",
            "  </comment>",
            "</vettr-review>",
        ]
        .join("\n");
        assert!(message.contains(&expected), "{}", message);
    }

    #[test]
    fn format_introduces_the_format_before_the_block() {
        assert!(format_review(&[comment()], 1).starts_with("Please address these review comments"));
    }

    #[test]
    fn format_marks_outdated_comments() {
        let c = ReviewComment {
            outdated: true,
            ..comment()
        };
        assert!(format_review(&[c], 1).contains("lines=\"2-3\" outdated=\"true\">"));
    }

    #[test]
    fn format_escapes_markup_in_code_notes_and_attributes() {
        let c = ReviewComment {
            file: "a\"<b>.ts".to_string(),
            snapshot: strings(&["x < y && </code>"]),
            text: "</note> & \"q\"".to_string(),
            ..comment()
        };
        let message = format_review(&[c], 1);
        assert!(message.contains("file=\"a&quot;&lt;b&gt;.ts\""));
        assert!(message.contains("x &lt; y &amp;&amp; &lt;/code&gt;"));
        assert!(message.contains("<note>&lt;/note&gt; &amp; &quot;q&quot;</note>"));
    }

    fn round_trip(comments: &[ReviewComment], round: u32) -> Vec<SentComment> {
        parse_review(&format_review(comments, round))
            .map(|p| p.comments)
            .unwrap_or_default()
    }

    #[test]
    fn parse_returns_none_when_there_is_no_review_block() {
        assert_eq!(parse_review("just a message"), None);
    }

    #[test]
    fn parse_reads_back_what_format_wrote() {
        let c = ReviewComment {
            outdated: true,
            ..comment()
        };
        assert_eq!(
            round_trip(&[c], 3),
            vec![SentComment {
                id: "c1".to_string(),
                file: "a.ts".to_string(),
                side: Side::New,
                start: 2,
                end: 3,
                snapshot: strings(&["TWO", "three"]),
                text: "Why?".to_string(),
                round: 3,
                outdated: true,
            }]
        );
    }

    #[test]
    fn parse_reads_a_single_line_and_the_round() {
        let c = ReviewComment {
            start: 5,
            end: 5,
            ..comment()
        };
        let parsed = parse_review(&format_review(&[c], 4)).expect("a review");
        assert_eq!(parsed.round, 4);
        assert_eq!(parsed.comments[0].start, 5);
        assert_eq!(parsed.comments[0].end, 5);
    }

    #[test]
    fn parse_round_trips_hostile_text_exactly() {
        let nasty = ReviewComment {
            file: "we\"ird <&>.ts".to_string(),
            snapshot: strings(&["</code></comment>", "", "  ]]> & &amp; &lt;", "tab\there\r"]),
            text: "\n  </note></vettr-review> & \"quoted\" &amp;\r\nlast\n".to_string(),
            ..comment()
        };
        let back = round_trip(std::slice::from_ref(&nasty), 3);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].file, nasty.file);
        assert_eq!(back[0].snapshot, nasty.snapshot);
        assert_eq!(back[0].text, nasty.text);
    }

    #[test]
    fn parse_tells_no_snapshot_apart_from_one_blank_line() {
        let none = ReviewComment {
            snapshot: vec![],
            ..comment()
        };
        let blank = ReviewComment {
            snapshot: strings(&[""]),
            ..comment()
        };
        assert_eq!(round_trip(&[none], 3)[0].snapshot, Vec::<String>::new());
        assert_eq!(round_trip(&[blank], 3)[0].snapshot, strings(&[""]));
    }

    #[test]
    fn parse_finds_the_block_amid_other_text_and_skips_malformed_comments() {
        let message = format!("before\n{}\nafter", format_review(&[comment()], 1));
        assert_eq!(parse_review(&message).expect("a review").comments.len(), 1);
        let broken = "<vettr-review round=\"1\"><comment id=\"x\" file=\"f\" side=\"mid\" lines=\"1\"><code></code><note>n</note></comment></vettr-review>";
        assert_eq!(parse_review(broken).expect("a review").comments, vec![]);
        let wrap = |attrs: &str| -> String {
            format!(
                "<vettr-review round=\"1\"><comment {}><code></code><note>n</note></comment></vettr-review>",
                attrs
            )
        };
        for attrs in [
            "file=\"f\" side=\"new\" lines=\"1\"",
            "id=\"x\" side=\"new\" lines=\"1\"",
            "id=\"x\" file=\"f\" lines=\"1\"",
            "id=\"x\" file=\"f\" side=\"new\"",
            "id=\"x\" file=\"f\" side=\"new\" lines=\"a-b\"",
        ] {
            assert_eq!(
                parse_review(&wrap(attrs)).expect("a review").comments,
                vec![],
                "{}",
                attrs
            );
        }
        assert_eq!(
            parse_review(&wrap("id=\"x\" file=\"f\" side=\"new\" lines=\"1\""))
                .expect("a review")
                .comments
                .len(),
            1
        );
    }

    #[test]
    fn ends_at_never_anchors_an_outdated_comment_to_a_line() {
        let c = ReviewComment {
            outdated: true,
            ..comment()
        };
        assert!(!ends_at(&c, "a.ts", false, Side::New, 3));
    }

    fn changes(unstaged: Vec<FileChange>, staged: Vec<FileChange>) -> RepoChanges {
        RepoChanges { staged, unstaged }
    }

    fn shifted(offset: u32, texts: &[&str]) -> FileChange {
        let lines: Vec<DiffLine> = texts
            .iter()
            .enumerate()
            .map(|(n, t)| line(LineKind::Add, None, Some(2 + offset + n as u32), t))
            .collect();
        FileChange {
            hunks: vec![Hunk {
                header: "@@".to_string(),
                lines,
            }],
            ..file()
        }
    }

    #[test]
    fn reanchor_reports_no_change_when_nothing_moved() {
        let mut comments = vec![comment()];
        assert!(!reanchor_in_place(
            &mut comments,
            &changes(vec![file()], vec![])
        ));
        assert_eq!(comments, vec![comment()]);
        assert_eq!(
            reanchor(&comments, &changes(vec![file()], vec![])),
            comments
        );
    }

    #[test]
    fn reanchor_follows_the_snapshot_to_its_new_line_numbers() {
        let out = reanchor(
            &[comment()],
            &changes(vec![shifted(10, &["TWO", "three"])], vec![]),
        );
        assert_eq!(out[0].start, 12);
        assert_eq!(out[0].end, 13);
        assert!(!out[0].outdated);
    }

    #[test]
    fn reanchor_prefers_the_match_nearest_the_old_position() {
        let twice = FileChange {
            hunks: vec![Hunk {
                header: "@@".to_string(),
                lines: vec![
                    line(LineKind::Add, None, Some(1), "TWO"),
                    line(LineKind::Add, None, Some(2), "three"),
                    line(LineKind::Add, None, Some(20), "TWO"),
                    line(LineKind::Add, None, Some(21), "three"),
                    line(LineKind::Add, None, Some(40), "TWO"),
                    line(LineKind::Add, None, Some(41), "three"),
                ],
            }],
            ..file()
        };
        let c = ReviewComment {
            start: 19,
            end: 20,
            ..comment()
        };
        let out = reanchor(&[c], &changes(vec![twice], vec![]));
        assert_eq!(out[0].start, 20);
        assert_eq!(out[0].end, 21);
    }

    #[test]
    fn reanchor_does_not_match_lines_that_are_not_consecutive() {
        let mut gap = shifted(0, &["TWO"]);
        gap.hunks[0]
            .lines
            .push(line(LineKind::Add, None, Some(9), "three"));
        let out = reanchor(&[comment()], &changes(vec![gap], vec![]));
        assert!(out[0].outdated);
    }

    #[test]
    fn reanchor_marks_a_comment_outdated_when_the_text_changed_keeping_its_position() {
        let out = reanchor(
            &[comment()],
            &changes(vec![shifted(0, &["other", "three"])], vec![]),
        );
        assert_eq!(out[0].start, 2);
        assert_eq!(out[0].end, 3);
        assert!(out[0].outdated);
    }

    #[test]
    fn reanchor_marks_outdated_when_the_file_left_the_diff_or_the_snapshot_is_empty() {
        assert!(reanchor(&[comment()], &changes(vec![], vec![]))[0].outdated);
        let empty = ReviewComment {
            snapshot: vec![],
            ..comment()
        };
        assert!(reanchor(&[empty], &changes(vec![file()], vec![]))[0].outdated);
    }

    #[test]
    fn reanchor_revives_an_outdated_comment_when_its_text_comes_back() {
        let c = ReviewComment {
            outdated: true,
            ..comment()
        };
        assert!(!reanchor(&[c], &changes(vec![file()], vec![]))[0].outdated);
    }

    #[test]
    fn reanchor_follows_a_file_moved_between_the_unstaged_and_staged_diffs() {
        let out = reanchor(&[comment()], &changes(vec![], vec![file()]));
        assert!(out[0].staged);
        assert!(!out[0].outdated);
        let staged = ReviewComment {
            staged: true,
            ..comment()
        };
        let out = reanchor(&[staged], &changes(vec![file()], vec![]));
        assert!(!out[0].staged);
        assert!(!out[0].outdated);
    }

    #[test]
    fn reanchor_tolerates_the_same_file_being_absent_from_the_preferred_diff_only() {
        let other = FileChange {
            path: "b.ts".to_string(),
            ..file()
        };
        let out = reanchor(&[comment()], &changes(vec![other], vec![file()]));
        assert!(out[0].staged);
    }

    fn sent(id: &str, round: u32, text: &str) -> SentComment {
        SentComment {
            id: id.to_string(),
            file: "a.ts".to_string(),
            side: Side::New,
            start: 2,
            end: 3,
            snapshot: strings(&["TWO", "three"]),
            text: text.to_string(),
            round,
            outdated: false,
        }
    }

    fn here() -> RepoChanges {
        changes(vec![file()], vec![])
    }

    #[test]
    fn rehydrate_restores_sent_comments_anchored_where_their_snapshot_is() {
        let out = rehydrate(
            &[],
            &[sent("c1", 1, "c1"), sent("c2", 2, "c2")],
            Some(&here()),
        );
        let got: Vec<(String, bool, bool, u32)> = out
            .comments
            .iter()
            .map(|c| (c.id.clone(), c.sent, c.outdated, c.start))
            .collect();
        assert_eq!(
            got,
            vec![
                ("c1".to_string(), true, false, 2),
                ("c2".to_string(), true, false, 2),
            ]
        );
        assert_eq!(out.round, 3);
    }

    #[test]
    fn rehydrate_finds_a_comment_that_is_now_on_the_staged_diff() {
        let out = rehydrate(
            &[],
            &[sent("c1", 1, "c1")],
            Some(&changes(vec![], vec![file()])),
        );
        assert!(out.comments[0].staged);
        assert!(!out.comments[0].outdated);
    }

    #[test]
    fn rehydrate_marks_a_comment_outdated_when_its_code_is_gone() {
        let out = rehydrate(&[], &[sent("c1", 1, "c1")], Some(&changes(vec![], vec![])));
        assert!(out.comments[0].outdated);
    }

    #[test]
    fn rehydrate_replaces_old_sent_comments_keeps_pending_ones_and_moves_them_to_the_new_round() {
        let current = vec![
            ReviewComment {
                id: "old".to_string(),
                sent: true,
                ..comment()
            },
            ReviewComment {
                id: "draft".to_string(),
                round: 1,
                ..comment()
            },
        ];
        let out = rehydrate(&current, &[sent("c1", 4, "c1")], Some(&here()));
        let ids: Vec<&str> = out.comments.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["c1", "draft"]);
        assert_eq!(out.comments[1].round, out.round);
        assert_eq!(out.round, 5);
    }

    #[test]
    fn rehydrate_keeps_the_last_text_of_a_comment_sent_more_than_once() {
        let out = rehydrate(
            &[],
            &[sent("c1", 1, "first"), sent("c1", 2, "second")],
            Some(&here()),
        );
        assert_eq!(out.comments.len(), 1);
        assert_eq!(out.comments[0].text, "second");
        assert_eq!(out.comments[0].round, 2);
    }

    #[test]
    fn rehydrate_starts_at_round_1_with_nothing_sent_and_skips_anchoring_without_changes() {
        assert_eq!(rehydrate(&[], &[], Some(&here())).round, 1);
        assert!(!rehydrate(&[], &[sent("c1", 1, "c1")], None).comments[0].outdated);
    }
}
