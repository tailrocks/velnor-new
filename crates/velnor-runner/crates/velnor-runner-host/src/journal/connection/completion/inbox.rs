//! Completion inbox keyed by connection and Scale Set.

use crate::config::ConnectionId;
use crate::error::HostError;
use crate::journal::{MAX_COMPLETION_BODY_BYTES, MAX_COMPLETION_INBOX_SCAN};

use super::super::ConnectionJournal;
use super::super::storage::{in_transaction, read_transaction};

/// One persisted completion body awaiting an exact local launch match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConnectionCompletionInboxEntry {
    pub(crate) connection_id: ConnectionId,
    pub(crate) scale_set_id: i64,
    pub(crate) message_id: i64,
    pub(crate) raw_body: String,
    pub(crate) attempts: i64,
}

impl ConnectionJournal {
    /// Store an unmatched completion body before acknowledging its queue message.
    pub(crate) async fn store_completion_inbox(
        &self,
        message_id: i64,
        raw_body: &str,
    ) -> Result<(), HostError> {
        validate_body(message_id, raw_body)?;
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let set_id = self.required_set_id(connection).await?;
            let mut rows = connection
                .query(
                    "SELECT raw_body FROM connection_completion_inbox WHERE connection_id = ?1 AND scale_set_id = ?2 AND message_id = ?3",
                    (self.connection_id.as_str(), set_id, message_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
                let existing: String = row.get(0).map_err(|_| HostError::Journal)?;
                return if existing == raw_body {
                    Ok(())
                } else {
                    Err(HostError::Journal)
                };
            }
            connection
                .execute(
                    "INSERT INTO connection_completion_inbox (connection_id, scale_set_id, message_id, raw_body) VALUES (?1, ?2, ?3, ?4)",
                    (self.connection_id.as_str(), set_id, message_id, raw_body),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(())
        })
        .await
    }

    /// Read this connection's due completion bodies.
    pub(crate) async fn pending_completion_inbox(
        &self,
        now: i64,
        limit: u32,
    ) -> Result<Vec<ConnectionCompletionInboxEntry>, HostError> {
        if now < 0 {
            return Err(HostError::Journal);
        }
        let bounded = limit.min(MAX_COMPLETION_INBOX_SCAN);
        if bounded == 0 {
            return Ok(Vec::new());
        }
        read_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let set_id = self.required_set_id(connection).await?;
            let mut rows = connection
                .query(
                    "SELECT message_id, raw_body, attempts FROM connection_completion_inbox WHERE connection_id = ?1 AND scale_set_id = ?2 AND retry_after <= ?3 ORDER BY retry_after, message_id LIMIT ?4",
                    (self.connection_id.as_str(), set_id, now, i64::from(bounded)),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let mut entries = Vec::new();
            while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
                entries.push(ConnectionCompletionInboxEntry {
                    connection_id: self.connection_id.clone(),
                    scale_set_id: set_id,
                    message_id: row.get(0).map_err(|_| HostError::Journal)?,
                    raw_body: row.get(1).map_err(|_| HostError::Journal)?,
                    attempts: row.get(2).map_err(|_| HostError::Journal)?,
                });
            }
            Ok(entries)
        })
        .await
    }

    /// Schedule one exact body for a bounded retry.
    pub(crate) async fn defer_completion_inbox(
        &self,
        entry: &ConnectionCompletionInboxEntry,
        now: i64,
    ) -> Result<(), HostError> {
        validate_entry(self, entry, now)?;
        let attempts = entry.attempts.checked_add(1).ok_or(HostError::Journal)?;
        let delay = 1_i64
            .checked_shl(u32::try_from(entry.attempts.min(8)).map_err(|_| HostError::Journal)?)
            .ok_or(HostError::Journal)?
            .min(300);
        let retry_after = now.checked_add(delay).ok_or(HostError::Journal)?;
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let set_id = self.required_set_id(connection).await?;
            if set_id != entry.scale_set_id {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE connection_completion_inbox SET attempts = ?1, retry_after = ?2 WHERE connection_id = ?3 AND scale_set_id = ?4 AND message_id = ?5 AND raw_body = ?6 AND attempts = ?7",
                    (attempts, retry_after, self.connection_id.as_str(), set_id, entry.message_id, entry.raw_body.as_str(), entry.attempts),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1 {
                Ok(())
            } else {
                Err(HostError::Journal)
            }
        })
        .await
    }

    /// Remove only the exact body after each completion has a durable match.
    pub(crate) async fn resolve_completion_inbox(
        &self,
        entry: &ConnectionCompletionInboxEntry,
    ) -> Result<(), HostError> {
        validate_entry(self, entry, 0)?;
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let set_id = self.required_set_id(connection).await?;
            if set_id != entry.scale_set_id {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "DELETE FROM connection_completion_inbox WHERE connection_id = ?1 AND scale_set_id = ?2 AND message_id = ?3 AND raw_body = ?4",
                    (self.connection_id.as_str(), set_id, entry.message_id, entry.raw_body.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1 {
                return Ok(());
            }
            let mut rows = connection
                .query(
                    "SELECT raw_body FROM connection_completion_inbox WHERE connection_id = ?1 AND scale_set_id = ?2 AND message_id = ?3",
                    (self.connection_id.as_str(), set_id, entry.message_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
                Err(HostError::Journal)
            } else {
                Ok(())
            }
        })
        .await
    }
}

fn validate_body(message_id: i64, raw_body: &str) -> Result<(), HostError> {
    if message_id < 0 || raw_body.is_empty() || raw_body.len() > MAX_COMPLETION_BODY_BYTES {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn validate_entry(
    scope: &ConnectionJournal,
    entry: &ConnectionCompletionInboxEntry,
    now: i64,
) -> Result<(), HostError> {
    if entry.connection_id != *scope.connection_id()
        || entry.scale_set_id <= 0
        || entry.message_id < 0
        || entry.attempts < 0
        || now < 0
        || entry.raw_body.is_empty()
        || entry.raw_body.len() > MAX_COMPLETION_BODY_BYTES
    {
        return Err(HostError::Journal);
    }
    Ok(())
}
