//! Everything vettr asks of the user's `git` (port of `src/main/git.ts`).
//!
//! All functions block; call them from a worker thread. Failures that the TypeScript reported as
//! `null` are `None` here, and the ones that resolved to `null | string` are `Result<(), String>`
//! with git's message in the error.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::diff::{parse_diff, FileChange, RepoChanges};
use crate::git_actions::{invalid_branch_name, parse_branches, plan_git_action, Branch, GitAction};
use crate::repo_status::{parse_repo_status, RepoStatus};

/// Run `git` in `dir` with extra environment variables. Ok holds stdout; Err holds what git said
/// (stderr, else stdout, else the exit status), or the spawn error if git could not run.
fn run_git(dir: &Path, args: &[&str], env: &[(String, String)]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return Err(stderr);
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        return Err(stdout);
    }
    Err(format!("git exited with {}", output.status))
}

/// The root of the git working tree containing `dir`, or None if `dir` is not inside one (or
/// does not exist). Uses git itself so worktrees, submodules and `.git` files are handled.
pub fn find_repo_root(dir: impl AsRef<Path>) -> Option<String> {
    let out = run_git(dir.as_ref(), &["rev-parse", "--show-toplevel"], &[]).ok()?;
    Some(out.trim().to_string())
}

/// Paths inside `dir` that git ignores, relative to it with `/` separators, or None if git cannot
/// say. One command covers the whole tree: `--directory` reports a fully ignored directory
/// (`vendor/`) as one entry instead of listing its files. Trailing slashes are dropped.
pub fn list_ignored(dir: impl AsRef<Path>) -> Option<Vec<String>> {
    let out = run_git(
        dir.as_ref(),
        &[
            "--no-optional-locks",
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
            "-z",
        ],
        &[],
    )
    .ok()?;
    let paths: Vec<String> = out
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(|p| p.strip_suffix('/').unwrap_or(p).to_string())
        .collect();
    Some(paths)
}

/// Branch, upstream divergence and change count for the repo at `dir`, or None if it cannot be
/// read. `--no-optional-locks` stops this background query contending with the user's own git
/// commands over the index lock.
pub fn get_repo_status(dir: impl AsRef<Path>) -> Option<RepoStatus> {
    let out = run_git(
        dir.as_ref(),
        &[
            "--no-optional-locks",
            "status",
            "--porcelain=v2",
            "--branch",
        ],
        &[],
    )
    .ok()?;
    Some(parse_repo_status(&out))
}

/// A throwaway copy of the index (via `GIT_INDEX_FILE`) plus what is needed to use it.
struct Scratch {
    dir: PathBuf,
    env: Vec<(String, String)>,
    /// `HEAD`, or the empty tree before the first commit.
    head: String,
}

impl Scratch {
    /// Run git against the scratch index.
    fn git(&self, args: &[&str]) -> Result<String, String> {
        run_git(&self.dir, args, &self.env)
    }

    fn cached_diff(&self, base: &str) -> Result<String, String> {
        self.git(&[
            "-c",
            "core.quotepath=false",
            "diff",
            "--cached",
            "--no-color",
            "--no-ext-diff",
            "-M",
            base,
        ])
    }
}

/// Removes the scratch directory when dropped.
struct DirGuard(PathBuf);

impl Drop for DirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Run `f` against a throwaway copy of the index so the user's real index is never touched.
/// None on failure.
fn with_scratch_index<T>(dir: &Path, f: impl FnOnce(&Scratch) -> Result<T, String>) -> Option<T> {
    let index_path = run_git(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
        &[],
    )
    .ok()?;
    let index_path = index_path.trim().to_string();
    let scratch_dir = std::env::temp_dir().join(format!("vettr-index-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&scratch_dir).ok()?;
    let _guard = DirGuard(scratch_dir.clone());
    let temp_index = scratch_dir.join("index");
    // A missing index (fresh repo) is fine: git starts from an empty one
    let _ = fs::copy(&index_path, &temp_index);
    let env = vec![(
        "GIT_INDEX_FILE".to_string(),
        temp_index.to_string_lossy().into_owned(),
    )];
    let head = match run_git(dir, &["rev-parse", "--verify", "--quiet", "HEAD"], &[]) {
        Ok(out) => out.trim().to_string(),
        Err(_) => run_git(dir, &["hash-object", "-t", "tree", "/dev/null"], &[])
            .ok()?
            .trim()
            .to_string(),
    };
    let scratch = Scratch {
        dir: dir.to_path_buf(),
        env,
        head,
    };
    f(&scratch).ok()
}

/// The changes in `dir` split by index state, or None if they cannot be read. `staged` is the
/// index against `HEAD` (the empty tree before the first commit); `unstaged` is the working tree
/// against the index, untracked files included. A partially staged file is in both.
///
/// Everything runs against a throwaway copy of the index: the staged diff reads it as is, then
/// `add --all` on the copy picks up untracked files for the unstaged diff.
pub fn get_changes(dir: impl AsRef<Path>) -> Option<RepoChanges> {
    with_scratch_index(dir.as_ref(), |s: &Scratch| {
        let staged = parse_diff(&s.cached_diff(&s.head)?);
        let index_tree = s.git(&["write-tree"])?.trim().to_string();
        s.git(&["add", "--all"])?;
        let unstaged = parse_diff(&s.cached_diff(&index_tree)?);
        Ok(RepoChanges { staged, unstaged })
    })
}

/// Record the whole working tree (untracked files included, ignored ones not) as a git tree
/// object and return its id, or None on failure. Used as the baseline for the round-to-round
/// diff. The object is unreferenced, so git may collect it after its prune grace period.
pub fn snapshot_tree(dir: impl AsRef<Path>) -> Option<String> {
    with_scratch_index(dir.as_ref(), |s: &Scratch| {
        s.git(&["add", "--all"])?;
        Ok(s.git(&["write-tree"])?.trim().to_string())
    })
}

/// The working tree against a tree from `snapshot_tree`: what changed since then. None on failure.
pub fn get_changes_since(dir: impl AsRef<Path>, tree: &str) -> Option<Vec<FileChange>> {
    with_scratch_index(dir.as_ref(), |s: &Scratch| {
        s.git(&["add", "--all"])?;
        Ok(parse_diff(&s.cached_diff(tree)?))
    })
}

fn index_op(dir: &Path, args: &[&str], paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    // Literal pathspecs: file names containing `*` or `[` must not be treated as globs
    let mut full: Vec<&str> = vec!["--literal-pathspecs"];
    full.extend_from_slice(args);
    full.push("--");
    for p in paths {
        full.push(p.as_str());
    }
    run_git(dir, &full, &[]).map(|_| ())
}

/// Stage whole files, including deletions and untracked files. Paths are relative to the repo
/// root `dir`. Err holds git's message.
pub fn stage_files(dir: impl AsRef<Path>, paths: &[String]) -> Result<(), String> {
    index_op(dir.as_ref(), &["add", "--all"], paths)
}

/// Unstage whole files. `git reset` rather than `restore --staged` because it also works before
/// the first commit. For a staged rename pass both the old and new path.
pub fn unstage_files(dir: impl AsRef<Path>, paths: &[String]) -> Result<(), String> {
    index_op(dir.as_ref(), &["reset", "-q"], paths)
}

/// Throw away all changes to whole files, staged and unstaged: tracked files go back to `HEAD`,
/// files that are not in `HEAD` (new or untracked) are deleted. For a rename pass both the old
/// and new path. This cannot be undone.
pub fn discard_files(dir: impl AsRef<Path>, paths: &[String]) -> Result<(), String> {
    let dir = dir.as_ref();
    // Drop anything staged first (also works before the first commit)
    index_op(dir, &["reset", "-q"], paths)?;
    for path in paths {
        let tracked = run_git(
            dir,
            &[
                "--literal-pathspecs",
                "ls-files",
                "--error-unmatch",
                "--",
                path.as_str(),
            ],
            &[],
        )
        .is_ok();
        let args: &[&str] = if tracked {
            &["checkout", "-q"]
        } else {
            &["clean", "-fdq"]
        };
        index_op(dir, args, std::slice::from_ref(path))?;
    }
    Ok(())
}

/// Commit what is staged with the user's message, using the host's `git` and hooks. Err holds
/// git's message (nothing staged, a hook rejected it, ...). The message is a single argument,
/// never run through a shell.
pub fn commit_staged(dir: impl AsRef<Path>, message: &str) -> Result<(), String> {
    run_git(dir.as_ref(), &["commit", "-m", message], &[]).map(|_| ())
}

/// Environment that makes git fail instead of prompting, since there is no terminal.
fn no_prompt_env() -> Vec<(String, String)> {
    let ssh =
        std::env::var("GIT_SSH_COMMAND").unwrap_or_else(|_| "ssh -o BatchMode=yes".to_string());
    vec![
        ("GIT_TERMINAL_PROMPT".to_string(), "0".to_string()),
        ("GIT_ASKPASS".to_string(), "true".to_string()),
        ("SSH_ASKPASS".to_string(), "true".to_string()),
        ("GIT_SSH_COMMAND".to_string(), ssh),
    ]
}

fn push(dir: &Path, args: &[&str]) -> Result<(), String> {
    let mut full: Vec<&str> = vec!["push"];
    full.extend_from_slice(args);
    run_git(dir, &full, &no_prompt_env()).map(|_| ())
}

/// Push the current branch to its upstream using the host's `git`, credentials and config.
/// Prompting is disabled so a push that needs input fails rather than hanging.
pub fn push_current(dir: impl AsRef<Path>) -> Result<(), String> {
    push(dir.as_ref(), &[])
}

/// Names of the configured remotes, `origin` first; empty if there are none or on error.
pub fn list_remotes(dir: impl AsRef<Path>) -> Vec<String> {
    let out = match run_git(dir.as_ref(), &["remote"], &[]) {
        Ok(out) => out,
        Err(_) => return Vec::new(),
    };
    let names: Vec<String> = out
        .split('\n')
        .filter(|n| !n.is_empty())
        .map(|n| n.to_string())
        .collect();
    if names.iter().any(|n| n == "origin") {
        let mut sorted = vec!["origin".to_string()];
        sorted.extend(names.into_iter().filter(|n| n != "origin"));
        sorted
    } else {
        names
    }
}

/// Publish the current branch to `remote` and set it as the upstream (`git push -u`), like VS
/// Code's "Publish Branch". Pushes `HEAD` so the branch name is never interpolated;
/// `--end-of-options` stops a remote name starting with `-` being read as a flag.
pub fn publish_branch(dir: impl AsRef<Path>, remote: &str) -> Result<(), String> {
    push(dir.as_ref(), &["-u", "--end-of-options", remote, "HEAD"])
}

/// Local and remote-tracking branches (local first), or an empty list on error.
pub fn list_branches(dir: impl AsRef<Path>) -> Vec<Branch> {
    match run_git(
        dir.as_ref(),
        &[
            "for-each-ref",
            "--format=%(HEAD)%(refname)",
            "refs/heads",
            "refs/remotes",
        ],
        &[],
    ) {
        Ok(out) => parse_branches(&out),
        Err(_) => Vec::new(),
    }
}

/// The branch name carried by an action, if it has one.
fn action_branch_name(action: &GitAction) -> Option<&str> {
    match action {
        GitAction::CreateBranch { name }
        | GitAction::Checkout { name }
        | GitAction::DeleteBranch { name }
        | GitAction::Merge { name }
        | GitAction::Rebase { name } => Some(name.as_str()),
        _ => None,
    }
}

/// Run a command palette git action with the host's `git`. Prompting is disabled and merges never
/// open an editor, since there is no terminal. Err holds git's message.
pub fn run_git_action(dir: impl AsRef<Path>, action: &GitAction) -> Result<(), String> {
    let dir = dir.as_ref();
    if let Some(name) = action_branch_name(action) {
        if let Some(invalid) = invalid_branch_name(name) {
            return Err(invalid);
        }
    }
    let plan = plan_git_action(action);
    let mut env = no_prompt_env();
    env.push(("GIT_MERGE_AUTOEDIT".to_string(), "no".to_string()));
    for step in &plan.steps {
        let args: Vec<&str> = step.iter().map(|s| s.as_str()).collect();
        if let Err(message) = run_git(dir, &args, &env) {
            let abort = match &plan.abort_on_failure {
                Some(abort) => abort,
                None => return Err(message),
            };
            // Leave the repo as it was rather than half-merged; a failed abort just means
            // there was nothing to undo
            let abort_args: Vec<&str> = abort.iter().map(|s| s.as_str()).collect();
            let _ = run_git(dir, &abort_args, &env);
            let note = plan.abort_note.clone().unwrap_or_default();
            return Err(format!("{}\n\n{}", message, note));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    /// A temporary directory with its canonical path (macOS tmp is a symlink).
    struct Root {
        _tmp: TempDir,
        path: PathBuf,
    }

    fn root() -> Root {
        let tmp = tempfile::tempdir().unwrap();
        let path = fs::canonicalize(tmp.path()).unwrap();
        Root { _tmp: tmp, path }
    }

    /// Run git in `dir` with a committer identity; panics on failure and returns stdout.
    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn init(dir: &Path) {
        git(dir, &["init", "-q", "-b", "main"]);
    }

    fn write(dir: &Path, name: &str, content: &str) {
        fs::write(dir.join(name), content).unwrap();
    }

    fn strs(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn paths(files: &[FileChange]) -> Vec<String> {
        let mut out: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
        out.sort();
        out
    }

    fn commit_all(dir: &Path) {
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", "c"]);
    }

    // list_ignored

    #[test]
    fn list_ignored_lists_ignored_directories_whole_and_ignored_files() {
        let r = root();
        let p = &r.path;
        git(p, &["init", "-q"]);
        write(p, ".gitignore", "vendor/\n*.log\n");
        fs::create_dir_all(p.join("vendor").join("pkg")).unwrap();
        write(&p.join("vendor").join("pkg"), "a.php", "1");
        fs::create_dir(p.join("src")).unwrap();
        write(&p.join("src"), "debug.log", "1");
        write(&p.join("src"), "a.ts", "1");
        let mut ignored = list_ignored(p).unwrap();
        ignored.sort();
        assert_eq!(ignored, strs(&["src/debug.log", "vendor"]));
    }

    #[test]
    fn list_ignored_returns_an_empty_list_when_nothing_is_ignored() {
        let r = root();
        git(&r.path, &["init", "-q"]);
        assert_eq!(list_ignored(&r.path), Some(Vec::new()));
    }

    #[test]
    fn list_ignored_returns_none_outside_a_repository() {
        let r = root();
        assert_eq!(list_ignored(&r.path), None);
    }

    // find_repo_root

    #[test]
    fn find_repo_root_returns_the_folder_itself_when_it_is_a_repo_root() {
        let r = root();
        git(&r.path, &["init", "-q"]);
        assert_eq!(
            find_repo_root(&r.path),
            Some(r.path.to_string_lossy().into_owned())
        );
    }

    #[test]
    fn find_repo_root_resolves_a_subdirectory_to_the_repo_root() {
        let r = root();
        git(&r.path, &["init", "-q"]);
        let sub = r.path.join("a").join("b");
        fs::create_dir_all(&sub).unwrap();
        assert_eq!(
            find_repo_root(&sub),
            Some(r.path.to_string_lossy().into_owned())
        );
    }

    #[test]
    fn find_repo_root_returns_none_for_a_folder_that_is_not_in_a_repo() {
        let r = root();
        assert_eq!(find_repo_root(&r.path), None);
    }

    #[test]
    fn find_repo_root_returns_none_for_a_folder_that_does_not_exist() {
        let r = root();
        assert_eq!(find_repo_root(r.path.join("gone")), None);
    }

    // get_repo_status

    #[test]
    fn repo_status_reports_a_fresh_repo_with_an_untracked_file() {
        let r = root();
        init(&r.path);
        write(&r.path, "a.txt", "a");
        let status = get_repo_status(&r.path).unwrap();
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert!(status.sha.is_none());
        assert!(status.upstream.is_none());
        assert!(status.ahead == 0);
        assert!(status.behind == 0);
        assert!(status.changes == 1);
    }

    #[test]
    fn repo_status_reports_ahead_behind_against_an_upstream() {
        let r = root();
        let p = &r.path;
        init(p);
        git(p, &["commit", "-q", "--allow-empty", "-m", "one"]);
        let clone = p.join("clone");
        git(
            p,
            &["clone", "-q", p.to_str().unwrap(), clone.to_str().unwrap()],
        );
        git(&clone, &["commit", "-q", "--allow-empty", "-m", "local"]);
        git(p, &["commit", "-q", "--allow-empty", "-m", "upstream"]);
        git(&clone, &["fetch", "-q"]);
        let status = get_repo_status(&clone).unwrap();
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert_eq!(status.upstream.as_deref(), Some("origin/main"));
        assert!(status.ahead == 1);
        assert!(status.behind == 1);
        assert!(status.changes == 0);
        let sha = status.sha.clone().unwrap();
        assert_eq!(sha.len(), 7);
        assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn repo_status_reports_a_detached_head() {
        let r = root();
        init(&r.path);
        git(&r.path, &["commit", "-q", "--allow-empty", "-m", "one"]);
        git(&r.path, &["checkout", "-q", "--detach"]);
        assert!(get_repo_status(&r.path).unwrap().branch.is_none());
    }

    #[test]
    fn repo_status_is_none_when_the_folder_is_not_a_repo() {
        let r = root();
        assert!(get_repo_status(&r.path).is_none());
    }

    // get_changes

    #[test]
    fn get_changes_is_none_when_the_folder_is_not_a_repo() {
        let r = root();
        assert!(get_changes(&r.path).is_none());
    }

    #[test]
    fn get_changes_returns_no_changes_for_a_clean_repo() {
        let r = root();
        init(&r.path);
        write(&r.path, "a.txt", "a\n");
        commit_all(&r.path);
        let changes = get_changes(&r.path).unwrap();
        assert!(changes.staged.is_empty());
        assert!(changes.unstaged.is_empty());
    }

    #[test]
    fn get_changes_lists_untracked_files_as_unstaged_in_a_repo_with_no_commits() {
        let r = root();
        init(&r.path);
        write(&r.path, "a.txt", "one\ntwo\n");
        let changes = get_changes(&r.path).unwrap();
        assert!(changes.staged.is_empty());
        let file = &changes.unstaged[0];
        assert_eq!(file.path, "a.txt");
        assert_eq!(file.status, crate::diff::ChangeStatus::Added);
        assert!(file.additions == 2);
        assert!(file.deletions == 0);
    }

    #[test]
    fn get_changes_lists_files_staged_before_the_first_commit_as_staged() {
        let r = root();
        init(&r.path);
        write(&r.path, "a.txt", "one\n");
        git(&r.path, &["add", "a.txt"]);
        let changes = get_changes(&r.path).unwrap();
        assert_eq!(changes.staged[0].path, "a.txt");
        assert_eq!(changes.staged[0].status, crate::diff::ChangeStatus::Added);
        assert!(changes.unstaged.is_empty());
    }

    #[test]
    fn get_changes_reports_modified_deleted_added_and_renamed_files() {
        use crate::diff::ChangeStatus;
        let r = root();
        let p = &r.path;
        init(p);
        write(p, "mod.txt", "a\nb\nc\n");
        write(p, "del.txt", "bye\n");
        write(
            p,
            "old name.txt",
            "some long enough content\nto be detected\nas a rename\n",
        );
        commit_all(p);
        write(p, "mod.txt", "a\nB\nc\n");
        fs::remove_file(p.join("del.txt")).unwrap();
        fs::rename(p.join("old name.txt"), p.join("new name.txt")).unwrap();
        write(p, "fresh.txt", "new\n");
        git(p, &["add", "-A"]);
        let changes = get_changes(p).unwrap();
        assert_eq!(
            paths(&changes.staged),
            strs(&["del.txt", "fresh.txt", "mod.txt", "new name.txt"])
        );
        let by =
            |path: &str| -> usize { changes.staged.iter().position(|f| f.path == path).unwrap() };
        assert_eq!(changes.staged[by("mod.txt")].status, ChangeStatus::Modified);
        assert!(changes.staged[by("mod.txt")].additions == 1);
        assert!(changes.staged[by("mod.txt")].deletions == 1);
        assert_eq!(changes.staged[by("del.txt")].status, ChangeStatus::Deleted);
        assert_eq!(changes.staged[by("fresh.txt")].status, ChangeStatus::Added);
        assert_eq!(
            changes.staged[by("new name.txt")].status,
            ChangeStatus::Renamed
        );
        assert_eq!(
            changes.staged[by("new name.txt")].old_path.as_deref(),
            Some("old name.txt")
        );
        assert!(changes.unstaged.is_empty());
    }

    #[test]
    fn get_changes_separates_staged_unstaged_and_untracked_files() {
        let r = root();
        let p = &r.path;
        init(p);
        write(p, "staged.txt", "a\n");
        write(p, "unstaged.txt", "a\n");
        commit_all(p);
        write(p, "staged.txt", "b\n");
        git(p, &["add", "staged.txt"]);
        write(p, "unstaged.txt", "b\n");
        write(p, "untracked.txt", "x\n");
        let changes = get_changes(p).unwrap();
        assert_eq!(paths(&changes.staged), strs(&["staged.txt"]));
        assert_eq!(
            paths(&changes.unstaged),
            strs(&["unstaged.txt", "untracked.txt"])
        );
    }

    #[test]
    fn get_changes_puts_a_partially_staged_file_in_both_lists() {
        let r = root();
        let p = &r.path;
        init(p);
        write(p, "a.txt", "one\n");
        commit_all(p);
        write(p, "a.txt", "one\ntwo\n");
        git(p, &["add", "a.txt"]);
        write(p, "a.txt", "one\ntwo\nthree\n");
        let changes = get_changes(p).unwrap();
        assert_eq!(changes.staged[0].path, "a.txt");
        assert!(changes.staged[0].additions == 1);
        assert_eq!(changes.unstaged[0].path, "a.txt");
        assert!(changes.unstaged[0].additions == 1);
    }

    #[test]
    fn get_changes_does_not_modify_the_real_index_or_leave_scratch_files_behind() {
        let r = root();
        let p = &r.path;
        init(p);
        write(p, "a.txt", "one\n");
        commit_all(p);
        write(p, "untracked.txt", "x\n");
        let index_path = p.join(".git").join("index");
        let before = fs::read(&index_path).unwrap();
        get_changes(p);
        assert_eq!(fs::read(&index_path).unwrap(), before);
        assert_eq!(git(p, &["status", "--porcelain"]), "?? untracked.txt\n");
        assert!(!p.join(".git").join("index.lock").exists());
    }

    #[test]
    fn get_changes_flags_binary_files() {
        let r = root();
        init(&r.path);
        fs::write(r.path.join("img.bin"), [0u8, 1, 2, 0, 255, 0]).unwrap();
        let changes = get_changes(&r.path).unwrap();
        assert_eq!(changes.unstaged[0].path, "img.bin");
        assert!(changes.unstaged[0].binary);
        assert!(changes.unstaged[0].hunks.is_empty());
    }

    // stage_files and unstage_files

    fn status(p: &Path) -> String {
        git(p, &["status", "--porcelain"])
    }

    #[test]
    fn stages_and_unstages_an_untracked_file_before_the_first_commit() {
        let r = root();
        let p = &r.path;
        init(p);
        write(p, "a.txt", "a\n");
        assert_eq!(stage_files(p, &strs(&["a.txt"])), Ok(()));
        assert_eq!(status(p), "A  a.txt\n");
        assert_eq!(unstage_files(p, &strs(&["a.txt"])), Ok(()));
        assert_eq!(status(p), "?? a.txt\n");
    }

    #[test]
    fn discards_tracked_changes_and_removes_new_files() {
        let r = root();
        let p = &r.path;
        init(p);
        write(p, "mod.txt", "a\n");
        write(p, "del.txt", "a\n");
        commit_all(p);
        write(p, "mod.txt", "b\n");
        fs::remove_file(p.join("del.txt")).unwrap();
        write(p, "new.txt", "n\n");
        write(p, "staged.txt", "s\n");
        assert_eq!(stage_files(p, &strs(&["mod.txt", "staged.txt"])), Ok(()));
        let all = strs(&["mod.txt", "del.txt", "new.txt", "staged.txt"]);
        assert_eq!(discard_files(p, &all), Ok(()));
        assert_eq!(status(p), "");
        assert_eq!(fs::read_to_string(p.join("mod.txt")).unwrap(), "a\n");
        assert!(!p.join("new.txt").exists());
        assert!(!p.join("staged.txt").exists());
    }

    #[test]
    fn stages_modifications_and_deletions() {
        let r = root();
        let p = &r.path;
        init(p);
        write(p, "mod.txt", "a\n");
        write(p, "del.txt", "a\n");
        commit_all(p);
        write(p, "mod.txt", "b\n");
        fs::remove_file(p.join("del.txt")).unwrap();
        assert_eq!(stage_files(p, &strs(&["mod.txt", "del.txt"])), Ok(()));
        assert_eq!(status(p), "D  del.txt\nM  mod.txt\n");
        assert_eq!(unstage_files(p, &strs(&["mod.txt", "del.txt"])), Ok(()));
        assert_eq!(status(p), " D del.txt\n M mod.txt\n");
    }

    #[test]
    fn moves_a_rename_in_one_step_when_given_both_paths() {
        let r = root();
        let p = &r.path;
        init(p);
        write(
            p,
            "old.txt",
            "some long enough content\nto be detected\nas a rename\n",
        );
        commit_all(p);
        fs::rename(p.join("old.txt"), p.join("new.txt")).unwrap();
        let _ = stage_files(p, &strs(&["old.txt", "new.txt"]));
        assert_eq!(status(p), "R  old.txt -> new.txt\n");
        let _ = unstage_files(p, &strs(&["old.txt", "new.txt"]));
        assert_eq!(status(p), " D old.txt\n?? new.txt\n");
    }

    #[test]
    fn treats_paths_literally_not_as_globs() {
        let r = root();
        let p = &r.path;
        init(p);
        write(p, "a.txt", "a\n");
        write(p, "*.txt", "star\n");
        assert_eq!(stage_files(p, &strs(&["*.txt"])), Ok(()));
        assert_eq!(status(p), "A  *.txt\n?? a.txt\n");
    }

    #[test]
    fn does_nothing_for_an_empty_list() {
        let r = root();
        let p = &r.path;
        init(p);
        write(p, "a.txt", "a\n");
        assert_eq!(stage_files(p, &[]), Ok(()));
        assert_eq!(unstage_files(p, &[]), Ok(()));
        assert_eq!(status(p), "?? a.txt\n");
    }

    #[test]
    fn stage_returns_gits_message_when_it_fails() {
        let r = root();
        init(&r.path);
        let err = stage_files(&r.path, &strs(&["missing.txt"])).unwrap_err();
        assert!(err.contains("missing.txt"), "{}", err);
    }

    #[test]
    fn stage_returns_the_error_text_when_git_cannot_run_at_all() {
        let r = root();
        let err = stage_files(r.path.join("gone"), &strs(&["a"])).unwrap_err();
        assert!(!err.is_empty());
    }

    // push_current

    #[test]
    fn push_current_pushes_commits_to_the_upstream() {
        let r = root();
        let remote = r.path.join("remote.git");
        let work = r.path.join("work");
        git(
            &r.path,
            &[
                "init",
                "-q",
                "--bare",
                "-b",
                "main",
                remote.to_str().unwrap(),
            ],
        );
        git(
            &r.path,
            &["init", "-q", "-b", "main", work.to_str().unwrap()],
        );
        git(
            &work,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        write(&work, "a.txt", "a\n");
        git(&work, &["add", "."]);
        git(&work, &["commit", "-q", "-m", "one"]);
        git(&work, &["push", "-q", "-u", "origin", "main"]);
        write(&work, "b.txt", "b\n");
        git(&work, &["add", "."]);
        git(&work, &["commit", "-q", "-m", "two"]);
        assert_eq!(push_current(&work), Ok(()));
        assert_eq!(git(&remote, &["log", "--format=%s", "main"]), "two\none\n");
    }

    #[test]
    fn push_current_returns_git_output_when_the_push_fails() {
        let r = root();
        init(&r.path);
        let err = push_current(&r.path).unwrap_err();
        assert!(err.contains("fatal"), "{}", err);
    }

    #[test]
    fn push_current_falls_back_to_the_error_message_when_git_gives_no_stderr() {
        let r = root();
        let err = push_current(r.path.join("missing")).unwrap_err();
        assert!(!err.is_empty());
    }

    // list_remotes and publish_branch

    fn remote_setup(r: &Root) -> (PathBuf, PathBuf) {
        let remote = r.path.join("remote.git");
        let work = r.path.join("work");
        git(
            &r.path,
            &[
                "init",
                "-q",
                "--bare",
                "-b",
                "main",
                remote.to_str().unwrap(),
            ],
        );
        git(
            &r.path,
            &["init", "-q", "-b", "main", work.to_str().unwrap()],
        );
        write(&work, "a.txt", "a\n");
        git(&work, &["add", "."]);
        git(&work, &["commit", "-q", "-m", "one"]);
        (remote, work)
    }

    #[test]
    fn lists_remotes_with_origin_first() {
        let r = root();
        let (remote, work) = remote_setup(&r);
        git(
            &work,
            &["remote", "add", "backup", remote.to_str().unwrap()],
        );
        git(
            &work,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        assert_eq!(list_remotes(&work), strs(&["origin", "backup"]));
    }

    #[test]
    fn lists_remotes_without_origin_as_they_come() {
        let r = root();
        let (remote, work) = remote_setup(&r);
        git(
            &work,
            &["remote", "add", "backup", remote.to_str().unwrap()],
        );
        assert_eq!(list_remotes(&work), strs(&["backup"]));
    }

    #[test]
    fn lists_no_remotes_when_there_are_none_or_outside_a_repo() {
        let r = root();
        let (_remote, work) = remote_setup(&r);
        assert!(list_remotes(&work).is_empty());
        assert!(list_remotes(r.path.join("missing")).is_empty());
    }

    #[test]
    fn publishes_the_branch_and_sets_its_upstream() {
        let r = root();
        let (remote, work) = remote_setup(&r);
        git(
            &work,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        assert_eq!(publish_branch(&work, "origin"), Ok(()));
        assert_eq!(git(&remote, &["log", "--format=%s", "main"]), "one\n");
        assert_eq!(
            git(&work, &["rev-parse", "--abbrev-ref", "@{u}"]).trim(),
            "origin/main"
        );
    }

    #[test]
    fn publish_returns_git_output_when_publishing_fails() {
        let r = root();
        let (_remote, work) = remote_setup(&r);
        let err = publish_branch(&work, "nope").unwrap_err();
        assert!(err.contains("fatal"), "{}", err);
    }

    // commit_staged

    #[test]
    fn commits_the_staged_files_with_the_message() {
        let r = root();
        let p = &r.path;
        init(p);
        git(p, &["config", "user.name", "t"]);
        git(p, &["config", "user.email", "t@t"]);
        write(p, "a.txt", "a\n");
        git(p, &["add", "."]);
        assert_eq!(commit_staged(p, "hello"), Ok(()));
        assert_eq!(git(p, &["log", "--format=%s"]), "hello\n");
    }

    #[test]
    fn commit_returns_git_output_when_there_is_nothing_to_commit() {
        let r = root();
        init(&r.path);
        git(&r.path, &["config", "user.name", "t"]);
        git(&r.path, &["config", "user.email", "t@t"]);
        let err = commit_staged(&r.path, "hello").unwrap_err();
        assert!(!err.is_empty());
    }

    // snapshot_tree and get_changes_since

    #[test]
    fn snapshot_and_changes_since_are_none_when_the_folder_is_not_a_repo() {
        let r = root();
        assert!(snapshot_tree(&r.path).is_none());
        assert!(get_changes_since(&r.path, "abc").is_none());
    }

    #[test]
    fn changes_since_is_none_for_a_tree_that_does_not_exist() {
        let r = root();
        init(&r.path);
        assert!(get_changes_since(&r.path, &"0".repeat(40)).is_none());
    }

    #[test]
    fn changes_since_shows_only_what_changed_after_the_snapshot() {
        let r = root();
        let p = &r.path;
        init(p);
        write(p, "a.txt", "one\n");
        commit_all(p);
        write(p, "a.txt", "one\ntwo\n");
        write(p, "new.txt", "x\n");
        let tree = snapshot_tree(p).unwrap();
        assert_eq!(tree.len(), 40);
        assert!(tree.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(get_changes_since(p, &tree).unwrap().is_empty());

        write(p, "a.txt", "one\ntwo\nthree\n");
        write(p, "later.txt", "y\n");
        let since = get_changes_since(p, &tree).unwrap();
        assert_eq!(paths(&since), strs(&["a.txt", "later.txt"]));
        let a = since.iter().find(|f| f.path == "a.txt").unwrap();
        assert!(a.additions == 1);
    }

    #[test]
    fn snapshot_leaves_the_real_index_alone() {
        let r = root();
        init(&r.path);
        write(&r.path, "a.txt", "one\n");
        snapshot_tree(&r.path);
        assert_eq!(status(&r.path), "?? a.txt\n");
    }

    // list_branches and run_git_action

    /// A repo on `main` with one commit.
    fn actions_repo() -> Root {
        let r = root();
        let p = r.path.clone();
        init(&p);
        git(&p, &["config", "user.name", "t"]);
        git(&p, &["config", "user.email", "t@t"]);
        commit_file(&p, "a.txt", "a\n");
        r
    }

    fn commit_file(p: &Path, name: &str, content: &str) {
        write(p, name, content);
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", &format!("edit {}", name)]);
    }

    fn branch(p: &Path) -> String {
        git(p, &["branch", "--show-current"]).trim().to_string()
    }

    fn named(f: fn(String) -> GitAction, name: &str) -> GitAction {
        f(name.to_string())
    }

    fn create_branch(name: String) -> GitAction {
        GitAction::CreateBranch { name }
    }
    fn checkout(name: String) -> GitAction {
        GitAction::Checkout { name }
    }
    fn delete_branch(name: String) -> GitAction {
        GitAction::DeleteBranch { name }
    }
    fn merge(name: String) -> GitAction {
        GitAction::Merge { name }
    }
    fn rebase(name: String) -> GitAction {
        GitAction::Rebase { name }
    }

    #[test]
    fn lists_local_and_remote_branches_marking_the_current_one() {
        let r = actions_repo();
        let p = &r.path;
        let remotes = root();
        let remote = remotes.path.join("remote.git");
        git(
            p,
            &[
                "init",
                "-q",
                "--bare",
                "-b",
                "main",
                remote.to_str().unwrap(),
            ],
        );
        git(p, &["remote", "add", "origin", remote.to_str().unwrap()]);
        git(p, &["push", "-q", "-u", "origin", "main"]);
        git(p, &["branch", "dev"]);
        let branches = list_branches(p);
        let expected = vec![
            Branch {
                r#ref: "dev".to_string(),
                remote: false,
                current: false,
            },
            Branch {
                r#ref: "main".to_string(),
                remote: false,
                current: true,
            },
            Branch {
                r#ref: "origin/main".to_string(),
                remote: true,
                current: false,
            },
        ];
        assert_eq!(branches, expected);
    }

    #[test]
    fn lists_no_branches_when_the_folder_is_not_a_repo() {
        let r = root();
        assert!(list_branches(r.path.join("missing")).is_empty());
    }

    #[test]
    fn creates_switches_to_and_deletes_branches() {
        let r = actions_repo();
        let p = &r.path;
        assert_eq!(run_git_action(p, &named(create_branch, "topic")), Ok(()));
        assert_eq!(branch(p), "topic");
        assert_eq!(run_git_action(p, &named(checkout, "main")), Ok(()));
        assert_eq!(branch(p), "main");
        assert_eq!(run_git_action(p, &named(delete_branch, "topic")), Ok(()));
        assert_eq!(git(p, &["branch", "--list", "topic"]), "");
    }

    #[test]
    fn refuses_to_delete_an_unmerged_branch() {
        let r = actions_repo();
        let p = &r.path;
        git(p, &["switch", "-c", "topic"]);
        commit_file(p, "b.txt", "b\n");
        git(p, &["switch", "main"]);
        let err = run_git_action(p, &named(delete_branch, "topic")).unwrap_err();
        assert!(err.contains("not fully merged"), "{}", err);
    }

    #[test]
    fn rejects_bad_branch_names_before_running_git() {
        let r = actions_repo();
        let err = run_git_action(&r.path, &named(create_branch, "-x")).unwrap_err();
        assert!(err.contains("cannot start"), "{}", err);
        assert_eq!(branch(&r.path), "main");
    }

    #[test]
    fn stages_and_unstages_everything() {
        let r = actions_repo();
        let p = &r.path;
        write(p, "b.txt", "b\n");
        assert_eq!(run_git_action(p, &GitAction::StageAll), Ok(()));
        assert_eq!(status(p), "A  b.txt\n");
        assert_eq!(run_git_action(p, &GitAction::UnstageAll), Ok(()));
        assert_eq!(status(p), "?? b.txt\n");
    }

    #[test]
    fn discards_tracked_changes_and_untracked_files() {
        let r = actions_repo();
        let p = &r.path;
        write(p, "a.txt", "changed\n");
        write(p, "new.txt", "x\n");
        assert_eq!(run_git_action(p, &GitAction::DiscardAll), Ok(()));
        assert_eq!(status(p), "");
        assert_eq!(fs::read_to_string(p.join("a.txt")).unwrap(), "a\n");
    }

    #[test]
    fn stashes_and_pops_changes() {
        let r = actions_repo();
        let p = &r.path;
        write(p, "a.txt", "changed\n");
        write(p, "new.txt", "x\n");
        assert_eq!(run_git_action(p, &GitAction::Stash), Ok(()));
        assert_eq!(status(p), "");
        assert_eq!(run_git_action(p, &GitAction::StashPop), Ok(()));
        assert!(status(p).contains("a.txt"));
    }

    #[test]
    fn reports_an_error_when_there_is_nothing_to_pop() {
        let r = actions_repo();
        let err = run_git_action(&r.path, &GitAction::StashPop).unwrap_err();
        assert!(err.contains("No stash"), "{}", err);
    }

    #[test]
    fn fetches_and_pulls_from_a_remote() {
        let r = actions_repo();
        let p = &r.path;
        let others = root();
        let remote = others.path.join("remote.git");
        git(
            p,
            &[
                "init",
                "-q",
                "--bare",
                "-b",
                "main",
                remote.to_str().unwrap(),
            ],
        );
        git(p, &["remote", "add", "origin", remote.to_str().unwrap()]);
        git(p, &["push", "-q", "-u", "origin", "main"]);
        let other = others.path.join("clone");
        git(
            p,
            &[
                "clone",
                "-q",
                remote.to_str().unwrap(),
                other.to_str().unwrap(),
            ],
        );
        write(&other, "c.txt", "c\n");
        git(&other, &["add", "."]);
        git(&other, &["commit", "-q", "-m", "remote change"]);
        git(&other, &["push", "-q"]);
        assert_eq!(run_git_action(p, &GitAction::Fetch), Ok(()));
        assert_eq!(
            git(p, &["rev-list", "--count", "HEAD..origin/main"]).trim(),
            "1"
        );
        assert_eq!(run_git_action(p, &GitAction::Pull), Ok(()));
        assert!(p.join("c.txt").exists());
    }

    #[test]
    fn merges_a_branch() {
        let r = actions_repo();
        let p = &r.path;
        git(p, &["switch", "-c", "topic"]);
        commit_file(p, "b.txt", "b\n");
        git(p, &["switch", "main"]);
        assert_eq!(run_git_action(p, &named(merge, "topic")), Ok(()));
        assert!(p.join("b.txt").exists());
    }

    #[test]
    fn aborts_a_conflicting_merge_and_says_so() {
        let r = actions_repo();
        let p = &r.path;
        git(p, &["switch", "-c", "topic"]);
        commit_file(p, "a.txt", "topic\n");
        git(p, &["switch", "main"]);
        commit_file(p, "a.txt", "main\n");
        let err = run_git_action(p, &named(merge, "topic")).unwrap_err();
        assert!(err.contains("The merge was aborted"), "{}", err);
        assert_eq!(status(p), "");
    }

    #[test]
    fn rebases_onto_a_branch() {
        let r = actions_repo();
        let p = &r.path;
        git(p, &["switch", "-c", "topic"]);
        commit_file(p, "b.txt", "b\n");
        git(p, &["switch", "main"]);
        commit_file(p, "c.txt", "c\n");
        git(p, &["switch", "topic"]);
        assert_eq!(run_git_action(p, &named(rebase, "main")), Ok(()));
        let log = git(p, &["log", "--format=%s"]);
        assert_eq!(log.split('\n').next().unwrap(), "edit b.txt");
        assert!(p.join("c.txt").exists());
    }

    #[test]
    fn aborts_a_conflicting_rebase() {
        let r = actions_repo();
        let p = &r.path;
        git(p, &["switch", "-c", "topic"]);
        commit_file(p, "a.txt", "topic\n");
        git(p, &["switch", "main"]);
        commit_file(p, "a.txt", "main\n");
        git(p, &["switch", "topic"]);
        let err = run_git_action(p, &named(rebase, "main")).unwrap_err();
        assert!(err.contains("The rebase was aborted"), "{}", err);
        assert_eq!(branch(p), "topic");
    }

    #[test]
    fn still_reports_git_output_when_the_abort_itself_fails() {
        // Merging a missing branch fails before any merge starts, so there is nothing to abort
        let r = actions_repo();
        let err = run_git_action(&r.path, &named(merge, "nope")).unwrap_err();
        assert!(err.contains("nope"), "{}", err);
        assert!(err.contains("The merge was aborted"), "{}", err);
    }
}
