//! Typed journal entry points for shared Scale Set capacity headers.

use crate::config::ConnectionId;
use crate::error::HostError;
use crate::journal::Journal;
use crate::journal::capacity_header::{
    HeaderAllocation, HeaderDeleteAttempt, HeaderPollAttempt, PollOutcome, bootstrap, load,
    transition, validate, validate_capacity_policy_connection,
};
use crate::journal::connection::storage::read_transaction;
use crate::launch::{CompletedHeaderPoll, DeletedSession204};

impl Journal {
    /// Recover saved header intent after this process owns the engine guard.
    pub(crate) async fn bootstrap_header_authority(&self) -> Result<(), HostError> {
        bootstrap(self).await
    }

    /// Validate the shared header snapshot without changing it.
    pub(crate) async fn validate_header_authority(&self) -> Result<(), HostError> {
        validate(self).await
    }

    /// Reserve one exact session's header against the host-wide limit.
    pub(crate) async fn reserve_header_allocation(
        &self,
        connection_id: ConnectionId,
        scale_set_id: i64,
        session_id: &str,
        target: u32,
    ) -> Result<HeaderAllocation, HostError> {
        transition(self, |ledger| {
            ledger.reserve(connection_id, scale_set_id, session_id, target)
        })
        .await
    }

    /// Commit a poll intent before its caller sends the HTTP request.
    pub(crate) async fn begin_header_poll(
        &self,
        allocation: &HeaderAllocation,
    ) -> Result<HeaderPollAttempt, HostError> {
        transition(self, |ledger| ledger.begin_poll(allocation)).await
    }

    /// Commit a same-session zero-header intent before its caller sends it.
    pub(crate) async fn begin_zero_header_poll(
        &self,
        allocation: &HeaderAllocation,
    ) -> Result<HeaderPollAttempt, HostError> {
        transition(self, |ledger| ledger.begin_zero_poll(allocation)).await
    }

    /// Settle only the poll attempt carried by the source adapter's proof.
    pub(crate) async fn settle_header_poll(
        &self,
        completed: CompletedHeaderPoll,
    ) -> Result<(), HostError> {
        let attempt = completed.into_attempt();
        transition(self, |ledger| {
            ledger.finish_poll(&attempt, PollOutcome::Response)
        })
        .await
    }

    /// Keep a poll charged when its remote outcome is unknown.
    pub(crate) async fn mark_header_poll_uncertain(
        &self,
        attempt: HeaderPollAttempt,
    ) -> Result<(), HostError> {
        transition(self, |ledger| {
            ledger.finish_poll(&attempt, PollOutcome::Unknown)
        })
        .await
    }

    /// Commit an exact session-delete intent before its caller sends DELETE.
    pub(crate) async fn begin_header_session_delete(
        &self,
        allocation: &HeaderAllocation,
    ) -> Result<HeaderDeleteAttempt, HostError> {
        transition(self, |ledger| ledger.begin_delete(allocation)).await
    }

    /// Retire an allocation only after its source adapter proves exact DELETE 204.
    pub(crate) async fn retire_header_allocation(
        &self,
        deleted: DeletedSession204,
    ) -> Result<(), HostError> {
        let attempt = deleted.into_attempt();
        transition(self, |ledger| ledger.finish_delete(&attempt)).await
    }

    /// Keep an allocation charged when DELETE has an unknown remote outcome.
    pub(crate) async fn mark_header_delete_uncertain(
        &self,
        attempt: HeaderDeleteAttempt,
    ) -> Result<(), HostError> {
        transition(self, |ledger| ledger.mark_delete_unknown(&attempt)).await
    }

    /// Give all charged session allocations in stable identity order.
    pub(crate) async fn header_allocations(&self) -> Result<Vec<HeaderAllocation>, HostError> {
        read_transaction(self, async |connection| {
            let ledger = load(connection).await?;
            Ok(ledger.charged_allocations())
        })
        .await
    }

    /// Give the total capacity still charged to remote sessions.
    pub(crate) async fn charged_header_capacity(&self) -> Result<u32, HostError> {
        read_transaction(self, async |connection| {
            let ledger = load(connection).await?;
            Ok(ledger.charged_capacity())
        })
        .await
    }
}

/// Validate an updated host capacity policy in its caller's write transaction.
pub(crate) async fn validate_header_capacity_policy(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    validate_capacity_policy_connection(connection).await
}

#[cfg(test)]
#[path = "header_authority/tests.rs"]
mod tests;
