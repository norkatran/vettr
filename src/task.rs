//! Running blocking work off the UI thread and polling its result from egui frames.
//!
//! `Task::spawn` runs a closure on a new thread. When it finishes the result is stored and the
//! egui context is asked to repaint, so the next frame can `poll` it.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// A unit of background work with a result the UI picks up once.
pub struct Task<T> {
    slot: Arc<Mutex<Option<T>>>,
    finished: Arc<AtomicBool>,
}

impl<T: Send + 'static> Task<T> {
    /// Run `f` on a worker thread. The result is available from `poll` once it is done.
    pub fn spawn<F>(ctx: &egui::Context, f: F) -> Task<T>
    where
        F: FnOnce() -> T + Send + 'static,
    {
        let slot: Arc<Mutex<Option<T>>> = Arc::new(Mutex::new(None));
        let finished = Arc::new(AtomicBool::new(false));
        let slot_for_thread = slot.clone();
        let finished_for_thread = finished.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            // A panic must not leave the task running forever; it just yields no result.
            let outcome = catch_unwind(AssertUnwindSafe(f));
            if let Ok(value) = outcome {
                let mut guard = match slot_for_thread.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                *guard = Some(value);
            }
            finished_for_thread.store(true, Ordering::SeqCst);
            ctx.request_repaint();
        });
        Task { slot, finished }
    }

    /// The result, once, after the work has finished; `None` while it runs (or after it was taken).
    pub fn poll(&mut self) -> Option<T> {
        let mut guard = match self.slot.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.take()
    }

    /// Whether the work has not finished yet.
    pub fn is_running(&self) -> bool {
        !self.finished.load(Ordering::SeqCst)
    }
}

/// Run `f` on a worker thread and ignore its result (then repaint, in case it changed state).
pub fn fire_and_forget<F>(ctx: &egui::Context, f: F)
where
    F: FnOnce() + Send + 'static,
{
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let _ = catch_unwind(AssertUnwindSafe(f));
        ctx.request_repaint();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait_until_done<T: Send + 'static>(task: &mut Task<T>) -> Option<T> {
        let start = Instant::now();
        while task.is_running() && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
        task.poll()
    }

    #[test]
    fn a_task_yields_its_result_once() {
        let ctx = egui::Context::default();
        let mut task = Task::spawn(&ctx, || 40 + 2);
        assert_eq!(wait_until_done(&mut task), Some(42));
        assert_eq!(task.poll(), None);
        assert!(!task.is_running());
    }

    #[test]
    fn a_panicking_task_finishes_without_a_result() {
        let ctx = egui::Context::default();
        let mut task: Task<i32> = Task::spawn(&ctx, || panic!("boom"));
        assert_eq!(wait_until_done(&mut task), None);
        assert!(!task.is_running());
    }
}
