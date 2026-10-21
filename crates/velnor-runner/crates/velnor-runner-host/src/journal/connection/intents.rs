#![cfg(test)]

//! Connection-scoped idempotent intents.

use crate::error::HostError;
use crate::journal::sql::{one_row, token_rejected};
use crate::journal::{IntentState, Outcome};

use super::AssignmentKey;
use super::ConnectionJournal;
use super::storage::{has_ambiguous_legacy_work, in_transaction, read_transaction, scoped_live_id};

impl ConnectionJournal {
    /// Insert or replay a pending intent inside this connection.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for an invalid key or journal error.
    pub(crate) async fn begin(&self, kind: &str, subject: &str) -> Result<i64, HostError> {
        if token_rejected(kind)
            || token_rejected(subject)
            || kind.len() > 128
            || subject.len() > 1024
            || kind == "launch"
        {
            return Err(HostError::Journal);
        }
        let key = if kind == "session" {
            AssignmentKey::Session {
                name: subject.to_owned(),
            }
        } else {
            AssignmentKey::Remote {
                kind: kind.to_owned(),
                subject: subject.to_owned(),
            }
        };
        let identity = self.assignment_identity(key).await?;
        let set_id = identity.scale_set_id();
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            self.validate_assignment(connection, &identity).await?;
            if has_ambiguous_legacy_work(connection).await? {
                return Err(HostError::Journal);
            }
            if let Some(id) = scoped_live_id(
                connection,
                self.connection_id.as_str(),
                kind,
                subject,
            )
            .await?
            {
                return Ok(id);
            }
            connection
                .execute(
                    "INSERT INTO connection_intents (connection_id, kind, subject, state, scale_set_id) VALUES (?1, ?2, ?3, 'pending', ?4)",
                    (self.connection_id.as_str(), kind, subject, set_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(connection.last_insert_rowid())
        })
        .await
    }

    /// Record the outcome of one intent in this connection.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the row is missing or terminal.
    pub(crate) async fn finish(&self, id: i64, outcome: Outcome) -> Result<(), HostError> {
        let state = match outcome {
            Outcome::Done => IntentState::Done,
            Outcome::Uncertain => IntentState::Uncertain,
            Outcome::DefiniteFailure => IntentState::Failed,
        };
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let set_id = self.required_set_id(connection).await?;
            let changed = connection
                .execute(
                    "UPDATE connection_intents SET state = ?1 WHERE id = ?2 AND connection_id = ?3 AND scale_set_id = ?4 AND cleanup_proven = 0",
                    (state.as_str(), id, self.connection_id.as_str(), set_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            one_row(changed)
        })
        .await
    }

    /// Read one intent only from this connection.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the row is missing or belongs elsewhere.
    pub(crate) async fn read(&self, id: i64) -> Result<IntentState, HostError> {
        read_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let set_id = self.required_set_id(connection).await?;
            let mut rows = connection
                .query(
                    "SELECT state FROM connection_intents WHERE id = ?1 AND connection_id = ?2 AND scale_set_id = ?3",
                    (id, self.connection_id.as_str(), set_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let row = rows
                .next()
                .await
                .map_err(|_| HostError::Journal)?
                .ok_or(HostError::Journal)?;
            let state: String = row.get(0).map_err(|_| HostError::Journal)?;
            IntentState::parse(&state)
        })
        .await
    }

    /// Mark an intent clean after the caller proves its remote resources are absent.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the row is missing or still reserved.
    pub(crate) async fn record_cleanup(&self, id: i64) -> Result<(), HostError> {
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let set_id = self.required_set_id(connection).await?;
            let changed = connection
                .execute(
                    "UPDATE connection_intents SET cleanup_proven = 1 WHERE id = ?1 AND connection_id = ?2 AND scale_set_id = ?3 AND cleanup_proven = 0 AND NOT EXISTS (SELECT 1 FROM capacity_reservations WHERE intent_id = ?1 AND state != 'released') AND NOT EXISTS (SELECT 1 FROM connection_completion_cleanup WHERE intent_id = ?1)",
                    (id, self.connection_id.as_str(), set_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            one_row(changed)
        })
        .await
    }
}
