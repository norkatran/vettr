//! Owns the agent's lifecycle and readiness for the open project (port of `src/main/agentManager.ts`,
//! design 0002).
//!
//! All transitions run on one worker thread fed by a queue, so a start never overlaps a stop, and
//! work for a project that has been replaced since it was queued is dropped. Public methods block
//! until done; callers run them on worker threads. The state mutex is never held while calling
//! into the adapter or any callback.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use crate::agent::{AgentAdapter, AgentEvent, EventListener, SlashCommandInfo};
use crate::readiness::{readiness_block_reason, Readiness, ReadinessReason, ReadinessStatus};
use crate::sandbox::{DockerProblem, DockerProblemKind};

/// Unrequested exits in a row (with no finished turn between) before giving up restarting.
const MAX_CRASHES: u32 = 3;

/// The result of an image build attempt.
#[derive(Debug, Clone, PartialEq)]
pub enum BuildOutcome {
    /// The image was built.
    Built,
    /// The build failed with this message.
    Failed(String),
    /// The app cannot build the image here, so the original problem stands.
    Unsupported,
}

/// What is wrong with Docker or the image, or `None` when the sandbox can start.
pub type CheckDocker = Box<dyn Fn() -> Option<DockerProblem> + Send + Sync>;
/// Build the sandbox image, reporting build output lines.
pub type BuildImage = Box<dyn Fn(&dyn Fn(&str)) -> BuildOutcome + Send + Sync>;
/// Whether a credential is available.
pub type HasKey = Box<dyn Fn() -> bool + Send + Sync>;
/// Listener for readiness changes.
pub type ReadinessListener = Box<dyn Fn(Readiness) + Send + Sync>;

pub struct AgentManagerDeps {
    pub agent: Arc<dyn AgentAdapter>,
    pub check_docker: CheckDocker,
    /// `None` when the app cannot build the image at all.
    pub build_image: Option<BuildImage>,
    pub has_key: HasKey,
}

type SharedReadinessListener = Arc<dyn Fn(Readiness) + Send + Sync>;
type SharedEventListener = Arc<EventListener>;
type JobResult = Result<(), String>;

enum Job {
    Cycle {
        generation: u64,
        resume: Option<String>,
        warm: bool,
        done: Sender<JobResult>,
    },
    /// Replies once everything queued before it has finished.
    Barrier { done: Sender<JobResult> },
}

struct State {
    readiness: Readiness,
    project: Option<String>,
    /// Bumped when the project changes, so queued and in-flight work for the old one is discarded.
    generation: u64,
    /// True while the manager itself stops the agent, so that exit is not taken for a crash.
    stopping: bool,
    /// A turn is running.
    busy: bool,
    /// The warm agent has received a prompt, so a new session needs a fresh one.
    used: bool,
    warm_resume: Option<String>,
    session_id: Option<String>,
    crashes: u32,
    /// The latest slash commands the agent reported; empty while no agent is running.
    commands: Vec<SlashCommandInfo>,
}

struct Inner {
    deps: AgentManagerDeps,
    state: Mutex<State>,
    readiness_listeners: Mutex<Vec<(u64, SharedReadinessListener)>>,
    event_listeners: Mutex<Vec<(u64, SharedEventListener)>>,
    next_id: AtomicU64,
    queue: Mutex<Sender<Job>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn plain(status: ReadinessStatus) -> Readiness {
    Readiness {
        status,
        reason: None,
        message: None,
    }
}

/// Block until a queued job finishes.
pub fn wait_for_job(rx: Receiver<Result<(), String>>) -> Result<(), String> {
    match rx.recv() {
        Ok(result) => result,
        Err(_) => Err("The agent manager stopped.".to_string()),
    }
}

pub struct AgentManager {
    inner: Arc<Inner>,
    subscription: u64,
}

impl AgentManager {
    pub fn new(deps: AgentManagerDeps) -> AgentManager {
        let (tx, rx) = channel::<Job>();
        let agent = deps.agent.clone();
        let inner = Arc::new(Inner {
            deps,
            state: Mutex::new(State {
                readiness: Readiness::initial(),
                project: None,
                generation: 0,
                stopping: false,
                busy: false,
                used: false,
                warm_resume: None,
                session_id: None,
                crashes: 0,
                commands: Vec::new(),
            }),
            readiness_listeners: Mutex::new(Vec::new()),
            event_listeners: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
            queue: Mutex::new(tx),
        });

        // The worker and the adapter listener hold weak references so dropping the manager ends
        // both.
        let weak_worker: Weak<Inner> = Arc::downgrade(&inner);
        std::thread::spawn(move || {
            for job in rx {
                let strong = match weak_worker.upgrade() {
                    Some(s) => s,
                    None => break,
                };
                match job {
                    Job::Cycle {
                        generation,
                        resume,
                        warm,
                        done,
                    } => {
                        let result = strong.cycle(generation, resume, warm);
                        let _ = done.send(result);
                    }
                    Job::Barrier { done } => {
                        let _ = done.send(Ok(()));
                    }
                }
            }
        });

        let weak_events: Weak<Inner> = Arc::downgrade(&inner);
        let listener: EventListener = Box::new(move |event: AgentEvent| {
            if let Some(strong) = weak_events.upgrade() {
                strong.on_agent_event(&event);
            }
        });
        let subscription = agent.on_event(listener);

        AgentManager {
            inner,
            subscription,
        }
    }

    /// The slash commands the running agent offers (the UI may load after the event).
    pub fn slash_commands(&self) -> Vec<SlashCommandInfo> {
        lock(&self.inner.state).commands.clone()
    }

    pub fn readiness(&self) -> Readiness {
        lock(&self.inner.state).readiness.clone()
    }

    /// Whether the agent is working on a turn (losing it would lose work in progress).
    pub fn is_busy(&self) -> bool {
        lock(&self.inner.state).busy
    }

    /// Subscribe to readiness changes; returns an id for `off_readiness`.
    pub fn on_readiness(&self, listener: ReadinessListener) -> u64 {
        let id = self.inner.next_id.fetch_add(1, Ordering::SeqCst);
        let shared: SharedReadinessListener = Arc::from(listener);
        lock(&self.inner.readiness_listeners).push((id, shared));
        id
    }

    pub fn off_readiness(&self, id: u64) {
        lock(&self.inner.readiness_listeners).retain(|(i, _)| *i != id);
    }

    /// Subscribe to the agent's events (forwarded after the manager has processed them); returns
    /// an id for `off_event`.
    pub fn on_event(&self, listener: EventListener) -> u64 {
        let id = self.inner.next_id.fetch_add(1, Ordering::SeqCst);
        lock(&self.inner.event_listeners).push((id, Arc::new(listener)));
        id
    }

    pub fn off_event(&self, id: u64) {
        lock(&self.inner.event_listeners).retain(|(i, _)| *i != id);
    }

    /// Open `project` (or none). The same project is a no-op; another one tears the agent down.
    pub fn set_project(&self, project: Option<&str>) -> Result<(), String> {
        wait_for_job(self.set_project_async(project))
    }

    /// Like `set_project` but returns at once; wait for the result with `wait_for_job`.
    pub fn set_project_async(&self, project: Option<&str>) -> Receiver<Result<(), String>> {
        let project: Option<String> = project.map(|p| p.to_string());
        let generation;
        {
            let mut s = lock(&self.inner.state);
            if project == s.project {
                drop(s);
                return self.inner.barrier();
            }
            s.project = project;
            s.generation += 1;
            s.session_id = None;
            s.crashes = 0;
            s.busy = false;
            generation = s.generation;
        }
        self.inner.enqueue(generation, None, true)
    }

    /// The credential changed or was removed: always restart, keeping the stored session.
    pub fn key_changed(&self) -> Result<(), String> {
        wait_for_job(self.key_changed_async())
    }

    pub fn key_changed_async(&self) -> Receiver<Result<(), String>> {
        let (generation, session) = {
            let s = lock(&self.inner.state);
            (s.generation, s.session_id.clone())
        };
        self.inner.enqueue(generation, session, true)
    }

    /// End the current session and warm a fresh agent (nothing to do if it was never used).
    pub fn new_session(&self) -> Result<(), String> {
        let generation;
        {
            let mut s = lock(&self.inner.state);
            s.session_id = None;
            if !s.used && s.readiness.status == ReadinessStatus::Ready {
                return Ok(());
            }
            generation = s.generation;
        }
        wait_for_job(self.inner.enqueue(generation, None, true))
    }

    /// Start a session with a prompt, resuming stored session `resume` if given.
    pub fn start(&self, prompt: &str, resume: Option<&str>) -> Result<(), String> {
        let resume: Option<String> = resume.filter(|r| !r.is_empty()).map(|r| r.to_string());
        self.inner.require_ready()?;
        let (used, warm_resume, generation) = {
            let s = lock(&self.inner.state);
            (s.used, s.warm_resume.clone(), s.generation)
        };
        if used || resume != warm_resume {
            wait_for_job(self.inner.enqueue(generation, resume.clone(), true))?;
            self.inner.require_ready()?;
        }
        let project;
        {
            let mut s = lock(&self.inner.state);
            s.used = true;
            s.busy = true;
            if let Some(r) = &resume {
                s.session_id = Some(r.clone());
            }
            project = s.project.clone().unwrap_or_default();
        }
        let result = self
            .inner
            .deps
            .agent
            .start(prompt, &project, resume.as_deref());
        if result.is_err() {
            lock(&self.inner.state).busy = false;
        }
        result
    }

    pub fn send(&self, message: &str) -> Result<(), String> {
        self.inner.require_ready()?;
        {
            let mut s = lock(&self.inner.state);
            s.used = true;
            s.busy = true;
        }
        let result = self.inner.deps.agent.send(message);
        if result.is_err() {
            lock(&self.inner.state).busy = false;
        }
        result
    }

    pub fn interrupt(&self) -> Result<(), String> {
        self.inner.require_ready()?;
        self.inner.deps.agent.interrupt()
    }

    /// Stop the agent for good (app quit).
    pub fn shutdown(&self) -> Result<(), String> {
        let generation = {
            let mut s = lock(&self.inner.state);
            s.generation += 1;
            s.generation
        };
        wait_for_job(self.inner.enqueue(generation, None, false))
    }
}

impl Drop for AgentManager {
    fn drop(&mut self) {
        self.inner.deps.agent.off_event(self.subscription);
    }
}

impl Inner {
    fn require_ready(&self) -> Result<(), String> {
        let readiness = lock(&self.state).readiness.clone();
        match readiness_block_reason(&readiness) {
            Some(block) => Err(block.to_string()),
            None => Ok(()),
        }
    }

    fn is_stale(&self, generation: u64) -> bool {
        lock(&self.state).generation != generation
    }

    fn set(&self, next: Readiness) {
        lock(&self.state).readiness = next.clone();
        let listeners: Vec<SharedReadinessListener> = lock(&self.readiness_listeners)
            .iter()
            .map(|(_, l)| l.clone())
            .collect();
        for listener in listeners {
            listener(next.clone());
        }
    }

    fn enqueue(&self, generation: u64, resume: Option<String>, warm: bool) -> Receiver<JobResult> {
        let (done, rx) = channel::<JobResult>();
        let job = Job::Cycle {
            generation,
            resume,
            warm,
            done: done.clone(),
        };
        let sent = lock(&self.queue).send(job);
        if sent.is_err() {
            let _ = done.send(Err("The agent manager stopped.".to_string()));
        }
        rx
    }

    fn barrier(&self) -> Receiver<JobResult> {
        let (done, rx) = channel::<JobResult>();
        let sent = lock(&self.queue).send(Job::Barrier { done: done.clone() });
        if sent.is_err() {
            let _ = done.send(Ok(()));
        }
        rx
    }

    /// Stop whatever is running, then check the prerequisites and warm an agent.
    fn cycle(&self, generation: u64, resume: Option<String>, warm: bool) -> Result<(), String> {
        let was_ready = lock(&self.state).readiness.status == ReadinessStatus::Ready;
        if was_ready {
            self.set(plain(ReadinessStatus::Stopping));
        }
        lock(&self.state).stopping = true;
        let stop_result = self.deps.agent.stop();
        lock(&self.state).stopping = false;
        stop_result?;
        let project;
        {
            let mut s = lock(&self.state);
            s.busy = false;
            s.used = false;
            s.warm_resume = None;
            project = s.project.clone();
        }
        if self.is_stale(generation) {
            return Ok(());
        }
        let project = match project {
            Some(p) if warm => p,
            _ => {
                self.set(Readiness::initial());
                return Ok(());
            }
        };
        let mut docker = (self.deps.check_docker)();
        if self.is_stale(generation) {
            return Ok(());
        }
        let needs_build = matches!(&docker, Some(p) if matches!(p.kind, DockerProblemKind::Image));
        if needs_build {
            if let Some(build) = self.deps.build_image.as_ref() {
                self.set(Readiness {
                    status: ReadinessStatus::Starting,
                    reason: Some(ReadinessReason::BuildingImage),
                    message: None,
                });
                let progress = |line: &str| {
                    if !self.is_stale(generation) {
                        self.set(Readiness {
                            status: ReadinessStatus::Starting,
                            reason: Some(ReadinessReason::BuildingImage),
                            message: Some(format!("Building the sandbox image… {}", line)),
                        });
                    }
                };
                let outcome = build(&progress);
                if self.is_stale(generation) {
                    return Ok(());
                }
                match outcome {
                    // This app cannot build the image, so the original problem stands.
                    BuildOutcome::Unsupported => {}
                    BuildOutcome::Failed(message) if !message.is_empty() => {
                        docker = Some(DockerProblem {
                            kind: DockerProblemKind::Image,
                            message,
                        });
                    }
                    _ => {
                        docker = (self.deps.check_docker)();
                        if self.is_stale(generation) {
                            return Ok(());
                        }
                    }
                }
            }
        }
        if let Some(problem) = docker {
            let reason = match problem.kind {
                DockerProblemKind::Docker => ReadinessReason::DockerUnavailable,
                _ => ReadinessReason::Error,
            };
            self.set(Readiness {
                status: ReadinessStatus::Error,
                reason: Some(reason),
                message: Some(problem.message),
            });
            return Ok(());
        }
        let has_key = (self.deps.has_key)();
        if self.is_stale(generation) {
            return Ok(());
        }
        if !has_key {
            self.set(Readiness {
                status: ReadinessStatus::Idle,
                reason: Some(ReadinessReason::NoKey),
                message: None,
            });
            return Ok(());
        }
        let crashed =
            lock(&self.state).readiness.reason == Some(ReadinessReason::CrashedRestarting);
        self.set(Readiness {
            status: ReadinessStatus::Starting,
            reason: Some(if crashed {
                ReadinessReason::CrashedRestarting
            } else {
                ReadinessReason::Starting
            }),
            message: None,
        });
        if let Err(message) = self.deps.agent.warm(&project, resume.as_deref()) {
            if self.is_stale(generation) {
                return Ok(());
            }
            self.set(Readiness {
                status: ReadinessStatus::Error,
                reason: Some(ReadinessReason::Error),
                message: Some(message),
            });
            return Ok(());
        }
        // A stale warm agent is stopped by the next queued cycle, which always stops first.
        if self.is_stale(generation) {
            return Ok(());
        }
        lock(&self.state).warm_resume = resume;
        self.set(plain(ReadinessStatus::Ready));
        Ok(())
    }

    fn on_agent_event(&self, event: &AgentEvent) {
        let mut restart = false;
        {
            let mut s = lock(&self.state);
            match event {
                AgentEvent::Commands { commands } => s.commands = commands.clone(),
                AgentEvent::Exited { .. } => s.commands = Vec::new(),
                _ => {}
            }
            match event {
                AgentEvent::SessionStarted { session_id } => {
                    s.session_id = Some(session_id.clone());
                }
                AgentEvent::TurnFinished => {
                    s.busy = false;
                    s.crashes = 0;
                }
                AgentEvent::Exited { .. }
                    if !s.stopping && s.readiness.status == ReadinessStatus::Ready =>
                {
                    s.busy = false;
                    restart = true;
                }
                _ => {}
            }
        }
        if restart {
            self.restart_after_crash();
        }
        let listeners: Vec<SharedEventListener> = lock(&self.event_listeners)
            .iter()
            .map(|(_, l)| l.clone())
            .collect();
        for listener in listeners {
            let f: &EventListener = &listener;
            f(event.clone());
        }
    }

    fn restart_after_crash(&self) {
        let (give_up, generation, session) = {
            let mut s = lock(&self.state);
            s.crashes += 1;
            (s.crashes > MAX_CRASHES, s.generation, s.session_id.clone())
        };
        if give_up {
            self.set(Readiness {
                status: ReadinessStatus::Error,
                reason: Some(ReadinessReason::Error),
                message: Some("The agent keeps stopping unexpectedly.".to_string()),
            });
            return;
        }
        self.set(Readiness {
            status: ReadinessStatus::Starting,
            reason: Some(ReadinessReason::CrashedRestarting),
            message: None,
        });
        // Fire and forget: dropping the receiver is harmless.
        let _rx = self.enqueue(generation, session, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, AtomicUsize};
    use std::time::{Duration, Instant};

    // ---- gates: let a test pause a fake at a call and release it with a value ----

    struct Gate<T> {
        entered: Sender<()>,
        release: Receiver<T>,
    }

    struct GateHandle<T> {
        entered: Receiver<()>,
        release: Sender<T>,
    }

    fn gate<T>() -> (Gate<T>, GateHandle<T>) {
        let (entered_tx, entered_rx) = channel::<()>();
        let (release_tx, release_rx) = channel::<T>();
        (
            Gate {
                entered: entered_tx,
                release: release_rx,
            },
            GateHandle {
                entered: entered_rx,
                release: release_tx,
            },
        )
    }

    impl<T> Gate<T> {
        fn pass(self) -> T {
            let _ = self.entered.send(());
            self.release
                .recv_timeout(Duration::from_secs(5))
                .expect("gate was not released")
        }
    }

    impl<T> GateHandle<T> {
        fn wait_entered(&self) {
            self.entered
                .recv_timeout(Duration::from_secs(5))
                .expect("gate was never reached");
        }
        fn release(&self, value: T) {
            let _ = self.release.send(value);
        }
    }

    fn take_gate<T>(slot: &Mutex<Option<Gate<T>>>) -> Option<Gate<T>> {
        lock(slot).take()
    }

    fn wait_until(what: &str, cond: impl Fn() -> bool) {
        let start = Instant::now();
        while !cond() {
            if start.elapsed() > Duration::from_secs(5) {
                panic!("timed out waiting for {}", what);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    // ---- fake agent ----

    struct FakeAgent {
        log: Mutex<Vec<String>>,
        listener: Mutex<Option<SharedEventListener>>,
        warm_calls: AtomicUsize,
        stop_calls: AtomicUsize,
        interrupt_calls: AtomicUsize,
        starts: Mutex<Vec<(String, String, Option<String>)>>,
        sends: Mutex<Vec<String>>,
        warm_errors: Mutex<VecDeque<String>>,
        start_error: Mutex<Option<String>>,
        send_error: Mutex<Option<String>>,
        stop_error: Mutex<Option<String>>,
        warm_gate: Mutex<Option<Gate<Result<(), String>>>>,
        stop_gate: Mutex<Option<Gate<()>>>,
        exit_on_stop: AtomicBool,
    }

    impl FakeAgent {
        fn new() -> FakeAgent {
            FakeAgent {
                log: Mutex::new(Vec::new()),
                listener: Mutex::new(None),
                warm_calls: AtomicUsize::new(0),
                stop_calls: AtomicUsize::new(0),
                interrupt_calls: AtomicUsize::new(0),
                starts: Mutex::new(Vec::new()),
                sends: Mutex::new(Vec::new()),
                warm_errors: Mutex::new(VecDeque::new()),
                start_error: Mutex::new(None),
                send_error: Mutex::new(None),
                stop_error: Mutex::new(None),
                warm_gate: Mutex::new(None),
                stop_gate: Mutex::new(None),
                exit_on_stop: AtomicBool::new(false),
            }
        }

        fn emit(&self, event: AgentEvent) {
            let current: Option<SharedEventListener> = lock(&self.listener).clone();
            if let Some(l) = current {
                let f: &EventListener = &l;
                f(event);
            }
        }

        fn log(&self) -> Vec<String> {
            lock(&self.log).clone()
        }

        fn warms(&self) -> usize {
            self.warm_calls.load(Ordering::SeqCst)
        }
    }

    impl AgentAdapter for FakeAgent {
        fn warm(&self, cwd: &str, resume: Option<&str>) -> Result<(), String> {
            self.warm_calls.fetch_add(1, Ordering::SeqCst);
            if let Some(g) = take_gate(&self.warm_gate) {
                return g.pass();
            }
            let failure = lock(&self.warm_errors).pop_front();
            if let Some(message) = failure {
                return Err(message);
            }
            let line = match resume {
                Some(r) => format!("warm {} {}", cwd, r),
                None => format!("warm {}", cwd),
            };
            lock(&self.log).push(line);
            Ok(())
        }

        fn start(&self, prompt: &str, cwd: &str, resume: Option<&str>) -> Result<(), String> {
            lock(&self.starts).push((
                prompt.to_string(),
                cwd.to_string(),
                resume.map(|r| r.to_string()),
            ));
            let failure = lock(&self.start_error).take();
            match failure {
                Some(message) => Err(message),
                None => Ok(()),
            }
        }

        fn send(&self, message: &str) -> Result<(), String> {
            lock(&self.sends).push(message.to_string());
            let failure = lock(&self.send_error).take();
            match failure {
                Some(message) => Err(message),
                None => Ok(()),
            }
        }

        fn interrupt(&self) -> Result<(), String> {
            self.interrupt_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn on_event(&self, listener: EventListener) -> u64 {
            *lock(&self.listener) = Some(Arc::new(listener));
            1
        }

        fn off_event(&self, _subscription: u64) {}

        fn respond_to_approval(&self, _id: &str, _allow: bool) -> Result<(), String> {
            Ok(())
        }

        fn stop(&self) -> Result<(), String> {
            self.stop_calls.fetch_add(1, Ordering::SeqCst);
            if let Some(g) = take_gate(&self.stop_gate) {
                g.pass();
            }
            let failure = lock(&self.stop_error).take();
            if let Some(message) = failure {
                return Err(message);
            }
            if self.exit_on_stop.load(Ordering::SeqCst) {
                self.emit(AgentEvent::Exited { code: Some(143) });
            } else {
                lock(&self.log).push("stop".to_string());
            }
            Ok(())
        }
    }

    // ---- harness ----

    #[derive(Default)]
    struct Opts {
        docker: Option<String>,
        key: Option<bool>,
        check_docker: Option<CheckDocker>,
        build_image: Option<BuildImage>,
        has_key: Option<HasKey>,
    }

    struct Harness {
        manager: AgentManager,
        agent: Arc<FakeAgent>,
        key: Arc<AtomicBool>,
        states: Arc<Mutex<Vec<Readiness>>>,
    }

    impl Harness {
        fn states(&self) -> Vec<Readiness> {
            lock(&self.states).clone()
        }
        fn emit(&self, event: AgentEvent) {
            self.agent.emit(event);
        }
        fn readiness(&self) -> Readiness {
            self.manager.readiness()
        }
        fn wait_status(&self, status: ReadinessStatus) {
            wait_until("readiness", || self.manager.readiness().status == status);
        }
    }

    fn setup(opts: Opts) -> Harness {
        let agent = Arc::new(FakeAgent::new());
        let adapter: Arc<dyn AgentAdapter> = agent.clone();
        let docker: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(opts.docker.clone()));
        let key = Arc::new(AtomicBool::new(opts.key.unwrap_or(true)));
        let check_docker: CheckDocker = match opts.check_docker {
            Some(c) => c,
            None => {
                let d = docker.clone();
                Box::new(move || {
                    let message: Option<String> = lock(&d).clone();
                    message.map(|m| DockerProblem {
                        kind: DockerProblemKind::Docker,
                        message: m,
                    })
                })
            }
        };
        let has_key: HasKey = match opts.has_key {
            Some(h) => h,
            None => {
                let k = key.clone();
                Box::new(move || k.load(Ordering::SeqCst))
            }
        };
        let manager = AgentManager::new(AgentManagerDeps {
            agent: adapter,
            check_docker,
            build_image: opts.build_image,
            has_key,
        });
        let states: Arc<Mutex<Vec<Readiness>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = states.clone();
        manager.on_readiness(Box::new(move |r: Readiness| {
            lock(&sink).push(r);
        }));
        Harness {
            manager,
            agent,
            key,
            states,
        }
    }

    fn rd(
        status: ReadinessStatus,
        reason: Option<ReadinessReason>,
        message: Option<&str>,
    ) -> Readiness {
        Readiness {
            status,
            reason,
            message: message.map(|m| m.to_string()),
        }
    }

    fn ready() -> Readiness {
        rd(ReadinessStatus::Ready, None, None)
    }

    fn idle() -> Readiness {
        rd(ReadinessStatus::Idle, None, None)
    }

    fn statuses(states: &[Readiness]) -> Vec<ReadinessStatus> {
        states.iter().map(|s| s.status).collect()
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn missing() -> DockerProblem {
        DockerProblem {
            kind: DockerProblemKind::Image,
            message: "image is missing".to_string(),
        }
    }

    fn docker_down() -> DockerProblem {
        DockerProblem {
            kind: DockerProblemKind::Docker,
            message: "Docker is not available.".to_string(),
        }
    }

    fn build_with(f: impl Fn(&dyn Fn(&str)) -> BuildOutcome + Send + Sync + 'static) -> BuildImage {
        Box::new(f)
    }

    // ---- launch ----

    #[test]
    fn prewarms_the_agent_for_the_project_and_becomes_ready() {
        let h = setup(Opts::default());
        h.manager.set_project(Some("/p")).unwrap();
        assert_eq!(h.agent.log(), strings(&["stop", "warm /p"]));
        assert_eq!(
            statuses(&h.states()),
            vec![ReadinessStatus::Starting, ReadinessStatus::Ready]
        );
        assert_eq!(h.readiness(), ready());
    }

    #[test]
    fn reports_docker_problems_as_an_error_and_does_not_warm() {
        let h = setup(Opts {
            docker: Some("Docker is not available.".to_string()),
            ..Opts::default()
        });
        h.manager.set_project(Some("/p")).unwrap();
        assert_eq!(
            h.readiness(),
            rd(
                ReadinessStatus::Error,
                Some(ReadinessReason::DockerUnavailable),
                Some("Docker is not available.")
            )
        );
        assert_eq!(h.agent.warms(), 0);
    }

    #[test]
    fn waits_for_a_key_then_warms_when_the_key_is_saved() {
        let h = setup(Opts {
            key: Some(false),
            ..Opts::default()
        });
        h.manager.set_project(Some("/p")).unwrap();
        assert_eq!(
            h.readiness(),
            rd(ReadinessStatus::Idle, Some(ReadinessReason::NoKey), None)
        );
        h.key.store(true, Ordering::SeqCst);
        h.manager.key_changed().unwrap();
        assert_eq!(h.readiness(), ready());
        assert_eq!(h.agent.warms(), 1);
    }

    #[test]
    fn reports_a_failed_warm() {
        let h = setup(Opts::default());
        lock(&h.agent.warm_errors).push_back("boom".to_string());
        h.manager.set_project(Some("/p")).unwrap();
        assert_eq!(
            h.readiness(),
            rd(
                ReadinessStatus::Error,
                Some(ReadinessReason::Error),
                Some("boom")
            )
        );
        lock(&h.agent.warm_errors).push_back("plain".to_string());
        h.manager.key_changed().unwrap();
        assert_eq!(h.readiness().message, Some("plain".to_string()));
    }

    #[test]
    fn goes_idle_with_no_project() {
        let h = setup(Opts::default());
        h.manager.set_project(Some("/p")).unwrap();
        h.manager.set_project(None).unwrap();
        assert_eq!(h.readiness(), idle());
    }

    // ---- project changes ----

    #[test]
    fn does_nothing_for_the_same_project() {
        let h = setup(Opts::default());
        h.manager.set_project(Some("/p")).unwrap();
        h.manager.set_project(Some("/p")).unwrap();
        assert_eq!(h.agent.warms(), 1);
    }

    #[test]
    fn stops_the_old_agent_before_warming_for_the_new_project() {
        let h = setup(Opts::default());
        h.manager.set_project(Some("/a")).unwrap();
        h.manager.set_project(Some("/b")).unwrap();
        assert_eq!(
            h.agent.log(),
            strings(&["stop", "warm /a", "stop", "warm /b"])
        );
        assert!(statuses(&h.states()).contains(&ReadinessStatus::Stopping));
    }

    #[test]
    fn discards_work_for_a_project_replaced_while_it_was_starting() {
        let h = setup(Opts::default());
        let (g, handle) = gate::<()>();
        *lock(&h.agent.stop_gate) = Some(g);
        let first = h.manager.set_project_async(Some("/a"));
        handle.wait_entered();
        let second = h.manager.set_project_async(Some("/b"));
        handle.release(());
        wait_for_job(first).unwrap();
        wait_for_job(second).unwrap();
        assert_eq!(h.agent.log(), strings(&["stop", "stop", "warm /b"]));
        assert_eq!(h.readiness(), ready());
    }

    #[test]
    fn discards_a_warm_that_finishes_after_the_project_changed() {
        let h = setup(Opts::default());
        let (g, handle) = gate::<Result<(), String>>();
        *lock(&h.agent.warm_gate) = Some(g);
        let first = h.manager.set_project_async(Some("/a"));
        handle.wait_entered();
        let second = h.manager.set_project_async(Some("/b"));
        handle.release(Ok(()));
        wait_for_job(first).unwrap();
        wait_for_job(second).unwrap();
        assert_eq!(h.agent.log(), strings(&["stop", "stop", "warm /b"]));
    }

    #[test]
    fn drops_a_failed_warm_for_a_replaced_project() {
        let h = setup(Opts::default());
        let (g, handle) = gate::<Result<(), String>>();
        *lock(&h.agent.warm_gate) = Some(g);
        let first = h.manager.set_project_async(Some("/a"));
        handle.wait_entered();
        let second = h.manager.set_project_async(Some("/b"));
        handle.release(Err("late".to_string()));
        wait_for_job(first).unwrap();
        wait_for_job(second).unwrap();
        assert_eq!(h.readiness(), ready());
    }

    #[test]
    fn drops_a_stale_result_after_the_docker_check() {
        let (g, handle) = gate::<Option<DockerProblem>>();
        let slot = Mutex::new(Some(g));
        let check: CheckDocker = Box::new(move || match take_gate(&slot) {
            Some(g) => g.pass(),
            None => None,
        });
        let h = setup(Opts {
            check_docker: Some(check),
            ..Opts::default()
        });
        let first = h.manager.set_project_async(Some("/a"));
        handle.wait_entered();
        let second = h.manager.set_project_async(None);
        handle.release(Some(docker_down()));
        wait_for_job(first).unwrap();
        wait_for_job(second).unwrap();
        assert_eq!(h.readiness(), idle());
    }

    #[test]
    fn drops_a_stale_result_after_the_key_check() {
        let (g, handle) = gate::<bool>();
        let slot = Mutex::new(Some(g));
        let has_key: HasKey = Box::new(move || match take_gate(&slot) {
            Some(g) => g.pass(),
            None => true,
        });
        let h = setup(Opts {
            has_key: Some(has_key),
            ..Opts::default()
        });
        let first = h.manager.set_project_async(Some("/a"));
        handle.wait_entered();
        let second = h.manager.set_project_async(None);
        handle.release(false);
        wait_for_job(first).unwrap();
        wait_for_job(second).unwrap();
        assert_eq!(h.readiness(), idle());
    }

    // ---- gating ----

    #[test]
    fn rejects_agent_directed_calls_until_ready() {
        let h = setup(Opts {
            key: Some(false),
            ..Opts::default()
        });
        h.manager.set_project(Some("/p")).unwrap();
        assert!(h.manager.start("go", None).unwrap_err().contains("API key"));
        assert!(h.manager.send("go").unwrap_err().contains("API key"));
        assert!(h.manager.interrupt().unwrap_err().contains("API key"));
        assert!(lock(&h.agent.starts).is_empty());
    }

    #[test]
    fn starts_on_the_warm_agent_without_restarting_it() {
        let h = setup(Opts::default());
        h.manager.set_project(Some("/p")).unwrap();
        h.manager.start("go", None).unwrap();
        assert_eq!(
            lock(&h.agent.starts).clone(),
            vec![("go".to_string(), "/p".to_string(), None)]
        );
        assert_eq!(h.agent.log(), strings(&["stop", "warm /p"]));
        assert!(h.manager.is_busy());
    }

    #[test]
    fn restarts_to_resume_a_stored_session() {
        let h = setup(Opts::default());
        h.manager.set_project(Some("/p")).unwrap();
        h.manager.start("go", Some("s1")).unwrap();
        assert_eq!(
            h.agent.log(),
            strings(&["stop", "warm /p", "stop", "warm /p s1"])
        );
        assert_eq!(
            lock(&h.agent.starts).clone(),
            vec![("go".to_string(), "/p".to_string(), Some("s1".to_string()))]
        );
    }

    #[test]
    fn restarts_for_a_new_session_once_the_warm_agent_was_used() {
        let h = setup(Opts::default());
        h.manager.set_project(Some("/p")).unwrap();
        h.manager.start("one", None).unwrap();
        h.manager.start("two", None).unwrap();
        assert_eq!(
            h.agent.log(),
            strings(&["stop", "warm /p", "stop", "warm /p"])
        );
    }

    #[test]
    fn fails_the_start_when_the_restart_does_not_leave_it_ready() {
        let h = setup(Opts::default());
        h.manager.set_project(Some("/p")).unwrap();
        lock(&h.agent.warm_errors).push_back("no".to_string());
        let err = h.manager.start("go", Some("s1")).unwrap_err();
        assert!(err.contains("no"));
    }

    #[test]
    fn clears_busy_when_start_or_send_fail() {
        let h = setup(Opts::default());
        h.manager.set_project(Some("/p")).unwrap();
        *lock(&h.agent.start_error) = Some("x".to_string());
        assert!(h.manager.start("go", None).unwrap_err().contains("x"));
        assert!(!h.manager.is_busy());
        *lock(&h.agent.send_error) = Some("y".to_string());
        assert!(h.manager.send("go").unwrap_err().contains("y"));
        assert!(!h.manager.is_busy());
    }

    #[test]
    fn sends_follow_ups_and_interrupts_when_ready() {
        let h = setup(Opts::default());
        h.manager.set_project(Some("/p")).unwrap();
        h.manager.send("more").unwrap();
        assert_eq!(lock(&h.agent.sends).clone(), strings(&["more"]));
        assert!(h.manager.is_busy());
        h.manager.interrupt().unwrap();
        assert_eq!(h.agent.interrupt_calls.load(Ordering::SeqCst), 1);
        h.emit(AgentEvent::TurnFinished);
        assert!(!h.manager.is_busy());
    }

    // ---- sessions ----

    #[test]
    fn keeps_a_never_used_warm_agent_on_new_session() {
        let h = setup(Opts::default());
        h.manager.set_project(Some("/p")).unwrap();
        h.manager.new_session().unwrap();
        assert_eq!(h.agent.warms(), 1);
    }

    #[test]
    fn replaces_a_used_agent_on_new_session() {
        let h = setup(Opts::default());
        h.manager.set_project(Some("/p")).unwrap();
        h.manager.send("x").unwrap();
        h.manager.new_session().unwrap();
        assert_eq!(h.agent.warms(), 2);
    }

    #[test]
    fn warms_again_for_a_new_session_when_the_agent_is_not_ready() {
        let h = setup(Opts {
            key: Some(false),
            ..Opts::default()
        });
        h.manager.set_project(Some("/p")).unwrap();
        h.key.store(true, Ordering::SeqCst);
        h.manager.new_session().unwrap();
        assert_eq!(h.agent.warms(), 1);
    }

    #[test]
    fn stops_the_agent_for_good_on_shutdown() {
        let h = setup(Opts::default());
        h.manager.set_project(Some("/p")).unwrap();
        h.manager.shutdown().unwrap();
        assert_eq!(h.agent.stop_calls.load(Ordering::SeqCst), 2);
        assert_eq!(h.readiness(), idle());
    }

    #[test]
    fn unsubscribes_readiness_listeners() {
        let h = setup(Opts::default());
        let seen: Arc<Mutex<Vec<Readiness>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let id = h.manager.on_readiness(Box::new(move |r: Readiness| {
            lock(&sink).push(r);
        }));
        h.manager.off_readiness(id);
        h.manager.set_project(Some("/p")).unwrap();
        assert!(lock(&seen).is_empty());
    }

    #[test]
    fn forwards_agent_events_to_subscribers() {
        let h = setup(Opts::default());
        let seen: Arc<Mutex<Vec<AgentEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let id = h.manager.on_event(Box::new(move |e: AgentEvent| {
            lock(&sink).push(e);
        }));
        h.emit(AgentEvent::TurnFinished);
        h.manager.off_event(id);
        h.emit(AgentEvent::TurnFinished);
        assert_eq!(lock(&seen).clone(), vec![AgentEvent::TurnFinished]);
    }

    // ---- crash recovery ----

    #[test]
    fn restarts_a_crashed_agent_resuming_the_stored_session() {
        let h = setup(Opts::default());
        h.manager.set_project(Some("/p")).unwrap();
        // Hold the restart's warm so the intermediate state can be observed.
        let (g, handle) = gate::<Result<(), String>>();
        *lock(&h.agent.warm_gate) = Some(g);
        h.emit(AgentEvent::SessionStarted {
            session_id: "s1".to_string(),
        });
        h.emit(AgentEvent::Exited { code: Some(1) });
        assert_eq!(
            h.readiness(),
            rd(
                ReadinessStatus::Starting,
                Some(ReadinessReason::CrashedRestarting),
                None
            )
        );
        handle.wait_entered();
        handle.release(Ok(()));
        h.wait_status(ReadinessStatus::Ready);
        assert!(h
            .states()
            .iter()
            .any(|s| s.reason == Some(ReadinessReason::CrashedRestarting)));
        // The gated warm is not logged, so check the resume through a second, logged restart.
        h.emit(AgentEvent::Exited { code: Some(1) });
        wait_until("the logged restart", || {
            h.agent.log().last().map(|l| l.as_str()) == Some("warm /p s1")
                && h.readiness().status == ReadinessStatus::Ready
        });
        let log = h.agent.log();
        assert_eq!(
            log[log.len() - 2..].to_vec(),
            strings(&["stop", "warm /p s1"])
        );
    }

    #[test]
    fn ignores_exits_the_manager_asked_for_and_exits_while_not_ready() {
        let h = setup(Opts {
            key: Some(false),
            ..Opts::default()
        });
        h.manager.set_project(Some("/p")).unwrap();
        h.emit(AgentEvent::Exited { code: Some(0) });
        assert_eq!(h.agent.warms(), 0);
    }

    #[test]
    fn does_not_treat_its_own_stop_as_a_crash() {
        let h = setup(Opts::default());
        h.agent.exit_on_stop.store(true, Ordering::SeqCst);
        h.manager.set_project(Some("/a")).unwrap();
        h.manager.set_project(Some("/b")).unwrap();
        assert_eq!(h.agent.warms(), 2);
        assert_eq!(h.readiness(), ready());
    }

    #[test]
    fn gives_up_after_repeated_crashes_without_a_finished_turn() {
        let h = setup(Opts::default());
        h.manager.set_project(Some("/p")).unwrap();
        for _ in 0..3 {
            h.emit(AgentEvent::Exited { code: Some(1) });
            h.wait_status(ReadinessStatus::Ready);
        }
        h.emit(AgentEvent::Exited { code: Some(1) });
        assert_eq!(
            h.readiness(),
            rd(
                ReadinessStatus::Error,
                Some(ReadinessReason::Error),
                Some("The agent keeps stopping unexpectedly.")
            )
        );
    }

    #[test]
    fn resets_the_crash_count_when_a_turn_finishes() {
        let h = setup(Opts::default());
        h.manager.set_project(Some("/p")).unwrap();
        for _ in 0..5 {
            h.emit(AgentEvent::Exited { code: Some(1) });
            h.wait_status(ReadinessStatus::Ready);
            h.emit(AgentEvent::TurnFinished);
        }
        assert_eq!(h.readiness().status, ReadinessStatus::Ready);
    }

    // ---- queue ----

    #[test]
    fn keeps_processing_after_a_transition_fails() {
        let h = setup(Opts::default());
        *lock(&h.agent.stop_error) = Some("stuck".to_string());
        assert!(h
            .manager
            .set_project(Some("/a"))
            .unwrap_err()
            .contains("stuck"));
        h.manager.set_project(Some("/b")).unwrap();
        assert_eq!(h.readiness(), ready());
    }

    // ---- image build ----

    #[test]
    fn builds_a_missing_image_reports_progress_and_then_warms() {
        let built = Arc::new(AtomicBool::new(false));
        let built_for_build = built.clone();
        let build = build_with(move |on_progress: &dyn Fn(&str)| {
            on_progress("step 1");
            built_for_build.store(true, Ordering::SeqCst);
            BuildOutcome::Built
        });
        let check: CheckDocker = Box::new(move || {
            if built.load(Ordering::SeqCst) {
                None
            } else {
                Some(missing())
            }
        });
        let h = setup(Opts {
            check_docker: Some(check),
            build_image: Some(build),
            ..Opts::default()
        });
        h.manager.set_project(Some("/p")).unwrap();
        let states = h.states();
        let reasons: Vec<Option<ReadinessReason>> = states.iter().map(|s| s.reason).collect();
        assert_eq!(
            reasons,
            vec![
                Some(ReadinessReason::BuildingImage),
                Some(ReadinessReason::BuildingImage),
                Some(ReadinessReason::Starting),
                None
            ]
        );
        assert_eq!(
            states[1].message,
            Some("Building the sandbox image… step 1".to_string())
        );
        assert_eq!(h.agent.warms(), 1);
    }

    #[test]
    fn reports_a_failed_build() {
        let check: CheckDocker = Box::new(|| Some(missing()));
        let h = setup(Opts {
            check_docker: Some(check),
            build_image: Some(build_with(|_: &dyn Fn(&str)| {
                BuildOutcome::Failed("build failed".to_string())
            })),
            ..Opts::default()
        });
        h.manager.set_project(Some("/p")).unwrap();
        assert_eq!(
            h.readiness(),
            rd(
                ReadinessStatus::Error,
                Some(ReadinessReason::Error),
                Some("build failed")
            )
        );
        assert_eq!(h.agent.warms(), 0);
    }

    #[test]
    fn keeps_the_original_problem_when_the_app_cannot_build_the_image() {
        let check: CheckDocker = Box::new(|| Some(missing()));
        let h = setup(Opts {
            check_docker: Some(check),
            build_image: Some(build_with(|_: &dyn Fn(&str)| BuildOutcome::Unsupported)),
            ..Opts::default()
        });
        h.manager.set_project(Some("/p")).unwrap();
        assert_eq!(
            h.readiness(),
            rd(
                ReadinessStatus::Error,
                Some(ReadinessReason::Error),
                Some("image is missing")
            )
        );
    }

    #[test]
    fn does_not_build_when_there_is_no_builder() {
        let check: CheckDocker = Box::new(|| Some(missing()));
        let h = setup(Opts {
            check_docker: Some(check),
            ..Opts::default()
        });
        h.manager.set_project(Some("/p")).unwrap();
        assert_eq!(h.readiness().message, Some("image is missing".to_string()));
    }

    #[test]
    fn stops_when_the_project_changes_during_the_build() {
        let (g, handle) = gate::<()>();
        let slot = Mutex::new(Some(g));
        let build = build_with(move |on_progress: &dyn Fn(&str)| {
            if let Some(g) = take_gate(&slot) {
                g.pass();
            }
            on_progress("late line");
            BuildOutcome::Built
        });
        let check: CheckDocker = Box::new(|| Some(missing()));
        let h = setup(Opts {
            check_docker: Some(check),
            build_image: Some(build),
            ..Opts::default()
        });
        let first = h.manager.set_project_async(Some("/a"));
        handle.wait_entered();
        let second = h.manager.set_project_async(None);
        handle.release(());
        wait_for_job(first).unwrap();
        wait_for_job(second).unwrap();
        assert_eq!(h.readiness(), idle());
        assert!(!h
            .states()
            .iter()
            .any(|s| s.message == Some("Building the sandbox image… late line".to_string())));
        assert_eq!(h.agent.warms(), 0);
    }

    #[test]
    fn stops_when_the_project_changes_after_the_rebuild_check() {
        let (g, handle) = gate::<Option<DockerProblem>>();
        let slot = Mutex::new(Some(g));
        let calls = Arc::new(AtomicUsize::new(0));
        let check: CheckDocker = Box::new(move || {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                return Some(missing());
            }
            match take_gate(&slot) {
                Some(g) => g.pass(),
                None => None,
            }
        });
        let h = setup(Opts {
            check_docker: Some(check),
            build_image: Some(build_with(|_: &dyn Fn(&str)| BuildOutcome::Built)),
            ..Opts::default()
        });
        let first = h.manager.set_project_async(Some("/a"));
        handle.wait_entered();
        let second = h.manager.set_project_async(None);
        handle.release(None);
        wait_for_job(first).unwrap();
        wait_for_job(second).unwrap();
        assert_eq!(h.readiness(), idle());
        assert_eq!(h.agent.warms(), 0);
    }

    // ---- slash commands ----

    #[test]
    fn keeps_the_latest_slash_commands_and_clears_them_when_the_agent_exits() {
        let h = setup(Opts::default());
        h.manager.set_project(Some("/p")).unwrap();
        assert!(h.manager.slash_commands().is_empty());
        let commands = vec![SlashCommandInfo {
            name: "init".to_string(),
            description: "Set up".to_string(),
            argument_hint: String::new(),
            aliases: None,
        }];
        h.emit(AgentEvent::Commands {
            commands: commands.clone(),
        });
        assert_eq!(h.manager.slash_commands(), commands);
        h.emit(AgentEvent::Exited { code: Some(0) });
        assert!(h.manager.slash_commands().is_empty());
        // wait out the crash restart this exit triggers while ready
        h.wait_status(ReadinessStatus::Ready);
    }
}
