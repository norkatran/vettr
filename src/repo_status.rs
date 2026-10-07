//! A snapshot of the repository state shown in the status bar (port of `src/shared/repoStatus.ts`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoStatus {
    /// Current branch, or `None` when HEAD is detached.
    pub branch: Option<String>,
    /// Abbreviated commit SHA of HEAD, or `None` before the first commit.
    pub sha: Option<String>,
    /// Upstream branch (for example `origin/main`), or `None` when none is configured.
    pub upstream: Option<String>,
    /// Commits ahead of and behind the upstream; both 0 when there is no upstream.
    pub ahead: u32,
    pub behind: u32,
    /// Number of changed paths, including untracked files.
    pub changes: u32,
}

const SHA_LENGTH: usize = 7;

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// Parse `# branch.ab +A -B`; `None` when malformed.
fn parse_ahead_behind(line: &str) -> Option<(u32, u32)> {
    let rest = line.strip_prefix("# branch.ab +")?;
    let (a, b) = rest.split_once(" -")?;
    if !all_digits(a) || !all_digits(b) {
        return None;
    }
    let ahead: u32 = a.parse().ok()?;
    let behind: u32 = b.parse().ok()?;
    Some((ahead, behind))
}

fn is_change_line(line: &str) -> bool {
    let bytes = line.as_bytes();
    bytes.len() >= 2 && matches!(bytes[0], b'1' | b'2' | b'u' | b'?') && bytes[1] == b' '
}

/// Parse the output of `git status --porcelain=v2 --branch`.
pub fn parse_repo_status(output: &str) -> RepoStatus {
    let mut status = RepoStatus {
        branch: None,
        sha: None,
        upstream: None,
        ahead: 0,
        behind: 0,
        changes: 0,
    };
    for line in output.split('\n') {
        if let Some(oid) = line.strip_prefix("# branch.oid ") {
            status.sha = if oid == "(initial)" {
                None
            } else {
                Some(oid.chars().take(SHA_LENGTH).collect::<String>())
            };
        } else if let Some(head) = line.strip_prefix("# branch.head ") {
            status.branch = if head == "(detached)" {
                None
            } else {
                Some(head.to_string())
            };
        } else if let Some(upstream) = line.strip_prefix("# branch.upstream ") {
            status.upstream = Some(upstream.to_string());
        } else if line.starts_with("# branch.ab ") {
            if let Some((ahead, behind)) = parse_ahead_behind(line) {
                status.ahead = ahead;
                status.behind = behind;
            }
        } else if is_change_line(line) {
            status.changes += 1;
        }
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_branch_with_an_upstream_divergence_and_changes() {
        let out = [
            "# branch.oid 0123456789abcdef",
            "# branch.head main",
            "# branch.upstream origin/main",
            "# branch.ab +2 -3",
            "1 .M N... 100644 100644 100644 aaa bbb src/a.ts",
            "2 R. N... 100644 100644 100644 aaa bbb R100 new.ts\told.ts",
            "u UU N... 100644 100644 100644 100644 aaa bbb ccc conflict.ts",
            "? untracked.ts",
            "! ignored.log",
            "",
        ]
        .join("\n");
        assert_eq!(
            parse_repo_status(&out),
            RepoStatus {
                branch: Some("main".to_string()),
                sha: Some("0123456".to_string()),
                upstream: Some("origin/main".to_string()),
                ahead: 2,
                behind: 3,
                changes: 4,
            }
        );
    }

    #[test]
    fn reports_no_upstream_and_a_clean_tree() {
        let out = "# branch.oid 0123456789abcdef\n# branch.head topic\n";
        assert_eq!(
            parse_repo_status(out),
            RepoStatus {
                branch: Some("topic".to_string()),
                sha: Some("0123456".to_string()),
                upstream: None,
                ahead: 0,
                behind: 0,
                changes: 0,
            }
        );
    }

    #[test]
    fn reports_a_detached_head_as_no_branch() {
        let s = parse_repo_status("# branch.oid 0123456789abcdef\n# branch.head (detached)\n");
        assert_eq!(s.branch, None);
        assert_eq!(s.sha, Some("0123456".to_string()));
    }

    #[test]
    fn reports_no_sha_before_the_first_commit() {
        let s = parse_repo_status("# branch.oid (initial)\n# branch.head main\n");
        assert_eq!(s.branch, Some("main".to_string()));
        assert_eq!(s.sha, None);
    }

    #[test]
    fn ignores_a_malformed_ahead_behind_line() {
        let s = parse_repo_status("# branch.ab nonsense\n");
        assert_eq!((s.ahead, s.behind), (0, 0));
    }

    #[test]
    fn handles_empty_output() {
        let s = parse_repo_status("");
        assert_eq!(s.branch, None);
        assert_eq!(s.sha, None);
        assert_eq!(s.changes, 0);
    }
}
