//! Docker checks, starting and stopping the sandbox container, and the orphan sweep
//! (port of `src/main/sandbox.ts`).
//!
//! Processes are abstracted behind [`SandboxProcess`] so that the adapter and the image build can
//! be tested against fakes, as the TypeScript tests did with `EventEmitter`s.

use std::io::{Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::sandbox::{
    build_run_args, parse_orphan_candidates, DockerProblem, DockerProblemKind, RunOptions,
    CONTAINER_LABEL, OWNER_LABEL, SANDBOX_IMAGE,
};

/// Lock a mutex, ignoring poisoning (a panicking listener must not wedge the app).
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// A running child process with piped stdio. All methods take `&self` so that one thread can
/// block in `wait` while another writes or kills.
pub trait SandboxProcess: Send + Sync {
    /// Write to the process's stdin and flush. Fails when the process is gone.
    fn write_stdin(&self, data: &str) -> std::io::Result<()>;
    /// The stdout stream; `Some` only the first time.
    fn take_stdout(&self) -> Option<Box<dyn Read + Send>>;
    /// The stderr stream; `Some` only the first time.
    fn take_stderr(&self) -> Option<Box<dyn Read + Send>>;
    /// Ask the process to stop (SIGTERM). Does not wait.
    fn kill(&self);
    /// Block until the process ends. `Ok(None)` means it was killed by a signal; `Err` means it
    /// could not be run or waited for (the equivalent of Node's `error` event).
    fn wait(&self) -> Result<Option<i32>, String>;
}

/// Starts a command with piped stdio; injected so Docker can be faked.
pub type Spawn =
    Box<dyn Fn(&str, &[String]) -> Result<Arc<dyn SandboxProcess>, String> + Send + Sync>;

/// A real child process (`std::process`).
pub struct ChildProcess {
    pid: u32,
    stdin: Mutex<Option<ChildStdin>>,
    stdout: Mutex<Option<Box<dyn Read + Send>>>,
    stderr: Mutex<Option<Box<dyn Read + Send>>>,
    child: Mutex<Child>,
}

impl SandboxProcess for ChildProcess {
    fn write_stdin(&self, data: &str) -> std::io::Result<()> {
        let mut guard = lock(&self.stdin);
        match guard.as_mut() {
            Some(stdin) => {
                stdin.write_all(data.as_bytes())?;
                stdin.flush()
            }
            None => Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "stdin is closed",
            )),
        }
    }

    fn take_stdout(&self) -> Option<Box<dyn Read + Send>> {
        lock(&self.stdout).take()
    }

    fn take_stderr(&self) -> Option<Box<dyn Read + Send>> {
        lock(&self.stderr).take()
    }

    fn kill(&self) {
        // SAFETY: plain signal delivery to a pid we spawned.
        unsafe {
            libc::kill(self.pid as libc::pid_t, libc::SIGTERM);
        }
    }

    fn wait(&self) -> Result<Option<i32>, String> {
        let mut child = lock(&self.child);
        match child.wait() {
            Ok(status) => Ok(status.code()),
            Err(error) => Err(error.to_string()),
        }
    }
}

/// The real `Spawn`: runs the command with piped stdin, stdout and stderr.
pub fn spawn_process(file: &str, args: &[String]) -> Result<Arc<dyn SandboxProcess>, String> {
    let mut child = Command::new(file)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let pid = child.id();
    let stdin = child.stdin.take();
    let stdout: Option<Box<dyn Read + Send>> = match child.stdout.take() {
        Some(s) => Some(Box::new(s)),
        None => None,
    };
    let stderr: Option<Box<dyn Read + Send>> = match child.stderr.take() {
        Some(s) => Some(Box::new(s)),
        None => None,
    };
    Ok(Arc::new(ChildProcess {
        pid,
        stdin: Mutex::new(stdin),
        stdout: Mutex::new(stdout),
        stderr: Mutex::new(stderr),
        child: Mutex::new(child),
    }))
}

/// The real `Spawn` as a boxed closure.
pub fn default_spawn() -> Spawn {
    Box::new(|file: &str, args: &[String]| spawn_process(file, args))
}

/// Runs a command and resolves to its stdout, or an error message when it fails; injected so
/// Docker checks can be tested without Docker.
pub type Exec<'a> = &'a dyn Fn(&str, &[String]) -> Result<String, String>;

fn run_command(cwd: Option<&str>, file: &str, args: &[String]) -> Result<String, String> {
    let mut command = Command::new(file);
    command.args(args);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    let output = command.output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("{} failed ({})", file, output.status)
        } else {
            stderr
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// The real `Exec`.
pub fn exec_command(file: &str, args: &[String]) -> Result<String, String> {
    run_command(None, file, args)
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

/// What is wrong with the Docker setup, or `None` when the sandbox can be started.
pub fn check_docker_detailed(exec: Exec) -> Option<DockerProblem> {
    if exec(
        "docker",
        &strings(&["version", "--format", "{{.Server.Version}}"]),
    )
    .is_err()
    {
        return Some(DockerProblem {
            kind: DockerProblemKind::Docker,
            message: "Docker is not available. Install Docker and make sure its daemon is running and your user can use it.".to_string(),
        });
    }
    if exec("docker", &strings(&["image", "inspect", SANDBOX_IMAGE])).is_err() {
        return Some(DockerProblem {
            kind: DockerProblemKind::Image,
            message: format!(
                "The sandbox image \"{}\" is missing. Build it with \"npm run build:sandbox\".",
                SANDBOX_IMAGE
            ),
        });
    }
    None
}

/// A user-facing problem with the Docker setup, or `None` when the sandbox can be started.
pub fn check_docker(exec: Exec) -> Option<String> {
    check_docker_detailed(exec).map(|p| p.message)
}

/// Paths inside or beside the project that the container must only read: the git directory (so
/// the agent cannot commit or edit hooks and config that the host's git would later run), the
/// common directory for linked worktrees, and the `.git` file that points at them in worktrees
/// and submodules (otherwise the agent could redirect it).
pub fn read_only_git_paths(project: &str) -> Result<Vec<String>, String> {
    let stdout = run_command(
        Some(project),
        "git",
        &strings(&[
            "rev-parse",
            "--path-format=absolute",
            "--git-dir",
            "--git-common-dir",
        ]),
    )?;
    let mut paths: Vec<String> = stdout
        .split('\n')
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect();
    let dot_git = std::path::Path::new(project).join(".git");
    let meta = std::fs::symlink_metadata(&dot_git).map_err(|e| e.to_string())?;
    if meta.is_file() {
        paths.push(dot_git.to_string_lossy().to_string());
    }
    Ok(paths)
}

pub struct StartOptions<'a> {
    pub project: String,
    /// Pid of this app process, recorded on the container for the orphan sweep.
    pub owner_pid: u32,
    /// Host dir for the container's Claude config (see `transcripts_dir`); created if missing.
    pub transcripts_dir: String,
    pub spawn: &'a Spawn,
    pub uid: u32,
    pub gid: u32,
}

/// Start a sandbox container for `project`. The returned process speaks the runner protocol on
/// stdin and stdout; stderr is diagnostics. Fails if the git paths cannot be resolved.
pub fn start_sandbox(options: StartOptions) -> Result<Arc<dyn SandboxProcess>, String> {
    // Made by the host user before docker runs, so the container user (same uid) can write to it
    std::fs::create_dir_all(&options.transcripts_dir).map_err(|e| e.to_string())?;
    let read_only_paths = read_only_git_paths(&options.project)?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let args = build_run_args(&RunOptions {
        name: format!("vettr-{}", &id[..8]),
        owner_pid: options.owner_pid,
        project: options.project.clone(),
        read_only_paths,
        transcripts_dir: options.transcripts_dir.clone(),
        uid: options.uid,
        gid: options.gid,
    });
    (options.spawn)("docker", &args).map_err(|e| format!("Could not run Docker: {}", e))
}

/// Ask the container to stop. `--init` forwards the signal, so the runner and SDK shut down.
pub fn stop_sandbox(container: &dyn SandboxProcess) {
    container.kill();
}

/// Remove vettr containers left behind by an app process that is gone (a crash or a kill, which
/// `--rm` does not cover). Containers owned by a live process, such as another running vettr,
/// are left alone. Returns how many were removed.
pub fn sweep_orphans(exec: Exec, is_alive: &dyn Fn(u32) -> bool) -> Result<usize, String> {
    let listing = exec(
        "docker",
        &[
            "ps".to_string(),
            "-a".to_string(),
            "--filter".to_string(),
            format!("label={}", CONTAINER_LABEL),
            "--format".to_string(),
            format!("{{{{.ID}}}} {{{{.Label \"{}\"}}}}", OWNER_LABEL),
        ],
    )?;
    let mut removed = 0usize;
    for candidate in parse_orphan_candidates(&listing) {
        let owner: Option<u32> = candidate.pid.map(|p| p);
        let orphan = match owner {
            None => true,
            Some(pid) => !is_alive(pid),
        };
        if !orphan {
            continue;
        }
        removed += 1;
        let _ = exec("docker", &strings(&["rm", "-f", candidate.id.as_str()]));
    }
    Ok(removed)
}

/// Whether a process with this pid exists, given a function that sends signal 0 and returns the
/// errno on failure (EPERM means the process exists but is someone else's).
pub fn is_process_alive_with(pid: u32, kill: &dyn Fn(u32) -> Result<(), i32>) -> bool {
    match kill(pid) {
        Ok(()) => true,
        Err(errno) => errno == libc::EPERM,
    }
}

/// Whether a process with this pid exists (signal 0 only checks).
pub fn is_process_alive(pid: u32) -> bool {
    is_process_alive_with(pid, &|pid: u32| {
        // SAFETY: signal 0 delivers nothing; it only checks that the process exists.
        let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
        if result == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(0))
        }
    })
}

/// The uid and gid of this process, so the container writes files as the host user.
pub fn current_uid_gid() -> (u32, u32) {
    // SAFETY: getuid and getgid cannot fail.
    unsafe { (libc::getuid() as u32, libc::getgid() as u32) }
}

/// Incrementally decodes UTF-8 from byte chunks, keeping a split multi-byte character for the
/// next chunk.
#[derive(Default)]
pub(crate) struct Utf8Decoder {
    pending: Vec<u8>,
}

impl Utf8Decoder {
    pub(crate) fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut out = String::new();
        loop {
            let decoded: Result<String, (usize, Option<usize>)> =
                match std::str::from_utf8(&self.pending) {
                    Ok(s) => Ok(s.to_string()),
                    Err(e) => Err((e.valid_up_to(), e.error_len())),
                };
            match decoded {
                Ok(s) => {
                    out.push_str(&s);
                    self.pending.clear();
                    break;
                }
                Err((valid, error_len)) => {
                    out.push_str(&String::from_utf8_lossy(&self.pending[..valid]));
                    match error_len {
                        Some(n) => {
                            out.push('\u{FFFD}');
                            self.pending.drain(..valid + n);
                        }
                        None => {
                            self.pending.drain(..valid);
                            break;
                        }
                    }
                }
            }
        }
        out
    }
}

/// The last `n` characters of `text`.
pub(crate) fn tail_chars(text: &str, n: usize) -> String {
    let count = text.chars().count();
    if count <= n {
        return text.to_string();
    }
    text.chars().skip(count - n).collect()
}

#[cfg(test)]
pub(crate) mod testing {
    //! A fake process shared by the tests of the adapter and the image build.

    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{channel, Receiver, Sender};

    pub struct ChannelReader {
        rx: Receiver<Vec<u8>>,
        buf: Vec<u8>,
    }

    impl Read for ChannelReader {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            if self.buf.is_empty() {
                match self.rx.recv() {
                    Ok(chunk) => self.buf = chunk,
                    Err(_) => return Ok(0),
                }
            }
            let n = std::cmp::min(out.len(), self.buf.len());
            out[..n].copy_from_slice(&self.buf[..n]);
            self.buf.drain(..n);
            Ok(n)
        }
    }

    type Exit = Result<Option<i32>, String>;

    pub struct FakeProcess {
        written: Mutex<String>,
        pub fail_writes: AtomicBool,
        stdout_tx: Mutex<Option<Sender<Vec<u8>>>>,
        stderr_tx: Mutex<Option<Sender<Vec<u8>>>>,
        stdout_rx: Mutex<Option<Receiver<Vec<u8>>>>,
        stderr_rx: Mutex<Option<Receiver<Vec<u8>>>>,
        exit_tx: Mutex<Option<Sender<Exit>>>,
        exit_rx: Mutex<Option<Receiver<Exit>>>,
    }

    impl FakeProcess {
        pub fn new() -> Arc<FakeProcess> {
            let (otx, orx) = channel::<Vec<u8>>();
            let (etx, erx) = channel::<Vec<u8>>();
            let (xtx, xrx) = channel::<Exit>();
            Arc::new(FakeProcess {
                written: Mutex::new(String::new()),
                fail_writes: AtomicBool::new(false),
                stdout_tx: Mutex::new(Some(otx)),
                stderr_tx: Mutex::new(Some(etx)),
                stdout_rx: Mutex::new(Some(orx)),
                stderr_rx: Mutex::new(Some(erx)),
                exit_tx: Mutex::new(Some(xtx)),
                exit_rx: Mutex::new(Some(xrx)),
            })
        }

        pub fn stdout_write(&self, text: &str) {
            self.stdout_write_bytes(text.as_bytes());
        }

        pub fn stdout_write_bytes(&self, bytes: &[u8]) {
            if let Some(tx) = lock(&self.stdout_tx).as_ref() {
                let _ = tx.send(bytes.to_vec());
            }
        }

        pub fn stderr_write(&self, text: &str) {
            if let Some(tx) = lock(&self.stderr_tx).as_ref() {
                let _ = tx.send(text.as_bytes().to_vec());
            }
        }

        fn finish(&self, exit: Exit) {
            lock(&self.stdout_tx).take();
            lock(&self.stderr_tx).take();
            if let Some(tx) = lock(&self.exit_tx).take() {
                let _ = tx.send(exit);
            }
        }

        /// End the process like Node's `close` event.
        pub fn close(&self, code: Option<i32>) {
            self.finish(Ok(code));
        }

        /// End the process like Node's `error` event followed by `close`.
        pub fn close_error(&self, message: &str) {
            self.finish(Err(message.to_string()));
        }

        pub fn written(&self) -> String {
            lock(&self.written).clone()
        }

        /// Everything written to stdin, parsed as JSON lines.
        pub fn commands(&self) -> Vec<serde_json::Value> {
            self.written()
                .split('\n')
                .filter(|l| !l.is_empty())
                .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
                .collect()
        }
    }

    impl SandboxProcess for FakeProcess {
        fn write_stdin(&self, data: &str) -> std::io::Result<()> {
            if self.fail_writes.load(Ordering::SeqCst) {
                return Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "EPIPE"));
            }
            lock(&self.written).push_str(data);
            Ok(())
        }

        fn take_stdout(&self) -> Option<Box<dyn Read + Send>> {
            let rx = lock(&self.stdout_rx).take()?;
            Some(Box::new(ChannelReader {
                rx,
                buf: Vec::new(),
            }))
        }

        fn take_stderr(&self) -> Option<Box<dyn Read + Send>> {
            let rx = lock(&self.stderr_rx).take()?;
            Some(Box::new(ChannelReader {
                rx,
                buf: Vec::new(),
            }))
        }

        /// Like a real container, SIGTERM ends it with exit code 143.
        fn kill(&self) {
            self.close(Some(143));
        }

        fn wait(&self) -> Result<Option<i32>, String> {
            let rx = lock(&self.exit_rx).take();
            match rx {
                Some(rx) => match rx.recv() {
                    Ok(exit) => exit,
                    Err(_) => Ok(None),
                },
                None => Ok(None),
            }
        }
    }

    /// A `Spawn` that hands out the given process and records the calls.
    pub fn fake_spawn(
        process: Arc<FakeProcess>,
    ) -> (Spawn, Arc<Mutex<Vec<(String, Vec<String>)>>>) {
        let calls: Arc<Mutex<Vec<(String, Vec<String>)>>> = Arc::new(Mutex::new(Vec::new()));
        let recorded = calls.clone();
        let spawn: Spawn = Box::new(move |file: &str, args: &[String]| {
            lock(&recorded).push((file.to_string(), args.to_vec()));
            let handle: Arc<dyn SandboxProcess> = process.clone();
            Ok(handle)
        });
        (spawn, calls)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::path::Path;

    fn git(cwd: &Path, args: &[&str]) {
        let mut full: Vec<&str> = vec!["-c", "user.name=t", "-c", "user.email=t@t"];
        full.extend_from_slice(args);
        let status = Command::new("git")
            .args(&full)
            .current_dir(cwd)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
    }

    fn temp_root() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        (dir, root)
    }

    fn s(path: &Path) -> String {
        path.to_string_lossy().to_string()
    }

    // checkDocker

    #[test]
    fn check_docker_passes_when_the_daemon_answers_and_the_image_exists() {
        let calls = Cell::new(0);
        let exec = |_: &str, _: &[String]| -> Result<String, String> {
            calls.set(calls.get() + 1);
            Ok(String::new())
        };
        assert_eq!(check_docker(&exec), None);
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn check_docker_reports_docker_as_unavailable_when_the_daemon_check_fails() {
        let calls = Cell::new(0);
        let exec = |_: &str, _: &[String]| -> Result<String, String> {
            calls.set(calls.get() + 1);
            Err("no daemon".to_string())
        };
        assert!(check_docker(&exec)
            .unwrap()
            .contains("Docker is not available"));
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn check_docker_tells_the_user_how_to_build_a_missing_image() {
        let results: RefCell<VecDeque<Result<String, String>>> =
            RefCell::new(VecDeque::from(vec![
                Ok(String::new()),
                Err("no image".to_string()),
            ]));
        let exec = |_: &str, _: &[String]| -> Result<String, String> {
            results.borrow_mut().pop_front().unwrap()
        };
        assert!(check_docker(&exec)
            .unwrap()
            .contains("npm run build:sandbox"));
    }

    #[test]
    fn check_docker_detailed_tells_a_missing_daemon_from_a_missing_image() {
        let no_daemon = |_: &str, _: &[String]| -> Result<String, String> { Err("no".to_string()) };
        assert!(check_docker_detailed(&no_daemon).unwrap().kind == DockerProblemKind::Docker);
        let results: RefCell<VecDeque<Result<String, String>>> =
            RefCell::new(VecDeque::from(vec![
                Ok(String::new()),
                Err("no".to_string()),
            ]));
        let no_image = |_: &str, _: &[String]| -> Result<String, String> {
            results.borrow_mut().pop_front().unwrap()
        };
        assert!(check_docker_detailed(&no_image).unwrap().kind == DockerProblemKind::Image);
        let fine = |_: &str, _: &[String]| -> Result<String, String> { Ok(String::new()) };
        assert!(check_docker_detailed(&fine).is_none());
    }

    // readOnlyGitPaths

    #[test]
    fn read_only_git_paths_returns_the_git_directory_for_a_normal_repo() {
        let (_guard, root) = temp_root();
        git(&root, &["init", "-q"]);
        let dot_git = s(&root.join(".git"));
        assert_eq!(
            read_only_git_paths(&s(&root)).unwrap(),
            vec![dot_git.clone(), dot_git]
        );
    }

    #[test]
    fn read_only_git_paths_returns_common_dir_worktree_dir_and_git_file_for_a_linked_worktree() {
        let (_guard, root) = temp_root();
        let main = root.join("main");
        let linked = root.join("linked");
        git(&root, &["init", "-q", "main"]);
        std::fs::write(main.join("f"), "x").unwrap();
        git(&main, &["add", "."]);
        git(&main, &["commit", "-q", "-m", "init"]);
        git(&main, &["worktree", "add", "-q", linked.to_str().unwrap()]);
        assert_eq!(
            read_only_git_paths(&s(&linked)).unwrap(),
            vec![
                s(&main.join(".git").join("worktrees").join("linked")),
                s(&main.join(".git")),
                s(&linked.join(".git")),
            ]
        );
    }

    #[test]
    fn read_only_git_paths_fails_outside_a_repo() {
        let (_guard, root) = temp_root();
        assert!(read_only_git_paths(&s(&root)).is_err());
    }

    // startSandbox

    #[test]
    fn start_sandbox_spawns_docker_with_the_run_arguments_for_the_project() {
        let (_guard, root) = temp_root();
        git(&root, &["init", "-q"]);
        let transcripts = root.join("data").join("transcripts");
        let (spawn, calls) = fake_spawn(FakeProcess::new());
        let result = start_sandbox(StartOptions {
            project: s(&root),
            owner_pid: 7,
            transcripts_dir: s(&transcripts),
            spawn: &spawn,
            uid: 1,
            gid: 2,
        });
        assert!(result.is_ok());
        let calls = lock(&calls);
        let (file, args) = &calls[0];
        assert_eq!(file, "docker");
        let has = |needle: String| args.contains(&needle);
        assert!(has(format!("{}:{}", s(&root), s(&root))));
        let git_dir = s(&root.join(".git"));
        assert!(has(format!("{}:{}:ro", git_dir, git_dir)));
        assert!(has("1:2".to_string()));
        assert!(has(format!("{}:/vettr-config", s(&transcripts))));
        assert!(transcripts.exists());
        let name_at = args.iter().position(|a| a == "--name").unwrap();
        let name = &args[name_at + 1];
        assert!(name.starts_with("vettr-"));
        assert_eq!(name.len(), "vettr-".len() + 8);
        assert!(name["vettr-".len()..]
            .chars()
            .all(|c| c.is_ascii_hexdigit()));
    }

    // stopSandbox

    #[test]
    fn stop_sandbox_sends_sigterm_to_the_docker_process() {
        struct Recorder(Mutex<u32>);
        impl SandboxProcess for Recorder {
            fn write_stdin(&self, _: &str) -> std::io::Result<()> {
                Ok(())
            }
            fn take_stdout(&self) -> Option<Box<dyn Read + Send>> {
                None
            }
            fn take_stderr(&self) -> Option<Box<dyn Read + Send>> {
                None
            }
            fn kill(&self) {
                *lock(&self.0) += 1;
            }
            fn wait(&self) -> Result<Option<i32>, String> {
                Ok(None)
            }
        }
        let recorder = Recorder(Mutex::new(0));
        stop_sandbox(&recorder);
        assert_eq!(*lock(&recorder.0), 1);
    }

    // sweepOrphans

    struct ScriptedExec {
        responses: RefCell<VecDeque<Result<String, String>>>,
        calls: RefCell<Vec<(String, Vec<String>)>>,
    }

    impl ScriptedExec {
        fn new(responses: Vec<Result<String, String>>) -> ScriptedExec {
            ScriptedExec {
                responses: RefCell::new(VecDeque::from(responses)),
                calls: RefCell::new(Vec::new()),
            }
        }
        fn call(&self, file: &str, args: &[String]) -> Result<String, String> {
            self.calls
                .borrow_mut()
                .push((file.to_string(), args.to_vec()));
            self.responses
                .borrow_mut()
                .pop_front()
                .unwrap_or_else(|| Ok(String::new()))
        }
        fn removed(&self, id: &str) -> bool {
            self.calls.borrow().iter().any(|(f, a)| {
                f == "docker" && *a == vec!["rm".to_string(), "-f".to_string(), id.to_string()]
            })
        }
    }

    #[test]
    fn sweep_removes_labelled_containers_whose_owner_is_gone_and_keeps_live_ones() {
        let scripted = ScriptedExec::new(vec![Ok("dead1 100\nlive1 200\nnopid\n".to_string())]);
        let exec = |f: &str, a: &[String]| scripted.call(f, a);
        let removed = sweep_orphans(&exec, &|pid: u32| pid == 200).unwrap();
        assert_eq!(removed, 2);
        assert!(scripted.calls.borrow()[0]
            .1
            .contains(&"label=vettr.app=1".to_string()));
        assert!(scripted.removed("dead1"));
        assert!(scripted.removed("nopid"));
        assert!(!scripted.removed("live1"));
    }

    #[test]
    fn sweep_keeps_going_when_one_removal_fails() {
        let scripted =
            ScriptedExec::new(vec![Ok("a 1\nb 2\n".to_string()), Err("gone".to_string())]);
        let exec = |f: &str, a: &[String]| scripted.call(f, a);
        assert_eq!(sweep_orphans(&exec, &|_: u32| false).unwrap(), 2);
        assert!(scripted.removed("b"));
    }

    #[test]
    fn sweep_does_nothing_when_there_are_no_containers() {
        let scripted = ScriptedExec::new(vec![Ok("\n".to_string())]);
        let exec = |f: &str, a: &[String]| scripted.call(f, a);
        assert_eq!(sweep_orphans(&exec, &|_: u32| false).unwrap(), 0);
        assert_eq!(scripted.calls.borrow().len(), 1);
    }

    // isProcessAlive

    #[test]
    fn is_alive_when_the_signal_is_delivered() {
        assert!(is_process_alive_with(1, &|_: u32| Ok(())));
    }

    #[test]
    fn is_alive_when_permission_is_denied_and_not_when_there_is_no_such_process() {
        assert!(is_process_alive_with(1, &|_: u32| Err(libc::EPERM)));
        assert!(!is_process_alive_with(1, &|_: u32| Err(libc::ESRCH)));
    }

    #[test]
    fn this_process_is_alive() {
        assert!(is_process_alive(std::process::id()));
    }

    // helpers

    #[test]
    fn utf8_decoder_keeps_split_characters_together() {
        let mut d = Utf8Decoder::default();
        let bytes = "aé".as_bytes().to_vec();
        assert_eq!(d.push(&bytes[..2]), "a");
        assert_eq!(d.push(&bytes[2..]), "é");
    }

    #[test]
    fn tail_chars_keeps_the_end() {
        assert_eq!(tail_chars("abcdef", 3), "def");
        assert_eq!(tail_chars("ab", 3), "ab");
    }
}
