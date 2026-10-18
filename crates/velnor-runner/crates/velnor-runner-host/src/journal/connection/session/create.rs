//! One-shot session-create intents and returned-ID binding.

use crate::error::HostError;
use crate::journal::capacity_header::{HeaderLedgerError, load, save};
use crate::journal::sql::one_row;

use super::super::ConnectionJournal;
use super::super::storage::{has_ambiguous_legacy_work, in_transaction, read_transaction};
use super::model::{
    BoundConnectionSession, SessionCreateClaim, SessionCreateRecord, SessionCreateState,
    validate_nonce, validate_session_id,
};

impl ConnectionJournal {
    /// Record or read one exact create intent without authorizing a request.
    pub(crate) async fn prepare_session_create(
        &self,
        local_nonce: &str,
    ) -> Result<SessionCreateRecord, HostError> {
        validate_nonce(local_nonce)?;
        let connection_id = self.connection_id().as_str().to_owned();
        let local_nonce = local_nonce.to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let scale_set_id = self.required_set_id(connection).await?;
            if has_ambiguous_legacy_work(connection).await? {
                return Err(HostError::Journal);
            }
            if let Some(record) =
                read_record(connection, self.connection_id().clone(), scale_set_id, &local_nonce)
                    .await?
            {
                return Ok(record);
            }
            let allocations = load(connection).await?.charged_allocations();
            if allocations.iter().any(|allocation| {
                allocation.connection_id() == self.connection_id()
                    && allocation.scale_set_id() == scale_set_id
            }) {
                return Err(HostError::Journal);
            }
            let mut rows = connection
                .query(
                    "SELECT EXISTS (SELECT 1 FROM connection_intents WHERE connection_id = ?1 AND scale_set_id = ?2 AND kind = 'session' AND cleanup_proven = 0)",
                    (connection_id.as_str(), scale_set_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let active_session = rows
                .next()
                .await
                .map_err(|_| HostError::Journal)?
                .ok_or(HostError::Journal)?
                .get::<i64>(0)
                .map_err(|_| HostError::Journal)?
                != 0;
            if active_session {
                return Err(HostError::Journal);
            }
            connection
                .execute(
                    "INSERT INTO connection_session_creates (connection_id, scale_set_id, local_nonce, state) VALUES (?1, ?2, ?3, 'prepared')",
                    (connection_id.as_str(), scale_set_id, local_nonce.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            read_record(connection, self.connection_id().clone(), scale_set_id, &local_nonce)
                .await?
                .ok_or(HostError::Journal)
        })
        .await
    }

    /// Give one-shot authority only while the exact attempt remains prepared.
    pub(crate) async fn claim_session_create(
        &self,
        local_nonce: &str,
    ) -> Result<Option<SessionCreateClaim>, HostError> {
        validate_nonce(local_nonce)?;
        let connection_id = self.connection_id().as_str().to_owned();
        let local_nonce = local_nonce.to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            if has_ambiguous_legacy_work(connection).await? {
                return Ok(None);
            }
            let scale_set_id = self.required_set_id(connection).await?;
            let changed = connection
                .execute(
                    "UPDATE connection_session_creates SET state = 'requested' WHERE connection_id = ?1 AND scale_set_id = ?2 AND local_nonce = ?3 AND state = 'prepared'",
                    (connection_id.as_str(), scale_set_id, local_nonce.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            match changed {
                0 => Ok(None),
                1 => Ok(Some(SessionCreateClaim::new(
                    self.connection_id().clone(),
                    scale_set_id,
                    local_nonce.clone(),
                ))),
                _ => Err(HostError::Journal),
            }
        })
        .await
    }

    /// Bind the returned remote ID and session row in the claim's transaction.
    pub(crate) async fn record_created_session(
        &self,
        claim: &SessionCreateClaim,
        session_id: &str,
        target: u32,
    ) -> Result<BoundConnectionSession, HostError> {
        validate_session_id(session_id)?;
        if claim.connection_id != *self.connection_id() {
            return Err(HostError::Journal);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        let local_nonce = claim.local_nonce.clone();
        let session_id = session_id.to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let scale_set_id = self.required_set_id(connection).await?;
            if scale_set_id != claim.scale_set_id {
                return Err(HostError::Journal);
            }
            let mut rows = connection
                .query(
                    "SELECT state FROM connection_session_creates WHERE connection_id = ?1 AND scale_set_id = ?2 AND local_nonce = ?3",
                    (connection_id.as_str(), scale_set_id, local_nonce.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let state = rows
                .next()
                .await
                .map_err(|_| HostError::Journal)?
                .ok_or(HostError::Journal)?
                .get::<String>(0)
                .map_err(|_| HostError::Journal)?;
            if state != "requested" {
                return Err(HostError::Journal);
            }
            connection
                .execute(
                    "INSERT INTO connection_intents (connection_id, kind, subject, state, scale_set_id) VALUES (?1, 'session', ?2, 'done', ?3)",
                    (connection_id.as_str(), session_id.as_str(), scale_set_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let intent_id = connection.last_insert_rowid();
            let mut ledger = load(connection).await?;
            let allocation = match ledger.reserve(
                self.connection_id().clone(),
                scale_set_id,
                &session_id,
                target,
            ) {
                Ok(allocation) => allocation,
                Err(HeaderLedgerError::OverBudget) if target != 0 => ledger
                    .reserve(self.connection_id().clone(), scale_set_id, &session_id, 0)
                    .map_err(|_| HostError::Journal)?,
                Err(_) => return Err(HostError::Journal),
            };
            let changed = connection
                .execute(
                    "UPDATE connection_session_creates SET state = 'bound', session_id = ?1, intent_id = ?2 WHERE connection_id = ?3 AND scale_set_id = ?4 AND local_nonce = ?5 AND state = 'requested'",
                    (session_id.as_str(), intent_id, connection_id.as_str(), scale_set_id, local_nonce.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            one_row(changed)?;
            save(connection, &ledger).await?;
            Ok(BoundConnectionSession {
                intent_id,
                allocation,
            })
        })
        .await
    }

    /// Keep the claimed create charged when its remote outcome is unknown.
    pub(crate) async fn mark_session_create_uncertain(
        &self,
        claim: &SessionCreateClaim,
    ) -> Result<(), HostError> {
        if claim.connection_id != *self.connection_id() {
            return Err(HostError::Journal);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            if self.required_set_id(connection).await? != claim.scale_set_id {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE connection_session_creates SET state = 'uncertain' WHERE connection_id = ?1 AND scale_set_id = ?2 AND local_nonce = ?3 AND state = 'requested'",
                    (connection_id.as_str(), claim.scale_set_id, claim.local_nonce.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            one_row(changed)
        })
        .await
    }

    /// Read all create and bound-session records for this immutable connection.
    pub(crate) async fn session_creates(&self) -> Result<Vec<SessionCreateRecord>, HostError> {
        read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let mut rows = connection
                .query(
                    "SELECT scale_set_id, local_nonce, state, session_id, intent_id FROM connection_session_creates WHERE connection_id = ?1 ORDER BY scale_set_id, local_nonce",
                    [self.connection_id().as_str()],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let mut records = Vec::new();
            while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
                records.push(SessionCreateRecord {
                    connection_id: self.connection_id().clone(),
                    scale_set_id: row.get(0).map_err(|_| HostError::Journal)?,
                    local_nonce: row.get(1).map_err(|_| HostError::Journal)?,
                    state: SessionCreateState::parse(
                        &row.get::<String>(2).map_err(|_| HostError::Journal)?,
                    )?,
                    session_id: row.get(3).map_err(|_| HostError::Journal)?,
                    intent_id: row.get(4).map_err(|_| HostError::Journal)?,
                });
            }
            Ok(records)
        })
        .await
    }
}

async fn read_record(
    connection: &turso::Connection,
    connection_id: crate::config::ConnectionId,
    scale_set_id: i64,
    local_nonce: &str,
) -> Result<Option<SessionCreateRecord>, HostError> {
    let mut rows = connection
        .query(
            "SELECT state, session_id, intent_id FROM connection_session_creates WHERE connection_id = ?1 AND scale_set_id = ?2 AND local_nonce = ?3",
            (connection_id.as_str(), scale_set_id, local_nonce),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    Ok(Some(SessionCreateRecord {
        connection_id,
        scale_set_id,
        local_nonce: local_nonce.to_owned(),
        state: SessionCreateState::parse(&row.get::<String>(0).map_err(|_| HostError::Journal)?)?,
        session_id: row.get(1).map_err(|_| HostError::Journal)?,
        intent_id: row.get(2).map_err(|_| HostError::Journal)?,
    }))
}
