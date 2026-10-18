//! Shared SQL transactions and connection-scope queries.

use sha2::{Digest, Sha256};
use std::ops::AsyncFnOnce;

use crate::error::HostError;

pub(in crate::journal) async fn in_transaction<T>(
    journal: &crate::journal::Journal,
    operation: impl AsyncFnOnce(&turso::Connection) -> Result<T, HostError>,
) -> Result<T, HostError> {
    let _write = journal.write_guard().await;
    journal.sync_lineage().await?;
    let connection = journal.connection().await?;
    connection
        .execute("BEGIN IMMEDIATE", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let revision = match crate::journal_schema::revision(&connection).await {
        Ok(revision) => revision,
        Err(error) => {
            connection
                .execute("ROLLBACK", ())
                .await
                .map_err(|_| HostError::Journal)?;
            return Err(error);
        }
    };
    let next_revision = revision
        .checked_add(1)
        .and_then(|revision| i64::try_from(revision).ok());
    let Some(next_revision) = next_revision else {
        connection
            .execute("ROLLBACK", ())
            .await
            .map_err(|_| HostError::Journal)?;
        return Err(HostError::Journal);
    };
    let result = operation(&connection).await;
    match result {
        Ok(value) => {
            let advanced = connection
                .execute(
                    "UPDATE journal_meta SET revision = ?1 WHERE singleton = 1",
                    [next_revision],
                )
                .await;
            if advanced.is_err() {
                connection
                    .execute("ROLLBACK", ())
                    .await
                    .map_err(|_| HostError::Journal)?;
                return Err(HostError::Journal);
            }
            if connection.execute("COMMIT", ()).await.is_ok() {
                journal.sync_after(Ok(value)).await
            } else {
                connection
                    .execute("ROLLBACK", ())
                    .await
                    .map_err(|_| HostError::Journal)?;
                Err(HostError::Journal)
            }
        }
        Err(error) => {
            connection
                .execute("ROLLBACK", ())
                .await
                .map_err(|_| HostError::Journal)?;
            Err(error)
        }
    }
}

pub(in crate::journal) async fn read_transaction<T>(
    journal: &crate::journal::Journal,
    operation: impl AsyncFnOnce(&turso::Connection) -> Result<T, HostError>,
) -> Result<T, HostError> {
    let _write = journal.write_guard().await;
    journal.sync_lineage().await?;
    let connection = journal.connection().await?;
    connection
        .execute("BEGIN", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let result = operation(&connection).await;
    match result {
        Ok(value) => {
            connection
                .execute("COMMIT", ())
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(value)
        }
        Err(error) => {
            connection
                .execute("ROLLBACK", ())
                .await
                .map_err(|_| HostError::Journal)?;
            Err(error)
        }
    }
}

pub(super) async fn connection_record(
    connection: &turso::Connection,
    connection_id: &str,
) -> Result<Option<(String, bool)>, HostError> {
    let mut rows = connection
        .query(
            "SELECT identity, retired FROM connection_scopes WHERE connection_id = ?1",
            [connection_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    Ok(Some((
        row.get(0).map_err(|_| HostError::Journal)?,
        row.get::<i64>(1).map_err(|_| HostError::Journal)? != 0,
    )))
}

pub(super) async fn scoped_live_id(
    connection: &turso::Connection,
    connection_id: &str,
    kind: &str,
    subject: &str,
) -> Result<Option<i64>, HostError> {
    let mut rows = connection
        .query(
            "SELECT id FROM connection_intents WHERE connection_id = ?1 AND kind = ?2 AND subject = ?3 AND state != 'failed' AND cleanup_proven = 0 ORDER BY id DESC LIMIT 1",
            (connection_id, kind, subject),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    Ok(Some(row.get(0).map_err(|_| HostError::Journal)?))
}

pub(super) async fn scalar_count(
    connection: &turso::Connection,
    query: &str,
) -> Result<u64, HostError> {
    let mut rows = connection
        .query(query, ())
        .await
        .map_err(|_| HostError::Journal)?;
    let count = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    u64::try_from(count).map_err(|_| HostError::Journal)
}

pub(super) async fn has_ambiguous_legacy_work(
    connection: &turso::Connection,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT EXISTS (SELECT 1 FROM intents WHERE cleanup_proven = 0) OR EXISTS (SELECT 1 FROM completion_inbox) OR EXISTS (SELECT 1 FROM completion_cleanup AS c JOIN intents AS i ON i.id = c.intent_id WHERE i.cleanup_proven = 0) OR EXISTS (SELECT 1 FROM launch_recovery AS r JOIN intents AS i ON i.id = r.intent_id WHERE i.cleanup_proven = 0)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let result = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    Ok(result != 0)
}

pub(super) async fn has_unresolved_work(connection: &turso::Connection) -> Result<bool, HostError> {
    if has_ambiguous_legacy_work(connection).await? {
        return Ok(true);
    }
    let count = scalar_count(
        connection,
        "SELECT COUNT(*) FROM connection_intents WHERE cleanup_proven = 0",
    )
    .await?;
    if count != 0 {
        return Ok(true);
    }
    let scoped_inbox = scalar_count(
        connection,
        "SELECT COUNT(*) FROM connection_completion_inbox",
    )
    .await?;
    if scoped_inbox != 0 {
        return Ok(true);
    }
    let image_imports = scalar_count(
        connection,
        "SELECT COUNT(*) FROM image_imports WHERE state IN ('prepared', 'requested', 'uncertain')",
    )
    .await?;
    if image_imports != 0 {
        return Ok(true);
    }
    let scale_set_creates = scalar_count(
        connection,
        "SELECT COUNT(*) FROM connection_scale_set_intents WHERE state IN ('prepared', 'requested', 'uncertain')",
    )
    .await?;
    Ok(scale_set_creates != 0)
}

pub(super) async fn has_connection_work(
    connection: &turso::Connection,
    connection_id: &str,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT EXISTS (SELECT 1 FROM capacity_reservations WHERE connection_id = ?1 AND state != 'released') OR EXISTS (SELECT 1 FROM connection_intents WHERE connection_id = ?1 AND cleanup_proven = 0) OR EXISTS (SELECT 1 FROM connection_completion_inbox WHERE connection_id = ?1) OR EXISTS (SELECT 1 FROM connection_scale_set_intents WHERE connection_id = ?1 AND state IN ('prepared', 'requested', 'uncertain'))",
            [connection_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let result = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    Ok(result != 0)
}

pub(super) fn digest(value: &[u8]) -> String {
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(value) {
        output.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        output.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
    }
    output
}
