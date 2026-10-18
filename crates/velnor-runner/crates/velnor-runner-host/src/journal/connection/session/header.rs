//! Scope-bound header intent and atomic session retirement.

use crate::error::HostError;
use crate::journal::ConnectionJournal;
use crate::journal::capacity_header::{
    HeaderAllocation, HeaderDeleteAttempt, HeaderLedger, HeaderPollAttempt, PollOutcome, load, save,
};
use crate::journal::connection::storage::{in_transaction, read_transaction};
use crate::launch::{CompletedHeaderPoll, DeletedSession204};

impl ConnectionJournal {
    /// Read the exact live allocation bound to one returned session.
    pub(crate) async fn session_header_allocation(
        &self,
        session_intent_id: i64,
    ) -> Result<HeaderAllocation, HostError> {
        read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let scale_set_id = self.required_set_id(connection).await?;
            let session_id =
                active_session_id(connection, self, scale_set_id, session_intent_id).await?;
            let ledger = load(connection).await?;
            ledger
                .charged_allocations()
                .into_iter()
                .find(|allocation| {
                    allocation.matches_session(self.connection_id(), scale_set_id, &session_id)
                })
                .ok_or(HostError::Journal)
        })
        .await
    }

    /// Reserve this bound session's header against the host-wide budget.
    pub(crate) async fn reserve_session_header(
        &self,
        session_intent_id: i64,
        target: u32,
    ) -> Result<HeaderAllocation, HostError> {
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let scale_set_id = self.required_set_id(connection).await?;
            let session_id =
                active_session_id(connection, self, scale_set_id, session_intent_id).await?;
            let mut ledger = load(connection).await?;
            let allocation = ledger
                .reserve(
                    self.connection_id().clone(),
                    scale_set_id,
                    &session_id,
                    target,
                )
                .map_err(|_| HostError::Journal)?;
            save(connection, &ledger).await?;
            Ok(allocation)
        })
        .await
    }

    /// Commit one exact session poll before sending it.
    pub(crate) async fn begin_session_poll(
        &self,
        session_intent_id: i64,
        allocation: &HeaderAllocation,
    ) -> Result<HeaderPollAttempt, HostError> {
        self.header_poll_transition(session_intent_id, allocation, |ledger| {
            ledger.begin_poll(allocation)
        })
        .await
    }

    /// Commit the required zero-header poll before sending it.
    pub(crate) async fn begin_zero_header_poll(
        &self,
        session_intent_id: i64,
        allocation: &HeaderAllocation,
    ) -> Result<HeaderPollAttempt, HostError> {
        self.header_poll_transition(session_intent_id, allocation, |ledger| {
            ledger.begin_zero_poll(allocation)
        })
        .await
    }

    /// Record the exact handled response for the sent poll.
    pub(crate) async fn settle_session_poll(
        &self,
        session_intent_id: i64,
        completed: CompletedHeaderPoll,
    ) -> Result<(), HostError> {
        let attempt = completed.into_attempt();
        self.poll_result(session_intent_id, attempt, PollOutcome::Response)
            .await
    }

    /// Keep the session charge when a sent poll has an unknown result.
    pub(crate) async fn mark_session_poll_uncertain(
        &self,
        session_intent_id: i64,
        attempt: HeaderPollAttempt,
    ) -> Result<(), HostError> {
        self.poll_result(session_intent_id, attempt, PollOutcome::Unknown)
            .await
    }

    /// Commit the exact DELETE attempt after the session and its work drain.
    pub(crate) async fn begin_session_delete(
        &self,
        session_intent_id: i64,
        allocation: &HeaderAllocation,
    ) -> Result<HeaderDeleteAttempt, HostError> {
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let scale_set_id = self.required_set_id(connection).await?;
            let session_id = verify_allocation(
                connection,
                self,
                scale_set_id,
                session_intent_id,
                allocation,
            )
            .await?;
            if has_other_session_work(connection, self.connection_id().as_str(), session_intent_id)
                .await?
            {
                return Err(HostError::Journal);
            }
            let mut ledger = load(connection).await?;
            let attempt = ledger
                .begin_delete(allocation)
                .map_err(|_| HostError::Journal)?;
            if !attempt.matches_session(self.connection_id(), scale_set_id, &session_id) {
                return Err(HostError::Journal);
            }
            save(connection, &ledger).await?;
            Ok(attempt)
        })
        .await
    }

    /// Keep the header and session intent charged after an ambiguous DELETE.
    pub(crate) async fn mark_session_delete_uncertain(
        &self,
        session_intent_id: i64,
        attempt: HeaderDeleteAttempt,
    ) -> Result<(), HostError> {
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let scale_set_id = self.required_set_id(connection).await?;
            let session_id =
                active_session_id(connection, self, scale_set_id, session_intent_id).await?;
            if !attempt.matches_session(self.connection_id(), scale_set_id, &session_id) {
                return Err(HostError::Journal);
            }
            let mut ledger = load(connection).await?;
            ledger
                .mark_delete_unknown(&attempt)
                .map_err(|_| HostError::Journal)?;
            save(connection, &ledger).await
        })
        .await
    }

    /// Retire the session intent and its shared header in one journal transaction.
    pub(crate) async fn retire_session_after_delete(
        &self,
        session_intent_id: i64,
        deleted: DeletedSession204,
    ) -> Result<(), HostError> {
        let attempt = deleted.into_attempt();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let scale_set_id = self.required_set_id(connection).await?;
            let session_id = active_session_id(connection, self, scale_set_id, session_intent_id)
                .await?;
            if !attempt.matches_session(self.connection_id(), scale_set_id, &session_id)
                || has_other_session_work(
                    connection,
                    self.connection_id().as_str(),
                    session_intent_id,
                )
                .await?
            {
                return Err(HostError::Journal);
            }
            let mut ledger = load(connection).await?;
            ledger
                .finish_delete(&attempt)
                .map_err(|_| HostError::Journal)?;
            let changed = connection
                .execute(
                    "UPDATE connection_intents SET cleanup_proven = 1 WHERE id = ?1 AND connection_id = ?2 AND scale_set_id = ?3 AND kind = 'session' AND subject = ?4 AND cleanup_proven = 0",
                    (session_intent_id, self.connection_id().as_str(), scale_set_id, session_id.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE connection_session_creates SET state = 'retired' WHERE connection_id = ?1 AND scale_set_id = ?2 AND session_id = ?3 AND intent_id = ?4 AND state = 'bound'",
                    (self.connection_id().as_str(), scale_set_id, session_id.as_str(), session_intent_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            save(connection, &ledger).await
        })
        .await
    }

    async fn header_poll_transition<T>(
        &self,
        session_intent_id: i64,
        allocation: &HeaderAllocation,
        operation: impl FnOnce(
            &mut HeaderLedger,
        ) -> Result<T, crate::journal::capacity_header::HeaderLedgerError>,
    ) -> Result<T, HostError> {
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let scale_set_id = self.required_set_id(connection).await?;
            verify_allocation(
                connection,
                self,
                scale_set_id,
                session_intent_id,
                allocation,
            )
            .await?;
            let mut ledger = load(connection).await?;
            let attempt = operation(&mut ledger).map_err(|_| HostError::Journal)?;
            save(connection, &ledger).await?;
            Ok(attempt)
        })
        .await
    }

    async fn poll_result(
        &self,
        session_intent_id: i64,
        attempt: HeaderPollAttempt,
        outcome: PollOutcome,
    ) -> Result<(), HostError> {
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let scale_set_id = self.required_set_id(connection).await?;
            let session_id =
                active_session_id(connection, self, scale_set_id, session_intent_id).await?;
            if !attempt.matches_session(self.connection_id(), scale_set_id, &session_id) {
                return Err(HostError::Journal);
            }
            let mut ledger = load(connection).await?;
            ledger
                .finish_poll(&attempt, outcome)
                .map_err(|_| HostError::Journal)?;
            save(connection, &ledger).await
        })
        .await
    }
}

async fn active_session_id(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    scale_set_id: i64,
    session_intent_id: i64,
) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT i.subject FROM connection_intents AS i JOIN connection_session_creates AS s ON s.intent_id = i.id WHERE i.id = ?1 AND i.connection_id = ?2 AND i.scale_set_id = ?3 AND i.kind = 'session' AND i.cleanup_proven = 0 AND s.state = 'bound'",
            (session_intent_id, scope.connection_id().as_str(), scale_set_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}

async fn verify_allocation(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    scale_set_id: i64,
    session_intent_id: i64,
    allocation: &HeaderAllocation,
) -> Result<String, HostError> {
    let session_id = active_session_id(connection, scope, scale_set_id, session_intent_id).await?;
    if !allocation.matches_session(scope.connection_id(), scale_set_id, &session_id) {
        return Err(HostError::Journal);
    }
    Ok(session_id)
}

async fn has_other_session_work(
    connection: &turso::Connection,
    connection_id: &str,
    session_intent_id: i64,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT EXISTS (SELECT 1 FROM connection_intents WHERE connection_id = ?1 AND id != ?2 AND cleanup_proven = 0) OR EXISTS (SELECT 1 FROM connection_completion_inbox WHERE connection_id = ?1) OR EXISTS (SELECT 1 FROM capacity_reservations WHERE connection_id = ?1 AND state != 'released') OR EXISTS (SELECT 1 FROM connection_session_creates WHERE connection_id = ?1 AND state IN ('prepared', 'requested', 'uncertain'))",
            (connection_id, session_intent_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let active = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    Ok(active != 0)
}
