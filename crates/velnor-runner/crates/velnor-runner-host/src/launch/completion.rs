//! Bounded background reconciliation for journaled runner completion.

mod cleanup;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use bollard::Docker;
use tokio::sync::oneshot;
use zeroize::Zeroize;

use crate::docker_client::DOCKER_OPERATION_TIMEOUT;
use crate::error::HostError;
use crate::https::HttpsTransport;
use crate::journal::Journal;
use crate::listen::Secret;

const CLEANUP_DOCKER_REQUEST_TIMEOUT: Duration = DOCKER_OPERATION_TIMEOUT;

#[cfg(test)]
#[path = "completion/worker_tests.rs"]
mod worker_tests;

pub(super) struct Resources {
    pub(super) journal: Journal,
    pub(super) docker: Docker,
    pub(super) transport: HttpsTransport,
    pub(super) admin: Secret,
}

/// One independently scheduled completion cleanup worker.
pub(crate) struct CompletionWorker {
    wake: SyncSender<()>,
    stopping: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    finished: Option<oneshot::Receiver<Result<(), HostError>>>,
    #[cfg(test)]
    reaped: Option<mpsc::Sender<()>>,
}

struct ReapTask {
    thread: JoinHandle<()>,
    finished: Option<oneshot::Receiver<Result<(), HostError>>>,
    joined: Option<oneshot::Sender<Result<(), HostError>>>,
    #[cfg(test)]
    reaped: Option<mpsc::Sender<()>>,
}

struct Reaper {
    queue: Arc<ReapQueue>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

struct ReapQueue {
    tasks: Mutex<VecDeque<ReapTask>>,
    wake: Condvar,
}

static REAPER: OnceLock<Reaper> = OnceLock::new();

impl CompletionWorker {
    /// Start the bounded completer on its own thread and runtime.
    pub(crate) fn start(
        journal: Journal,
        docker: Docker,
        admin_base: &str,
        admin_token: String,
    ) -> Result<Self, HostError> {
        if admin_token.is_empty() {
            return Err(HostError::EmptySecret);
        }
        let transport = HttpsTransport::new(admin_base)?;
        let mut token = admin_token;
        let admin = Secret::new(&token);
        token.zeroize();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| HostError::Journal)?;
        let stopping = Arc::new(AtomicBool::new(false));
        let thread_stopping = Arc::clone(&stopping);
        let (wake, receiver) = mpsc::sync_channel(1);
        let (finished_tx, finished) = oneshot::channel();
        let resources = Resources {
            journal,
            docker: bounded_docker(docker),
            transport,
            admin,
        };
        worker_reaper().ensure_started()?;
        let thread = thread::Builder::new()
            .name("velnor-completion-reconciler".to_owned())
            .spawn(move || {
                completion_thread(
                    &runtime,
                    resources,
                    &thread_stopping,
                    &receiver,
                    finished_tx,
                );
            })
            .map_err(|_| HostError::Journal)?;
        Ok(Self {
            wake,
            stopping,
            thread: Some(thread),
            finished: Some(finished),
            #[cfg(test)]
            reaped: None,
        })
    }

    /// Coalesce a wake request; the periodic scan also recovers missed wakes.
    pub(crate) fn notify(&self) {
        wake(&self.wake);
    }

    /// Stop the worker and await its bounded in-flight cleanup attempt.
    pub(crate) async fn shutdown(mut self) -> Result<(), HostError> {
        self.request_stop();
        let finished = match self.finished.as_mut() {
            Some(receiver) => (&mut *receiver).await.map_err(|_| HostError::Journal),
            None => Err(HostError::Journal),
        };
        drop(self.finished.take());
        let joined = match self.thread.take() {
            Some(thread) => {
                let (joined_tx, joined_rx) = oneshot::channel();
                let dispatch = defer_join(ReapTask {
                    thread,
                    finished: None,
                    joined: Some(joined_tx),
                    #[cfg(test)]
                    reaped: self.reaped.take(),
                });
                match dispatch {
                    Ok(()) => match joined_rx.await {
                        Ok(result) => result,
                        Err(_) => Err(HostError::Journal),
                    },
                    Err(error) => Err(error),
                }
            }
            None => Err(HostError::Journal),
        };
        match (finished, joined) {
            (Ok(Err(error)) | Err(error), _) | (Ok(Ok(())), Err(error)) => Err(error),
            (Ok(Ok(())), Ok(())) => Ok(()),
        }
    }

    fn request_stop(&self) {
        self.stopping.store(true, Ordering::Release);
        wake(&self.wake);
    }
}

fn bounded_docker(docker: Docker) -> Docker {
    docker.with_timeout(CLEANUP_DOCKER_REQUEST_TIMEOUT)
}

impl Drop for CompletionWorker {
    fn drop(&mut self) {
        self.request_stop();
        if let Some(thread) = self.thread.take()
            && let Err(error) = defer_join(ReapTask {
                thread,
                finished: self.finished.take(),
                joined: None,
                #[cfg(test)]
                reaped: self.reaped.take(),
            })
        {
            eprintln!("completion worker failed phase=join_dispatch reason={error}");
        }
    }
}

fn worker_reaper() -> &'static Reaper {
    REAPER.get_or_init(Reaper::new)
}

impl Reaper {
    fn new() -> Self {
        Self {
            queue: Arc::new(ReapQueue {
                tasks: Mutex::new(VecDeque::new()),
                wake: Condvar::new(),
            }),
            thread: Mutex::new(None),
        }
    }

    fn ensure_started(&self) -> Result<(), HostError> {
        self.ensure_started_with(spawn_reaper_thread)
    }

    fn ensure_started_with(
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

    fn enqueue(&self, task: ReapTask) {
        let mut tasks = self
            .queue
            .tasks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        tasks.push_back(task);
        self.queue.wake.notify_one();
    }

    #[cfg(test)]
    fn pending_count(&self) -> usize {
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

fn defer_join(task: ReapTask) -> Result<(), HostError> {
    let reaper = worker_reaper();
    // Store the worker handle before any fallible thread creation. If the
    // reaper cannot start, the queue keeps ownership for a later retry.
    reaper.enqueue(task);
    reaper.ensure_started()
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

fn wake(sender: &SyncSender<()>) {
    match sender.try_send(()) {
        Ok(()) | Err(TrySendError::Full(()) | TrySendError::Disconnected(())) => {}
    }
}

fn completion_thread(
    runtime: &tokio::runtime::Runtime,
    resources: Resources,
    stopping: &Arc<AtomicBool>,
    receiver: &mpsc::Receiver<()>,
    finished: oneshot::Sender<Result<(), HostError>>,
) {
    let result = cleanup::run(runtime, resources, stopping, receiver);
    if let Err(Err(error)) = finished.send(result) {
        eprintln!("completion worker failed phase=shutdown reason={error}");
    }
}
