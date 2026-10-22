//! Bounded background reconciliation for journaled runner completion.

mod cleanup;
mod diagnostics;
mod journal_ref;
mod reaper;

pub(in crate::launch) use cleanup::remove_owned_resources_for_retirement;

#[cfg(test)]
pub(crate) use cleanup::reconcile_for_test;

use reaper::ReapTask;
#[cfg(test)]
use reaper::Reaper;
use reaper::{defer_join, ensure_started as ensure_reaper_started, wake};

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use bollard::Docker;
use tokio::sync::oneshot;
use zeroize::Zeroize;

use crate::docker_client::DOCKER_OPERATION_TIMEOUT;
use crate::error::HostError;
use crate::https::HttpsTransport;
use crate::journal::ConnectionJournal;
use crate::launch::SelectedEngineContext;
use crate::launch::bootstrap::NativeCleanupRuntime;
use crate::listen::Secret;

const CLEANUP_DOCKER_REQUEST_TIMEOUT: Duration = DOCKER_OPERATION_TIMEOUT;

#[cfg(test)]
#[path = "completion/worker_tests.rs"]
mod worker_tests;

pub(super) struct Resources {
    pub(super) journal: journal_ref::CompletionJournal,
    pub(super) docker: Docker,
    pub(super) transport: HttpsTransport,
    pub(super) admin: Secret,
    pub(super) diagnostics: diagnostics::DiagnosticsRoot,
    pub(super) native_cleanup: NativeCleanupRuntime,
    pub(super) selected_engine: SelectedEngineContext,
}

struct StartWithJournalInput<'a> {
    journal: journal_ref::CompletionJournal,
    connection_id: &'a str,
    docker: Docker,
    admin_base: &'a str,
    admin_token: String,
    state: &'a Path,
    native_cleanup: NativeCleanupRuntime,
    selected_engine: SelectedEngineContext,
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

impl CompletionWorker {
    /// Start cleanup for one repository connection's completion rows.
    pub(crate) fn start_scoped(
        journal: ConnectionJournal,
        native_cleanup: NativeCleanupRuntime,
        docker: Docker,
        selected: SelectedEngineContext,
        admin_base: &str,
        admin_token: String,
        state: &Path,
    ) -> Result<Self, HostError> {
        let connection_id = journal.connection_id().as_str().to_owned();
        Self::start_with_journal(StartWithJournalInput {
            journal: journal_ref::CompletionJournal::scoped(journal),
            connection_id: &connection_id,
            docker,
            admin_base,
            admin_token,
            state,
            native_cleanup,
            selected_engine: selected,
        })
    }

    fn start_with_journal(input: StartWithJournalInput<'_>) -> Result<Self, HostError> {
        let StartWithJournalInput {
            journal,
            connection_id,
            docker,
            admin_base,
            admin_token,
            state,
            native_cleanup,
            selected_engine,
        } = input;
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
        let journal_identity = journal.completion_state_identity();
        let resources = Resources {
            diagnostics: diagnostics::DiagnosticsRoot::open_for_journal(
                state,
                journal_identity,
                connection_id,
            )?,
            journal,
            docker: bounded_docker(docker),
            transport,
            admin,
            native_cleanup,
            selected_engine,
        };
        ensure_reaper_started()?;
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
