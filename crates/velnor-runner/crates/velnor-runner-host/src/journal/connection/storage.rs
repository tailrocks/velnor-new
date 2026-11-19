// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use sha2::{Digest, Sha256};
use std::ops::AsyncFnOnce;
use tokio::{
    sync::OwnedMutexGuard,
    time::{Instant, timeout_at},
};

use crate::error::HostError;

pub(in crate::journal) async fn in_transaction<T>(
    journal: &crate::journal::Journal,
    operation: impl AsyncFnOnce(&turso::Connection) -> Result<T, HostError>,
) -> Result<T, HostError> {
    let write = journal.write_guard().await;
    Box::pin(execute_transaction(journal, write, operation)).await
}

pub(in crate::journal) async fn in_transaction_before<T>(
    journal: &crate::journal::Journal,
    deadline: Instant,
    operation: impl AsyncFnOnce(&turso::Connection) -> Result<T, HostError>,
) -> Result<T, HostError> {
    let write = timeout_at(deadline, journal.write_guard())
        .await
        .map_err(|_| HostError::DockerTimeout)?;
    if Instant::now() >= deadline {
        return Err(HostError::DockerTimeout);
    }
    Box::pin(execute_transaction(journal, write, operation)).await
}

async fn execute_transaction<T>(
    journal: &crate::journal::Journal,
    _write: OwnedMutexGuard<()>,
    operation: impl AsyncFnOnce(&turso::Connection) -> Result<T, HostError>,
) -> Result<T, HostError> {
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
        "SELECT COUNT(*) FROM connection_intents AS i WHERE i.cleanup_proven = 0 AND NOT (i.kind = 'session' AND EXISTS (SELECT 1 FROM connection_session_creates AS s WHERE s.intent_id = i.id AND s.state = 'bound'))",
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
        "SELECT (SELECT COUNT(*) FROM image_imports WHERE state IN ('prepared', 'requested', 'uncertain')) + (SELECT COUNT(*) FROM image_import_registry WHERE state IN ('prepared', 'requested', 'uncertain'))",
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
    if scale_set_creates != 0 {
        return Ok(true);
    }
    let session_creates = scalar_count(
        connection,
        "SELECT COUNT(*) FROM connection_session_creates WHERE state IN ('prepared', 'requested', 'uncertain')",
    )
    .await?;
    if session_creates != 0 {
        return Ok(true);
    }
    let sessions = bound_sessions(connection, None).await?;
    if has_charged_outside_sessions(connection, None, &sessions).await? {
        return Ok(true);
    }
    let session_refreshes = scalar_count(
        connection,
        "SELECT COUNT(*) FROM connection_session_refreshes AS r WHERE r.state IN ('requested', 'transport_error', 'abandoned_uncertain', 'uncertain') AND r.generation = (SELECT MAX(latest.generation) FROM connection_session_refreshes AS latest WHERE latest.intent_id = r.intent_id)",
    )
    .await?;
    if session_refreshes != 0 {
        return Ok(true);
    }
    let scale_set_deletes = scalar_count(
        connection,
        "SELECT COUNT(*) FROM connection_scale_set_deletes WHERE state IN ('requested', 'uncertain')",
    )
    .await?;
    if scale_set_deletes != 0 {
        return Ok(true);
    }
    let operation_leases = scalar_count(
        connection,
        "SELECT COUNT(*) FROM host_operation_leases WHERE state != 'released'",
    )
    .await?;
    Ok(operation_leases != 0)
}

pub(super) async fn has_connection_work(
    connection: &turso::Connection,
    connection_id: &str,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT EXISTS (SELECT 1 FROM capacity_reservations WHERE connection_id = ?1 AND state != 'released') OR EXISTS (SELECT 1 FROM connection_intents WHERE connection_id = ?1 AND cleanup_proven = 0) OR EXISTS (SELECT 1 FROM connection_completion_inbox WHERE connection_id = ?1) OR EXISTS (SELECT 1 FROM connection_scale_set_intents WHERE connection_id = ?1 AND state IN ('prepared', 'requested', 'uncertain')) OR EXISTS (SELECT 1 FROM connection_session_creates WHERE connection_id = ?1 AND state IN ('prepared', 'requested', 'uncertain', 'bound'))",
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
    Ok(result != 0
        || crate::journal::capacity_header::has_charged_connection(connection, connection_id)
            .await?)
}

#[expect(
    clippy::struct_field_names,
    reason = "these fields form one immutable connection and session identity"
)]
pub(super) struct BoundSession {
    pub(super) connection_id: String,
    pub(super) intent_id: i64,
    pub(super) scale_set_id: i64,
    pub(super) session_id: String,
}

pub(super) async fn bound_sessions(
    connection: &turso::Connection,
    connection_id: Option<&str>,
) -> Result<Vec<BoundSession>, HostError> {
    let mut rows = connection
        .query(
            "SELECT s.connection_id, s.intent_id, s.scale_set_id, s.session_id FROM connection_session_creates AS s JOIN connection_intents AS i ON i.id = s.intent_id WHERE s.state = 'bound' AND i.kind = 'session' AND i.cleanup_proven = 0 AND (?1 IS NULL OR s.connection_id = ?1)",
            [connection_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut sessions = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        sessions.push(BoundSession {
            connection_id: row.get(0).map_err(|_| HostError::Journal)?,
            intent_id: row.get(1).map_err(|_| HostError::Journal)?,
            scale_set_id: row.get(2).map_err(|_| HostError::Journal)?,
            session_id: row.get(3).map_err(|_| HostError::Journal)?,
        });
    }
    Ok(sessions)
}

pub(super) async fn has_charged_outside_sessions(
    connection: &turso::Connection,
    connection_id: Option<&str>,
    sessions: &[BoundSession],
) -> Result<bool, HostError> {
    let mut rows = connection
        .query("SELECT 1 FROM capacity_header_meta WHERE singleton = 1", ())
        .await
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_none() {
        return Ok(false);
    }
    let ledger = crate::journal::capacity_header::load(connection).await?;
    let unsettled = ledger.unsettled_allocations();
    Ok(ledger
        .charged_allocations()
        .iter()
        .filter(|allocation| {
            connection_id.is_none_or(|id| allocation.connection_id().as_str() == id)
        })
        .any(|allocation| {
            unsettled.contains(allocation)
                || !sessions.iter().any(|session| {
                    allocation.connection_id().as_str() == session.connection_id
                        && allocation.scale_set_id() == session.scale_set_id
                        && allocation.session_id() == session.session_id
                })
        }))
}

pub(super) fn digest(value: &[u8]) -> String {
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(value) {
        output.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        output.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
    }
    output
}
