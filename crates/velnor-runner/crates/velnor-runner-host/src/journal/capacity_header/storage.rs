//! Durable header-ledger snapshots in the shared journal file.

use crate::error::HostError;
use crate::journal::Journal;
use crate::journal::connection::storage::{in_transaction, read_transaction};

use super::{HeaderLedger, HeaderLedgerError, HeaderLedgerSnapshot};

const SCHEMA_VERSION: i64 = 2;
const PREVIOUS_SCHEMA_VERSION: i64 = 1;
const MAX_SNAPSHOT_BYTES: usize = 1_048_576;
const CREATE_TABLE: &str = "CREATE TABLE IF NOT EXISTS capacity_header_meta (singleton INTEGER PRIMARY KEY CHECK (singleton = 1), schema_version INTEGER NOT NULL CHECK (schema_version > 0), snapshot_json TEXT NOT NULL CHECK (length(CAST(snapshot_json AS BLOB)) <= 1048576), snapshot_digest TEXT NOT NULL CHECK (length(snapshot_digest) = 64 AND snapshot_digest NOT GLOB '*[^0-9a-f]*'))";

/// Recover the snapshot at process startup using the journal's write transaction.
///
/// Call only after this process owns the authoritative engine/journal guard.
/// Persisted in-flight polls and deletes are marked uncertain and remain charged.
/// Do not call during ordinary transactions or config reloads.
pub(in crate::journal) async fn bootstrap(journal: &Journal) -> Result<(), HostError> {
    in_transaction(journal, async |connection| {
        bootstrap_connection(connection).await
    })
    .await
}

/// Validate the table and any saved snapshot using a consistent journal read.
pub(in crate::journal) async fn validate(journal: &Journal) -> Result<(), HostError> {
    read_transaction(journal, async |connection| {
        validate_connection(connection).await
    })
    .await
}

/// Validate and persist the ledger after a caller changes the host job limit.
///
/// Call inside the same journal transaction that updates `host_capacity_policy`.
/// The write fails when the new limit is below the sum of charged headers.
pub(in crate::journal) async fn validate_capacity_policy_connection(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    let ledger = load(connection).await?;
    save(connection, &ledger).await
}

/// Run one in-memory transition and save it in the same journal transaction.
///
/// Poll-intent callers must wait for this function to commit before sending
/// the corresponding HTTP request.
pub(in crate::journal) async fn transition<T>(
    journal: &Journal,
    operation: impl FnOnce(&mut HeaderLedger) -> Result<T, HeaderLedgerError>,
) -> Result<T, HostError> {
    in_transaction(journal, async move |connection| {
        let mut ledger = load(connection).await?;
        let output = operation(&mut ledger).map_err(|_| HostError::Journal)?;
        save(connection, &ledger).await?;
        Ok(output)
    })
    .await
}

/// Create and validate only the local table inside an existing journal transaction.
///
/// This migration helper does not classify persisted polls as interrupted.
pub(in crate::journal) async fn bootstrap_schema_connection(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    connection
        .execute(CREATE_TABLE, ())
        .await
        .map_err(|_| HostError::Journal)?;
    validate_table(connection).await?;
    Ok(())
}

/// Recover the snapshot inside an existing startup journal transaction.
pub(in crate::journal) async fn bootstrap_connection(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    bootstrap_schema_connection(connection).await?;
    let max_jobs = read_host_max_jobs(connection).await?;
    let ledger = match read_snapshot(connection).await? {
        Some((version, json, digest)) => {
            let mut ledger = decode_snapshot(version, &json, &digest, true)?;
            if ledger.max_jobs() != max_jobs {
                ledger
                    .set_max_jobs(max_jobs)
                    .map_err(|_| HostError::Journal)?;
            }
            ledger
        }
        None => HeaderLedger::new(max_jobs).map_err(|_| HostError::Journal)?,
    };
    save(connection, &ledger).await
}

/// Validate the local table and its optional singleton row.
pub(in crate::journal) async fn validate_connection(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    validate_table(connection).await?;
    let snapshot = read_snapshot(connection).await?;
    let mut rows = connection
        .query("SELECT 1 FROM host_capacity_policy WHERE singleton = 1", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let policy_exists = rows.next().await.map_err(|_| HostError::Journal)?.is_some();
    if snapshot.is_some() || policy_exists {
        let _ledger = load(connection).await?;
    }
    Ok(())
}

/// Load the singleton ledger inside the caller's `BEGIN IMMEDIATE` transaction.
pub(in crate::journal) async fn load(
    connection: &turso::Connection,
) -> Result<HeaderLedger, HostError> {
    validate_table(connection).await?;
    let max_jobs = read_host_max_jobs(connection).await?;
    let Some((version, json, digest)) = read_snapshot(connection).await? else {
        return HeaderLedger::new(max_jobs).map_err(|_| HostError::Journal);
    };
    let mut ledger = decode_snapshot(version, &json, &digest, false)?;
    if ledger.max_jobs() != max_jobs {
        ledger
            .set_max_jobs(max_jobs)
            .map_err(|_| HostError::Journal)?;
    }
    Ok(ledger)
}

/// Check whether the ledger keeps any generation for one repository connection charged.
pub(in crate::journal) async fn has_charged_connection(
    connection: &turso::Connection,
    connection_id: &str,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query("SELECT 1 FROM capacity_header_meta WHERE singleton = 1", ())
        .await
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_none() {
        return Ok(false);
    }
    Ok(load(connection)
        .await?
        .charged_allocations()
        .iter()
        .any(|allocation| allocation.connection_id().as_str() == connection_id))
}

/// Check whether any session header generation remains charged.
pub(in crate::journal) async fn has_any_charged(
    connection: &turso::Connection,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query("SELECT 1 FROM capacity_header_meta WHERE singleton = 1", ())
        .await
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_none() {
        return Ok(false);
    }
    Ok(!load(connection).await?.charged_allocations().is_empty())
}

/// Save one validated ledger snapshot inside the caller's transaction.
pub(in crate::journal) async fn save(
    connection: &turso::Connection,
    ledger: &HeaderLedger,
) -> Result<(), HostError> {
    validate_table(connection).await?;
    if ledger.max_jobs() != read_host_max_jobs(connection).await?
        || ledger.charged_capacity() > ledger.max_jobs()
    {
        return Err(HostError::Journal);
    }
    let snapshot = ledger.snapshot();
    HeaderLedger::restore_live(snapshot.clone()).map_err(|_| HostError::Journal)?;
    let json = serde_json::to_vec(&snapshot).map_err(|_| HostError::Journal)?;
    check_snapshot_size(&json)?;
    let json = String::from_utf8(json).map_err(|_| HostError::Journal)?;
    let digest = snapshot_digest(json.as_bytes());
    let changed = connection
        .execute(
            "INSERT INTO capacity_header_meta (singleton, schema_version, snapshot_json, snapshot_digest) VALUES (1, ?1, ?2, ?3) ON CONFLICT (singleton) DO UPDATE SET schema_version = excluded.schema_version, snapshot_json = excluded.snapshot_json, snapshot_digest = excluded.snapshot_digest",
            (SCHEMA_VERSION, json, digest),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed != 1 {
        return Err(HostError::Journal);
    }
    Ok(())
}

#[path = "storage/persistence.rs"]
mod persistence;
#[cfg(test)]
use persistence::{HeaderLedgerSnapshotV1, HeaderRecordV1};
use persistence::{
    check_snapshot_size, decode_snapshot, read_host_max_jobs, read_snapshot, snapshot_digest,
    validate_table,
};

#[cfg(test)]
#[path = "storage/tests.rs"]
mod tests;
