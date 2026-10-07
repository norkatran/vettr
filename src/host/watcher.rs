//! Working tree watcher (port of `src/main/watcher.ts`, see `docs/designs/0005`).
//!
//! `watch_tree` calls a callback (debounced, so a burst of edits fires once) whenever something
//! in the working tree changes, leaving out what git ignores. Ignored directories are never
//! watched at all: instead of one recursive watch, every non-ignored directory gets its own
//! non-recursive watch, so a huge `vendor/` or `node_modules/` costs no watch handles.
//! `ProjectWatcher` keeps at most one watcher alive across project switches.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

use super::git::list_ignored;

/// Quiet time after the last event before the callback fires.
pub const DEBOUNCE_MS: u64 = 250;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Errors that mean the system is out of watch handles: keep going and it only gets worse.
pub fn is_exhausted(error: &notify::Error) -> bool {
    match &error.kind {
        notify::ErrorKind::MaxFilesWatch => true,
        notify::ErrorKind::Io(io) => match io.raw_os_error() {
            Some(code) => code == libc::EMFILE || code == libc::ENFILE || code == libc::ENOSPC,
            None => false,
        },
        _ => false,
    }
}

fn is_git_state(name: &str) -> bool {
    name == "HEAD" || name == "index"
}

/// Whether the watcher should skip `path`. `ignored` holds the paths (relative to `root`, with
/// `/` separators) that git ignores; a path is skipped when it or any parent is listed. Nothing
/// is skipped by name, so a project that tracks `node_modules` gets it watched. Inside `.git`
/// only `HEAD` and `index` matter: they change when the user stages, commits or switches
/// branch, which changes what the diff against `HEAD` shows.
pub fn is_ignored(root: &Path, path: &Path, ignored: &HashSet<String>) -> bool {
    let rel = match path.strip_prefix(root) {
        Ok(rel) => rel,
        Err(_) => return false,
    };
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.first().map(|p| p.as_str()) == Some(".git") {
        return parts.len() > 1 && !(parts.len() == 2 && is_git_state(&parts[1]));
    }
    let mut prefix = String::new();
    for part in &parts {
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(part);
        if ignored.contains(&prefix) {
            return true;
        }
    }
    false
}

enum Msg {
    /// Something changed; `new_dirs` are directories that appeared and need watching.
    Change { new_dirs: Vec<PathBuf> },
    /// The system ran out of watch handles: stop watching.
    Exhausted,
    /// The handle was dropped.
    Stop,
}

/// Watch `dir` and every non-ignored directory below it, each non-recursively. Only running out
/// of handles is an error; directories that vanish meanwhile are skipped.
fn add_tree(
    watcher: &mut RecommendedWatcher,
    root: &Path,
    dir: &Path,
    ignored: &HashSet<String>,
) -> Result<(), notify::Error> {
    if let Err(error) = watcher.watch(dir, RecursiveMode::NonRecursive) {
        if is_exhausted(&error) {
            return Err(error);
        }
        return Ok(());
    }
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        // `file_type` does not follow symlinks, so linked directories are not walked
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if !is_dir {
            continue;
        }
        let path = entry.path();
        if is_ignored(root, &path, ignored) {
            continue;
        }
        add_tree(watcher, root, &path, ignored)?;
    }
    Ok(())
}

/// A running watcher. Dropping it stops watching and cancels a pending callback.
pub struct Watcher {
    inner: Arc<Mutex<Option<RecommendedWatcher>>>,
    tx: Sender<Msg>,
    stopped: Arc<AtomicBool>,
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = self.tx.send(Msg::Stop);
        let taken = {
            let mut guard = lock(&self.inner);
            guard.take()
        };
        drop(taken);
    }
}

/// Start watching the directories that appeared. Returns true if the handles ran out (the
/// watcher has then been closed).
fn watch_new_dirs(
    inner: &Arc<Mutex<Option<RecommendedWatcher>>>,
    root: &Path,
    ignored: &HashSet<String>,
    new_dirs: &[PathBuf],
) -> bool {
    let mut exhausted = false;
    let mut guard = lock(inner);
    if let Some(watcher) = guard.as_mut() {
        for dir in new_dirs {
            if !dir.is_dir() || is_ignored(root, dir, ignored) {
                continue;
            }
            if let Err(error) = add_tree(watcher, root, dir, ignored) {
                eprintln!("vettr: file watcher error {}", error);
                exhausted = true;
                break;
            }
        }
    }
    if exhausted {
        let taken = guard.take();
        drop(guard);
        drop(taken);
    }
    exhausted
}

fn run_debounce<F: Fn()>(
    rx: Receiver<Msg>,
    debounce: Duration,
    on_change: F,
    inner: Arc<Mutex<Option<RecommendedWatcher>>>,
    stopped: Arc<AtomicBool>,
    root: PathBuf,
    ignored: HashSet<String>,
) {
    let mut pending = false;
    loop {
        let msg: Result<Msg, RecvTimeoutError> = if pending {
            rx.recv_timeout(debounce)
        } else {
            rx.recv().map_err(|_| RecvTimeoutError::Disconnected)
        };
        match msg {
            Ok(Msg::Change { new_dirs }) => {
                if !new_dirs.is_empty() && watch_new_dirs(&inner, &root, &ignored, &new_dirs) {
                    return;
                }
                pending = true;
            }
            Ok(Msg::Exhausted) => {
                let taken = {
                    let mut guard = lock(&inner);
                    guard.take()
                };
                drop(taken);
                return;
            }
            Ok(Msg::Stop) => return,
            Err(RecvTimeoutError::Timeout) => {
                pending = false;
                if !stopped.load(Ordering::SeqCst) {
                    on_change();
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// Call `on_change` (debounced by `DEBOUNCE_MS`) whenever something in the working tree at
/// `root` changes, leaving out what git ignores. Returns once the watches are in place; the
/// callback runs on a background thread. Dropping the returned `Watcher` stops it.
pub fn watch_tree<F>(root: impl AsRef<Path>, on_change: F) -> Result<Watcher, String>
where
    F: Fn() + Send + 'static,
{
    watch_tree_with_debounce(root, on_change, Duration::from_millis(DEBOUNCE_MS))
}

/// `watch_tree` with a custom debounce.
pub fn watch_tree_with_debounce<F>(
    root: impl AsRef<Path>,
    on_change: F,
    debounce: Duration,
) -> Result<Watcher, String>
where
    F: Fn() + Send + 'static,
{
    let root: PathBuf = root.as_ref().to_path_buf();
    if !root.is_dir() {
        return Err(format!("{} is not a directory", root.display()));
    }
    let ignored: HashSet<String> = match list_ignored(&root) {
        Some(list) => list.into_iter().collect(),
        None => {
            eprintln!(
                "vettr: could not list ignored paths in {}; watching everything",
                root.display()
            );
            HashSet::new()
        }
    };

    let (tx, rx) = mpsc::channel::<Msg>();
    let handler_tx = tx.clone();
    let handler_root = root.clone();
    let handler_ignored = ignored.clone();
    let handler = move |res: notify::Result<notify::Event>| match res {
        Ok(event) => {
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            let relevant: Vec<PathBuf> = event
                .paths
                .iter()
                .filter(|p| !is_ignored(&handler_root, p.as_path(), &handler_ignored))
                .cloned()
                .collect();
            if relevant.is_empty() {
                return;
            }
            let may_add_dirs = matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Modify(notify::event::ModifyKind::Name(_))
            );
            let new_dirs: Vec<PathBuf> = if may_add_dirs {
                relevant.into_iter().filter(|p| p.is_dir()).collect()
            } else {
                Vec::new()
            };
            let _ = handler_tx.send(Msg::Change { new_dirs });
        }
        Err(error) => {
            // Errors must not take the app down. Running out of handles is not survivable for
            // the machine, so stop watching; the window-focus reload still refreshes the view.
            eprintln!("vettr: file watcher error {}", error);
            if is_exhausted(&error) {
                let _ = handler_tx.send(Msg::Exhausted);
            }
        }
    };
    let mut watcher: RecommendedWatcher =
        notify::recommended_watcher(handler).map_err(|e| e.to_string())?;
    add_tree(&mut watcher, &root, &root, &ignored).map_err(|e| e.to_string())?;

    let inner = Arc::new(Mutex::new(Some(watcher)));
    let stopped = Arc::new(AtomicBool::new(false));
    let thread_inner = inner.clone();
    let thread_stopped = stopped.clone();
    thread::spawn(move || {
        run_debounce(
            rx,
            debounce,
            on_change,
            thread_inner,
            thread_stopped,
            root,
            ignored,
        )
    });
    Ok(Watcher { inner, tx, stopped })
}

/// The callback shared between watchers.
pub type OnChange = Arc<dyn Fn() + Send + Sync>;

type StartFn<H> = Box<dyn Fn(&str, OnChange) -> Result<H, String> + Send + Sync>;

/// Keeps at most one project watcher alive. Switches run one at a time, and a watcher that
/// finished starting after the project changed again is dropped straight away, so quick project
/// switches cannot leak a full-tree watcher. Generic over the handle type so the logic can be
/// tested; dropping a handle stops it. Use `ProjectWatcher::new` for the real thing.
pub struct ProjectWatcher<H> {
    on_change: OnChange,
    start: StartFn<H>,
    current: Mutex<Option<H>>,
    generation: AtomicU64,
}

impl<H: Send + 'static> ProjectWatcher<H> {
    pub fn with_start(on_change: OnChange, start: StartFn<H>) -> Self {
        ProjectWatcher {
            on_change,
            start,
            current: Mutex::new(None),
            generation: AtomicU64::new(0),
        }
    }

    /// Watch `path` instead of whatever was watched before. Blocks until its turn has run; a
    /// call replaced by a later one before its turn does nothing.
    pub fn watch(&self, path: &str) {
        let mine = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let mut current = lock(&self.current);
        let previous = current.take();
        drop(previous);
        if mine != self.generation.load(Ordering::SeqCst) {
            return;
        }
        match (self.start)(path, self.on_change.clone()) {
            Ok(next) => {
                if mine == self.generation.load(Ordering::SeqCst) {
                    *current = Some(next);
                } else {
                    drop(next);
                }
            }
            Err(error) => eprintln!("vettr: watcher {}", error),
        }
    }

    /// Stop watching (app quit).
    pub fn close(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        let previous = {
            let mut current = lock(&self.current);
            current.take()
        };
        drop(previous);
    }
}

impl ProjectWatcher<Watcher> {
    pub fn new(on_change: OnChange) -> Self {
        let start: StartFn<Watcher> =
            Box::new(|path: &str, cb: OnChange| watch_tree(path, move || (*cb)()));
        ProjectWatcher::with_start(on_change, start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Instant;

    // Native file events are not instant: right after the watcher starts the first writes can
    // be missed or arrive late, and a burst can be split into batches. The tests allow for that
    // with a short warm-up, a generous wait for the first call and a debounce well above the
    // batch gap.
    const WARM_UP_MS: u64 = 100;
    const FIRST_CALL_TIMEOUT_MS: u64 = 5000;

    fn sleep(ms: u64) {
        thread::sleep(Duration::from_millis(ms));
    }

    fn wait_for(timeout_ms: u64, cond: impl Fn() -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(timeout_ms) {
            if cond() {
                return true;
            }
            sleep(20);
        }
        cond()
    }

    fn temp_root() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let path = fs::canonicalize(tmp.path()).unwrap();
        (tmp, path)
    }

    fn counter() -> (Arc<AtomicUsize>, impl Fn() + Send + 'static) {
        let count = Arc::new(AtomicUsize::new(0));
        let inner = count.clone();
        (count, move || {
            inner.fetch_add(1, Ordering::SeqCst);
        })
    }

    fn set(items: &[&str]) -> HashSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn git_init(root: &Path) {
        let out = std::process::Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(root)
            .output()
            .unwrap();
        assert!(out.status.success());
    }

    // is_ignored

    #[test]
    fn ignores_listed_paths_and_everything_under_them() {
        let root = PathBuf::from("/proj");
        let ignored = set(&["vendor", "pkg/node_modules", "debug.log"]);
        assert!(is_ignored(&root, &root.join("vendor"), &ignored));
        assert!(is_ignored(
            &root,
            &root.join("vendor").join("a").join("b.php"),
            &ignored
        ));
        assert!(is_ignored(
            &root,
            &root.join("pkg").join("node_modules").join("x.js"),
            &ignored
        ));
        assert!(is_ignored(&root, &root.join("debug.log"), &ignored));
    }

    #[test]
    fn skips_nothing_by_name_so_node_modules_is_watched_unless_git_ignores_it() {
        let root = PathBuf::from("/proj");
        let none = HashSet::new();
        assert!(!is_ignored(
            &root,
            &root.join("node_modules").join("x.js"),
            &none
        ));
        assert!(!is_ignored(
            &root,
            &root.join("vendor").join("x.php"),
            &none
        ));
        let ignored = set(&["vendor"]);
        assert!(!is_ignored(&root, &root.join("node_modules"), &ignored));
        assert!(!is_ignored(
            &root,
            &root.join("src").join("vendor.ts"),
            &ignored
        ));
    }

    #[test]
    fn keeps_ordinary_files_and_the_root_itself() {
        let root = PathBuf::from("/proj");
        let none = HashSet::new();
        assert!(!is_ignored(&root, &root, &none));
        assert!(!is_ignored(&root, &root.join("src").join("a.ts"), &none));
    }

    #[test]
    fn ignores_git_contents_except_head_and_index() {
        let root = PathBuf::from("/proj");
        let none = HashSet::new();
        assert!(!is_ignored(&root, &root.join(".git"), &none));
        assert!(!is_ignored(&root, &root.join(".git").join("HEAD"), &none));
        assert!(!is_ignored(&root, &root.join(".git").join("index"), &none));
        assert!(is_ignored(&root, &root.join(".git").join("objects"), &none));
        assert!(is_ignored(
            &root,
            &root.join(".git").join("refs").join("HEAD"),
            &none
        ));
    }

    // Error handling (watcher.error.test.ts: errors are survived, and running out of handles
    // closes the watcher; here the classification that decides that).

    #[test]
    fn running_out_of_handles_is_exhaustion() {
        assert!(is_exhausted(&notify::Error::new(
            notify::ErrorKind::MaxFilesWatch
        )));
        for code in [libc::EMFILE, libc::ENFILE, libc::ENOSPC] {
            let error = notify::Error::io(std::io::Error::from_raw_os_error(code));
            assert!(is_exhausted(&error));
        }
    }

    #[test]
    fn other_errors_do_not_stop_the_watcher() {
        assert!(!is_exhausted(&notify::Error::generic("boom")));
        assert!(!is_exhausted(&notify::Error::path_not_found()));
        let denied = notify::Error::io(std::io::Error::from_raw_os_error(libc::EACCES));
        assert!(!is_exhausted(&denied));
        let plain = notify::Error::io(std::io::Error::other("x"));
        assert!(!is_exhausted(&plain));
    }

    // watch_tree

    #[test]
    fn fires_once_for_a_burst_of_changes() {
        let (_tmp, root) = temp_root();
        let (count, on_change) = counter();
        let watcher =
            watch_tree_with_debounce(&root, on_change, Duration::from_millis(200)).unwrap();
        sleep(WARM_UP_MS);
        fs::write(root.join("a.txt"), "1").unwrap();
        fs::write(root.join("b.txt"), "2").unwrap();
        assert!(wait_for(FIRST_CALL_TIMEOUT_MS, || count
            .load(Ordering::SeqCst)
            >= 1));
        sleep(600);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        drop(watcher);
    }

    #[test]
    fn does_not_fire_for_paths_git_ignores_and_does_for_the_rest() {
        let (_tmp, root) = temp_root();
        git_init(&root);
        fs::write(root.join(".gitignore"), "vendor/\nnode_modules/\n").unwrap();
        fs::create_dir(root.join("vendor")).unwrap();
        fs::create_dir(root.join("node_modules")).unwrap();
        fs::create_dir(root.join("tracked_deps")).unwrap();
        let (count, on_change) = counter();
        let watcher =
            watch_tree_with_debounce(&root, on_change, Duration::from_millis(50)).unwrap();
        sleep(WARM_UP_MS);
        fs::write(root.join("vendor").join("x.php"), "1").unwrap();
        fs::write(root.join("node_modules").join("x.js"), "1").unwrap();
        sleep(300);
        assert_eq!(count.load(Ordering::SeqCst), 0);
        fs::write(root.join("tracked_deps").join("y.js"), "1").unwrap();
        assert!(wait_for(FIRST_CALL_TIMEOUT_MS, || count
            .load(Ordering::SeqCst)
            >= 1));
        drop(watcher);
    }

    #[test]
    fn watches_node_modules_when_gitignore_does_not_exclude_it() {
        let (_tmp, root) = temp_root();
        git_init(&root);
        fs::create_dir(root.join("node_modules")).unwrap();
        let (count, on_change) = counter();
        let watcher =
            watch_tree_with_debounce(&root, on_change, Duration::from_millis(50)).unwrap();
        sleep(WARM_UP_MS);
        fs::write(root.join("node_modules").join("x.js"), "1").unwrap();
        assert!(wait_for(FIRST_CALL_TIMEOUT_MS, || count
            .load(Ordering::SeqCst)
            >= 1));
        drop(watcher);
    }

    #[test]
    fn does_not_fire_after_being_stopped_even_with_a_change_pending() {
        let (_tmp, root) = temp_root();
        let (count, on_change) = counter();
        let watcher =
            watch_tree_with_debounce(&root, on_change, Duration::from_millis(100)).unwrap();
        fs::write(root.join("a.txt"), "1").unwrap();
        sleep(30);
        drop(watcher);
        sleep(200);
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn uses_the_default_debounce_when_none_is_given() {
        let (_tmp, root) = temp_root();
        let (count, on_change) = counter();
        let watcher = watch_tree(&root, on_change).unwrap();
        sleep(WARM_UP_MS);
        fs::write(root.join("a.txt"), "1").unwrap();
        assert!(wait_for(FIRST_CALL_TIMEOUT_MS, || count
            .load(Ordering::SeqCst)
            == 1));
        drop(watcher);
    }

    #[test]
    fn watches_directories_created_after_it_started() {
        let (_tmp, root) = temp_root();
        let (count, on_change) = counter();
        let watcher =
            watch_tree_with_debounce(&root, on_change, Duration::from_millis(50)).unwrap();
        sleep(WARM_UP_MS);
        fs::create_dir(root.join("sub")).unwrap();
        assert!(wait_for(FIRST_CALL_TIMEOUT_MS, || count
            .load(Ordering::SeqCst)
            >= 1));
        sleep(300);
        let before = count.load(Ordering::SeqCst);
        fs::write(root.join("sub").join("f.txt"), "1").unwrap();
        assert!(wait_for(FIRST_CALL_TIMEOUT_MS, || count
            .load(Ordering::SeqCst)
            > before));
        drop(watcher);
    }

    #[test]
    fn refuses_a_folder_that_does_not_exist() {
        let (_tmp, root) = temp_root();
        let (_count, on_change) = counter();
        assert!(watch_tree(root.join("gone"), on_change).is_err());
    }

    // ProjectWatcher

    type Log = Arc<Mutex<Vec<String>>>;

    /// A fake watcher handle that records when it is stopped (dropped).
    struct Handle {
        name: String,
        stops: Log,
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            lock(&self.stops).push(self.name.clone());
        }
    }

    fn noop() -> OnChange {
        Arc::new(|| {})
    }

    fn snapshot(log: &Log) -> Vec<String> {
        lock(log).clone()
    }

    fn strs(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// A project watcher whose `start` records each path, fails for `/bad` and blocks on
    /// `gate` for `/a` and `/gate` until it is released.
    fn fake_project_watcher() -> (
        Arc<ProjectWatcher<Handle>>,
        Log,
        Log,
        Arc<AtomicBool>,
        Sender<()>,
    ) {
        let starts: Log = Arc::new(Mutex::new(Vec::new()));
        let stops: Log = Arc::new(Mutex::new(Vec::new()));
        let entered = Arc::new(AtomicBool::new(false));
        let (gate_tx, gate_rx) = mpsc::channel::<()>();
        let gate_rx = Arc::new(Mutex::new(gate_rx));
        let (s, t, e) = (starts.clone(), stops.clone(), entered.clone());
        let start: StartFn<Handle> = Box::new(move |path: &str, _cb: OnChange| {
            lock(&s).push(path.to_string());
            if path == "/bad" {
                return Err("boom".to_string());
            }
            if path == "/a" || path == "/gate" {
                e.store(true, Ordering::SeqCst);
                let _ = lock(&gate_rx).recv();
            }
            Ok(Handle {
                name: path.to_string(),
                stops: t.clone(),
            })
        });
        let pw = Arc::new(ProjectWatcher::with_start(noop(), start));
        (pw, starts, stops, entered, gate_tx)
    }

    #[test]
    fn closes_the_previous_watcher_when_switching() {
        let (pw, _starts, stops, _entered, gate_tx) = fake_project_watcher();
        gate_tx.send(()).unwrap();
        pw.watch("/a");
        pw.watch("/b");
        assert_eq!(snapshot(&stops), strs(&["/a"]));
        pw.close();
        assert_eq!(snapshot(&stops), strs(&["/a", "/b"]));
    }

    #[test]
    fn does_not_leak_a_watcher_that_is_still_starting_when_another_project_opens() {
        let (pw, _starts, stops, entered, gate_tx) = fake_project_watcher();
        let first = {
            let pw = pw.clone();
            thread::spawn(move || pw.watch("/a"))
        };
        assert!(wait_for(FIRST_CALL_TIMEOUT_MS, || entered.load(Ordering::SeqCst)));
        let second = {
            let pw = pw.clone();
            thread::spawn(move || pw.watch("/b"))
        };
        sleep(100);
        gate_tx.send(()).unwrap();
        first.join().unwrap();
        second.join().unwrap();
        assert_eq!(snapshot(&stops), strs(&["/a"]));
        pw.close();
        assert_eq!(snapshot(&stops), strs(&["/a", "/b"]));
    }

    #[test]
    fn skips_projects_replaced_before_their_turn_and_keeps_going_after_a_failed_start() {
        let (pw, starts, stops, entered, gate_tx) = fake_project_watcher();
        let blocked = {
            let pw = pw.clone();
            thread::spawn(move || pw.watch("/gate"))
        };
        assert!(wait_for(FIRST_CALL_TIMEOUT_MS, || entered.load(Ordering::SeqCst)));
        let skipped = {
            let pw = pw.clone();
            thread::spawn(move || pw.watch("/skipped"))
        };
        sleep(100);
        let last = {
            let pw = pw.clone();
            thread::spawn(move || pw.watch("/c"))
        };
        sleep(100);
        gate_tx.send(()).unwrap();
        blocked.join().unwrap();
        skipped.join().unwrap();
        last.join().unwrap();
        assert_eq!(snapshot(&starts), strs(&["/gate", "/c"]));
        pw.close();
        assert_eq!(snapshot(&stops), strs(&["/gate", "/c"]));

        pw.watch("/bad");
        pw.watch("/d");
        assert_eq!(snapshot(&starts), strs(&["/gate", "/c", "/bad", "/d"]));
    }
}
