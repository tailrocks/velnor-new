//! Root or connection journal selected for completion cleanup.

use std::future::Future;

use crate::error::HostError;
use crate::journal::{CleanupClaim, CompletedLaunch, ConnectionJournal, Journal};

pub(super) enum CompletionJournal {
    Root(Journal),
    Scoped(ConnectionJournal),
}

impl CompletionJournal {
    pub(super) const fn root(journal: Journal) -> Self {
        Self::Root(journal)
    }

    pub(super) const fn scoped(journal: ConnectionJournal) -> Self {
        Self::Scoped(journal)
    }

    pub(super) async fn due_completed_launches(
        &self,
        now: i64,
        limit: u32,
    ) -> Result<Vec<CompletedLaunch>, HostError> {
        match self {
            Self::Root(journal) => journal.due_completed_launches(now, limit).await,
            Self::Scoped(journal) => journal.due_completed_launches(now, limit).await,
        }
    }

    pub(super) async fn claim_completion_cleanup(
        &self,
        id: i64,
        lease_seconds: i64,
    ) -> Result<Option<CleanupClaim>, HostError> {
        match self {
            Self::Root(journal) => journal.claim_completion_cleanup(id, lease_seconds).await,
            Self::Scoped(journal) => journal.claim_completion_cleanup(id, lease_seconds).await,
        }
    }

    pub(super) async fn retry_completion_cleanup(
        &self,
        id: i64,
        generation: i64,
        retry_delay_seconds: i64,
    ) -> Result<bool, HostError> {
        match self {
            Self::Root(journal) => {
                journal
                    .retry_completion_cleanup(id, generation, retry_delay_seconds)
                    .await
            }
            Self::Scoped(journal) => {
                journal
                    .retry_completion_cleanup(id, generation, retry_delay_seconds)
                    .await
            }
        }
    }

    pub(super) async fn run_completion_cleanup_effect<T, F, Fut>(
        &self,
        id: i64,
        generation: i64,
        effect_window_seconds: i64,
        effect: F,
    ) -> Result<Option<T>, HostError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, HostError>>,
    {
        match self {
            Self::Root(journal) => {
                journal
                    .run_completion_cleanup_effect(id, generation, effect_window_seconds, effect)
                    .await
            }
            Self::Scoped(journal) => {
                journal
                    .run_completion_cleanup_effect(id, generation, effect_window_seconds, effect)
                    .await
            }
        }
    }

    pub(super) async fn bind_completion_containers(
        &self,
        id: i64,
        generation: i64,
        runner_container_id: Option<&str>,
        dind_id: Option<&str>,
    ) -> Result<bool, HostError> {
        match self {
            Self::Root(journal) => {
                journal
                    .bind_completion_containers(id, generation, runner_container_id, dind_id)
                    .await
            }
            Self::Scoped(journal) => {
                journal
                    .bind_completion_containers(id, generation, runner_container_id, dind_id)
                    .await
            }
        }
    }

    pub(super) async fn record_completion_runner_absent(
        &self,
        id: i64,
        generation: i64,
    ) -> Result<bool, HostError> {
        match self {
            Self::Root(journal) => {
                journal
                    .record_completion_runner_absent(id, generation)
                    .await
            }
            Self::Scoped(journal) => {
                journal
                    .record_completion_runner_absent(id, generation)
                    .await
            }
        }
    }

    pub(super) async fn record_completion_cleanup(
        &self,
        id: i64,
        generation: i64,
    ) -> Result<bool, HostError> {
        match self {
            Self::Root(journal) => journal.record_completion_cleanup(id, generation).await,
            Self::Scoped(journal) => journal.record_completion_cleanup(id, generation).await,
        }
    }
}
