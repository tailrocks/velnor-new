//! Own sampler-thread joins outside Tokio's blocking-pool shutdown path.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::thread::{self, JoinHandle};

use tokio::sync::watch;

use super::super::GuestSampleFailure;

pub(super) struct SamplerThreadJoinGuard {
    thread: Option<JoinHandle<Result<(), GuestSampleFailure>>>,
    task_failure: watch::Sender<Option<GuestSampleFailure>>,
}

impl SamplerThreadJoinGuard {
    pub(super) fn new(
        thread: JoinHandle<Result<(), GuestSampleFailure>>,
        task_failure: watch::Sender<Option<GuestSampleFailure>>,
    ) -> Self {
        Self {
            thread: Some(thread),
            task_failure,
        }
    }

    pub(super) fn join(mut self) -> Result<(), GuestSampleFailure> {
        let result = match self.thread.take() {
            Some(thread) => join_sampler_thread(thread),
            None => Err(GuestSampleFailure::SamplerTask),
        };
        if let Err(reason) = result {
            let _ = self.task_failure.send_replace(Some(reason));
        }
        result
    }
}

impl Drop for SamplerThreadJoinGuard {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let request = SamplerJoinRequest {
                thread,
                task_failure: self.task_failure.clone(),
            };
            if let Err(reason) = sampler_join_reaper().enqueue(request) {
                let _ = self.task_failure.send_replace(Some(reason));
            }
        }
    }
}

struct SamplerJoinRequest {
    thread: JoinHandle<Result<(), GuestSampleFailure>>,
    task_failure: watch::Sender<Option<GuestSampleFailure>>,
}

impl SamplerJoinRequest {
    fn join(self) {
        if let Err(reason) = join_sampler_thread(self.thread) {
            let _ = self.task_failure.send_replace(Some(reason));
        }
    }
}

struct SamplerJoinReaper {
    queue: Arc<SamplerJoinQueue>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

struct SamplerJoinQueue {
    state: Mutex<SamplerJoinState>,
    ready: Condvar,
    #[cfg(test)]
    wait_hook: Mutex<Option<(Arc<std::sync::Barrier>, Arc<std::sync::Barrier>)>>,
}

struct SamplerJoinState {
    requests: VecDeque<SamplerJoinRequest>,
    stopping: bool,
}

impl SamplerJoinReaper {
    fn new() -> Self {
        Self {
            queue: Arc::new(SamplerJoinQueue {
                state: Mutex::new(SamplerJoinState {
                    requests: VecDeque::new(),
                    stopping: false,
                }),
                ready: Condvar::new(),
                #[cfg(test)]
                wait_hook: Mutex::new(None),
            }),
            worker: Mutex::new(None),
        }
    }

    fn enqueue(&self, request: SamplerJoinRequest) -> Result<(), GuestSampleFailure> {
        self.enqueue_with(request, spawn_join_worker)
    }

    fn enqueue_with(
        &self,
        request: SamplerJoinRequest,
        spawn_worker: impl FnOnce(Arc<SamplerJoinQueue>) -> Result<JoinHandle<()>, GuestSampleFailure>,
    ) -> Result<(), GuestSampleFailure> {
        let mut state = lock_queue(&self.queue.state);
        state.requests.push_back(request);
        self.queue.ready.notify_one();
        drop(state);

        self.ensure_worker_with(spawn_worker)
    }

    fn ensure_worker(&self) -> Result<(), GuestSampleFailure> {
        self.ensure_worker_with(spawn_join_worker)
    }

    fn ensure_worker_with(
        &self,
        spawn_worker: impl FnOnce(Arc<SamplerJoinQueue>) -> Result<JoinHandle<()>, GuestSampleFailure>,
    ) -> Result<(), GuestSampleFailure> {
        let mut worker = match self.worker.lock() {
            Ok(worker) => worker,
            Err(poisoned) => poisoned.into_inner(),
        };
        if worker.as_ref().is_some_and(|worker| !worker.is_finished()) {
            return Ok(());
        }
        if lock_queue(&self.queue.state).stopping {
            return Err(GuestSampleFailure::SamplerTask);
        }
        if let Some(finished) = worker.take()
            && finished.join().is_err()
        {
            return Err(GuestSampleFailure::SamplerTask);
        }

        let thread = spawn_worker(Arc::clone(&self.queue))?;
        *worker = Some(thread);
        Ok(())
    }

    #[cfg(test)]
    fn shutdown(&self) -> Result<(), GuestSampleFailure> {
        let mut state = lock_queue(&self.queue.state);
        state.stopping = true;
        self.queue.ready.notify_all();
        drop(state);

        let mut worker = lock_queue(&self.worker);
        match worker.take() {
            Some(worker) => worker.join().map_err(|_| GuestSampleFailure::SamplerTask),
            None => Ok(()),
        }
    }

    #[cfg(test)]
    fn set_wait_hook(&self, reached: Arc<std::sync::Barrier>, resume: Arc<std::sync::Barrier>) {
        *lock_queue(&self.queue.wait_hook) = Some((reached, resume));
    }
}

pub(super) fn ensure_sampler_join_reaper() -> Result<(), GuestSampleFailure> {
    sampler_join_reaper().ensure_worker()
}

fn sampler_join_reaper() -> &'static SamplerJoinReaper {
    static REAPER: OnceLock<SamplerJoinReaper> = OnceLock::new();
    REAPER.get_or_init(SamplerJoinReaper::new)
}

fn spawn_join_worker(queue: Arc<SamplerJoinQueue>) -> Result<JoinHandle<()>, GuestSampleFailure> {
    thread::Builder::new()
        .name("velnor-guest-sampler-joiner".to_owned())
        .spawn(move || sampler_join_loop(queue))
        .map_err(|_| GuestSampleFailure::RuntimeUnavailable)
}

fn sampler_join_loop(queue: Arc<SamplerJoinQueue>) {
    loop {
        let request = {
            let mut state = lock_queue(&queue.state);
            loop {
                if let Some(request) = state.requests.pop_front() {
                    break request;
                }
                if state.stopping {
                    return;
                }
                #[cfg(test)]
                wait_at_test_boundary(&queue);
                state = match queue.ready.wait(state) {
                    Ok(state) => state,
                    Err(poisoned) => poisoned.into_inner(),
                };
            }
        };
        request.join();
    }
}

fn lock_queue<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
fn wait_at_test_boundary(queue: &SamplerJoinQueue) {
    let hook = lock_queue(&queue.wait_hook).take();
    if let Some((reached, resume)) = hook {
        reached.wait();
        resume.wait();
    }
}

fn join_sampler_thread(
    thread: JoinHandle<Result<(), GuestSampleFailure>>,
) -> Result<(), GuestSampleFailure> {
    match thread.join() {
        Ok(result) => result,
        Err(_) => Err(GuestSampleFailure::SamplerTask),
    }
}

#[cfg(test)]
#[path = "reaper_tests.rs"]
mod tests;
