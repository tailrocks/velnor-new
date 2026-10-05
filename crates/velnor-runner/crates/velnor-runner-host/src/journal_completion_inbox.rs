//! Durable inbox for completion messages that do not yet match a launch.

use crate::error::HostError;
use crate::journal::Journal;

use super::finish_transaction;

pub(crate) const MAX_COMPLETION_BODY_BYTES: usize = velnor_runner_github::MAX_POLL_BODY_BYTES;
pub(crate) const MAX_COMPLETION_INBOX_SCAN: u32 = 4;

/// One persisted completion body awaiting durable launch identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompletionInboxEntry {
    /// Owning runner scale set.
    pub(crate) scale_set_id: i64,
    /// Stable poll message ID.
    pub(crate) message_id: i64,
    /// Exact inner message body from the authenticated queue envelope.
    pub(crate) raw_body: String,
    /// Number of bounded local match attempts.
    pub(crate) attempts: i64,
}

impl Journal {
    /// Persist an unmatched body before the queue message can be acknowledged.
    ///
    pub(crate) async fn store_completion_inbox(
        &self,
        scale_set_id: i64,
        message_id: i64,
        raw_body: &str,
    ) -> Result<(), HostError> {
        validate_key_body(scale_set_id, message_id, raw_body)?;
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = insert_or_check(&connection, scale_set_id, message_id, raw_body).await;
        finish_transaction(&connection, result).await
    }

    /// Read a small due prefix. The caller performs local journal work only.
    pub(crate) async fn pending_completion_inbox(
        &self,
        now: i64,
        limit: u32,
    ) -> Result<Vec<CompletionInboxEntry>, HostError> {
        if now < 0 {
            return Err(HostError::Journal);
        }
        let bounded = limit.min(MAX_COMPLETION_INBOX_SCAN);
        if bounded == 0 {
            return Ok(Vec::new());
        }
        let connection = self.connection().await?;
        let mut rows = connection
            .query(
                "SELECT scale_set_id, message_id, raw_body, attempts FROM completion_inbox WHERE retry_after <= ?1 ORDER BY retry_after, scale_set_id, message_id LIMIT ?2",
                (now, i64::from(bounded)),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let mut entries = Vec::new();
        while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
            entries.push(CompletionInboxEntry {
                scale_set_id: row.get(0).map_err(|_| HostError::Journal)?,
                message_id: row.get(1).map_err(|_| HostError::Journal)?,
                raw_body: row.get(2).map_err(|_| HostError::Journal)?,
                attempts: row.get(3).map_err(|_| HostError::Journal)?,
            });
        }
        Ok(entries)
    }

    /// Move one unresolved body to a bounded retry time.
    pub(crate) async fn defer_completion_inbox(
        &self,
        entry: &CompletionInboxEntry,
        now: i64,
    ) -> Result<(), HostError> {
        if now < 0 || entry.attempts < 0 {
            return Err(HostError::Journal);
        }
        let attempts = entry.attempts.checked_add(1).ok_or(HostError::Journal)?;
        let delay = 1_i64
            .checked_shl(u32::try_from(entry.attempts.min(8)).map_err(|_| HostError::Journal)?)
            .ok_or(HostError::Journal)?
            .min(300);
        let retry_after = now.checked_add(delay).ok_or(HostError::Journal)?;
        let connection = self.connection().await?;
        let changed = connection
            .execute(
                "UPDATE completion_inbox SET attempts = ?1, retry_after = ?2 WHERE scale_set_id = ?3 AND message_id = ?4 AND raw_body = ?5 AND attempts = ?6",
                (
                    attempts,
                    retry_after,
                    entry.scale_set_id,
                    entry.message_id,
                    entry.raw_body.clone(),
                    entry.attempts,
                ),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed == 1 {
            Ok(())
        } else {
            Err(HostError::Journal)
        }
    }

    /// Remove only the exact body after every completion in it has a journal match.
    pub(crate) async fn resolve_completion_inbox(
        &self,
        entry: &CompletionInboxEntry,
    ) -> Result<(), HostError> {
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = resolve_exact(&connection, entry).await;
        finish_transaction(&connection, result).await
    }
}

async fn insert_or_check(
    connection: &turso::Connection,
    scale_set_id: i64,
    message_id: i64,
    raw_body: &str,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT raw_body FROM completion_inbox WHERE scale_set_id = ?1 AND message_id = ?2",
            (scale_set_id, message_id),
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
            "INSERT INTO completion_inbox (scale_set_id, message_id, raw_body) VALUES (?1, ?2, ?3)",
            (scale_set_id, message_id, raw_body.to_owned()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn resolve_exact(
    connection: &turso::Connection,
    entry: &CompletionInboxEntry,
) -> Result<(), HostError> {
    let changed = connection
        .execute(
            "DELETE FROM completion_inbox WHERE scale_set_id = ?1 AND message_id = ?2 AND raw_body = ?3",
            (
                entry.scale_set_id,
                entry.message_id,
                entry.raw_body.clone(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        return Ok(());
    }
    let mut rows = connection
        .query(
            "SELECT raw_body FROM completion_inbox WHERE scale_set_id = ?1 AND message_id = ?2",
            (entry.scale_set_id, entry.message_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    match rows.next().await.map_err(|_| HostError::Journal)? {
        Some(_) => Err(HostError::Journal),
        None => Ok(()),
    }
}

fn validate_key_body(scale_set_id: i64, message_id: i64, raw_body: &str) -> Result<(), HostError> {
    if scale_set_id <= 0
        || message_id < 0
        || raw_body.is_empty()
        || raw_body.len() > MAX_COMPLETION_BODY_BYTES
    {
        return Err(HostError::Journal);
    }
    Ok(())
}
