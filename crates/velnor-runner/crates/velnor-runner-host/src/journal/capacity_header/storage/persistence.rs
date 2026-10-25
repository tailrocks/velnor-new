use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::super::model::{
    GenerationState, HeaderGeneration, HeaderRecord, HeaderSessionIdentity, InFlightPoll,
};
use super::{
    CREATE_TABLE, HeaderLedger, HeaderLedgerSnapshot, MAX_SNAPSHOT_BYTES, PREVIOUS_SCHEMA_VERSION,
    SCHEMA_VERSION,
};
use crate::error::HostError;

pub(super) async fn validate_table(connection: &turso::Connection) -> Result<(), HostError> {
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
        || normalize_sql(&sql) != normalize_sql(CREATE_TABLE)
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

pub(super) async fn read_snapshot(
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

pub(super) async fn read_host_max_jobs(connection: &turso::Connection) -> Result<u32, HostError> {
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

pub(super) fn decode_snapshot(
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
pub(super) struct HeaderLedgerSnapshotV1 {
    pub(super) max_jobs: u32,
    pub(super) next_generation: u64,
    pub(super) records: Vec<HeaderRecordV1>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HeaderRecordV1 {
    pub(super) identity: HeaderSessionIdentity,
    pub(super) generation: HeaderGeneration,
    pub(super) target: u32,
    pub(super) state: GenerationState,
    pub(super) attempt_sequence: u64,
    pub(super) in_flight_poll: Option<InFlightPoll>,
    pub(super) zero_header_settled: bool,
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
                    pressure_capacity: None,
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

pub(super) fn check_snapshot_size(bytes: &[u8]) -> Result<(), HostError> {
    if bytes.is_empty() || bytes.len() > MAX_SNAPSHOT_BYTES {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn normalize_sql(sql: &str) -> String {
    let mut compact = sql
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    if let Some(remainder) = compact.strip_prefix("createtableifnotexists") {
        compact = format!("createtable{remainder}");
    }
    compact
}

pub(super) fn snapshot_digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        output.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        output.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
    }
    output
}
