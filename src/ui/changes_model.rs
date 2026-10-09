//! State behind the Changes view (ports of `useChanges.ts` and `useReviewComments.ts`, and the
//! view state that lived in `Changes.tsx` and the commit box of `Sidebar.tsx`).
//!
//! Nothing here draws. `ChangesModel` loads the repository changes off the UI thread, keeps the
//! accordion and collapse state, the commit message and the caches of the diff renderer.
//! `ReviewModel` owns the review comments, the round, the baseline and the comment editors.
//! The small pure functions (comment ranges, commit rules, visible rows) have unit tests.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::notifications::Notifier;
use crate::backend::Backend;
use crate::comments::{
    pending_comments, range_of, reanchor_in_place, rehydrate, ReviewComment, SentComment, Side,
};
use crate::diff::{
    changed_file_count, split_rows, ChangeStatus, DiffLine, FileChange, LineKind, RepoChanges,
};
use crate::highlight::{HighlightCache, HunkHighlight};
use crate::task::Task;

// ----- small pure helpers -----

/// Unified or side by side diff layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffMode {
    #[default]
    Unified,
    Split,
}

/// Everything that differs from `HEAD`, or only what changed after the last review was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    All,
    Since,
}

/// Identifies a file diff in the staged or unstaged group (scroll targets, collapse state).
pub fn file_key(staged: bool, path: &str) -> String {
    format!("{}:{}", if staged { "s" } else { "u" }, path)
}

/// Identifies a file diff in the "since last review" list.
pub fn since_key(file: &FileChange) -> String {
    format!(
        "since:{}>{}",
        file.old_path.as_deref().unwrap_or(""),
        file.path
    )
}

/// Every path a file change touches in the index: a rename covers its old and new path.
pub fn file_paths(file: &FileChange) -> Vec<String> {
    match &file.old_path {
        Some(old) if !old.is_empty() => vec![old.clone(), file.path.clone()],
        _ => vec![file.path.clone()],
    }
}

/// The line to land on in the editor: the first added line, else the first line of the diff that
/// exists in the new file, else line 1.
pub fn first_line(file: &FileChange) -> u32 {
    let mut first_new: Option<u32> = None;
    for hunk in &file.hunks {
        for line in &hunk.lines {
            if line.kind == LineKind::Add {
                return line.new_no.unwrap_or(1);
            }
            if first_new.is_none() {
                first_new = line.new_no;
            }
        }
    }
    first_new.unwrap_or(1)
}

pub fn status_letter(status: ChangeStatus) -> &'static str {
    match status {
        ChangeStatus::Added => "A",
        ChangeStatus::Modified => "M",
        ChangeStatus::Deleted => "D",
        ChangeStatus::Renamed => "R",
    }
}

pub fn status_label(status: ChangeStatus) -> &'static str {
    match status {
        ChangeStatus::Added => "added",
        ChangeStatus::Modified => "modified",
        ChangeStatus::Deleted => "deleted",
        ChangeStatus::Renamed => "renamed",
    }
}

/// Whether Commit is enabled: a project, something staged, a message, and no commit running.
pub fn can_commit(has_project: bool, staged_count: usize, message: &str, committing: bool) -> bool {
    has_project && staged_count > 0 && !message.trim().is_empty() && !committing
}

/// The position of `line` (a reference into `lines`) within `lines`.
pub fn index_in(lines: &[DiffLine], line: &DiffLine) -> Option<usize> {
    let size = std::mem::size_of::<DiffLine>();
    if size == 0 {
        return None;
    }
    let base = lines.as_ptr() as usize;
    let at = line as *const DiffLine as usize;
    if at < base {
        return None;
    }
    let index = (at - base) / size;
    if index < lines.len() {
        Some(index)
    } else {
        None
    }
}

/// The rows of the split view as indices into the hunk's lines (left, right).
pub fn split_index_rows(lines: &[DiffLine]) -> Vec<(Option<usize>, Option<usize>)> {
    split_rows(lines)
        .iter()
        .map(|row| {
            (
                row.left.and_then(|l| index_in(lines, l)),
                row.right.and_then(|l| index_in(lines, l)),
            )
        })
        .collect()
}

/// Whether a row at `y` with height `h` is (nearly) inside the visible band, so it is worth drawing.
pub fn row_visible(y: f32, h: f32, clip_top: f32, clip_bottom: f32) -> bool {
    y + h >= clip_top - h && y <= clip_bottom + h
}

// ----- comment drafts -----

/// The lines being selected for a new comment.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub file: String,
    pub staged: bool,
    pub side: Side,
    /// The line first clicked; shift-click extends the range from here.
    pub anchor: u32,
    pub start: u32,
    pub end: u32,
}

/// The draft after clicking line `no`; shift extends the previous draft on the same file and side.
pub fn pick_draft(
    prev: Option<&Draft>,
    file: &str,
    staged: bool,
    side: Side,
    no: u32,
    shift: bool,
) -> Draft {
    let extend = match prev {
        Some(p) => shift && p.file == file && p.staged == staged && p.side == side,
        None => false,
    };
    let anchor = match prev {
        Some(p) if extend => p.anchor,
        _ => no,
    };
    let (start, end) = range_of(anchor, no);
    Draft {
        file: file.to_string(),
        staged,
        side,
        anchor,
        start,
        end,
    }
}

/// Whether line `no` on this side of this file diff is inside the draft selection.
pub fn draft_selects(draft: Option<&Draft>, file: &str, staged: bool, side: Side, no: u32) -> bool {
    match draft {
        Some(d) => {
            d.file == file && d.staged == staged && d.side == side && no >= d.start && no <= d.end
        }
        None => false,
    }
}

/// Whether the draft's editor belongs under line `no` (the last line of its range).
pub fn draft_ends_at(draft: Option<&Draft>, file: &str, staged: bool, side: Side, no: u32) -> bool {
    match draft {
        Some(d) => d.file == file && d.staged == staged && d.side == side && d.end == no,
        None => false,
    }
}

/// A comment as typed, before it gets an id and a round.
#[derive(Debug, Clone, PartialEq)]
pub struct NewComment {
    pub file: String,
    pub staged: bool,
    pub side: Side,
    pub start: u32,
    pub end: u32,
    pub snapshot: Vec<String>,
    pub text: String,
}

/// Editor state of the comment being written and of the comment being edited.
#[derive(Debug, Clone, Default)]
pub struct DraftState {
    /// The selected lines of a new comment.
    pub target: Option<Draft>,
    /// The text typed so far.
    pub text: String,
    /// Focus the new comment's editor on the next frame.
    pub focus: bool,
    /// The id of the pending comment being edited.
    pub editing: Option<String>,
    pub edit_text: String,
    pub edit_focus: bool,
}

// ----- review comments -----

/// The review comments for the open project (`useReviewComments`). They live above the views so
/// switching views keeps them, and reset when another project is opened.
pub struct ReviewModel {
    project: Option<String>,
    pub(super) comments: Vec<ReviewComment>,
    round: u32,
    baseline: Option<String>,
    needs_reanchor: bool,
    pub(super) draft: DraftState,
}

impl Default for ReviewModel {
    fn default() -> Self {
        ReviewModel::new()
    }
}

impl ReviewModel {
    pub fn new() -> ReviewModel {
        ReviewModel {
            project: None,
            comments: Vec::new(),
            round: 1,
            baseline: None,
            needs_reanchor: false,
            draft: DraftState::default(),
        }
    }

    /// Call when the open project changes; other values (including the same one) are harmless.
    pub fn set_project(&mut self, project: Option<&str>) {
        if self.project.as_deref() == project {
            return;
        }
        self.project = project.map(|p| p.to_string());
        self.comments.clear();
        self.round = 1;
        self.baseline = None;
        self.needs_reanchor = false;
        self.draft = DraftState::default();
    }

    pub fn comments(&self) -> &[ReviewComment] {
        &self.comments
    }

    /// Comments not yet sent to the agent.
    pub fn pending(&self) -> Vec<ReviewComment> {
        pending_comments(&self.comments)
    }

    /// How many comments are waiting to be sent.
    pub fn pending_count(&self) -> usize {
        self.comments.iter().filter(|c| !c.sent).count()
    }

    /// The current review round, starting at 1 and advancing each time comments are sent.
    pub fn round(&self) -> u32 {
        self.round
    }

    /// The working tree recorded when the last review was sent (a git tree id), the baseline for
    /// "changes since the last review"; `None` before the first send.
    pub fn baseline(&self) -> Option<&str> {
        self.baseline.as_deref()
    }

    pub fn add(&mut self, comment: NewComment) {
        self.comments.push(ReviewComment {
            id: uuid::Uuid::new_v4().to_string(),
            file: comment.file,
            staged: comment.staged,
            side: comment.side,
            start: comment.start,
            end: comment.end,
            snapshot: comment.snapshot,
            text: comment.text,
            round: self.round,
            sent: false,
            outdated: false,
        });
    }

    pub fn edit(&mut self, id: &str, text: &str) {
        for c in self.comments.iter_mut() {
            if c.id == id {
                c.text = text.to_string();
            }
        }
    }

    pub fn remove(&mut self, id: &str) {
        self.comments.retain(|c| c.id != id);
    }

    /// Mark the given comments as sent, record the round's baseline tree and start the next round.
    pub fn mark_sent(&mut self, ids: &[String], baseline: Option<String>) {
        if let Some(tree) = baseline {
            self.baseline = Some(tree);
        }
        for c in self.comments.iter_mut() {
            if ids.contains(&c.id) {
                c.sent = true;
            }
        }
        self.round += 1;
    }

    /// Replace the sent comments with those read from an opened session's transcript and continue
    /// from the round after the last one sent. They are re-anchored to the current changes the next
    /// time `ChangesModel::update` runs. The baseline of the old session no longer applies.
    pub fn restore(&mut self, sent: Vec<SentComment>) {
        let restored = rehydrate(&self.comments, &sent, None);
        self.comments = restored.comments;
        self.round = restored.round;
        self.baseline = None;
        self.needs_reanchor = true;
        self.draft.editing = None;
    }

    /// Cancel the comment being written.
    pub fn cancel_draft(&mut self) {
        self.draft.target = None;
        self.draft.text.clear();
    }

    pub(super) fn take_needs_reanchor(&mut self) -> bool {
        let flag = self.needs_reanchor;
        self.needs_reanchor = false;
        flag
    }
}

// ----- changes -----

/// What a finished background operation was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpKind {
    Stage,
    Unstage,
    Discard,
    Commit,
    Editor,
}

/// Cached per-hunk work, dropped whenever the changes reload.
#[derive(Default)]
pub(super) struct HunkCache {
    pub highlight: Option<Arc<HunkHighlight>>,
    pub split: Option<Arc<Vec<(Option<usize>, Option<usize>)>>>,
}

/// Display state of the diff view.
#[derive(Default)]
pub(super) struct ViewState {
    pub mode: DiffMode,
    pub scope: Scope,
    pub staged_collapsed: bool,
    pub unstaged_collapsed: bool,
    pub collapsed_files: HashSet<String>,
    pub hunks: HashMap<String, Vec<HunkCache>>,
    pub highlighter: HighlightCache,
}

impl ViewState {
    fn clear_caches(&mut self) {
        self.hunks.clear();
        self.highlighter.clear();
    }

    fn drop_since_cache(&mut self) {
        self.hunks.retain(|key, _| !key.starts_with("since:"));
    }
}

/// The commit message box.
#[derive(Default)]
pub(super) struct CommitState {
    pub message: String,
    pub committing: bool,
    pub focus: bool,
}

type LoadResult = (u64, Option<RepoChanges>);
type SinceResult = (u64, String, Option<Vec<FileChange>>);

/// Take a finished result out of `slot`; the slot is emptied once the work is done.
fn poll_slot<T: Send + 'static>(slot: &mut Option<Task<T>>) -> Option<T> {
    let finished = {
        let task = slot.as_ref()?;
        !task.is_running()
    };
    let polled = match slot.as_mut() {
        Some(task) => task.poll(),
        None => None,
    };
    if finished {
        *slot = None;
    }
    polled
}

/// The working-tree changes of the open project and the state of the Changes view.
pub struct ChangesModel {
    ctx: egui::Context,
    backend: Backend,
    notifier: Notifier,
    project: Option<String>,
    /// Bumped when the project changes, so late answers for an old project are dropped.
    generation: u64,
    loaded: bool,
    changes: Option<Arc<RepoChanges>>,
    since: Option<Arc<Vec<FileChange>>>,
    since_tree: Option<String>,
    load: Option<Task<LoadResult>>,
    since_load: Option<Task<SinceResult>>,
    reload_again: bool,
    since_reload_again: bool,
    ops: Vec<Task<(OpKind, Result<(), String>)>>,
    /// A file diff (see `file_key`) the main view should scroll to on its next frame.
    pub scroll_to: Option<String>,
    pub(super) view: ViewState,
    pub(super) commit: CommitState,
    /// The file shown in the read-only viewer modal, if open.
    pub viewer: Option<FileViewer>,
    /// The file (display path, and all paths to revert) awaiting confirmation to be discarded.
    pub pending_discard: Option<(String, Vec<String>)>,
}

/// A file open in the viewer modal: its text split into lines, with syntax highlighting.
pub struct FileViewer {
    pub path: String,
    pub content: Result<ViewedFile, String>,
}

pub struct ViewedFile {
    pub lines: Vec<String>,
    pub highlight: HunkHighlight,
}

impl ChangesModel {
    pub fn new(ctx: &egui::Context, backend: &Backend, notifier: &Notifier) -> Self {
        ChangesModel {
            ctx: ctx.clone(),
            backend: backend.clone(),
            notifier: notifier.clone(),
            project: None,
            generation: 0,
            loaded: false,
            changes: None,
            since: None,
            since_tree: None,
            load: None,
            since_load: None,
            reload_again: false,
            since_reload_again: false,
            ops: Vec::new(),
            scroll_to: None,
            view: ViewState::default(),
            commit: CommitState::default(),
            viewer: None,
            pending_discard: None,
        }
    }

    /// Open the viewer modal on a project file (read synchronously; files are size-capped).
    pub fn view_file(&mut self, path: String) {
        let Some(project) = self.project.clone() else {
            return;
        };
        let content = self
            .backend
            .read_project_file(&project, &path)
            .map(|text| {
                let lines: Vec<String> = text.lines().map(str::to_string).collect();
                let diff_lines: Vec<DiffLine> = lines
                    .iter()
                    .enumerate()
                    .map(|(i, t)| DiffLine {
                        kind: LineKind::Context,
                        old_no: Some(i as u32 + 1),
                        new_no: Some(i as u32 + 1),
                        text: t.clone(),
                    })
                    .collect();
                let highlight = crate::highlight::highlight_hunk(&path, &diff_lines);
                ViewedFile { lines, highlight }
            });
        self.viewer = Some(FileViewer { path, content });
    }

    /// Call when the open project changes (also fine to call every frame with the same value).
    pub fn set_project(&mut self, project: Option<&str>) {
        if self.project.as_deref() == project {
            return;
        }
        self.project = project.map(|p| p.to_string());
        self.generation += 1;
        self.loaded = false;
        self.changes = None;
        self.since = None;
        self.since_tree = None;
        self.load = None;
        self.since_load = None;
        self.reload_again = false;
        self.since_reload_again = false;
        self.scroll_to = None;
        self.view = ViewState::default();
        self.commit = CommitState::default();
        self.start_load();
    }

    /// Load the changes again (the watcher saw the tree change, or the window regained focus).
    pub fn reload(&mut self) {
        self.start_load();
        self.start_since_load();
    }

    /// The loaded changes; `None` until the first load finishes, or when they cannot be read.
    pub fn changes(&self) -> Option<&RepoChanges> {
        self.changes.as_deref()
    }

    /// Number of distinct changed paths (a partially staged file counts once).
    pub fn changed_count(&self) -> usize {
        match &self.changes {
            Some(changes) => changed_file_count(changes),
            None => 0,
        }
    }

    pub fn staged_count(&self) -> usize {
        match &self.changes {
            Some(changes) => changes.staged.len(),
            None => 0,
        }
    }

    /// True while the first load of the open project is running.
    pub fn loading(&self) -> bool {
        self.project.is_some() && !self.loaded
    }

    /// Focus the commit message box on the next frame.
    pub fn focus_commit(&mut self) {
        self.commit.focus = true;
    }

    /// Make the main view scroll to a file, opening whatever hides it.
    pub fn reveal(&mut self, staged: bool, path: &str) {
        let key = file_key(staged, path);
        if staged {
            self.view.staged_collapsed = false;
        } else {
            self.view.unstaged_collapsed = false;
        }
        self.view.collapsed_files.remove(&key);
        self.view.scope = Scope::All;
        self.scroll_to = Some(key);
    }

    pub(super) fn changes_arc(&self) -> Option<Arc<RepoChanges>> {
        self.changes.clone()
    }

    pub(super) fn since_arc(&self) -> Option<Arc<Vec<FileChange>>> {
        self.since.clone()
    }

    /// Stage files (a rename needs its old and new path; see `file_paths`).
    pub fn stage(&mut self, paths: Vec<String>) {
        self.spawn_op(OpKind::Stage, move |backend: Backend, project: String| {
            backend.stage(&project, &paths)
        });
    }

    pub fn unstage(&mut self, paths: Vec<String>) {
        self.spawn_op(OpKind::Unstage, move |backend: Backend, project: String| {
            backend.unstage(&project, &paths)
        });
    }

    /// Throw away all changes (staged and unstaged) to files. Irreversible: callers confirm first.
    pub fn discard(&mut self, paths: Vec<String>) {
        self.spawn_op(OpKind::Discard, move |backend: Backend, project: String| {
            backend.discard(&project, &paths)
        });
    }

    /// Commit the staged files with the typed message, if the rules allow it.
    pub fn start_commit(&mut self) {
        if !can_commit(
            self.project.is_some(),
            self.staged_count(),
            &self.commit.message,
            self.commit.committing,
        ) {
            return;
        }
        self.commit.committing = true;
        let message = self.commit.message.clone();
        self.spawn_op(OpKind::Commit, move |backend: Backend, project: String| {
            backend.commit(&project, &message)
        });
    }

    pub fn open_in_editor(&mut self, path: String, line: u32) {
        self.spawn_op(OpKind::Editor, move |backend: Backend, project: String| {
            backend.open_in_editor(&project, &path, line)
        });
    }

    fn spawn_op<F>(&mut self, kind: OpKind, work: F)
    where
        F: FnOnce(Backend, String) -> Result<(), String> + Send + 'static,
    {
        let project = match &self.project {
            Some(project) => project.clone(),
            None => {
                if kind == OpKind::Commit {
                    self.commit.committing = false;
                }
                return;
            }
        };
        let backend = self.backend.clone();
        self.ops.push(Task::spawn(&self.ctx, move || {
            (kind, work(backend, project))
        }));
    }

    fn start_load(&mut self) {
        let project = match &self.project {
            Some(project) => project.clone(),
            None => return,
        };
        if self.load.is_some() {
            self.reload_again = true;
            return;
        }
        let backend = self.backend.clone();
        let generation = self.generation;
        self.load = Some(Task::spawn(&self.ctx, move || {
            (generation, backend.changes(&project))
        }));
    }

    fn start_since_load(&mut self) {
        let (project, tree) = match (&self.project, &self.since_tree) {
            (Some(project), Some(tree)) => (project.clone(), tree.clone()),
            _ => return,
        };
        if self.since_load.is_some() {
            self.since_reload_again = true;
            return;
        }
        let backend = self.backend.clone();
        let generation = self.generation;
        self.since_load = Some(Task::spawn(&self.ctx, move || {
            let files = backend.changes_since(&project, &tree);
            (generation, tree, files)
        }));
    }

    fn apply_loaded(&mut self, result: Option<RepoChanges>, review: &mut ReviewModel) {
        self.loaded = true;
        match result {
            Some(new) => {
                let same = match self.changes.as_deref() {
                    Some(old) => *old == new,
                    None => false,
                };
                if !same {
                    let arc = Arc::new(new);
                    reanchor_in_place(&mut review.comments, &arc);
                    self.changes = Some(arc);
                    self.view.clear_caches();
                }
            }
            None => {
                self.changes = None;
                self.view.clear_caches();
            }
        }
    }

    /// Poll the background work. Call once per frame. Comments are re-anchored to the new diff
    /// (or marked outdated) whenever the changes reload.
    pub fn update(&mut self, review: &mut ReviewModel) {
        let polled = poll_slot(&mut self.load);
        if let Some((generation, result)) = polled {
            if generation == self.generation {
                self.apply_loaded(result, review);
            }
        }
        if self.load.is_none() && self.reload_again {
            self.reload_again = false;
            self.start_load();
        }

        // The baseline moves when a review is sent or a session is restored
        let baseline: Option<String> = review.baseline().map(|b| b.to_string());
        if baseline != self.since_tree {
            self.since_tree = baseline;
            self.since = None;
            self.since_load = None;
            self.since_reload_again = false;
            self.view.drop_since_cache();
            self.start_since_load();
        }
        let polled = poll_slot(&mut self.since_load);
        if let Some((generation, tree, files)) = polled {
            let current = self.since_tree.as_deref() == Some(tree.as_str());
            if generation == self.generation && current {
                self.since = files.map(Arc::new);
                self.view.drop_since_cache();
            }
        }
        if self.since_load.is_none() && self.since_reload_again {
            self.since_reload_again = false;
            self.start_since_load();
        }

        if review.take_needs_reanchor() {
            if let Some(changes) = &self.changes {
                reanchor_in_place(&mut review.comments, changes);
            }
        }

        self.poll_ops();
    }

    fn poll_ops(&mut self) {
        let mut results: Vec<(OpKind, Result<(), String>)> = Vec::new();
        let mut keep: Vec<bool> = Vec::new();
        for task in self.ops.iter_mut() {
            let finished = !task.is_running();
            if let Some(result) = task.poll() {
                results.push(result);
            }
            keep.push(!finished);
        }
        let mut index = 0usize;
        self.ops.retain(|_| {
            let k = keep.get(index).copied().unwrap_or(true);
            index += 1;
            k
        });
        for (kind, result) in results {
            match (kind, result) {
                (OpKind::Stage, Err(msg)) => self.notifier.notify("Stage failed", &msg),
                (OpKind::Unstage, Err(msg)) => self.notifier.notify("Unstage failed", &msg),
                (OpKind::Discard, Err(msg)) => self.notifier.notify("Discard failed", &msg),
                (OpKind::Editor, Err(msg)) => self.notifier.notify("Open in editor failed", &msg),
                (OpKind::Commit, Err(msg)) => {
                    // Keep the typed message so nothing is lost
                    self.commit.committing = false;
                    self.notifier.notify("Commit failed", &msg);
                }
                (OpKind::Commit, Ok(())) => {
                    self.commit.committing = false;
                    self.commit.message.clear();
                }
                (_, Ok(())) => {}
            }
            if kind != OpKind::Editor {
                self.reload();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::Hunk;

    fn dl(kind: LineKind, old_no: Option<u32>, new_no: Option<u32>, text: &str) -> DiffLine {
        DiffLine {
            kind,
            old_no,
            new_no,
            text: text.to_string(),
        }
    }

    fn file_with(lines: Vec<DiffLine>) -> FileChange {
        FileChange {
            path: "a.rs".to_string(),
            old_path: None,
            status: ChangeStatus::Modified,
            binary: false,
            too_large: false,
            additions: 0,
            deletions: 0,
            hunks: vec![Hunk {
                header: "@@ -1 +1 @@".to_string(),
                lines,
            }],
        }
    }

    fn new_comment(text: &str) -> NewComment {
        NewComment {
            file: "a.rs".to_string(),
            staged: false,
            side: Side::New,
            start: 3,
            end: 4,
            snapshot: vec!["x".to_string(), "y".to_string()],
            text: text.to_string(),
        }
    }

    #[test]
    fn clicking_a_line_selects_it_and_shift_extends_the_range() {
        let first = pick_draft(None, "a.rs", false, Side::New, 5, false);
        assert_eq!((first.start, first.end, first.anchor), (5, 5, 5));
        let extended = pick_draft(Some(&first), "a.rs", false, Side::New, 8, true);
        assert_eq!((extended.start, extended.end, extended.anchor), (5, 8, 5));
        let backwards = pick_draft(Some(&extended), "a.rs", false, Side::New, 2, true);
        assert_eq!(
            (backwards.start, backwards.end, backwards.anchor),
            (2, 5, 5)
        );
    }

    #[test]
    fn shift_click_on_another_file_or_side_starts_over() {
        let first = pick_draft(None, "a.rs", false, Side::New, 5, false);
        let other_file = pick_draft(Some(&first), "b.rs", false, Side::New, 8, true);
        assert_eq!((other_file.start, other_file.end), (8, 8));
        let other_side = pick_draft(Some(&first), "a.rs", false, Side::Old, 8, true);
        assert_eq!((other_side.start, other_side.end), (8, 8));
        let other_group = pick_draft(Some(&first), "a.rs", true, Side::New, 8, true);
        assert_eq!((other_group.start, other_group.end), (8, 8));
        let plain = pick_draft(Some(&first), "a.rs", false, Side::New, 8, false);
        assert_eq!((plain.start, plain.end), (8, 8));
    }

    #[test]
    fn draft_selection_and_editor_position() {
        let draft = pick_draft(None, "a.rs", false, Side::New, 4, false);
        let extended = pick_draft(Some(&draft), "a.rs", false, Side::New, 6, true);
        assert!(draft_selects(Some(&extended), "a.rs", false, Side::New, 5));
        assert!(!draft_selects(Some(&extended), "a.rs", false, Side::New, 7));
        assert!(!draft_selects(Some(&extended), "a.rs", false, Side::Old, 5));
        assert!(!draft_selects(None, "a.rs", false, Side::New, 5));
        assert!(draft_ends_at(Some(&extended), "a.rs", false, Side::New, 6));
        assert!(!draft_ends_at(Some(&extended), "a.rs", false, Side::New, 4));
    }

    #[test]
    fn commit_needs_a_project_staged_files_a_message_and_no_commit_running() {
        assert!(can_commit(true, 1, "fix", false));
        assert!(!can_commit(false, 1, "fix", false));
        assert!(!can_commit(true, 0, "fix", false));
        assert!(!can_commit(true, 1, "   \n", false));
        assert!(!can_commit(true, 1, "fix", true));
    }

    #[test]
    fn rename_paths_cover_old_and_new() {
        let mut file = file_with(vec![]);
        assert_eq!(file_paths(&file), vec!["a.rs".to_string()]);
        file.old_path = Some("old.rs".to_string());
        assert_eq!(
            file_paths(&file),
            vec!["old.rs".to_string(), "a.rs".to_string()]
        );
    }

    #[test]
    fn first_line_prefers_the_first_added_line() {
        let file = file_with(vec![
            dl(LineKind::Context, Some(9), Some(9), "ctx"),
            dl(LineKind::Del, Some(10), None, "gone"),
            dl(LineKind::Add, None, Some(10), "new"),
        ]);
        assert_eq!(first_line(&file), 10);
        let only_deletions = file_with(vec![
            dl(LineKind::Del, Some(4), None, "gone"),
            dl(LineKind::Context, Some(5), Some(4), "ctx"),
        ]);
        assert_eq!(first_line(&only_deletions), 4);
        assert_eq!(first_line(&file_with(vec![])), 1);
    }

    #[test]
    fn split_rows_become_indices_into_the_hunk() {
        let lines = vec![
            dl(LineKind::Context, Some(1), Some(1), "a"),
            dl(LineKind::Del, Some(2), None, "b"),
            dl(LineKind::Del, Some(3), None, "c"),
            dl(LineKind::Add, None, Some(2), "d"),
        ];
        let rows = split_index_rows(&lines);
        assert_eq!(
            rows,
            vec![(Some(0), Some(0)), (Some(1), Some(3)), (Some(2), None),]
        );
    }

    #[test]
    fn rows_outside_the_viewport_are_skipped() {
        assert!(row_visible(100.0, 18.0, 90.0, 500.0));
        assert!(!row_visible(-500.0, 18.0, 0.0, 500.0));
        assert!(!row_visible(900.0, 18.0, 0.0, 500.0));
        // One row of margin on both sides
        assert!(row_visible(515.0, 18.0, 0.0, 500.0));
    }

    #[test]
    fn comments_are_added_edited_and_removed() {
        let mut review = ReviewModel::new();
        review.add(new_comment("first"));
        review.add(new_comment("second"));
        assert_eq!(review.comments().len(), 2);
        assert_eq!(review.comments()[0].round, 1);
        assert!(!review.comments()[0].sent);
        let id = review.comments()[0].id.clone();
        review.edit(&id, "changed");
        assert_eq!(review.comments()[0].text, "changed");
        review.remove(&id);
        assert_eq!(review.comments().len(), 1);
        assert_eq!(review.comments()[0].text, "second");
    }

    #[test]
    fn marking_sent_advances_the_round_and_records_the_baseline() {
        let mut review = ReviewModel::new();
        review.add(new_comment("one"));
        review.add(new_comment("two"));
        let id = review.comments()[0].id.clone();
        review.mark_sent(&[id], Some("tree1".to_string()));
        assert_eq!(review.round(), 2);
        assert_eq!(review.baseline(), Some("tree1"));
        assert_eq!(review.pending().len(), 1);
        assert_eq!(review.pending_count(), 1);
        review.add(new_comment("three"));
        assert_eq!(review.comments()[2].round, 2);
        // A send without a baseline keeps the earlier one
        review.mark_sent(&[], None);
        assert_eq!(review.baseline(), Some("tree1"));
        assert_eq!(review.round(), 3);
    }

    #[test]
    fn changing_project_resets_the_review() {
        let mut review = ReviewModel::new();
        review.set_project(Some("/p"));
        review.add(new_comment("one"));
        review.set_project(Some("/p"));
        assert_eq!(review.comments().len(), 1);
        review.set_project(Some("/q"));
        assert!(review.comments().is_empty());
        assert_eq!(review.round(), 1);
        assert_eq!(review.baseline(), None);
    }

    #[test]
    fn restoring_a_session_keeps_pending_comments_and_continues_the_round() {
        let mut review = ReviewModel::new();
        review.add(new_comment("pending"));
        let sent = vec![SentComment {
            id: "c1".to_string(),
            file: "a.rs".to_string(),
            side: Side::New,
            start: 1,
            end: 1,
            snapshot: vec!["x".to_string()],
            text: "old".to_string(),
            round: 2,
            outdated: false,
        }];
        review.restore(sent);
        assert_eq!(review.round(), 3);
        assert_eq!(review.comments().len(), 2);
        assert!(review.comments()[0].sent);
        assert_eq!(review.comments()[1].round, 3);
        assert!(review.take_needs_reanchor());
        assert!(!review.take_needs_reanchor());
        assert_eq!(review.baseline(), None);
    }

    #[test]
    fn keys_tell_groups_and_renames_apart() {
        assert_ne!(file_key(true, "a"), file_key(false, "a"));
        let mut file = file_with(vec![]);
        let plain = since_key(&file);
        file.old_path = Some("old".to_string());
        assert_ne!(plain, since_key(&file));
        assert!(since_key(&file).starts_with("since:"));
    }
}
