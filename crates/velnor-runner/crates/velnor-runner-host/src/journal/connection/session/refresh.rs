//! Exact-ID same-session credential refreshes under one connection scope.

mod model;

pub(crate) use model::{SessionRefreshClaim, SessionRefreshRecord, SessionRefreshState};

use crate::config::ConnectionId;
use crate::error::HostError;
use crate::journal::ConnectionJournal;
#[cfg(test)]
use crate::journal::connection::storage::read_transaction;
use crate::journal::connection::storage::{has_ambiguous_legacy_work, in_transaction};
use crate::launch::{ReturnedSessionRefreshTransportError, VerifiedSessionRefresh};

use super::header::active_session_id;
use super::model::validate_session_id;

const MAX_REFRESH_GENERATIONS: i64 = 1024;

impl ConnectionJournal {
    /// Claim one exact session PATCH after storing its next local generation.
    pub(crate) async fn claim_session_refresh(
        &self,
        session_intent_id: i64,
        expected_session_id: &str,
    ) -> Result<Option<SessionRefreshClaim>, HostError> {
        validate_session_id(expected_session_id)?;
        let expected_session_id = expected_session_id.to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            if has_ambiguous_legacy_work(connection).await? {
                return Ok(None);
            }
            let scale_set_id = self.required_set_id(connection).await?;
            let session_id = active_session_id(connection, self, scale_set_id, session_intent_id)
                .await?;
            if session_id != expected_session_id
                || !has_charged_session_header(
                    connection,
                    self.connection_id(),
                    scale_set_id,
                    &session_id,
                )
                .await?
            {
                return Err(HostError::Journal);
            }
            let generation = next_generation(
                connection,
                session_intent_id,
                self.connection_id().as_str(),
            )
            .await?;
            let Some(generation) = generation else {
                return Ok(None);
            };
            connection
                .execute(
                    "INSERT INTO connection_session_refreshes (intent_id, connection_id, scale_set_id, session_id, generation, state) VALUES (?1, ?2, ?3, ?4, ?5, 'requested')",
                    (session_intent_id, self.connection_id().as_str(), scale_set_id, session_id.as_str(), generation),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(Some(SessionRefreshClaim::new(
                self.connection_id().clone(),
                scale_set_id,
                session_intent_id,
                session_id,
                generation,
            )))
        })
        .await
    }

    /// Record exact-ID authenticated PATCH evidence for credential use only.
    pub(crate) async fn record_session_refreshed(
        &self,
        claim: SessionRefreshClaim,
        proof: VerifiedSessionRefresh,
    ) -> Result<SessionRefreshRecord, HostError> {
        if proof.session_intent_id() != claim.session_intent_id
            || proof.session_id() != claim.session_id
            || proof.generation() != claim.generation
        {
            return Err(HostError::Journal);
        }
        self.update_refresh_state(claim, SessionRefreshState::Succeeded)
            .await
    }

    /// Record only the pinned client's returned retryable transport error.
    pub(crate) async fn record_session_refresh_transport_error(
        &self,
        claim: SessionRefreshClaim,
        proof: ReturnedSessionRefreshTransportError,
    ) -> Result<SessionRefreshRecord, HostError> {
        if proof.session_intent_id() != claim.session_intent_id
            || proof.session_id() != claim.session_id
            || proof.generation() != claim.generation
        {
            return Err(HostError::Journal);
        }
        self.update_refresh_state(claim, SessionRefreshState::TransportError)
            .await
    }

    /// Keep an unexpected or unvalidated result from authorizing another PATCH.
    pub(crate) async fn mark_session_refresh_uncertain(
        &self,
        claim: SessionRefreshClaim,
    ) -> Result<SessionRefreshRecord, HostError> {
        self.update_refresh_state(claim, SessionRefreshState::Uncertain)
            .await
    }

    /// Read the complete bounded refresh history for one exact session intent.
    #[cfg(test)]
    pub(crate) async fn session_refreshes(
        &self,
        session_intent_id: i64,
    ) -> Result<Vec<SessionRefreshRecord>, HostError> {
        read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let scale_set_id = self.required_set_id(connection).await?;
            let mut rows = connection
                .query(
                    "SELECT session_id, generation, state FROM connection_session_refreshes WHERE connection_id = ?1 AND scale_set_id = ?2 AND intent_id = ?3 ORDER BY generation",
                    (self.connection_id().as_str(), scale_set_id, session_intent_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let mut records = Vec::new();
            while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
                records.push(SessionRefreshRecord {
                    connection_id: self.connection_id().clone(),
                    scale_set_id,
                    session_intent_id,
                    session_id: row.get(0).map_err(|_| HostError::Journal)?,
                    generation: row.get(1).map_err(|_| HostError::Journal)?,
                    state: SessionRefreshState::parse(
                        &row.get::<String>(2).map_err(|_| HostError::Journal)?,
                    )?,
                });
            }
            Ok(records)
        })
        .await
    }

    async fn update_refresh_state(
        &self,
        claim: SessionRefreshClaim,
        state: SessionRefreshState,
    ) -> Result<SessionRefreshRecord, HostError> {
        if claim.connection_id != *self.connection_id() {
            return Err(HostError::Journal);
        }
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let scale_set_id = self.required_set_id(connection).await?;
            if scale_set_id != claim.scale_set_id
                || active_session_id(
                    connection,
                    self,
                    scale_set_id,
                    claim.session_intent_id,
                )
                .await?
                    != claim.session_id
            {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE connection_session_refreshes SET state = ?1 WHERE intent_id = ?2 AND connection_id = ?3 AND scale_set_id = ?4 AND session_id = ?5 AND generation = ?6 AND state = 'requested'",
                    (state.as_str(), claim.session_intent_id, self.connection_id().as_str(), scale_set_id, claim.session_id.as_str(), claim.generation),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            Ok(SessionRefreshRecord {
                connection_id: claim.connection_id,
                scale_set_id,
                session_intent_id: claim.session_intent_id,
                session_id: claim.session_id,
                generation: claim.generation,
                state,
            })
        })
        .await
    }
}

async fn has_charged_session_header(
    connection: &turso::Connection,
    connection_id: &ConnectionId,
    scale_set_id: i64,
    session_id: &str,
) -> Result<bool, HostError> {
    let ledger = crate::journal::capacity_header::load(connection).await?;
    Ok(ledger
        .charged_allocations()
        .iter()
        .any(|allocation| allocation.matches_session(connection_id, scale_set_id, session_id)))
}

async fn next_generation(
    connection: &turso::Connection,
    session_intent_id: i64,
    connection_id: &str,
) -> Result<Option<i64>, HostError> {
    let mut rows = connection
        .query(
            "SELECT generation, state FROM connection_session_refreshes WHERE intent_id = ?1 AND connection_id = ?2 ORDER BY generation DESC LIMIT 1",
            (session_intent_id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(Some(1));
    };
    let previous: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let state = SessionRefreshState::parse(&row.get::<String>(1).map_err(|_| HostError::Journal)?)?;
    if !state.permits_next() || previous >= MAX_REFRESH_GENERATIONS {
        return Ok(None);
    }
    previous.checked_add(1).map(Some).ok_or(HostError::Journal)
}
