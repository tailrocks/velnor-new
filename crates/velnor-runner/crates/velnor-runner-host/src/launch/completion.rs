// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod cleanup;
mod diagnostics;
mod journal_ref;
mod reaper;

pub(in crate::launch) use cleanup::{
    remove_owned_resources_for_jit_conflict, remove_owned_resources_for_retirement,
};

use reaper::ReapTask;

use reaper::{defer_join, ensure_started as ensure_reaper_started, wake};

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use bollard::Docker;
use tokio::sync::oneshot;
use zeroize::Zeroizing;

use crate::completion_admin::CompletionAdminClient;
use crate::docker_client::DOCKER_OPERATION_TIMEOUT;
use crate::error::HostError;
use crate::journal::ConnectionJournal;
use crate::launch::SelectedEngineContext;
use crate::launch::bootstrap::NativeCleanupRuntime;

const CLEANUP_DOCKER_REQUEST_TIMEOUT: Duration = DOCKER_OPERATION_TIMEOUT;

pub(super) struct Resources {
    pub(super) journal: journal_ref::CompletionJournal,
    pub(super) docker: Docker,
    pub(super) admin: CompletionAdminClient,
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

#[derive(Debug)]
pub struct CompletionWorker {
    admin: CompletionAdminClient,
    wake: SyncSender<()>,
    stopping: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    finished: Option<oneshot::Receiver<Result<(), HostError>>>,
}

impl CompletionWorker {
    pub async fn start_for_connection(
        journal: &crate::journal::Journal,
        binding: &crate::config::RepositoryConnection,
        docker: Docker,
        admin_base: &str,
        admin_token: String,
        release_read_token: &str,
        state: &Path,
    ) -> Result<Self, HostError> {
        let scope = journal.existing_connection_scope(binding).await?;
        let engine_context = scope.engine_context().await?.ok_or(HostError::Ownership)?;
        let bounded = bounded_docker(docker);
        let info = tokio::time::timeout(CLEANUP_DOCKER_REQUEST_TIMEOUT, bounded.info())
            .await
            .map_err(|_| HostError::Docker)?
            .map_err(|_| HostError::Docker)?;
        let selected = SelectedEngineContext::capture(&engine_context, &info)?;
        journal
            .ensure_image_imports_resolved(selected.engine_id())
            .await?;
        let release_secret = Arc::new(crate::launch::ReleaseReadSecret::parse(release_read_token)?);
        let native_cleanup = NativeCleanupRuntime::new(
            bounded.clone(),
            journal.clone(),
            engine_context,
            selected.clone(),
            release_secret,
        )?;
        Self::start_scoped(
            scope,
            native_cleanup,
            bounded,
            selected,
            admin_base,
            admin_token,
            state,
        )
    }

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
        let token = Zeroizing::new(admin_token);
        let admin = CompletionAdminClient::new(admin_base, &token)?;
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
            admin: admin.clone(),
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
            admin,
            wake,
            stopping,
            thread: Some(thread),
            finished: Some(finished),
        })
    }

    pub(crate) fn refresh_admin(
        &self,
        admin_base: &str,
        admin_token: &str,
    ) -> Result<(), HostError> {
        self.admin.refresh(admin_base, admin_token)
    }

    pub fn notify(&self) {
        wake(&self.wake);
    }

    pub async fn shutdown(mut self) -> Result<(), HostError> {
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
