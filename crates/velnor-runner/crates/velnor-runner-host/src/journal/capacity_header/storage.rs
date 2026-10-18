//! Durable header-ledger snapshots in the shared journal file.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::HostError;
use crate::journal::Journal;
use crate::journal::connection::storage::{in_transaction, read_transaction};

use super::model::{
    GenerationState, HeaderGeneration, HeaderRecord, HeaderSessionIdentity, InFlightPoll,
};
use super::{HeaderLedger, HeaderLedgerError, HeaderLedgerSnapshot};

const SCHEMA_VERSION: i64 = 2;
const PREVIOUS_SCHEMA_VERSION: i64 = 1;
const MAX_SNAPSHOT_BYTES: usize = 1_048_576;
const CREATE_TABLE: &str = "CREATE TABLE IF NOT EXISTS capacity_header_meta (singleton INTEGER PRIMARY KEY CHECK (singleton = 1), schema_version INTEGER NOT NULL CHECK (schema_version > 0), snapshot_json TEXT NOT NULL CHECK (length(CAST(snapshot_json AS BLOB)) <= 1048576), snapshot_digest TEXT NOT NULL CHECK (length(snapshot_digest) = 64 AND snapshot_digest NOT GLOB '*[^0-9a-f]*'))";
const EXPECTED_TABLE: &str = "CREATE TABLE capacity_header_meta (singleton INTEGER PRIMARY KEY CHECK (singleton = 1), schema_version INTEGER NOT NULL CHECK (schema_version > 0), snapshot_json TEXT NOT NULL CHECK (length(CAST(snapshot_json AS BLOB)) <= 1048576), snapshot_digest TEXT NOT NULL CHECK (length(snapshot_digest) = 64 AND snapshot_digest NOT GLOB '*[^0-9a-f]*'))";

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
    let _ledger = load(connection).await?;
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

async fn validate_table(connection: &turso::Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'capacity_header_meta'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Err(HostError::Journal);
    };
    let sql: String = row.get(0).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || normalize_sql(&sql) != normalize_sql(EXPECTED_TABLE)
    {
        return Err(HostError::Journal);
    }

    let expected = [
        ("singleton", "INTEGER", false, 1),
        ("schema_version", "INTEGER", true, 0),
        ("snapshot_json", "TEXT", true, 0),
        ("snapshot_digest", "TEXT", true, 0),
    ];
    let mut rows = connection
        .query("PRAGMA table_info(capacity_header_meta)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    for (name, declared_type, not_null, primary_key) in expected {
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        let actual = (
            row.get::<String>(1).map_err(|_| HostError::Journal)?,
            row.get::<String>(2).map_err(|_| HostError::Journal)?,
            row.get::<i64>(3).map_err(|_| HostError::Journal)? != 0,
            row.get::<i64>(5).map_err(|_| HostError::Journal)?,
        );
        if actual
            != (
                name.to_owned(),
                declared_type.to_owned(),
                not_null,
                primary_key,
            )
        {
            return Err(HostError::Journal);
        }
    }
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }

    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM sqlite_master WHERE tbl_name = 'capacity_header_meta' AND type IN ('index', 'trigger') AND sql IS NOT NULL",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let extra_objects = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    if extra_objects != 0 {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn read_snapshot(
    connection: &turso::Connection,
) -> Result<Option<(i64, String, String)>, HostError> {
    let mut rows = connection
        .query(
            "SELECT schema_version, snapshot_json, snapshot_digest FROM capacity_header_meta WHERE singleton = 1",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let saved = (
        row.get(0).map_err(|_| HostError::Journal)?,
        row.get(1).map_err(|_| HostError::Journal)?,
        row.get(2).map_err(|_| HostError::Journal)?,
    );
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(Some(saved))
}

async fn read_host_max_jobs(connection: &turso::Connection) -> Result<u32, HostError> {
    let mut rows = connection
        .query(
            "SELECT max_jobs FROM host_capacity_policy WHERE singleton = 1",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let max_jobs = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    let max_jobs = u32::try_from(max_jobs).map_err(|_| HostError::Journal)?;
    if max_jobs == 0 {
        return Err(HostError::Journal);
    }
    Ok(max_jobs)
}

fn decode_snapshot(
    version: i64,
    json: &str,
    digest: &str,
    recover_interrupted_polls: bool,
) -> Result<HeaderLedger, HostError> {
    if !matches!(version, SCHEMA_VERSION | PREVIOUS_SCHEMA_VERSION)
        || digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(HostError::Journal);
    }
    let bytes = json.as_bytes();
    check_snapshot_size(bytes)?;
    if snapshot_digest(bytes) != digest {
        return Err(HostError::Journal);
    }
    let snapshot = if version == PREVIOUS_SCHEMA_VERSION {
        let previous: HeaderLedgerSnapshotV1 =
            serde_json::from_slice(bytes).map_err(|_| HostError::Journal)?;
        if serde_json::to_vec(&previous).map_err(|_| HostError::Journal)? != bytes {
            return Err(HostError::Journal);
        }
        previous.into_current()
    } else {
        let snapshot: HeaderLedgerSnapshot =
            serde_json::from_slice(bytes).map_err(|_| HostError::Journal)?;
        if serde_json::to_vec(&snapshot).map_err(|_| HostError::Journal)? != bytes {
            return Err(HostError::Journal);
        }
        snapshot
    };
    if snapshot
        .records
        .windows(2)
        .any(|records| records[0].identity >= records[1].identity)
    {
        return Err(HostError::Journal);
    }
    let restore = if recover_interrupted_polls {
        HeaderLedger::restore
    } else {
        HeaderLedger::restore_live
    };
    restore(snapshot).map_err(|_| HostError::Journal)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HeaderLedgerSnapshotV1 {
    max_jobs: u32,
    next_generation: u64,
    records: Vec<HeaderRecordV1>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HeaderRecordV1 {
    identity: HeaderSessionIdentity,
    generation: HeaderGeneration,
    target: u32,
    state: GenerationState,
    attempt_sequence: u64,
    in_flight_poll: Option<InFlightPoll>,
    zero_header_settled: bool,
}

impl HeaderLedgerSnapshotV1 {
    fn into_current(self) -> HeaderLedgerSnapshot {
        HeaderLedgerSnapshot {
            max_jobs: self.max_jobs,
            next_generation: self.next_generation,
            records: self
                .records
                .into_iter()
                .map(|record| HeaderRecord {
                    identity: record.identity,
                    generation: record.generation,
                    target: record.target,
                    state: record.state,
                    attempt_sequence: record.attempt_sequence,
                    in_flight_poll: record.in_flight_poll,
                    zero_header_settled: record.zero_header_settled,
                    delete_sequence: 0,
                    in_flight_delete: None,
                    delete_uncertain: false,
                })
                .collect(),
        }
    }
}

fn check_snapshot_size(bytes: &[u8]) -> Result<(), HostError> {
    if bytes.is_empty() || bytes.len() > MAX_SNAPSHOT_BYTES {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn normalize_sql(sql: &str) -> String {
    let compact = sql
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    compact
        .strip_prefix("createtableifnotexists")
        .map_or(compact.clone(), |remainder| {
            format!("createtable{remainder}")
        })
}

fn snapshot_digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        output.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        output.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
    }
    output
}

#[cfg(test)]
#[path = "storage/tests.rs"]
mod tests;
