//! Bounded background reconciliation for journaled runner completion.

mod cleanup;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
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
}

impl CompletionWorker {
    /// Start the bounded completer on its own thread and runtime.
    pub(crate) fn start(
        journal: Journal,
        docker: Docker,
        admin_base: String,
        admin_token: String,
    ) -> Result<Self, HostError> {
        if admin_token.is_empty() {
            return Err(HostError::EmptySecret);
        }
        let transport = HttpsTransport::new(&admin_base)?;
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
        let thread = thread::Builder::new()
            .name("velnor-completion-reconciler".to_owned())
            .spawn(move || {
                completion_thread(runtime, resources, thread_stopping, receiver, finished_tx)
            })
            .map_err(|_| HostError::Journal)?;
        Ok(Self {
            wake,
            stopping,
            thread: Some(thread),
            finished: Some(finished),
        })
    }

    /// Coalesce a wake request; the periodic scan also recovers missed wakes.
    pub(crate) fn notify(&self) {
        wake(&self.wake);
    }

    /// Stop the worker and await its bounded in-flight cleanup attempt.
    pub(crate) async fn shutdown(mut self) -> Result<(), HostError> {
        self.request_stop();
        let finished = match self.finished.take() {
            Some(receiver) => receiver.await.map_err(|_| HostError::Journal),
            None => Err(HostError::Journal),
        };
        let joined = match self.thread.take() {
            Some(thread) => thread.join().map_err(|_| HostError::Journal),
            None => Err(HostError::Journal),
        };
        match (finished, joined) {
            (Ok(Err(error)), _) | (Err(error), _) => Err(error),
            (Ok(Ok(())), Err(error)) => Err(error),
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
        if let Some(thread) = self.thread.take() {
            match thread.join() {
                Ok(()) => self.report_unobserved_result(),
                Err(_) => eprintln!("completion worker failed phase=join reason=panic"),
            }
        }
    }
}

impl CompletionWorker {
    fn report_unobserved_result(&mut self) {
        let Some(finished) = self.finished.as_mut() else {
            return;
        };
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
}

fn wake(sender: &SyncSender<()>) {
    match sender.try_send(()) {
        Ok(()) | Err(TrySendError::Full(())) | Err(TrySendError::Disconnected(())) => {}
    }
}

fn completion_thread(
    runtime: tokio::runtime::Runtime,
    resources: Resources,
    stopping: Arc<AtomicBool>,
    receiver: mpsc::Receiver<()>,
    finished: oneshot::Sender<Result<(), HostError>>,
) {
    let result = cleanup::run(runtime, resources, stopping, receiver);
    if let Err(Err(error)) = finished.send(result) {
        eprintln!("completion worker failed phase=shutdown reason={error}");
    }
}
