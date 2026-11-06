// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;
use crate::journal::Journal;
use crate::journal::connection::storage::{in_transaction, read_transaction};

use super::{HeaderLedger, HeaderLedgerSnapshot};

const SCHEMA_VERSION: i64 = 2;
const PREVIOUS_SCHEMA_VERSION: i64 = 1;
const MAX_SNAPSHOT_BYTES: usize = 1_048_576;
const CREATE_TABLE: &str = "CREATE TABLE IF NOT EXISTS capacity_header_meta (singleton INTEGER PRIMARY KEY CHECK (singleton = 1), schema_version INTEGER NOT NULL CHECK (schema_version > 0), snapshot_json TEXT NOT NULL CHECK (length(CAST(snapshot_json AS BLOB)) <= 1048576), snapshot_digest TEXT NOT NULL CHECK (length(snapshot_digest) = 64 AND snapshot_digest NOT GLOB '*[^0-9a-f]*'))";

pub(in crate::journal) async fn bootstrap(journal: &Journal) -> Result<(), HostError> {
    in_transaction(journal, async |connection| {
        bootstrap_connection(connection).await
    })
    .await
}

pub(in crate::journal) async fn validate(journal: &Journal) -> Result<(), HostError> {
    read_transaction(journal, async |connection| {
        validate_connection(connection).await
    })
    .await
}

pub(in crate::journal) async fn validate_capacity_policy_connection(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    let ledger = load(connection).await?;
    save(connection, &ledger).await
}

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

use persistence::{
    check_snapshot_size, decode_snapshot, read_host_max_jobs, read_snapshot, snapshot_digest,
    validate_table,
};
