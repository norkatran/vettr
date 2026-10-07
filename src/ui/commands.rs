//! The global command registry (port of `commands.ts`), as plain data and pure functions.
//!
//! The TypeScript commands awaited prompts; here each command's first step is computed by
//! `start_command`, which returns an `Effect`: the palette (`command_palette.rs`) performs it,
//! which may mean opening a follow-up pick or input and calling the matching function below with
//! the answer.

use crate::git_actions::{
    deletable_branches, invalid_branch_name, merge_targets, switch_name, switch_targets, Branch,
    GitAction,
};
use crate::sessions::{session_label, SessionInfo};

use super::command_palette::AppRequest;
use super::sidebar::View;

/// One row of a pick list.
#[derive(Debug, Clone, PartialEq)]
pub struct PaletteItem {
    pub id: String,
    pub label: String,
    /// Muted text on the right, such as a "remote" marker.
    pub detail: Option<String>,
}

impl PaletteItem {
    pub fn new(id: &str, label: &str) -> PaletteItem {
        PaletteItem {
            id: id.to_string(),
            label: label.to_string(),
            detail: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandId {
    NewSession,
    ListSessions,
    JumpSession,
    JumpChanges,
    JumpSettings,
    Fetch,
    Pull,
    Push,
    Publish,
    NewBranch,
    ChangeBranch,
    DeleteBranch,
    Commit,
    StageAll,
    UnstageAll,
    DiscardAll,
    Stash,
    StashPop,
    Merge,
    Rebase,
}

/// A global command. Every command is always listed; one that cannot run explains why.
#[derive(Debug, Clone, Copy)]
pub struct Command {
    pub id: CommandId,
    /// Stable identifier, also the id of its palette row.
    pub key: &'static str,
    pub category: &'static str,
    pub title: &'static str,
    /// False for commands that work without an open project.
    pub needs_project: bool,
}

const fn command(
    id: CommandId,
    key: &'static str,
    category: &'static str,
    title: &'static str,
    needs_project: bool,
) -> Command {
    Command {
        id,
        key,
        category,
        title,
        needs_project,
    }
}

pub static COMMANDS: [Command; 20] = [
    command(
        CommandId::NewSession,
        "new-session",
        "Session",
        "New Session",
        false,
    ),
    command(
        CommandId::ListSessions,
        "list-sessions",
        "Session",
        "List Sessions",
        true,
    ),
    command(
        CommandId::JumpSession,
        "jump-session",
        "Jump To",
        "Session",
        false,
    ),
    command(
        CommandId::JumpChanges,
        "jump-changes",
        "Jump To",
        "Changes",
        false,
    ),
    command(
        CommandId::JumpSettings,
        "jump-settings",
        "Jump To",
        "Settings",
        false,
    ),
    command(CommandId::Fetch, "fetch", "Git", "Fetch", true),
    command(CommandId::Pull, "pull", "Git", "Pull", true),
    command(CommandId::Push, "push", "Git", "Push", true),
    command(CommandId::Publish, "publish", "Git", "Publish Branch", true),
    command(
        CommandId::NewBranch,
        "new-branch",
        "Git",
        "New Branch",
        true,
    ),
    command(
        CommandId::ChangeBranch,
        "change-branch",
        "Git",
        "Change Branch",
        true,
    ),
    command(
        CommandId::DeleteBranch,
        "delete-branch",
        "Git",
        "Delete Branch",
        true,
    ),
    command(CommandId::Commit, "commit", "Git", "Commit", true),
    command(CommandId::StageAll, "stage-all", "Git", "Stage All", true),
    command(
        CommandId::UnstageAll,
        "unstage-all",
        "Git",
        "Unstage All",
        true,
    ),
    command(
        CommandId::DiscardAll,
        "discard-all",
        "Git",
        "Discard All Changes",
        true,
    ),
    command(CommandId::Stash, "stash", "Git", "Stash", true),
    command(CommandId::StashPop, "stash-pop", "Git", "Pop Stash", true),
    command(
        CommandId::Merge,
        "merge",
        "Git",
        "Merge Branch into Current",
        true,
    ),
    command(
        CommandId::Rebase,
        "rebase",
        "Git",
        "Rebase Current Branch onto",
        true,
    ),
];

impl Command {
    /// "Category: Title", as listed in the palette.
    pub fn label(&self) -> String {
        format!("{}: {}", self.category, self.title)
    }
}

/// The rows of the command list.
pub fn command_items() -> Vec<PaletteItem> {
    COMMANDS
        .iter()
        .map(|c| PaletteItem {
            id: c.key.to_string(),
            label: c.label(),
            detail: None,
        })
        .collect()
}

pub fn find_command(key: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.key == key)
}

/// A git operation run in the background.
#[derive(Debug, Clone, PartialEq)]
pub enum GitJob {
    Action(GitAction),
    Push,
    Publish(String),
}

/// Which branch command is picking a branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchOp {
    Change,
    Delete,
    Merge,
    Rebase,
}

impl BranchOp {
    /// Used in failure notifications ("<title> failed").
    pub fn title(&self) -> &'static str {
        match self {
            BranchOp::Change => "Change branch",
            BranchOp::Delete => "Delete branch",
            BranchOp::Merge => "Merge",
            BranchOp::Rebase => "Rebase",
        }
    }

    pub fn placeholder(&self) -> &'static str {
        match self {
            BranchOp::Change => "Switch to which branch?",
            BranchOp::Delete => "Delete which branch?",
            BranchOp::Merge => "Merge which branch into the current one?",
            BranchOp::Rebase => "Rebase the current branch onto?",
        }
    }

    /// The branches this operation can pick from.
    pub fn filter(&self, branches: &[Branch]) -> Vec<Branch> {
        match self {
            BranchOp::Change => switch_targets(branches),
            BranchOp::Delete => deletable_branches(branches),
            BranchOp::Merge | BranchOp::Rebase => merge_targets(branches),
        }
    }

    pub fn action(&self, branch: &Branch) -> GitAction {
        match self {
            BranchOp::Change => GitAction::Checkout {
                name: switch_name(branch),
            },
            BranchOp::Delete => GitAction::DeleteBranch {
                name: branch.r#ref.clone(),
            },
            BranchOp::Merge => GitAction::Merge {
                name: branch.r#ref.clone(),
            },
            BranchOp::Rebase => GitAction::Rebase {
                name: branch.r#ref.clone(),
            },
        }
    }
}

pub fn branch_items(branches: &[Branch]) -> Vec<PaletteItem> {
    branches
        .iter()
        .map(|b| PaletteItem {
            id: b.r#ref.clone(),
            label: b.r#ref.clone(),
            detail: if b.remote {
                Some("remote".to_string())
            } else {
                None
            },
        })
        .collect()
}

pub fn session_items(sessions: &[SessionInfo]) -> Vec<PaletteItem> {
    sessions
        .iter()
        .map(|s| PaletteItem {
            id: s.id.clone(),
            label: session_label(s),
            detail: None,
        })
        .collect()
}

/// What the palette should do next.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Request(AppRequest),
    Notify {
        title: String,
        detail: String,
    },
    Git {
        title: String,
        job: GitJob,
    },
    /// List the branches, then call `branch_effect`.
    ListBranches(BranchOp),
    /// List the remotes, then call `remote_effect`.
    ListRemotes,
    /// Pick one of these branches (already filtered), then call `pick_branch`.
    PickBranch(BranchOp, Vec<Branch>),
    /// Pick a remote to publish to.
    PickRemote(Vec<String>),
    AskBranchName,
    AskDiscard,
    PickSession(Vec<PaletteItem>),
}

fn notify(title: &str, detail: &str) -> Effect {
    Effect::Notify {
        title: title.to_string(),
        detail: detail.to_string(),
    }
}

fn git(title: &str, action: GitAction) -> Effect {
    Effect::Git {
        title: title.to_string(),
        job: GitJob::Action(action),
    }
}

/// The first step of a command.
pub fn start_command(
    command: &Command,
    project: Option<&str>,
    staged_count: usize,
    sessions: &[SessionInfo],
) -> Effect {
    if command.needs_project && project.is_none() {
        return notify(
            "No project open",
            "Open a project first (File > Open Project).",
        );
    }
    match command.id {
        CommandId::NewSession => Effect::Request(AppRequest::NewSession),
        CommandId::ListSessions => {
            if sessions.is_empty() {
                notify("No sessions", "This project has no stored sessions.")
            } else {
                Effect::PickSession(session_items(sessions))
            }
        }
        CommandId::JumpSession => Effect::Request(AppRequest::ShowView(View::Session)),
        CommandId::JumpChanges => Effect::Request(AppRequest::ShowView(View::Changes)),
        CommandId::JumpSettings => Effect::Request(AppRequest::ShowView(View::Settings)),
        CommandId::Fetch => git("Fetch", GitAction::Fetch),
        CommandId::Pull => git("Pull", GitAction::Pull),
        CommandId::Push => Effect::Git {
            title: "Push".to_string(),
            job: GitJob::Push,
        },
        CommandId::Publish => Effect::ListRemotes,
        CommandId::NewBranch => Effect::AskBranchName,
        CommandId::ChangeBranch => Effect::ListBranches(BranchOp::Change),
        CommandId::DeleteBranch => Effect::ListBranches(BranchOp::Delete),
        CommandId::Commit => {
            if staged_count == 0 {
                notify(
                    "Nothing to commit",
                    "No files are staged. Stage files first (Git: Stage All).",
                )
            } else {
                Effect::Request(AppRequest::FocusCommit)
            }
        }
        CommandId::StageAll => git("Stage all", GitAction::StageAll),
        CommandId::UnstageAll => git("Unstage all", GitAction::UnstageAll),
        CommandId::DiscardAll => Effect::AskDiscard,
        CommandId::Stash => git("Stash", GitAction::Stash),
        CommandId::StashPop => git("Pop stash", GitAction::StashPop),
        CommandId::Merge => Effect::ListBranches(BranchOp::Merge),
        CommandId::Rebase => Effect::ListBranches(BranchOp::Rebase),
    }
}

/// After listing the branches for `op`.
pub fn branch_effect(op: BranchOp, branches: &[Branch]) -> Effect {
    let choices = op.filter(branches);
    if choices.is_empty() {
        return Effect::Notify {
            title: format!("{} failed", op.title()),
            detail: "There are no other branches.".to_string(),
        };
    }
    Effect::PickBranch(op, choices)
}

/// After the user picked the branch `id` among `choices`; `None` if it is not one of them.
pub fn pick_branch(op: BranchOp, choices: &[Branch], id: &str) -> Option<Effect> {
    let branch = choices.iter().find(|b| b.r#ref == id)?;
    Some(Effect::Git {
        title: op.title().to_string(),
        job: GitJob::Action(op.action(branch)),
    })
}

/// After listing the remotes for "Publish Branch".
pub fn remote_effect(remotes: &[String]) -> Effect {
    if remotes.is_empty() {
        return notify(
            "Cannot publish branch",
            "This repository has no remotes. Add one with `git remote add`.",
        );
    }
    if remotes.len() == 1 {
        return Effect::Git {
            title: "Publish".to_string(),
            job: GitJob::Publish(remotes[0].clone()),
        };
    }
    Effect::PickRemote(remotes.to_vec())
}

pub fn remote_items(remotes: &[String]) -> Vec<PaletteItem> {
    remotes.iter().map(|r| PaletteItem::new(r, r)).collect()
}

/// After the user typed a branch name.
pub fn new_branch_effect(typed: &str) -> Effect {
    let name = typed.trim();
    if let Some(invalid) = invalid_branch_name(name) {
        return notify("Cannot create branch", &invalid);
    }
    git(
        "New branch",
        GitAction::CreateBranch {
            name: name.to_string(),
        },
    )
}

pub const DISCARD_PROMPT: &str =
    "Discard all changes and delete untracked files? This cannot be undone.";

pub fn discard_items() -> Vec<PaletteItem> {
    vec![
        PaletteItem::new("discard", "Discard all changes"),
        PaletteItem::new("cancel", "Cancel"),
    ]
}

/// After the user answered the discard question; `None` cancels.
pub fn discard_effect(answer: &str) -> Option<Effect> {
    if answer == "discard" {
        Some(git("Discard", GitAction::DiscardAll))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn branch(name: &str, remote: bool, current: bool) -> Branch {
        Branch {
            r#ref: name.to_string(),
            remote,
            current,
        }
    }

    fn start(key: &str, project: Option<&str>, staged: usize) -> Effect {
        start_command(find_command(key).unwrap(), project, staged, &[])
    }

    #[test]
    fn lists_every_command_with_a_category_prefix() {
        let items = command_items();
        assert_eq!(items.len(), COMMANDS.len());
        assert!(items.iter().any(|i| i.label == "Git: Pop Stash"));
        assert!(items.iter().any(|i| i.label == "Jump To: Settings"));
    }

    #[test]
    fn project_commands_explain_when_no_project_is_open() {
        match start("fetch", None, 0) {
            Effect::Notify { title, .. } => assert_eq!(title, "No project open"),
            other => panic!("unexpected {:?}", other),
        }
        assert_eq!(
            start("new-session", None, 0),
            Effect::Request(AppRequest::NewSession)
        );
        assert_eq!(
            start("jump-changes", None, 0),
            Effect::Request(AppRequest::ShowView(View::Changes))
        );
    }

    #[test]
    fn simple_git_commands_run_their_action() {
        assert_eq!(start("pull", Some("/p"), 0), git("Pull", GitAction::Pull));
        assert_eq!(
            start("stash-pop", Some("/p"), 0),
            git("Pop stash", GitAction::StashPop)
        );
    }

    #[test]
    fn commit_needs_staged_files() {
        assert!(matches!(
            start("commit", Some("/p"), 0),
            Effect::Notify { .. }
        ));
        assert_eq!(
            start("commit", Some("/p"), 2),
            Effect::Request(AppRequest::FocusCommit)
        );
    }

    #[test]
    fn list_sessions_notifies_when_there_are_none() {
        assert!(matches!(
            start("list-sessions", Some("/p"), 0),
            Effect::Notify { .. }
        ));
        let sessions = vec![SessionInfo {
            id: "a".to_string(),
            title: "Fix".to_string(),
            last_modified: 0,
        }];
        let effect = start_command(
            find_command("list-sessions").unwrap(),
            Some("/p"),
            0,
            &sessions,
        );
        match effect {
            Effect::PickSession(items) => assert_eq!(items[0].id, "a"),
            other => panic!("unexpected {:?}", other),
        }
    }

    #[test]
    fn branch_commands_pick_from_the_right_branches() {
        let branches = vec![
            branch("main", false, true),
            branch("topic", false, false),
            branch("origin/main", true, false),
            branch("origin/topic", true, false),
            branch("origin/other", true, false),
        ];
        match branch_effect(BranchOp::Change, &branches) {
            Effect::PickBranch(_, choices) => {
                let refs: Vec<&str> = choices.iter().map(|b| b.r#ref.as_str()).collect();
                assert_eq!(refs, vec!["topic", "origin/other"]);
            }
            other => panic!("unexpected {:?}", other),
        }
        match branch_effect(BranchOp::Delete, &branches) {
            Effect::PickBranch(_, choices) => assert_eq!(choices.len(), 1),
            other => panic!("unexpected {:?}", other),
        }
    }

    #[test]
    fn no_other_branches_is_reported() {
        let branches = vec![branch("main", false, true)];
        assert_eq!(
            branch_effect(BranchOp::Merge, &branches),
            Effect::Notify {
                title: "Merge failed".to_string(),
                detail: "There are no other branches.".to_string()
            }
        );
    }

    #[test]
    fn picking_a_remote_branch_switches_to_its_local_name() {
        let choices = vec![branch("origin/feature", true, false)];
        assert_eq!(
            pick_branch(BranchOp::Change, &choices, "origin/feature"),
            Some(git(
                "Change branch",
                GitAction::Checkout {
                    name: "feature".to_string()
                }
            ))
        );
        assert_eq!(pick_branch(BranchOp::Change, &choices, "nope"), None);
    }

    #[test]
    fn remote_items_mark_remote_branches() {
        let items = branch_items(&[branch("a", false, false), branch("o/a", true, false)]);
        assert_eq!(items[0].detail, None);
        assert_eq!(items[1].detail, Some("remote".to_string()));
    }

    #[test]
    fn publish_skips_the_question_with_one_remote() {
        assert!(matches!(remote_effect(&[]), Effect::Notify { .. }));
        assert_eq!(
            remote_effect(&["origin".to_string()]),
            Effect::Git {
                title: "Publish".to_string(),
                job: GitJob::Publish("origin".to_string())
            }
        );
        assert!(matches!(
            remote_effect(&["a".to_string(), "b".to_string()]),
            Effect::PickRemote(_)
        ));
    }

    #[test]
    fn new_branch_validates_and_trims_the_name() {
        assert!(matches!(new_branch_effect("  "), Effect::Notify { .. }));
        assert!(matches!(new_branch_effect("-x"), Effect::Notify { .. }));
        assert_eq!(
            new_branch_effect(" feature/x "),
            git(
                "New branch",
                GitAction::CreateBranch {
                    name: "feature/x".to_string()
                }
            )
        );
    }

    #[test]
    fn discard_needs_an_explicit_answer() {
        assert_eq!(
            discard_effect("discard"),
            Some(git("Discard", GitAction::DiscardAll))
        );
        assert_eq!(discard_effect("cancel"), None);
    }
}
