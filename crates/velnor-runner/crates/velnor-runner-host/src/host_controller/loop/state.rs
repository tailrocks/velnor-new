//! Per-connection live session and execution ownership.

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use super::PendingAcquiredRecovery;
use bollard::Docker;
use velnor_runner_github::QueueSession;

use crate::{
    config::{HostConfig, RepositoryConnection},
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
    worker::guest_image::GuestImageFile,
};

pub(super) struct PreparedConnection {
    pub(super) binding: RepositoryConnection,
    pub(super) scope: ConnectionJournal,
    pub(super) set_id: i64,
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
        tokio::task::JoinHandle<Result<AuthorizedScopedExecution, PendingScopedExecution>>,
    >,
    pub(super) pending_bootstrap: BTreeMap<i64, Vec<ScopedChannelOutcome>>,
    pub(super) unkeyed_bootstrap: Vec<ScopedChannelOutcome>,
    pub(super) authorized: BTreeMap<i64, Vec<AuthorizedScopedExecution>>,
    pub(super) pending_authorization: BTreeMap<i64, Vec<PendingScopedExecution>>,
    pub(super) unkeyed_authorization: Vec<PendingScopedExecution>,
    pub(super) pending_acquired_recovery: BTreeMap<i64, PendingAcquiredRecovery>,
    pub(super) unkeyed_acquired_recovery: Vec<PendingAcquiredRecovery>,
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

    pub(super) fn cursor_for_poll(&mut self, now: Instant) -> i64 {
        (self.poll_cursor, self.poll_retry_at) =
            expire_cursor(self.poll_cursor, self.poll_retry_at, now);
        crate::launch::steady::poll_cursor(self.poll_cursor, self.poll_retry_at, now)
    }

    pub(super) fn record_cursor_disposition(
        &mut self,
        message_id: Option<i64>,
        disposition: crate::launch::CursorDisposition,
        now: Instant,
    ) {
        (self.poll_cursor, self.poll_retry_at) = apply_cursor_disposition(
            self.poll_cursor,
            self.poll_retry_at,
            message_id,
            disposition,
            now,
        );
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
}

fn expire_cursor(cursor: i64, retry_at: Option<Instant>, now: Instant) -> (i64, Option<Instant>) {
    if retry_at.is_some_and(|retry_at| now >= retry_at) {
        (0, None)
    } else {
        (cursor, retry_at)
    }
}

fn apply_cursor_disposition(
    cursor: i64,
    retry_at: Option<Instant>,
    message_id: Option<i64>,
    disposition: crate::launch::CursorDisposition,
    now: Instant,
) -> (i64, Option<Instant>) {
    use crate::launch::CursorDisposition;

    match disposition {
        CursorDisposition::Deferred { message_id } => {
            let Some(cursor) = crate::launch::steady::steady_cursor(cursor, message_id) else {
                return (cursor, retry_at);
            };
            (
                cursor,
                Some(retry_at.unwrap_or(now + crate::launch::steady::RETRY)),
            )
        }
        CursorDisposition::ResetToHead if retry_at.is_some() => (
            message_id
                .and_then(|message_id| crate::launch::steady::steady_cursor(cursor, message_id))
                .unwrap_or(cursor),
            retry_at,
        ),
        CursorDisposition::ResetToHead => (0, None),
        CursorDisposition::Unchanged => (cursor, retry_at),
    }
}

#[cfg(test)]
mod cursor_tests {
    use std::time::{Duration, Instant};

    use crate::launch::CursorDisposition;

    use super::{
        apply_cursor_disposition, expire_cursor, record_assigned_total, session_assigned_total,
    };

    #[test]
    fn deferred_head_does_not_block_lifecycle_cursor_progress() {
        let now = Instant::now();
        let (cursor, retry_at) = apply_cursor_disposition(
            0,
            None,
            Some(40),
            CursorDisposition::Deferred { message_id: 40 },
            now,
        );
        assert_eq!(cursor, 40);
        let deadline = retry_at.expect("deferred response has retry deadline");

        // Later lifecycle-only responses are durably recorded then ACKed by
        // positive_poll_once. Keep advancing past them without losing the
        // original unacknowledged Available head or extending its retry.
        let (cursor, retained_deadline) = apply_cursor_disposition(
            cursor,
            retry_at,
            Some(41),
            CursorDisposition::ResetToHead,
            now + Duration::from_secs(1),
        );
        assert_eq!(cursor, 41);
        assert_eq!(retained_deadline, Some(deadline));

        let (cursor, retry_at) = expire_cursor(cursor, retained_deadline, deadline);
        assert_eq!(cursor, 0);
        assert_eq!(retry_at, None);

        // Message ID zero is real, but the pinned client omits its cursor
        // parameter at zero. Do not invent ID one or ACK the offer.
        assert_eq!(
            apply_cursor_disposition(0, None, Some(0), CursorDisposition::Unchanged, now,),
            (0, None)
        );
    }

    #[test]
    fn missing_poll_statistics_retain_the_current_session_snapshot() {
        let mut assigned_total = session_assigned_total(Some(&statistics(4)));
        assert_eq!(assigned_total, Some(4));
        record_assigned_total(&mut assigned_total, Some(6));
        assert_eq!(assigned_total, Some(6));
        record_assigned_total(&mut assigned_total, None);
        assert_eq!(assigned_total, Some(6));

        // A replacement session starts from its own create/refresh response.
        assigned_total = session_assigned_total(Some(&statistics(2)));
        record_assigned_total(&mut assigned_total, None);
        assert_eq!(assigned_total, Some(2));
    }

    fn statistics(total_assigned_jobs: i64) -> velnor_runner_github::Statistics {
        velnor_runner_github::Statistics {
            total_available_jobs: 0,
            total_acquired_jobs: 0,
            total_assigned_jobs,
            total_running_jobs: 0,
            total_registered_runners: 0,
            total_busy_runners: 0,
            total_idle_runners: 0,
        }
    }
}

fn record_assigned_total(current: &mut Option<i64>, sample: Option<i64>) {
    if sample.is_some() {
        *current = sample;
    }
}

pub(super) struct SessionCredentials {
    pub(super) link: Link,
    pub(super) session: QueueSession,
    pub(super) github_pat: Secret,
}

impl PreparedConnection {
    pub(super) fn is_for(&self, binding: &RepositoryConnection) -> bool {
        self.binding == *binding
    }

    pub(super) fn retain_runner_ready(
        &mut self,
        runner: RunnerReadyBootstrap,
    ) -> Result<i64, RunnerReadyBootstrap> {
        let Some(request_id) = runner.request_id() else {
            return Err(runner);
        };
        let intent_id = runner.intent_id();
        match self.runner_ready.insert(request_id, runner) {
            Ok(()) => {
                self.ready_intents.insert(request_id, intent_id);
                Ok(request_id)
            }
            Err(runner) => Err(runner),
        }
    }

    pub(super) fn start_authorization(
        &mut self,
        request_id: i64,
        runner: RunnerReadyBootstrap,
        config: &HostConfig,
        journal: &Journal,
        docker: &Docker,
        selected_engine: &SelectedEngineContext,
        worker_images: &VerifiedWorkerImages,
    ) -> Result<(), HostError> {
        let Some(connection) = config.connection(&self.binding.connection_id).cloned() else {
            self.retain_pending(
                request_id,
                PendingScopedExecution::Runner(runner, None, HostError::Config),
            );
            return Ok(());
        };
        let Some(policy) = config
            .validated_job_authority_policy(&self.binding.connection_id)?
            .cloned()
        else {
            self.retain_pending(
                request_id,
                PendingScopedExecution::Runner(runner, None, HostError::Config),
            );
            return Ok(());
        };
        let Some(credentials) = self.credentials.as_ref() else {
            self.retain_pending(
                request_id,
                PendingScopedExecution::Runner(runner, None, HostError::Keychain),
            );
            return Ok(());
        };
        if self.authorizing.contains_key(&request_id)
            || self.authorized.contains_key(&request_id)
            || self.pending_authorization.contains_key(&request_id)
        {
            self.retain_pending(
                request_id,
                PendingScopedExecution::Runner(runner, None, HostError::Ownership),
            );
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
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(_) => {
                    return Err(PendingScopedExecution::Runner(
                        runner,
                        None,
                        HostError::Journal,
                    ));
                }
            };
            let secret = match ReleaseReadSecret::load(&config) {
                Ok(secret) => secret,
                Err(error) => {
                    return Err(PendingScopedExecution::Runner(runner, None, error));
                }
            };
            runtime.block_on(crate::launch::scoped_runtime::authorize_started_execution(
                runner,
                &docker,
                &host,
                &scoped,
                &secret,
                &engine,
                &images,
                &connection,
                &policy,
                github_token.expose(),
                std::time::Instant::now() + Duration::from_secs(120),
            ))
        });
        self.authorizing.insert(request_id, handle);
        Ok(())
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
        let ready = self
            .ready_intents
            .iter()
            .map(|(request_id, intent_id)| (*request_id, *intent_id))
            .collect::<Vec<_>>();
        for (request_id, intent_id) in ready {
            if self.scope.intent(intent_id).await?.cleanup_proven
                && self
                    .runner_ready
                    .take_ready_after_cleanup(request_id)
                    .is_some()
            {
                self.ready_intents.remove(&request_id);
            }
        }

        let pending_bootstrap = self
            .pending_bootstrap
            .iter()
            .map(|(request_id, values)| {
                (
                    *request_id,
                    values
                        .iter()
                        .map(ScopedChannelOutcome::intent_id)
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        for (request_id, intents) in pending_bootstrap {
            let mut keep = Vec::with_capacity(intents.len());
            for intent_id in intents {
                keep.push(match intent_id {
                    Some(intent_id) => !self.scope.intent(intent_id).await?.cleanup_proven,
                    None => true,
                });
            }
            if let Some(values) = self.pending_bootstrap.get_mut(&request_id) {
                *values = std::mem::take(values)
                    .into_iter()
                    .zip(keep)
                    .filter_map(|(value, keep)| keep.then_some(value))
                    .collect();
                if values.is_empty() {
                    self.pending_bootstrap.remove(&request_id);
                }
            }
        }
        let unkeyed_bootstrap = self
            .unkeyed_bootstrap
            .iter()
            .map(ScopedChannelOutcome::intent_id)
            .collect::<Vec<_>>();
        let mut keep_unkeyed_bootstrap = Vec::with_capacity(unkeyed_bootstrap.len());
        for intent_id in unkeyed_bootstrap {
            keep_unkeyed_bootstrap.push(match intent_id {
                Some(intent_id) => !self.scope.intent(intent_id).await?.cleanup_proven,
                None => true,
            });
        }
        self.unkeyed_bootstrap = std::mem::take(&mut self.unkeyed_bootstrap)
            .into_iter()
            .zip(keep_unkeyed_bootstrap)
            .filter_map(|(value, keep)| keep.then_some(value))
            .collect();

        let pending_authorization = self
            .pending_authorization
            .iter()
            .map(|(request_id, values)| {
                (
                    *request_id,
                    values
                        .iter()
                        .map(PendingScopedExecution::intent_id)
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        for (request_id, intents) in pending_authorization {
            let mut keep = Vec::with_capacity(intents.len());
            for intent_id in intents {
                keep.push(!self.scope.intent(intent_id).await?.cleanup_proven);
            }
            if let Some(values) = self.pending_authorization.get_mut(&request_id) {
                *values = std::mem::take(values)
                    .into_iter()
                    .zip(keep)
                    .filter_map(|(value, keep)| keep.then_some(value))
                    .collect();
                if values.is_empty() {
                    self.pending_authorization.remove(&request_id);
                }
            }
        }
        let unkeyed_authorization = self
            .unkeyed_authorization
            .iter()
            .map(PendingScopedExecution::intent_id)
            .collect::<Vec<_>>();
        let mut keep_unkeyed_authorization = Vec::with_capacity(unkeyed_authorization.len());
        for intent_id in unkeyed_authorization {
            keep_unkeyed_authorization.push(!self.scope.intent(intent_id).await?.cleanup_proven);
        }
        self.unkeyed_authorization = std::mem::take(&mut self.unkeyed_authorization)
            .into_iter()
            .zip(keep_unkeyed_authorization)
            .filter_map(|(value, keep)| keep.then_some(value))
            .collect();

        let pending_acquired = self
            .pending_acquired_recovery
            .iter()
            .map(|(request_id, value)| (*request_id, value.intent_id))
            .collect::<Vec<_>>();
        for (request_id, intent_id) in pending_acquired {
            if self.scope.intent(intent_id).await?.cleanup_proven {
                self.pending_acquired_recovery.remove(&request_id);
            }
        }
        let unkeyed_acquired = self
            .unkeyed_acquired_recovery
            .iter()
            .map(|value| value.intent_id)
            .collect::<Vec<_>>();
        let mut keep_unkeyed_acquired = Vec::with_capacity(unkeyed_acquired.len());
        for intent_id in unkeyed_acquired {
            keep_unkeyed_acquired.push(!self.scope.intent(intent_id).await?.cleanup_proven);
        }
        self.unkeyed_acquired_recovery = std::mem::take(&mut self.unkeyed_acquired_recovery)
            .into_iter()
            .zip(keep_unkeyed_acquired)
            .filter_map(|(value, keep)| keep.then_some(value))
            .collect();
        Ok(())
    }

    pub(super) fn retain_pending(&mut self, request_id: i64, pending: PendingScopedExecution) {
        self.pending_authorization
            .entry(request_id)
            .or_default()
            .push(pending);
    }
}
