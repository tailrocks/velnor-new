//! This module waits for completion workers to finish.

use std::collections::VecDeque;
#[cfg(test)]
use std::sync::mpsc;
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::{self, JoinHandle};

use tokio::sync::oneshot;

use crate::error::HostError;

pub(super) struct ReapTask {
    pub(super) thread: JoinHandle<()>,
    pub(super) finished: Option<oneshot::Receiver<Result<(), HostError>>>,
    pub(super) joined: Option<oneshot::Sender<Result<(), HostError>>>,
    #[cfg(test)]
    pub(super) reaped: Option<mpsc::Sender<()>>,
}

pub(super) struct Reaper {
    queue: Arc<ReapQueue>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

pub(super) struct ReapQueue {
    tasks: Mutex<VecDeque<ReapTask>>,
    wake: Condvar,
}

static REAPER: OnceLock<Reaper> = OnceLock::new();

pub(super) fn ensure_started() -> Result<(), HostError> {
    worker_reaper().ensure_started()
}

pub(super) fn defer_join(task: ReapTask) -> Result<(), HostError> {
    let reaper = worker_reaper();
    // Store the worker handle before fallible thread creation. If the reaper
    // cannot start, the queue keeps ownership for a later retry.
    reaper.enqueue(task);
    reaper.ensure_started()
}

pub(super) fn wake(sender: &SyncSender<()>) {
    match sender.try_send(()) {
        Ok(()) | Err(TrySendError::Full(()) | TrySendError::Disconnected(())) => {}
    }
}

fn worker_reaper() -> &'static Reaper {
    REAPER.get_or_init(Reaper::new)
}

impl Reaper {
    pub(super) fn new() -> Self {
        Self {
            queue: Arc::new(ReapQueue {
                tasks: Mutex::new(VecDeque::new()),
                wake: Condvar::new(),
            }),
            thread: Mutex::new(None),
        }
    }

    pub(super) fn ensure_started(&self) -> Result<(), HostError> {
        self.ensure_started_with(spawn_reaper_thread)
    }

    pub(super) fn ensure_started_with(
        &self,
        spawn: impl FnOnce(Arc<ReapQueue>) -> Result<JoinHandle<()>, HostError>,
    ) -> Result<(), HostError> {
        let mut thread = self
            .thread
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if thread.as_ref().is_some_and(|handle| !handle.is_finished()) {
            return Ok(());
        }
        if let Some(finished) = thread.take()
            && finished.join().is_err()
        {
            eprintln!("completion worker failed phase=reaper reason=panic");
        }
        *thread = Some(spawn(Arc::clone(&self.queue))?);
        Ok(())
    }

    pub(super) fn enqueue(&self, task: ReapTask) {
        let mut tasks = self
            .queue
            .tasks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        tasks.push_back(task);
        self.queue.wake.notify_one();
    }

    #[cfg(test)]
    pub(super) fn pending_count(&self) -> usize {
        self.queue
            .tasks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }
}

fn spawn_reaper_thread(queue: Arc<ReapQueue>) -> Result<JoinHandle<()>, HostError> {
    thread::Builder::new()
        .name("velnor-completion-reaper".to_owned())
        .spawn(move || reap_loop(&queue))
        .map_err(|_| HostError::Journal)
}

fn reap_loop(queue: &Arc<ReapQueue>) {
    loop {
        let task = next_reap_task(queue);
        reap_task(task);
    }
}

fn next_reap_task(queue: &ReapQueue) -> ReapTask {
    let mut tasks = queue
        .tasks
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    loop {
        if let Some(task) = tasks.pop_front() {
            return task;
        }
        tasks = queue
            .wake
            .wait(tasks)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
    }
}

fn reap_task(task: ReapTask) {
    let joined = task.thread.join().map_err(|_| HostError::Journal);
    if joined.is_err() {
        eprintln!("completion worker failed phase=join reason=panic");
    }
    if let Some(mut finished) = task.finished {
        match finished.try_recv() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                eprintln!("completion worker failed phase=shutdown reason={error}");
            }
            Err(oneshot::error::TryRecvError::Empty | oneshot::error::TryRecvError::Closed) => {
                eprintln!("completion worker failed phase=shutdown reason=missing_result");
            }
        }
    }
    if let Some(joined_tx) = task.joined
        && let Err(Err(error)) = joined_tx.send(joined)
    {
        eprintln!("completion worker failed phase=join reason={error}");
    }
    #[cfg(test)]
    if let Some(reaped) = task.reaped
        && reaped.send(()).is_err()
    {
        eprintln!("completion worker test observer disconnected phase=join");
    }
}
