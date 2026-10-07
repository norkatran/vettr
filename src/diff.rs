//! Unified diff parser and the change types shared by the UI and the host (port of
//! `src/shared/diff.ts`).

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineKind {
    Context,
    Add,
    Del,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub kind: LineKind,
    /// Line numbers in the old and new file; `None` on the side the line does not exist.
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hunk {
    /// The `@@ -a,b +c,d @@ section` header line.
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    /// Path in the working tree (the new path for a rename).
    pub path: String,
    /// Previous path, only for renames.
    pub old_path: Option<String>,
    pub status: ChangeStatus,
    pub binary: bool,
    /// True when the diff was dropped for size; the counts are still accurate.
    pub too_large: bool,
    pub additions: usize,
    pub deletions: usize,
    pub hunks: Vec<Hunk>,
}

/// A file with more changed lines than this is listed but its diff is not rendered.
pub const MAX_CHANGED_LINES: usize = 5000;

/// One row of the split view: the old side on the left, the new side on the right.
#[derive(Debug, Clone, PartialEq)]
pub struct SplitRow<'a> {
    pub left: Option<&'a DiffLine>,
    pub right: Option<&'a DiffLine>,
}

/// Pair deletions with the additions that follow them so the split view lines them up.
pub fn split_rows(lines: &[DiffLine]) -> Vec<SplitRow<'_>> {
    let mut rows: Vec<SplitRow<'_>> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        if line.kind == LineKind::Context {
            rows.push(SplitRow {
                left: Some(line),
                right: Some(line),
            });
            i += 1;
            continue;
        }
        let mut dels: Vec<&DiffLine> = Vec::new();
        let mut adds: Vec<&DiffLine> = Vec::new();
        while i < lines.len() && lines[i].kind == LineKind::Del {
            dels.push(&lines[i]);
            i += 1;
        }
        while i < lines.len() && lines[i].kind == LineKind::Add {
            adds.push(&lines[i]);
            i += 1;
        }
        let count = dels.len().max(adds.len());
        for n in 0..count {
            rows.push(SplitRow {
                left: dels.get(n).copied(),
                right: adds.get(n).copied(),
            });
        }
    }
    rows
}

fn escape_byte(ch: char) -> Option<u8> {
    match ch {
        't' => Some(9),
        'n' => Some(10),
        'r' => Some(13),
        'a' => Some(7),
        'b' => Some(8),
        'f' => Some(12),
        'v' => Some(11),
        _ => None,
    }
}

/// Decode a path quoted by git (C-style escapes, octal for raw bytes).
pub fn unquote_path(raw: &str) -> String {
    if raw.chars().count() < 2 || !raw.starts_with('"') || !raw.ends_with('"') {
        return raw.to_string();
    }
    let body: Vec<char> = raw[1..raw.len() - 1].chars().collect();
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < body.len() {
        let ch = body[i];
        if ch != '\\' {
            let mut buf = [0u8; 4];
            bytes.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            i += 1;
            continue;
        }
        let is_octal =
            i + 3 < body.len() && body[i + 1..i + 4].iter().all(|c| ('0'..='7').contains(c));
        if is_octal {
            let mut value: u32 = 0;
            for c in &body[i + 1..i + 4] {
                value = value * 8 + (*c as u32 - '0' as u32);
            }
            bytes.push(value as u8);
            i += 4;
        } else {
            i += 1;
            match body.get(i) {
                Some(next) => {
                    let byte = match escape_byte(*next) {
                        Some(b) => b,
                        None => (*next as u32) as u8,
                    };
                    bytes.push(byte);
                    i += 1;
                }
                None => bytes.push(0),
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn quoted_header() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"^"a/((?:[^"\\]|\\.)*)" "b/(?:[^"\\]|\\.)*"$"#).expect("valid regex")
    })
}

fn hunk_header() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@").expect("valid regex"))
}

/// The path from a `diff --git a/X b/X` line. Only valid when both sides are the same path
/// (renames overwrite it from their `rename to` line, so the result is never used for those).
fn header_path(rest: &str) -> String {
    if let Some(caps) = quoted_header().captures(rest) {
        let inner = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        return unquote_path(&format!("\"{}\"", inner));
    }
    // "a/" + X + " b/" + X is 2|X| + 5 bytes long
    let end = rest.len().saturating_sub(5) / 2 + 2;
    rest.get(2..end).unwrap_or("").to_string()
}

/// Parse the output of `git diff` (patch format) into per-file changes.
pub fn parse_diff(output: &str) -> Vec<FileChange> {
    let mut files: Vec<FileChange> = Vec::new();
    let mut in_hunk = false;
    let mut old_no: u32 = 0;
    let mut new_no: u32 = 0;

    for line in output.split('\n') {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            files.push(FileChange {
                path: header_path(rest),
                old_path: None,
                status: ChangeStatus::Modified,
                binary: false,
                too_large: false,
                additions: 0,
                deletions: 0,
                hunks: Vec::new(),
            });
            in_hunk = false;
            continue;
        }
        let file = match files.last_mut() {
            Some(f) => f,
            None => continue,
        };
        if line.starts_with("@@ ") {
            let caps = hunk_header().captures(line);
            let num = |n: usize| -> u32 {
                caps.as_ref()
                    .and_then(|c| c.get(n))
                    .and_then(|m| m.as_str().parse::<u32>().ok())
                    .unwrap_or(0)
            };
            old_no = num(1);
            new_no = num(2);
            file.hunks.push(Hunk {
                header: line.to_string(),
                lines: Vec::new(),
            });
            in_hunk = true;
        } else if in_hunk {
            // Anything but these is "\ No newline at end of file" or the trailing blank line
            let text = line.get(1..).unwrap_or("").to_string();
            let hunk = match file.hunks.last_mut() {
                Some(h) => h,
                None => continue,
            };
            if line.starts_with('+') {
                hunk.lines.push(DiffLine {
                    kind: LineKind::Add,
                    old_no: None,
                    new_no: Some(new_no),
                    text,
                });
                new_no += 1;
                file.additions += 1;
            } else if line.starts_with('-') {
                hunk.lines.push(DiffLine {
                    kind: LineKind::Del,
                    old_no: Some(old_no),
                    new_no: None,
                    text,
                });
                old_no += 1;
                file.deletions += 1;
            } else if line.starts_with(' ') {
                hunk.lines.push(DiffLine {
                    kind: LineKind::Context,
                    old_no: Some(old_no),
                    new_no: Some(new_no),
                    text,
                });
                old_no += 1;
                new_no += 1;
            }
        } else if line.starts_with("new file mode") {
            file.status = ChangeStatus::Added;
        } else if line.starts_with("deleted file mode") {
            file.status = ChangeStatus::Deleted;
        } else if let Some(from) = line.strip_prefix("rename from ") {
            file.status = ChangeStatus::Renamed;
            file.old_path = Some(unquote_path(from));
        } else if let Some(to) = line.strip_prefix("rename to ") {
            file.path = unquote_path(to);
        } else if line.starts_with("Binary files ") || line.starts_with("GIT binary patch") {
            file.binary = true;
        }
    }

    for f in files.iter_mut() {
        if f.additions + f.deletions > MAX_CHANGED_LINES {
            f.too_large = true;
            f.hunks = Vec::new();
        }
    }
    files
}

/// The working-tree changes split by index state. A partially staged file is in both lists.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoChanges {
    /// Index against `HEAD`.
    pub staged: Vec<FileChange>,
    /// Working tree against the index, untracked files included.
    pub unstaged: Vec<FileChange>,
}

/// Number of distinct changed paths, so a partially staged file counts once.
pub fn changed_file_count(changes: &RepoChanges) -> usize {
    let mut paths: HashSet<&str> = HashSet::new();
    for f in changes.staged.iter().chain(changes.unstaged.iter()) {
        paths.insert(f.path.as_str());
    }
    paths.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dl(kind: LineKind, old_no: Option<u32>, new_no: Option<u32>, text: &str) -> DiffLine {
        DiffLine {
            kind,
            old_no,
            new_no,
            text: text.to_string(),
        }
    }

    #[test]
    fn returns_nothing_for_empty_output_or_text_before_the_first_file() {
        assert_eq!(parse_diff(""), vec![]);
        assert_eq!(parse_diff("warning: something\n"), vec![]);
    }

    #[test]
    fn parses_a_modified_file_with_line_numbers_and_counts() {
        let out = [
            "diff --git a/src/a.ts b/src/a.ts",
            "index 111..222 100644",
            "--- a/src/a.ts",
            "+++ b/src/a.ts",
            "@@ -1,3 +1,3 @@ function x() {",
            " keep",
            "-old",
            "+new",
            " tail",
            "\\ No newline at end of file",
            "@@ -10 +10,2 @@",
            "+added",
            " end",
            "",
        ]
        .join("\n");
        let files = parse_diff(&out);
        let file = &files[0];
        assert_eq!(file.path, "src/a.ts");
        assert_eq!(file.old_path, None);
        assert_eq!(file.status, ChangeStatus::Modified);
        assert!(!file.binary);
        assert!(!file.too_large);
        assert_eq!(file.additions, 2);
        assert_eq!(file.deletions, 1);
        assert_eq!(file.hunks.len(), 2);
        assert_eq!(file.hunks[0].header, "@@ -1,3 +1,3 @@ function x() {");
        assert_eq!(
            file.hunks[0].lines,
            vec![
                dl(LineKind::Context, Some(1), Some(1), "keep"),
                dl(LineKind::Del, Some(2), None, "old"),
                dl(LineKind::Add, None, Some(2), "new"),
                dl(LineKind::Context, Some(3), Some(3), "tail"),
            ]
        );
        assert_eq!(
            file.hunks[1].lines[0],
            dl(LineKind::Add, None, Some(10), "added")
        );
    }

    #[test]
    fn detects_added_and_deleted_files() {
        let out = [
            "diff --git a/new.txt b/new.txt",
            "new file mode 100644",
            "--- /dev/null",
            "+++ b/new.txt",
            "@@ -0,0 +1 @@",
            "+hi",
            "diff --git a/gone.txt b/gone.txt",
            "deleted file mode 100644",
            "--- a/gone.txt",
            "+++ /dev/null",
            "@@ -1 +0,0 @@",
            "-bye",
            "",
        ]
        .join("\n");
        let got: Vec<(String, ChangeStatus)> = parse_diff(&out)
            .iter()
            .map(|f| (f.path.clone(), f.status))
            .collect();
        assert_eq!(
            got,
            vec![
                ("new.txt".to_string(), ChangeStatus::Added),
                ("gone.txt".to_string(), ChangeStatus::Deleted),
            ]
        );
    }

    #[test]
    fn detects_renames_with_and_without_content_changes() {
        let out = [
            "diff --git a/old name.ts b/new name.ts",
            "similarity index 100%",
            "rename from old name.ts",
            "rename to new name.ts",
            "diff --git a/x.ts b/y.ts",
            "similarity index 90%",
            "rename from x.ts",
            "rename to y.ts",
            "@@ -1 +1 @@",
            "-a",
            "+b",
            "",
        ]
        .join("\n");
        let files = parse_diff(&out);
        let pure = &files[0];
        let edited = &files[1];
        assert_eq!(pure.path, "new name.ts");
        assert_eq!(pure.old_path, Some("old name.ts".to_string()));
        assert_eq!(pure.status, ChangeStatus::Renamed);
        assert!(pure.hunks.is_empty());
        assert_eq!(edited.path, "y.ts");
        assert_eq!(edited.old_path, Some("x.ts".to_string()));
        assert_eq!(edited.additions, 1);
        assert_eq!(edited.deletions, 1);
    }

    #[test]
    fn flags_binary_files() {
        let out = [
            "diff --git a/img.png b/img.png",
            "index 1..2 100644",
            "Binary files a/img.png and b/img.png differ",
            "diff --git a/blob.bin b/blob.bin",
            "GIT binary patch",
            "literal 3",
            "KcmZQz",
            "",
        ]
        .join("\n");
        let files = parse_diff(&out);
        let flags: Vec<bool> = files.iter().map(|f| f.binary).collect();
        assert_eq!(flags, vec![true, true]);
        assert!(files.iter().all(|f| f.hunks.is_empty()));
    }

    #[test]
    fn reads_paths_with_spaces_and_quoted_special_characters_from_the_header() {
        let out = [
            "diff --git a/my file.txt b/my file.txt",
            "old mode 100644",
            "new mode 100755",
            "diff --git \"a/tab\\there.txt\" \"b/tab\\there.txt\"",
            "new mode 100755",
            "",
        ]
        .join("\n");
        let paths: Vec<String> = parse_diff(&out).iter().map(|f| f.path.clone()).collect();
        assert_eq!(
            paths,
            vec!["my file.txt".to_string(), "tab\there.txt".to_string()]
        );
    }

    fn big_diff(count: usize) -> String {
        let mut lines: Vec<String> = vec![
            "diff --git a/big.txt b/big.txt".to_string(),
            "@@ -0,0 +1 @@".to_string(),
        ];
        for i in 0..count {
            lines.push(format!("+line {}", i));
        }
        lines.push(String::new());
        lines.join("\n")
    }

    #[test]
    fn drops_the_hunks_of_a_file_with_too_many_changed_lines_but_keeps_the_counts() {
        let files = parse_diff(&big_diff(MAX_CHANGED_LINES + 1));
        assert!(files[0].too_large);
        assert!(files[0].hunks.is_empty());
        assert_eq!(files[0].additions, MAX_CHANGED_LINES + 1);
    }

    #[test]
    fn keeps_a_file_at_exactly_the_limit() {
        assert!(!parse_diff(&big_diff(MAX_CHANGED_LINES))[0].too_large);
    }

    #[test]
    fn unquote_returns_unquoted_paths_unchanged() {
        assert_eq!(unquote_path("plain.txt"), "plain.txt");
        assert_eq!(unquote_path("\""), "\"");
        assert_eq!(unquote_path("\"half"), "\"half");
    }

    #[test]
    fn unquote_decodes_named_escapes_quotes_backslashes_and_unicode() {
        assert_eq!(unquote_path("\"a\\tb\\nc\\\"d\\\\e\\u\""), "a\tb\nc\"d\\eu");
        assert_eq!(unquote_path("\"café\""), "café");
    }

    #[test]
    fn unquote_decodes_octal_escapes_as_utf8_bytes() {
        assert_eq!(unquote_path("\"caf\\303\\251\""), "café");
    }

    fn ctx(n: u32) -> DiffLine {
        dl(LineKind::Context, Some(n), Some(n), &format!("c{}", n))
    }
    fn del(n: u32) -> DiffLine {
        dl(LineKind::Del, Some(n), None, &format!("d{}", n))
    }
    fn add(n: u32) -> DiffLine {
        dl(LineKind::Add, None, Some(n), &format!("a{}", n))
    }

    #[test]
    fn split_shows_context_on_both_sides() {
        let lines = vec![ctx(1)];
        assert_eq!(
            split_rows(&lines),
            vec![SplitRow {
                left: Some(&lines[0]),
                right: Some(&lines[0])
            }]
        );
    }

    #[test]
    fn split_pairs_deletions_with_the_additions_that_follow() {
        let lines = vec![del(1), del(2), add(1), add(2)];
        assert_eq!(
            split_rows(&lines),
            vec![
                SplitRow {
                    left: Some(&lines[0]),
                    right: Some(&lines[2])
                },
                SplitRow {
                    left: Some(&lines[1]),
                    right: Some(&lines[3])
                },
            ]
        );
    }

    #[test]
    fn split_pads_the_shorter_side_of_a_change_block() {
        let lines = vec![del(1), del(2), add(1), ctx(3)];
        assert_eq!(
            split_rows(&lines),
            vec![
                SplitRow {
                    left: Some(&lines[0]),
                    right: Some(&lines[2])
                },
                SplitRow {
                    left: Some(&lines[1]),
                    right: None
                },
                SplitRow {
                    left: Some(&lines[3]),
                    right: Some(&lines[3])
                },
            ]
        );
        let adds = vec![add(1), add(2)];
        assert_eq!(
            split_rows(&adds),
            vec![
                SplitRow {
                    left: None,
                    right: Some(&adds[0])
                },
                SplitRow {
                    left: None,
                    right: Some(&adds[1])
                },
            ]
        );
    }

    #[test]
    fn split_returns_no_rows_for_no_lines() {
        assert_eq!(split_rows(&[]), vec![]);
    }

    fn file(path: &str) -> FileChange {
        FileChange {
            path: path.to_string(),
            old_path: None,
            status: ChangeStatus::Modified,
            binary: false,
            too_large: false,
            additions: 1,
            deletions: 0,
            hunks: vec![],
        }
    }

    #[test]
    fn changed_file_count_is_zero_with_no_changes() {
        assert_eq!(changed_file_count(&RepoChanges::default()), 0);
    }

    #[test]
    fn changed_file_count_counts_a_partially_staged_file_once() {
        let changes = RepoChanges {
            staged: vec![file("a"), file("b")],
            unstaged: vec![file("b"), file("c")],
        };
        assert_eq!(changed_file_count(&changes), 3);
    }
}
