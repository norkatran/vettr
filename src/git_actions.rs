//! Global git commands run from the command palette (port of `src/shared/gitActions.ts`).
//! Each maps to one or more `git` invocations.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GitAction {
    Fetch,
    Pull,
    StageAll,
    UnstageAll,
    DiscardAll,
    Stash,
    StashPop,
    CreateBranch { name: String },
    Checkout { name: String },
    DeleteBranch { name: String },
    Merge { name: String },
    Rebase { name: String },
}

/// What to run for an action: commands in order (stopping at the first failure) and a cleanup.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitPlan {
    pub steps: Vec<Vec<String>>,
    /// Run (ignoring errors) when a step fails, so a half-done merge or rebase is not left behind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abort_on_failure: Option<Vec<String>>,
    /// Appended to git's output when the cleanup ran, so the user knows what state the repo is in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abort_note: Option<String>,
}

/// Why a branch name cannot be used, or `None`. A leading `-` would be read as an option by git.
pub fn invalid_branch_name(name: &str) -> Option<String> {
    if name.trim().is_empty() {
        return Some("Enter a branch name.".to_string());
    }
    if name.starts_with('-') {
        return Some("A branch name cannot start with \"-\".".to_string());
    }
    None
}

fn cmd(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| a.to_string()).collect()
}

fn simple(steps: Vec<Vec<String>>) -> GitPlan {
    GitPlan {
        steps,
        abort_on_failure: None,
        abort_note: None,
    }
}

/// The git commands for `action`. Names must pass `invalid_branch_name` first.
pub fn plan_git_action(action: &GitAction) -> GitPlan {
    match action {
        GitAction::Fetch => simple(vec![cmd(&["fetch", "--prune"])]),
        GitAction::Pull => simple(vec![cmd(&["pull"])]),
        GitAction::StageAll => simple(vec![cmd(&["add", "--all"])]),
        GitAction::UnstageAll => simple(vec![cmd(&["reset", "-q"])]),
        // Reset tracked files, then remove untracked ones (ignored files are kept)
        GitAction::DiscardAll => simple(vec![
            cmd(&["reset", "-q", "--hard"]),
            cmd(&["clean", "-fdq"]),
        ]),
        GitAction::Stash => simple(vec![cmd(&["stash", "push", "--include-untracked"])]),
        GitAction::StashPop => simple(vec![cmd(&["stash", "pop"])]),
        GitAction::CreateBranch { name } => simple(vec![cmd(&["switch", "-c", name.as_str()])]),
        GitAction::Checkout { name } => simple(vec![cmd(&["switch", name.as_str()])]),
        // `-d` refuses to delete a branch with unmerged work
        GitAction::DeleteBranch { name } => simple(vec![cmd(&["branch", "-d", name.as_str()])]),
        GitAction::Merge { name } => GitPlan {
            steps: vec![cmd(&["merge", "--no-edit", name.as_str()])],
            abort_on_failure: Some(cmd(&["merge", "--abort"])),
            abort_note: Some(
                "The merge was aborted. Resolve conflicts in a terminal with `git merge`."
                    .to_string(),
            ),
        },
        GitAction::Rebase { name } => GitPlan {
            steps: vec![cmd(&["rebase", name.as_str()])],
            abort_on_failure: Some(cmd(&["rebase", "--abort"])),
            abort_note: Some(
                "The rebase was aborted. Resolve conflicts in a terminal with `git rebase`."
                    .to_string(),
            ),
        },
    }
}

/// A branch or remote-tracking ref, as listed by `list_branches`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    /// `main` for a local branch, `origin/main` for a remote one. (`ref` is a Rust keyword, so the
    /// field is the raw identifier `r#ref`; it serialises as `ref`.)
    #[serde(rename = "ref")]
    pub r#ref: String,
    pub remote: bool,
    pub current: bool,
}

/// Parse `git for-each-ref --format=%(HEAD)%(refname)` output over refs/heads and refs/remotes.
pub fn parse_branches(stdout: &str) -> Vec<Branch> {
    let mut branches: Vec<Branch> = Vec::new();
    for line in stdout.split('\n') {
        let first = match line.chars().next() {
            Some(c) => c,
            None => continue,
        };
        let current = first == '*';
        let refname = line[first.len_utf8()..].trim();
        if let Some(name) = refname.strip_prefix("refs/heads/") {
            branches.push(Branch {
                r#ref: name.to_string(),
                remote: false,
                current,
            });
        } else if let Some(name) = refname.strip_prefix("refs/remotes/") {
            // `origin/HEAD` is a pointer to another branch, not a branch
            if !name.ends_with("/HEAD") {
                branches.push(Branch {
                    r#ref: name.to_string(),
                    remote: true,
                    current: false,
                });
            }
        }
    }
    // Stable sort: local branches first, order otherwise kept
    branches.sort_by_key(|b| b.remote);
    branches
}

/// The name `git switch` takes for a branch: remote ones drop the remote prefix (it then tracks).
pub fn switch_name(branch: &Branch) -> String {
    if branch.remote {
        match branch.r#ref.find('/') {
            Some(i) => branch.r#ref[i + 1..].to_string(),
            None => branch.r#ref.clone(),
        }
    } else {
        branch.r#ref.clone()
    }
}

/// Branches to offer for Change Branch: all but the current one, hiding remote copies of local branches.
pub fn switch_targets(branches: &[Branch]) -> Vec<Branch> {
    let local: HashSet<String> = branches
        .iter()
        .filter(|b| !b.remote)
        .map(|b| b.r#ref.clone())
        .collect();
    branches
        .iter()
        .filter(|b| !b.current && (!b.remote || !local.contains(&switch_name(b))))
        .cloned()
        .collect()
}

/// Local branches that can be deleted (not the checked-out one).
pub fn deletable_branches(branches: &[Branch]) -> Vec<Branch> {
    branches
        .iter()
        .filter(|b| !b.remote && !b.current)
        .cloned()
        .collect()
}

/// Refs to offer for merge and rebase: everything except the current branch.
pub fn merge_targets(branches: &[Branch]) -> Vec<Branch> {
    branches.iter().filter(|b| !b.current).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps(action: GitAction) -> Vec<Vec<String>> {
        plan_git_action(&action).steps
    }

    fn name(s: &str) -> String {
        s.to_string()
    }

    #[test]
    fn invalid_branch_name_rejects_blank_and_option_like_names() {
        assert!(invalid_branch_name("  ").unwrap().contains("Enter"));
        assert!(invalid_branch_name("-x").unwrap().contains("cannot start"));
        assert_eq!(invalid_branch_name("feature/x"), None);
    }

    #[test]
    fn plans_each_simple_action() {
        assert_eq!(steps(GitAction::Fetch), vec![cmd(&["fetch", "--prune"])]);
        assert_eq!(steps(GitAction::Pull), vec![cmd(&["pull"])]);
        assert_eq!(steps(GitAction::StageAll), vec![cmd(&["add", "--all"])]);
        assert_eq!(steps(GitAction::UnstageAll), vec![cmd(&["reset", "-q"])]);
        assert_eq!(
            steps(GitAction::DiscardAll),
            vec![cmd(&["reset", "-q", "--hard"]), cmd(&["clean", "-fdq"])]
        );
        assert_eq!(
            steps(GitAction::Stash),
            vec![cmd(&["stash", "push", "--include-untracked"])]
        );
        assert_eq!(steps(GitAction::StashPop), vec![cmd(&["stash", "pop"])]);
    }

    #[test]
    fn plans_branch_actions() {
        assert_eq!(
            steps(GitAction::CreateBranch { name: name("a") }),
            vec![cmd(&["switch", "-c", "a"])]
        );
        assert_eq!(
            steps(GitAction::Checkout { name: name("a") }),
            vec![cmd(&["switch", "a"])]
        );
        assert_eq!(
            steps(GitAction::DeleteBranch { name: name("a") }),
            vec![cmd(&["branch", "-d", "a"])]
        );
    }

    #[test]
    fn aborts_a_failed_merge_or_rebase() {
        let merge = plan_git_action(&GitAction::Merge { name: name("main") });
        assert_eq!(merge.steps, vec![cmd(&["merge", "--no-edit", "main"])]);
        assert_eq!(merge.abort_on_failure, Some(cmd(&["merge", "--abort"])));
        assert!(merge.abort_note.unwrap().contains("merge was aborted"));
        let rebase = plan_git_action(&GitAction::Rebase { name: name("main") });
        assert_eq!(rebase.steps, vec![cmd(&["rebase", "main"])]);
        assert_eq!(rebase.abort_on_failure, Some(cmd(&["rebase", "--abort"])));
        assert!(rebase.abort_note.unwrap().contains("rebase was aborted"));
    }

    fn listing() -> String {
        [
            "*refs/heads/main",
            " refs/heads/dev",
            " refs/remotes/origin/HEAD",
            " refs/remotes/origin/main",
            " refs/remotes/origin/feature/x",
            "",
        ]
        .join("\n")
    }

    fn branch(r: &str, remote: bool, current: bool) -> Branch {
        Branch {
            r#ref: r.to_string(),
            remote,
            current,
        }
    }

    fn refs(branches: &[Branch]) -> Vec<String> {
        branches.iter().map(|b| b.r#ref.clone()).collect()
    }

    #[test]
    fn parse_branches_local_then_remote_skipping_head_pointers_and_blanks() {
        assert_eq!(
            parse_branches(&listing()),
            vec![
                branch("main", false, true),
                branch("dev", false, false),
                branch("origin/main", true, false),
                branch("origin/feature/x", true, false),
            ]
        );
    }

    #[test]
    fn parse_branches_ignores_unrelated_refs() {
        assert_eq!(parse_branches(" refs/tags/v1"), Vec::<Branch>::new());
    }

    #[test]
    fn switch_name_drops_the_remote_prefix() {
        assert_eq!(
            switch_name(&branch("origin/feature/x", true, false)),
            "feature/x"
        );
        assert_eq!(switch_name(&branch("dev", false, false)), "dev");
    }

    #[test]
    fn switch_targets_exclude_current_and_remote_copies_of_local_ones() {
        let branches = parse_branches(&listing());
        assert_eq!(
            refs(&switch_targets(&branches)),
            vec![name("dev"), name("origin/feature/x")]
        );
    }

    #[test]
    fn deletable_branches_are_local_and_not_current() {
        let branches = parse_branches(&listing());
        assert_eq!(refs(&deletable_branches(&branches)), vec![name("dev")]);
    }

    #[test]
    fn merge_targets_are_everything_but_the_current_branch() {
        let branches = parse_branches(&listing());
        assert_eq!(
            refs(&merge_targets(&branches)),
            vec![name("dev"), name("origin/main"), name("origin/feature/x")]
        );
    }

    #[test]
    fn actions_serialise_like_the_ts_union() {
        let json = serde_json::to_string(&GitAction::StashPop).unwrap();
        assert_eq!(json, "{\"kind\":\"stashPop\"}");
        let json = serde_json::to_string(&GitAction::CreateBranch { name: name("a") }).unwrap();
        assert_eq!(json, "{\"kind\":\"createBranch\",\"name\":\"a\"}");
    }
}
