//! Per-connection live session and execution ownership.

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use super::PendingAcquiredRecovery;
use cursor::record_assigned_total;
#[path = "state/cleanup.rs"]
mod cleanup;
#[path = "state/cursor.rs"]
mod cursor;
use bollard::Docker;
use velnor_runner_github::{QueueSession, RunnerGroup};

use crate::{
    config::{HostConfig, RepositoryConnection},
    controller_ipc::SetupReceiptInput,
    error::HostError,
    journal::{ConnectionJournal, Journal},
    launch::{
        CompletionWorker, ReleaseReadSecret, SelectedEngineContext, VerifiedWorkerImages,
        scoped_runtime::{
            AuthorizedScopedExecution, PendingScopedExecution, RunnerReadyBootstrap,
            RunnerReadyRegistry, ScopedChannelOutcome,
        },
    },
    listen::{Link, Secret},
    scale_set::EnsuredSet,
    worker::guest_image::GuestImageFile,
};

pub(super) struct PreparedConnection {
    pub(super) binding: RepositoryConnection,
    pub(super) scope: ConnectionJournal,
    pub(super) set_id: i64,
    pub(super) setup: PreparedSetup,
    pub(super) session_intent: i64,
    /// Assigned job count for this `QueueSession`.
    pub(super) assigned_total: Option<i64>,
    /// Exclusive queue cursor while an unacknowledged head is deferred.
    pub(super) poll_cursor: i64,
    /// Time after which the deferred head should be retried from cursor zero.
    pub(super) poll_retry_at: Option<Instant>,
    pub(super) guest_image: GuestImageFile,
    pub(super) completion: Option<CompletionWorker>,
    pub(super) credentials: Option<SessionCredentials>,
    pub(super) runner_ready: RunnerReadyRegistry,
    pub(super) ready_intents: BTreeMap<i64, i64>,
    pub(super) authorizing: BTreeMap<
        i64,
        tokio::task::JoinHandle<Result<AuthorizedScopedExecution, Box<PendingScopedExecution>>>,
    >,
    pub(super) pending_bootstrap: BTreeMap<i64, Vec<ScopedChannelOutcome>>,
    pub(super) unkeyed_bootstrap: Vec<ScopedChannelOutcome>,
    pub(super) authorized: BTreeMap<i64, Vec<AuthorizedScopedExecution>>,
    pub(super) pending_authorization: BTreeMap<i64, Vec<PendingScopedExecution>>,
    pub(super) unkeyed_authorization: Vec<PendingScopedExecution>,
    pub(super) pending_acquired_recovery: BTreeMap<i64, PendingAcquiredRecovery>,
    pub(super) unkeyed_acquired_recovery: Vec<PendingAcquiredRecovery>,
}

pub(super) struct PreparedSetup {
    pub(super) set: EnsuredSet,
    pub(super) groups: Vec<RunnerGroup>,
    pub(super) captured_at_unix_ms: u64,
}

#[cfg(test)]
impl PreparedSetup {
    pub(super) fn test(binding: &RepositoryConnection, set_id: i64) -> Self {
        Self {
            set: EnsuredSet {
                id: set_id,
                ownership: crate::connect::SetOwnership::Adopted,
                name: binding.scale_set.name().to_owned(),
                runner_group_id: 1,
                disable_update: false,
                labels: binding.scale_set.labels().to_vec(),
            },
            groups: vec![RunnerGroup {
                id: 1,
                name: "test-group".to_owned(),
                is_default: true,
            }],
            captured_at_unix_ms: 1_700_000_000_000,
        }
    }
}

pub(super) fn session_assigned_total(
    statistics: Option<&velnor_runner_github::Statistics>,
) -> Option<i64> {
    statistics.map(velnor_runner_github::Statistics::assigned_population)
}

impl PreparedConnection {
    pub(super) fn record_assigned_total(&mut self, assigned_total: Option<i64>) {
        record_assigned_total(&mut self.assigned_total, assigned_total);
    }

    pub(super) fn has_live_execution_state(&self) -> bool {
        !self.runner_ready.is_empty()
            || !self.authorizing.is_empty()
            || !self.pending_bootstrap.is_empty()
            || !self.unkeyed_bootstrap.is_empty()
            || !self.authorized.is_empty()
            || !self.pending_authorization.is_empty()
            || !self.unkeyed_authorization.is_empty()
            || !self.pending_acquired_recovery.is_empty()
            || !self.unkeyed_acquired_recovery.is_empty()
    }

    pub(super) fn setup_receipt_input(
        &self,
        engine_id: &str,
        docker_root_digest: &str,
    ) -> Result<SetupReceiptInput, HostError> {
        let credentials = self.credentials.as_ref().ok_or(HostError::Keychain)?;
        Ok(SetupReceiptInput {
            binding: self.binding.clone(),
            set: self.setup.set.clone(),
            groups: self.setup.groups.clone(),
            captured_at_unix_ms: self.setup.captured_at_unix_ms,
            session_intent_id: self.session_intent,
            session_id: credentials.session.session_id.clone(),
            engine_id: engine_id.to_owned(),
            docker_root_digest: docker_root_digest.to_owned(),
        })
    }
}

pub(super) fn setup_receipt_inputs(
    connections: &[PreparedConnection],
    config: &HostConfig,
    engine_id: &str,
    docker_root_digest: &str,
) -> Result<Vec<SetupReceiptInput>, HostError> {
    let mut inputs = Vec::new();
    for connection in connections {
        if config
            .connection(&connection.binding.connection_id)
            .is_some_and(|binding| connection.is_for(binding))
        {
            inputs.push(connection.setup_receipt_input(engine_id, docker_root_digest)?);
        }
    }
    Ok(inputs)
}

pub(super) struct SessionCredentials {
    pub(super) link: Link,
    pub(super) session: QueueSession,
    pub(super) github_pat: Secret,
}

#[derive(Clone, Copy)]
pub(super) struct AuthorizationContext<'a> {
    pub(super) config: &'a HostConfig,
    pub(super) journal: &'a Journal,
    pub(super) docker: &'a Docker,
    pub(super) selected_engine: &'a SelectedEngineContext,
    pub(super) worker_images: &'a VerifiedWorkerImages,
}

impl PreparedConnection {
    pub(super) fn is_for(&self, binding: &RepositoryConnection) -> bool {
        self.binding == *binding
    }

    pub(super) fn retain_runner_ready(
        &mut self,
        runner: RunnerReadyBootstrap,
    ) -> Result<i64, Box<RunnerReadyBootstrap>> {
        let Some(request_id) = runner.request_id() else {
            return Err(Box::new(runner));
        };
        let intent_id = runner.intent_id();
        match self.runner_ready.insert(request_id, runner) {
            Ok(()) => {
                self.ready_intents.insert(request_id, intent_id);
                Ok(request_id)
            }
            Err(runner) => Err(Box::new(runner)),
        }
    }

    pub(super) fn start_authorization(
        &mut self,
        request_id: i64,
        runner: RunnerReadyBootstrap,
        context: AuthorizationContext<'_>,
    ) -> Result<(), HostError> {
        let AuthorizationContext {
            config,
            journal,
            docker,
            selected_engine,
            worker_images,
        } = context;
        let Some(connection) = config.connection(&self.binding.connection_id).cloned() else {
            self.defer_authorization(request_id, runner, HostError::Config);
            return Ok(());
        };
        let Some(policy) = config
            .validated_job_authority_policy(&self.binding.connection_id)?
            .cloned()
        else {
            self.defer_authorization(request_id, runner, HostError::Config);
            return Ok(());
        };
        let Some(credentials) = self.credentials.as_ref() else {
            self.defer_authorization(request_id, runner, HostError::Keychain);
            return Ok(());
        };
        if self.authorizing.contains_key(&request_id)
            || self.authorized.contains_key(&request_id)
            || self.pending_authorization.contains_key(&request_id)
        {
            self.defer_authorization(request_id, runner, HostError::Ownership);
            return Ok(());
        }
        let host = journal.clone();
        let scoped = self.scope.clone();
        let docker = docker.clone();
        let engine = selected_engine.clone();
        let images = worker_images.clone();
        let config = config.clone();
        let github_token = crate::listen::Secret::new(credentials.github_pat.expose());
        let handle = tokio::task::spawn_blocking(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return Err(Box::new(PendingScopedExecution::Runner(
                    Box::new(runner),
                    None,
                    HostError::Journal,
                )));
            };
            let secret = match ReleaseReadSecret::load(&config) {
                Ok(secret) => secret,
                Err(error) => {
                    return Err(Box::new(PendingScopedExecution::Runner(
                        Box::new(runner),
                        None,
                        error,
                    )));
                }
            };
            let context = crate::launch::scoped_runtime::ScopedHostContext {
                docker: &docker,
                host: &host,
                scoped: &scoped,
                secret: &secret,
                engine: &engine,
                images: &images,
            };
            runtime
                .block_on(crate::launch::scoped_runtime::authorize_started_execution(
                    runner,
                    &context,
                    &connection,
                    &policy,
                    github_token.expose(),
                    std::time::Instant::now() + Duration::from_secs(120),
                ))
                .map_err(Box::new)
        });
        self.authorizing.insert(request_id, handle);
        Ok(())
    }

    fn defer_authorization(
        &mut self,
        request_id: i64,
        runner: RunnerReadyBootstrap,
        error: HostError,
    ) {
        self.retain_pending(
            request_id,
            PendingScopedExecution::Runner(Box::new(runner), None, error),
        );
    }

    pub(super) async fn collect_authorization(&mut self) -> Result<(), HostError> {
        let finished = self
            .authorizing
            .iter()
            .filter_map(|(request_id, handle)| handle.is_finished().then_some(*request_id))
            .collect::<Vec<_>>();
        for request_id in finished {
            let handle = self
                .authorizing
                .remove(&request_id)
                .ok_or(HostError::Ownership)?;
            match handle.await.map_err(|_| HostError::Journal)? {
                Ok(authorized) => {
                    self.authorized
                        .entry(request_id)
                        .or_default()
                        .push(authorized);
                }
                Err(pending) => {
                    let pending = *pending;
                    let wake_cleanup = pending.requires_cleanup_wake();
                    self.retain_pending(request_id, pending);
                    if wake_cleanup && let Some(completion) = self.completion.as_ref() {
                        completion.notify();
                    }
                }
            }
        }
        let request_ids = self.authorized.keys().copied().collect::<Vec<_>>();
        for request_id in request_ids {
            let Some(runners) = self.authorized.get_mut(&request_id) else {
                continue;
            };
            let mut completed = Vec::with_capacity(runners.len());
            for runner in runners.iter() {
                let intent_id = runner.source().request().intent_id();
                completed.push(self.scope.intent(intent_id).await?.cleanup_proven);
            }
            let mut index = 0;
            runners.retain(|_| {
                let keep = !completed.get(index).copied().unwrap_or(false);
                index += 1;
                keep
            });
            if runners.is_empty() {
                self.authorized.remove(&request_id);
                let _retired = self.runner_ready.retire(request_id);
            }
        }
        self.collect_proven_pending_cleanup().await?;
        Ok(())
    }

    pub(super) async fn collect_proven_pending_cleanup(&mut self) -> Result<(), HostError> {
        cleanup::collect_proven_pending_cleanup(self).await
    }

    pub(super) fn retain_pending(&mut self, request_id: i64, pending: PendingScopedExecution) {
        self.pending_authorization
            .entry(request_id)
            .or_default()
            .push(pending);
    }
}
