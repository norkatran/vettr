//! Drives Claude Code through the in-container runner (port of `src/main/claudeAdapter.ts`).
//!
//! One adapter holds at most one session. The state lives in a single `Mutex<Inner>` that is
//! never held across a blocking call (docker checks, process writes, waiting for a close) or
//! while listeners run, so listeners may call back into the adapter.

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use crate::agent::{
    encode_command, parse_event_line, AgentAdapter, AgentEvent, EventListener, LineBuffer,
    RunnerCommand,
};
use crate::host::sandbox_runtime::{lock, tail_chars, SandboxProcess, Utf8Decoder};

/// Everything the adapter needs from the outside, injected so it can run against a fake process.
pub struct ClaudeAdapterDeps {
    /// A user-facing problem with Docker or the image, or `None` when the sandbox can start.
    pub check_docker: Box<dyn Fn() -> Option<String> + Send + Sync>,
    /// Start a sandbox container for the project; its stdio speaks the runner protocol.
    pub start_sandbox: Box<dyn Fn(&str) -> Result<Arc<dyn SandboxProcess>, String> + Send + Sync>,
    /// The credential (API key or OAuth token) of the active profile, if any.
    pub get_api_key: Box<dyn Fn() -> Option<String> + Send + Sync>,
    /// Ask the container to stop; its `wait` should then return.
    pub stop_sandbox: Box<dyn Fn(&dyn SandboxProcess) + Send + Sync>,
}

const STDERR_TAIL_CHARS: usize = 2000;

/// Set once the container's process has ended and its events were delivered.
struct Closed {
    done: Mutex<bool>,
    cv: Condvar,
}

impl Closed {
    fn new() -> Arc<Closed> {
        Arc::new(Closed {
            done: Mutex::new(false),
            cv: Condvar::new(),
        })
    }

    fn set(&self) {
        *lock(&self.done) = true;
        self.cv.notify_all();
    }

    fn wait(&self) {
        let mut guard = lock(&self.done);
        while !*guard {
            guard = match self.cv.wait(guard) {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };
        }
    }
}

/// A prewarm in progress, which later callers join instead of racing it.
struct Pending {
    result: Mutex<Option<Result<(), String>>>,
    cv: Condvar,
}

impl Pending {
    fn new() -> Arc<Pending> {
        Arc::new(Pending {
            result: Mutex::new(None),
            cv: Condvar::new(),
        })
    }

    fn finish(&self, result: Result<(), String>) {
        *lock(&self.result) = Some(result);
        self.cv.notify_all();
    }

    fn wait(&self) -> Result<(), String> {
        let mut guard = lock(&self.result);
        loop {
            if let Some(result) = guard.as_ref() {
                return result.clone();
            }
            guard = match self.cv.wait(guard) {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };
        }
    }
}

struct Inner {
    listeners: Vec<(u64, Arc<EventListener>)>,
    next_listener: u64,
    container: Option<Arc<dyn SandboxProcess>>,
    starting: bool,
    /// Set once the first prompt has gone to the current container; before that it is warm.
    active: bool,
    warm_cwd: Option<String>,
    warm_resume: Option<String>,
    warming: Option<Arc<Pending>>,
    closed: Option<Arc<Closed>>,
    /// Set by `stop` so the resulting exit is not reported as a failure.
    stop_requested: bool,
    /// Counts launched containers so a late watcher never resets a newer one.
    generation: u64,
}

struct Shared {
    deps: ClaudeAdapterDeps,
    inner: Mutex<Inner>,
}

/// Drives Claude Code through the in-container runner. One instance holds at most one session.
pub struct ClaudeAdapter {
    shared: Arc<Shared>,
}

impl ClaudeAdapter {
    pub fn new(deps: ClaudeAdapterDeps) -> ClaudeAdapter {
        ClaudeAdapter {
            shared: Arc::new(Shared {
                deps,
                inner: Mutex::new(Inner {
                    listeners: Vec::new(),
                    next_listener: 1,
                    container: None,
                    starting: false,
                    active: false,
                    warm_cwd: None,
                    warm_resume: None,
                    warming: None,
                    closed: None,
                    stop_requested: false,
                    generation: 0,
                }),
            }),
        }
    }

    /// Start the container and send `init`, leaving the agent warm and idle.
    fn launch(&self, cwd: &str, resume: Option<&str>) -> Result<(), String> {
        {
            // Claim the slot first so two concurrent starts cannot both proceed
            let mut inner = lock(&self.shared.inner);
            if inner.container.is_some() || inner.starting {
                return Err("A session is already running".to_string());
            }
            inner.starting = true;
        }
        let result = self.launch_claimed(cwd, resume);
        lock(&self.shared.inner).starting = false;
        result
    }

    fn launch_claimed(&self, cwd: &str, resume: Option<&str>) -> Result<(), String> {
        let shared = &self.shared;
        if let Some(problem) = (shared.deps.check_docker)() {
            return Err(problem);
        }
        let credential = match (shared.deps.get_api_key)() {
            Some(c) if !c.is_empty() => c,
            _ => {
                return Err("No API key or token is saved. Add one to start a session.".to_string())
            }
        };
        let container = (shared.deps.start_sandbox)(cwd)?;
        let resume_owned: Option<String> = match resume {
            Some(r) if !r.is_empty() => Some(r.to_string()),
            _ => None,
        };
        let closed = Closed::new();
        let generation;
        {
            let mut inner = lock(&shared.inner);
            inner.container = Some(container.clone());
            inner.active = false;
            inner.warm_cwd = Some(cwd.to_string());
            inner.warm_resume = resume_owned.clone();
            inner.stop_requested = false;
            inner.generation += 1;
            generation = inner.generation;
            inner.closed = Some(closed.clone());
        }
        watch(shared.clone(), container, generation, closed);
        self.write(&RunnerCommand::Init {
            credential,
            cwd: cwd.to_string(),
            resume: resume_owned,
        })
    }

    fn write(&self, command: &RunnerCommand) -> Result<(), String> {
        let container = lock(&self.shared.inner).container.clone();
        match container {
            None => Err("No session is running".to_string()),
            Some(container) => {
                // A write to a dead container fails here; the exit reports the real cause
                let _ = container.write_stdin(&encode_command(command));
                Ok(())
            }
        }
    }
}

impl Shared {
    fn emit(&self, event: AgentEvent) {
        let listeners: Vec<Arc<EventListener>> = lock(&self.inner)
            .listeners
            .iter()
            .map(|(_, l)| l.clone())
            .collect();
        for listener in listeners {
            (*listener)(event.clone());
        }
    }
}

/// Forward the container's events and report how it ended.
fn watch(
    shared: Arc<Shared>,
    container: Arc<dyn SandboxProcess>,
    generation: u64,
    closed: Arc<Closed>,
) {
    let saw_error = Arc::new(AtomicBool::new(false));
    let stderr_tail: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));

    let stdout = container.take_stdout();
    let stderr = container.take_stderr();

    let out_shared = shared.clone();
    let out_flag = saw_error.clone();
    let out_thread = thread::spawn(move || {
        if let Some(mut reader) = stdout {
            let mut decoder = Utf8Decoder::default();
            let mut lines = LineBuffer::new();
            let mut buf = [0u8; 8192];
            loop {
                let n = match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(_) => break,
                };
                let text = decoder.push(&buf[..n]);
                for line in lines.push(&text) {
                    let event = match parse_event_line(&line) {
                        Some(event) => event,
                        None => continue,
                    };
                    if let AgentEvent::Error { .. } = event {
                        out_flag.store(true, Ordering::SeqCst);
                    }
                    out_shared.emit(event);
                }
            }
        }
    });

    let err_tail = stderr_tail.clone();
    let err_thread = thread::spawn(move || {
        if let Some(mut reader) = stderr {
            let mut decoder = Utf8Decoder::default();
            let mut buf = [0u8; 4096];
            loop {
                let n = match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(_) => break,
                };
                let text = decoder.push(&buf[..n]);
                let mut tail = lock(&err_tail);
                let combined = format!("{}{}", *tail, text);
                *tail = tail_chars(&combined, STDERR_TAIL_CHARS);
            }
        }
    });

    thread::spawn(move || {
        let result = container.wait();
        // Deliver everything the container said before reporting its end
        let _ = out_thread.join();
        let _ = err_thread.join();
        finish(
            &shared,
            generation,
            result,
            saw_error.load(Ordering::SeqCst),
            &stderr_tail,
        );
        closed.set();
    });
}

fn finish(
    shared: &Arc<Shared>,
    generation: u64,
    result: Result<Option<i32>, String>,
    saw_error: bool,
    stderr_tail: &Arc<Mutex<String>>,
) {
    let (code, failure): (Option<i32>, Option<String>) = match result {
        Ok(code) => (code, None),
        Err(message) => (None, Some(format!("Could not run Docker: {}", message))),
    };
    let stop_requested;
    {
        let mut inner = lock(&shared.inner);
        if inner.generation != generation {
            // A newer container has launched, so this exit is stale. Reporting it would end the
            // live session in the UI (and trigger a crash restart) while it is still streaming.
            return;
        }
        inner.container = None;
        inner.active = false;
        inner.warm_cwd = None;
        inner.warm_resume = None;
        stop_requested = inner.stop_requested;
    }
    let message: Option<String> = match failure {
        Some(f) => Some(f),
        None => {
            if !stop_requested && !saw_error && code != Some(0) {
                Some(match code {
                    Some(c) => format!("The sandbox stopped unexpectedly (exit code {}).", c),
                    None => "The sandbox stopped unexpectedly.".to_string(),
                })
            } else {
                None
            }
        }
    };
    if let Some(message) = message {
        let detail = lock(stderr_tail).trim().to_string();
        let full = if detail.is_empty() {
            message
        } else {
            format!("{}\n{}", message, detail)
        };
        shared.emit(AgentEvent::Error { message: full });
    }
    shared.emit(AgentEvent::Exited { code });
}

impl AgentAdapter for ClaudeAdapter {
    /// Prewarm: start the container and its idle query without a prompt, so the first prompt does
    /// not wait for the container. A no-op when a matching warm agent already exists.
    fn warm(&self, cwd: &str, resume: Option<&str>) -> Result<(), String> {
        let pending = Pending::new();
        {
            let mut inner = lock(&self.shared.inner);
            if inner.container.is_some()
                && !inner.active
                && inner.warm_cwd.as_deref() == Some(cwd)
                && inner.warm_resume.as_deref() == resume
            {
                return Ok(());
            }
            // Join a prewarm already in progress rather than racing it
            if let Some(existing) = inner.warming.clone() {
                drop(inner);
                return existing.wait();
            }
            inner.warming = Some(pending.clone());
        }
        let result = self.launch(cwd, resume);
        lock(&self.shared.inner).warming = None;
        pending.finish(result.clone());
        result
    }

    fn start(&self, prompt: &str, cwd: &str, resume: Option<&str>) -> Result<(), String> {
        // A prewarm in progress is waited for; its failure is reported by launching again below
        let warming = lock(&self.shared.inner).warming.clone();
        if let Some(pending) = warming {
            let _ = pending.wait();
        }
        let reuse: Option<bool> = {
            let mut inner = lock(&self.shared.inner);
            if inner.container.is_some() && !inner.active {
                if inner.warm_cwd.as_deref() == Some(cwd) && inner.warm_resume.as_deref() == resume
                {
                    inner.active = true;
                    Some(true)
                } else {
                    Some(false)
                }
            } else {
                None
            }
        };
        match reuse {
            Some(true) => {
                return self.write(&RunnerCommand::Prompt {
                    text: prompt.to_string(),
                });
            }
            // A warm agent for another project, or one that must resume a stored session
            Some(false) => self.stop()?,
            None => {}
        }
        self.launch(cwd, resume)?;
        lock(&self.shared.inner).active = true;
        self.write(&RunnerCommand::Prompt {
            text: prompt.to_string(),
        })
    }

    fn send(&self, message: &str) -> Result<(), String> {
        self.write(&RunnerCommand::Prompt {
            text: message.to_string(),
        })
    }

    fn interrupt(&self) -> Result<(), String> {
        self.write(&RunnerCommand::Interrupt)
    }

    fn on_event(&self, listener: EventListener) -> u64 {
        let mut inner = lock(&self.shared.inner);
        let id = inner.next_listener;
        inner.next_listener += 1;
        inner.listeners.push((id, Arc::new(listener)));
        id
    }

    fn off_event(&self, subscription: u64) {
        lock(&self.shared.inner)
            .listeners
            .retain(|(id, _)| *id != subscription);
    }

    /// The MVP sandbox grants full permissions, so nothing ever asks for approval.
    fn respond_to_approval(&self, _id: &str, _allow: bool) -> Result<(), String> {
        Ok(())
    }

    fn stop(&self) -> Result<(), String> {
        let (container, closed) = {
            let mut inner = lock(&self.shared.inner);
            match inner.container.clone() {
                None => return Ok(()),
                Some(container) => {
                    inner.stop_requested = true;
                    (container, inner.closed.clone())
                }
            }
        };
        (self.shared.deps.stop_sandbox)(&*container);
        if let Some(closed) = closed {
            closed.wait();
        }
        Ok(())
    }
}

/// Run an action for the UI: `None` on success or the error message on failure.
pub fn attempt<F: FnOnce() -> Result<(), String>>(action: F) -> Option<String> {
    action().err()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::sandbox_runtime::testing::FakeProcess;
    use serde_json::json;
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc::{channel, Sender};
    use std::time::{Duration, Instant};

    type CheckFn = Box<dyn Fn() -> Option<String> + Send + Sync>;
    type StartFn = Box<dyn Fn(&str) -> Result<Arc<dyn SandboxProcess>, String> + Send + Sync>;
    type KeyFn = Box<dyn Fn() -> Option<String> + Send + Sync>;

    #[derive(Default)]
    struct Overrides {
        check_docker: Option<CheckFn>,
        start_sandbox: Option<StartFn>,
        get_api_key: Option<KeyFn>,
    }

    struct Setup {
        adapter: Arc<ClaudeAdapter>,
        containers: Arc<Mutex<Vec<Arc<FakeProcess>>>>,
        events: Arc<Mutex<Vec<AgentEvent>>>,
        stops: Arc<AtomicUsize>,
        starts: Arc<AtomicUsize>,
    }

    impl Setup {
        fn container(&self, index: usize) -> Arc<FakeProcess> {
            lock(&self.containers)[index].clone()
        }
        fn first(&self) -> Arc<FakeProcess> {
            self.container(0)
        }
        fn last(&self) -> Arc<FakeProcess> {
            lock(&self.containers).last().unwrap().clone()
        }
        fn events(&self) -> Vec<AgentEvent> {
            lock(&self.events).clone()
        }
        fn wait_for_events(&self, count: usize) {
            let events = self.events.clone();
            wait_until(move || lock(&events).len() >= count);
        }
    }

    fn wait_until<F: Fn() -> bool>(condition: F) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !condition() {
            if Instant::now() > deadline {
                panic!("timed out waiting for a condition");
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn pause() {
        thread::sleep(Duration::from_millis(100));
    }

    fn setup() -> Setup {
        setup_with(Overrides::default())
    }

    fn setup_with(overrides: Overrides) -> Setup {
        let containers: Arc<Mutex<Vec<Arc<FakeProcess>>>> = Arc::new(Mutex::new(Vec::new()));
        let stops = Arc::new(AtomicUsize::new(0));
        let starts = Arc::new(AtomicUsize::new(0));
        let default_start: StartFn = {
            let containers = containers.clone();
            let starts = starts.clone();
            Box::new(move |_cwd: &str| {
                starts.fetch_add(1, Ordering::SeqCst);
                let fake = FakeProcess::new();
                lock(&containers).push(fake.clone());
                let handle: Arc<dyn SandboxProcess> = fake;
                Ok(handle)
            })
        };
        let stop_counter = stops.clone();
        let deps = ClaudeAdapterDeps {
            check_docker: overrides.check_docker.unwrap_or_else(|| Box::new(|| None)),
            start_sandbox: overrides.start_sandbox.unwrap_or(default_start),
            get_api_key: overrides
                .get_api_key
                .unwrap_or_else(|| Box::new(|| Some("sk-key".to_string()))),
            stop_sandbox: Box::new(move |process: &dyn SandboxProcess| {
                stop_counter.fetch_add(1, Ordering::SeqCst);
                process.kill();
            }),
        };
        let adapter = Arc::new(ClaudeAdapter::new(deps));
        let events: Arc<Mutex<Vec<AgentEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        adapter.on_event(Box::new(move |event: AgentEvent| lock(&sink).push(event)));
        Setup {
            adapter,
            containers,
            events,
            stops,
            starts,
        }
    }

    /// A `check_docker` whose first call blocks until released, then answers `first_result`.
    fn gated_check(first_result: Option<String>) -> (CheckFn, Arc<AtomicBool>, Sender<()>) {
        let entered = Arc::new(AtomicBool::new(false));
        let (release, gate) = channel::<()>();
        let gate = Mutex::new(gate);
        let calls = AtomicUsize::new(0);
        let flag = entered.clone();
        let check: CheckFn = Box::new(move || {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                flag.store(true, Ordering::SeqCst);
                let _ = lock(&gate).recv();
                first_result.clone()
            } else {
                None
            }
        });
        (check, entered, release)
    }

    fn types(commands: &[serde_json::Value]) -> Vec<String> {
        commands
            .iter()
            .map(|c| c["type"].as_str().unwrap().to_string())
            .collect()
    }

    fn init(cwd: &str) -> serde_json::Value {
        json!({"type": "init", "credential": "sk-key", "cwd": cwd})
    }

    fn init_resume(cwd: &str, resume: &str) -> serde_json::Value {
        json!({"type": "init", "credential": "sk-key", "cwd": cwd, "resume": resume})
    }

    fn prompt(text: &str) -> serde_json::Value {
        json!({"type": "prompt", "text": text})
    }

    // start

    #[test]
    fn start_sends_init_with_the_key_first_then_the_prompt() {
        let t = setup();
        t.adapter.start("build it", "/proj", None).unwrap();
        assert_eq!(
            t.first().commands(),
            vec![init("/proj"), prompt("build it")]
        );
    }

    #[test]
    fn start_fails_with_the_docker_problem_and_does_not_start_a_container() {
        let t = setup_with(Overrides {
            check_docker: Some(Box::new(|| Some("Docker is not available.".to_string()))),
            ..Overrides::default()
        });
        let error = t.adapter.start("p", "/proj", None).unwrap_err();
        assert!(error.contains("Docker is not available."));
        assert_eq!(t.starts.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn start_fails_when_no_credential_is_saved() {
        let t = setup_with(Overrides {
            get_api_key: Some(Box::new(|| None)),
            ..Overrides::default()
        });
        let error = t.adapter.start("p", "/proj", None).unwrap_err();
        assert!(error.contains("No API key or token"));
    }

    #[test]
    fn start_rejects_a_second_start_while_a_session_is_running() {
        let t = setup();
        t.adapter.start("p", "/proj", None).unwrap();
        let error = t.adapter.start("p", "/proj", None).unwrap_err();
        assert!(error.contains("already running"));
    }

    #[test]
    fn start_rejects_a_start_that_loses_a_race_with_another_start() {
        let (check, entered, release) = gated_check(None);
        let t = setup_with(Overrides {
            check_docker: Some(check),
            ..Overrides::default()
        });
        let adapter = t.adapter.clone();
        let first = thread::spawn(move || adapter.start("p", "/proj", None));
        wait_until(|| entered.load(Ordering::SeqCst));
        let error = t.adapter.start("p", "/proj", None).unwrap_err();
        assert!(error.contains("already running"));
        release.send(()).unwrap();
        assert_eq!(first.join().unwrap(), Ok(()));
    }

    #[test]
    fn start_does_not_leave_a_session_behind_when_the_container_cannot_start() {
        let fail = Arc::new(AtomicBool::new(true));
        let flag = fail.clone();
        let start: StartFn = Box::new(move |_cwd: &str| {
            if flag.load(Ordering::SeqCst) {
                return Err("not a repo".to_string());
            }
            let handle: Arc<dyn SandboxProcess> = FakeProcess::new();
            Ok(handle)
        });
        let t = setup_with(Overrides {
            start_sandbox: Some(start),
            ..Overrides::default()
        });
        let error = t.adapter.start("p", "/proj", None).unwrap_err();
        assert!(error.contains("not a repo"));
        fail.store(false, Ordering::SeqCst);
        t.adapter.start("p", "/proj", None).unwrap();
    }

    // events

    #[test]
    fn forwards_runner_events_and_skips_malformed_lines() {
        let t = setup();
        t.adapter.start("p", "/proj", None).unwrap();
        let c = t.first();
        c.stdout_write("garbage\n{\"type\":\"bogus\"}\n");
        c.stdout_write("{\"type\":\"text\",\"text\":\"hi\"}\n{\"type\":\"turn-finished\"}\n");
        t.wait_for_events(2);
        assert_eq!(
            t.events(),
            vec![
                AgentEvent::Text {
                    text: "hi".to_string()
                },
                AgentEvent::TurnFinished
            ]
        );
    }

    #[test]
    fn reassembles_events_split_across_chunks() {
        let t = setup();
        t.adapter.start("p", "/proj", None).unwrap();
        let c = t.first();
        c.stdout_write("{\"type\":\"te");
        c.stdout_write("xt\",\"text\":\"h\u{e9}\"}\n");
        t.wait_for_events(1);
        assert_eq!(
            t.events(),
            vec![AgentEvent::Text {
                text: "h\u{e9}".to_string()
            }]
        );
    }

    #[test]
    fn stops_delivering_events_to_an_unsubscribed_listener() {
        let t = setup();
        let seen: Arc<Mutex<Vec<AgentEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let id = t
            .adapter
            .on_event(Box::new(move |event: AgentEvent| lock(&sink).push(event)));
        t.adapter.start("p", "/proj", None).unwrap();
        t.adapter.off_event(id);
        t.first().stdout_write("{\"type\":\"turn-finished\"}\n");
        t.wait_for_events(1);
        pause();
        assert!(lock(&seen).is_empty());
    }

    #[test]
    fn ignores_the_exit_of_a_superseded_container() {
        let t = setup();
        t.adapter.start("p", "/proj", None).unwrap();
        // A newer container has launched since the first one's watcher began
        lock(&t.adapter.shared.inner).generation += 1;
        let tail = Arc::new(Mutex::new(String::new()));
        finish(&t.adapter.shared, 1, Ok(Some(1)), false, &tail);
        assert_eq!(t.events(), vec![]);
        // The newer container's state is untouched
        assert!(lock(&t.adapter.shared.inner).container.is_some());
    }

    #[test]
    fn reports_a_clean_exit_without_an_error() {
        let t = setup();
        t.adapter.start("p", "/proj", None).unwrap();
        t.first().close(Some(0));
        t.wait_for_events(1);
        pause();
        assert_eq!(t.events(), vec![AgentEvent::Exited { code: Some(0) }]);
    }

    #[test]
    fn reports_an_unexpected_exit_with_the_stderr_tail() {
        let t = setup();
        t.adapter.start("p", "/proj", None).unwrap();
        let c = t.first();
        c.stderr_write("boom: image gone\n");
        c.close(Some(125));
        t.wait_for_events(2);
        assert_eq!(
            t.events(),
            vec![
                AgentEvent::Error {
                    message: "The sandbox stopped unexpectedly (exit code 125).\nboom: image gone"
                        .to_string()
                },
                AgentEvent::Exited { code: Some(125) },
            ]
        );
    }

    #[test]
    fn reports_a_signal_kill_without_an_exit_code() {
        let t = setup();
        t.adapter.start("p", "/proj", None).unwrap();
        t.first().close(None);
        t.wait_for_events(2);
        assert_eq!(
            t.events()[0],
            AgentEvent::Error {
                message: "The sandbox stopped unexpectedly.".to_string()
            }
        );
    }

    #[test]
    fn keeps_only_the_end_of_a_very_long_stderr() {
        let t = setup();
        t.adapter.start("p", "/proj", None).unwrap();
        let c = t.first();
        c.stderr_write(&format!("{}END", "x".repeat(5000)));
        c.close(Some(1));
        t.wait_for_events(2);
        match &t.events()[0] {
            AgentEvent::Error { message } => {
                assert!(message.ends_with("END"));
                assert!(message.len() < 2100);
            }
            other => panic!("unexpected event {:?}", other),
        }
    }

    #[test]
    fn does_not_add_a_second_error_when_the_runner_already_reported_one() {
        let t = setup();
        t.adapter.start("p", "/proj", None).unwrap();
        let c = t.first();
        c.stdout_write("{\"type\":\"error\",\"message\":\"bad key\"}\n");
        c.close(Some(1));
        t.wait_for_events(2);
        pause();
        assert_eq!(
            t.events(),
            vec![
                AgentEvent::Error {
                    message: "bad key".to_string()
                },
                AgentEvent::Exited { code: Some(1) },
            ]
        );
    }

    #[test]
    fn reports_docker_failing_to_run() {
        let t = setup();
        t.adapter.start("p", "/proj", None).unwrap();
        t.first().close_error("spawn docker ENOENT");
        t.wait_for_events(2);
        assert_eq!(
            t.events(),
            vec![
                AgentEvent::Error {
                    message: "Could not run Docker: spawn docker ENOENT".to_string()
                },
                AgentEvent::Exited { code: None },
            ]
        );
    }

    #[test]
    fn allows_a_new_session_after_the_container_exits() {
        let t = setup();
        t.adapter.start("p", "/proj", None).unwrap();
        t.first().close(Some(0));
        t.wait_for_events(1);
        t.adapter.start("again", "/proj", None).unwrap();
        assert_eq!(t.last().commands().last().unwrap(), &prompt("again"));
    }

    #[test]
    fn survives_writing_to_a_dead_container() {
        let t = setup();
        t.adapter.start("p", "/proj", None).unwrap();
        t.first().fail_writes.store(true, Ordering::SeqCst);
        assert_eq!(t.adapter.send("x"), Ok(()));
    }

    // commands

    #[test]
    fn sends_follow_ups_and_interrupts() {
        let t = setup();
        t.adapter.start("p", "/proj", None).unwrap();
        t.adapter.send("and also this").unwrap();
        t.adapter.interrupt().unwrap();
        assert_eq!(
            t.first().commands()[2..].to_vec(),
            vec![prompt("and also this"), json!({"type": "interrupt"})]
        );
    }

    #[test]
    fn refuses_to_send_or_interrupt_without_a_session() {
        let t = setup();
        assert!(t
            .adapter
            .send("x")
            .unwrap_err()
            .contains("No session is running"));
        assert!(t
            .adapter
            .interrupt()
            .unwrap_err()
            .contains("No session is running"));
    }

    #[test]
    fn accepts_approval_responses_as_a_no_op() {
        assert_eq!(setup().adapter.respond_to_approval("id", true), Ok(()));
    }

    // stop

    #[test]
    fn stop_does_nothing_without_a_session() {
        let t = setup();
        t.adapter.stop().unwrap();
        assert_eq!(t.stops.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn stop_stops_the_container_and_reports_a_quiet_exit() {
        let t = setup();
        t.adapter.start("p", "/proj", None).unwrap();
        t.adapter.stop().unwrap();
        assert_eq!(t.stops.load(Ordering::SeqCst), 1);
        assert_eq!(t.events(), vec![AgentEvent::Exited { code: Some(143) }]);
    }

    #[test]
    fn reports_a_later_unexpected_exit_again_after_a_stopped_session() {
        let t = setup();
        t.adapter.start("p", "/proj", None).unwrap();
        t.adapter.stop().unwrap();
        t.adapter.start("p", "/proj", None).unwrap();
        t.last().close(Some(1));
        t.wait_for_events(3);
        let events = t.events();
        assert!(matches!(events[events.len() - 2], AgentEvent::Error { .. }));
    }

    // attempt

    #[test]
    fn attempt_is_none_on_success_and_the_message_on_failure() {
        assert_eq!(attempt(|| Ok(())), None);
        assert_eq!(
            attempt(|| Err("nope".to_string())),
            Some("nope".to_string())
        );
    }

    // resume

    #[test]
    fn start_passes_the_session_to_resume_in_init() {
        let t = setup();
        t.adapter.start("go on", "/proj", Some("sess-1")).unwrap();
        assert_eq!(t.first().commands()[0], init_resume("/proj", "sess-1"));
    }

    // warm

    #[test]
    fn warm_starts_the_container_and_sends_only_init() {
        let t = setup();
        t.adapter.warm("/proj", None).unwrap();
        assert_eq!(t.first().commands(), vec![init("/proj")]);
    }

    #[test]
    fn warm_lets_the_first_prompt_use_the_warm_container() {
        let t = setup();
        t.adapter.warm("/proj", None).unwrap();
        t.adapter.start("go", "/proj", None).unwrap();
        assert_eq!(t.first().commands(), vec![init("/proj"), prompt("go")]);
    }

    #[test]
    fn warm_is_a_no_op_when_a_matching_warm_agent_exists() {
        let t = setup();
        t.adapter.warm("/proj", None).unwrap();
        t.adapter.warm("/proj", None).unwrap();
        assert_eq!(t.first().commands().len(), 1);
    }

    #[test]
    fn warm_waits_for_a_prewarm_in_progress_when_a_prompt_arrives() {
        let (check, entered, release) = gated_check(None);
        let t = setup_with(Overrides {
            check_docker: Some(check),
            ..Overrides::default()
        });
        let adapter = t.adapter.clone();
        let warming = thread::spawn(move || adapter.warm("/proj", None));
        wait_until(|| entered.load(Ordering::SeqCst));
        let adapter = t.adapter.clone();
        let starting = thread::spawn(move || adapter.start("go", "/proj", None));
        pause();
        release.send(()).unwrap();
        assert_eq!(warming.join().unwrap(), Ok(()));
        assert_eq!(starting.join().unwrap(), Ok(()));
        assert_eq!(types(&t.first().commands()), vec!["init", "prompt"]);
        assert_eq!(lock(&t.containers).len(), 1);
    }

    #[test]
    fn warm_starts_again_when_the_prewarm_in_progress_fails() {
        let (check, entered, release) = gated_check(Some("Docker is not available.".to_string()));
        let t = setup_with(Overrides {
            check_docker: Some(check),
            ..Overrides::default()
        });
        let adapter = t.adapter.clone();
        let warming = thread::spawn(move || adapter.warm("/proj", None));
        wait_until(|| entered.load(Ordering::SeqCst));
        let adapter = t.adapter.clone();
        let starting = thread::spawn(move || adapter.start("go", "/proj", None));
        pause();
        release.send(()).unwrap();
        let warmed = warming.join().unwrap();
        assert!(warmed.unwrap_err().contains("Docker is not available."));
        assert_eq!(starting.join().unwrap(), Ok(()));
        assert_eq!(types(&t.first().commands()), vec!["init", "prompt"]);
    }

    #[test]
    fn warm_restarts_a_warm_agent_that_must_resume_a_stored_session() {
        let t = setup();
        t.adapter.warm("/proj", None).unwrap();
        t.adapter.start("go", "/proj", Some("sess-1")).unwrap();
        assert_eq!(t.stops.load(Ordering::SeqCst), 1);
        let commands = t.last().commands();
        assert_eq!(commands.last().unwrap(), &prompt("go"));
        assert!(commands.contains(&init_resume("/proj", "sess-1")));
    }

    #[test]
    fn warm_restarts_a_warm_agent_that_belongs_to_another_project() {
        let t = setup();
        t.adapter.warm("/proj", None).unwrap();
        t.adapter.start("go", "/other", None).unwrap();
        assert_eq!(t.stops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn warm_refuses_to_warm_over_a_running_session() {
        let t = setup();
        t.adapter.start("go", "/proj", None).unwrap();
        let error = t.adapter.warm("/proj", None).unwrap_err();
        assert!(error.contains("already running"));
    }

    #[test]
    fn warm_joins_a_prewarm_already_in_progress() {
        let t = setup();
        let a = t.adapter.clone();
        let b = t.adapter.clone();
        let first = thread::spawn(move || a.warm("/proj", None));
        let second = thread::spawn(move || b.warm("/proj", None));
        assert_eq!(first.join().unwrap(), Ok(()));
        assert_eq!(second.join().unwrap(), Ok(()));
        assert_eq!(lock(&t.containers).len(), 1);
        assert_eq!(t.first().commands().len(), 1);
    }

    // warm agent matching

    #[test]
    fn reuses_a_warm_agent_that_was_warmed_to_resume_the_same_session() {
        let t = setup();
        t.adapter.warm("/proj", Some("sess-1")).unwrap();
        t.adapter.warm("/proj", Some("sess-1")).unwrap();
        t.adapter.start("go", "/proj", Some("sess-1")).unwrap();
        assert_eq!(t.stops.load(Ordering::SeqCst), 0);
        assert_eq!(
            t.first().commands(),
            vec![init_resume("/proj", "sess-1"), prompt("go")]
        );
    }

    #[test]
    fn does_not_reuse_a_fresh_warm_agent_for_a_resume_nor_a_resumed_one_for_a_fresh_start() {
        let t = setup();
        t.adapter.warm("/proj", None).unwrap();
        t.adapter.start("go", "/proj", Some("sess-1")).unwrap();
        assert_eq!(t.stops.load(Ordering::SeqCst), 1);
        t.adapter.stop().unwrap();
        t.adapter.warm("/proj", Some("sess-2")).unwrap();
        t.adapter.start("go", "/proj", None).unwrap();
        assert_eq!(t.stops.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn refuses_to_warm_over_a_warm_agent_that_is_for_another_session() {
        let t = setup();
        t.adapter.warm("/proj", Some("sess-1")).unwrap();
        let error = t.adapter.warm("/proj", Some("sess-2")).unwrap_err();
        assert!(error.contains("already running"));
    }
}
