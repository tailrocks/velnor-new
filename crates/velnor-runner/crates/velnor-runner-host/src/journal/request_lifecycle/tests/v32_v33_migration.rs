use std::path::{Path, PathBuf};

use crate::error::HostError;
use crate::journal::Journal;

use super::helpers::Scratch;

const V32: i64 = 32;
const V33: i64 = 33;

#[derive(Debug, PartialEq, Eq)]
struct ChargedState {
    reservation_id: i64,
    intent_id: i64,
    connection_id: String,
    engine_id: String,
    docker_root_digest: String,
    binding_digest: String,
    cpu_millicores: i64,
    memory_bytes: i64,
    reservation_state: String,
    remote_cleanup_proven: i64,
    local_cleanup_proven: i64,
    request_id: i64,
    acquire_state: String,
    started_message_id: i64,
    started_event_digest: String,
    started_job_id: String,
    started_runner_id: i64,
    started_runner_name: String,
    jit_state: String,
    witness_request_id: i64,
    jit_started_request_id: i64,
    jit_runner_id: i64,
    jit_runner_name: String,
}

#[derive(Debug, PartialEq, Eq)]
struct AttemptState {
    lineages: i64,
    attempts: i64,
    current_attempts: i64,
    generation: i64,
    reservation_id: i64,
    intent_id: i64,
    witness_request_id: i64,
}

#[tokio::test]
async fn v33_opens_empty_v32_database_and_reopen_is_idempotent() -> Result<(), HostError> {
    let (scratch, path, _source_bytes) = copied_fixture("empty-v32.db")?;
    assert_eq!(read_version(&path).await?, V32);
    migrate_and_reopen(&path).await?;
    assert_eq!(read_version(&path).await?, V33);
    assert_eq!(table_count(&path, "physical_attempt_lineages").await?, 0);
    assert_eq!(table_count(&path, "physical_attempts").await?, 0);
    assert_eq!(table_count(&path, "physical_attempt_current").await?, 0);
    assert_eq!(table_count(&path, "physical_attempt_outcomes").await?, 0);
    drop(scratch);
    Ok(())
}

#[tokio::test]
async fn v33_preserves_charged_started_v32_attempt_and_reopen_is_idempotent()
-> Result<(), HostError> {
    let (scratch, path, _source_bytes) = copied_fixture("charged-physical-attempt-v32.db")?;
    assert_eq!(read_version(&path).await?, V32);
    let before = read_charged_state(&path).await?;
    assert_eq!(before.reservation_state, "reserved");
    assert_eq!(before.acquire_state, "acquired");
    assert!(!before.started_event_digest.is_empty());
    assert_eq!(before.jit_state, "started");

    migrate_and_reopen(&path).await?;
    assert_eq!(read_version(&path).await?, V33);
    assert_eq!(read_charged_state(&path).await?, before);
    let migrated_attempt = read_attempt_state(&path).await?;
    assert_eq!(migrated_attempt.lineages, 1);
    assert_eq!(migrated_attempt.attempts, 1);
    assert_eq!(migrated_attempt.current_attempts, 1);
    assert_eq!(migrated_attempt.generation, 1);
    assert_eq!(migrated_attempt.reservation_id, before.reservation_id);
    assert_eq!(migrated_attempt.intent_id, before.intent_id);
    assert_eq!(
        migrated_attempt.witness_request_id,
        before.witness_request_id
    );

    migrate_and_reopen(&path).await?;
    assert_eq!(read_version(&path).await?, V33);
    assert_eq!(read_charged_state(&path).await?, before);
    assert_eq!(read_attempt_state(&path).await?, migrated_attempt);
    drop(scratch);
    Ok(())
}

fn fixture_bytes(name: &str) -> Result<&'static [u8], HostError> {
    match name {
        "empty-v32.db" => Ok(include_bytes!("v32_v33_migration/empty-v32.db")),
        "charged-physical-attempt-v32.db" => Ok(include_bytes!(
            "v32_v33_migration/charged-physical-attempt-v32.db"
        )),
        _ => Err(HostError::Path),
    }
}

fn copied_fixture(name: &str) -> Result<(Scratch, PathBuf, Vec<u8>), HostError> {
    let bytes = fixture_bytes(name)?;
    let scratch = Scratch::new()?;
    let path = scratch.file();
    std::fs::write(&path, bytes).map_err(|_| HostError::Path)?;
    let written = std::fs::read(&path).map_err(|_| HostError::Path)?;
    if written.as_slice() != bytes {
        return Err(HostError::Journal);
    }
    Ok((scratch, path, written))
}

async fn migrate_and_reopen(path: &Path) -> Result<(), HostError> {
    let journal = Journal::open(path).await?;
    assert_eq!(journal_version(&journal).await?, V33);
    drop(journal);
    let reopened = Journal::open(path).await?;
    assert_eq!(journal_version(&reopened).await?, V33);
    drop(reopened);
    Ok(())
}

async fn journal_version(journal: &Journal) -> Result<i64, HostError> {
    let connection = journal.connection().await?;
    let mut rows = connection
        .query("PRAGMA user_version", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    row.get(0).map_err(|_| HostError::Journal)
}

async fn read_version(path: &Path) -> Result<i64, HostError> {
    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    let mut rows = connection
        .query("PRAGMA user_version", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    row.get(0).map_err(|_| HostError::Journal)
}

async fn table_count(path: &Path, table: &str) -> Result<i64, HostError> {
    let journal = Journal::open(path).await?;
    let connection = journal.connection().await?;
    let mut rows = connection
        .query(&format!("SELECT COUNT(*) FROM {table}"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    row.get(0).map_err(|_| HostError::Journal)
}

async fn read_charged_state(path: &Path) -> Result<ChargedState, HostError> {
    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    let mut rows = connection
        .query(
            "SELECT r.id, r.intent_id, r.connection_id, r.engine_id, r.docker_root_digest, r.binding_digest, r.cpu_millicores, r.memory_bytes, r.state, r.remote_cleanup_proven, r.local_cleanup_proven, a.request_id, a.acquire_state, a.started_message_id, a.started_event_digest, a.started_job_id, a.started_runner_id, a.started_runner_name, j.state, j.witness_request_id, j.started_request_id, j.runner_id, j.runner_name FROM capacity_reservations AS r JOIN connection_request_reservations AS link ON link.reservation_id = r.id JOIN connection_request_authorizations AS a ON a.connection_id = link.connection_id AND a.scale_set_id = link.scale_set_id AND a.request_id = link.request_id JOIN connection_runner_jits AS j ON j.intent_id = r.intent_id",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let state = ChargedState {
        reservation_id: row.get(0).map_err(|_| HostError::Journal)?,
        intent_id: row.get(1).map_err(|_| HostError::Journal)?,
        connection_id: row.get(2).map_err(|_| HostError::Journal)?,
        engine_id: row.get(3).map_err(|_| HostError::Journal)?,
        docker_root_digest: row.get(4).map_err(|_| HostError::Journal)?,
        binding_digest: row.get(5).map_err(|_| HostError::Journal)?,
        cpu_millicores: row.get(6).map_err(|_| HostError::Journal)?,
        memory_bytes: row.get(7).map_err(|_| HostError::Journal)?,
        reservation_state: row.get(8).map_err(|_| HostError::Journal)?,
        remote_cleanup_proven: row.get(9).map_err(|_| HostError::Journal)?,
        local_cleanup_proven: row.get(10).map_err(|_| HostError::Journal)?,
        request_id: row.get(11).map_err(|_| HostError::Journal)?,
        acquire_state: row.get(12).map_err(|_| HostError::Journal)?,
        started_message_id: row.get(13).map_err(|_| HostError::Journal)?,
        started_event_digest: row.get(14).map_err(|_| HostError::Journal)?,
        started_job_id: row.get(15).map_err(|_| HostError::Journal)?,
        started_runner_id: row.get(16).map_err(|_| HostError::Journal)?,
        started_runner_name: row.get(17).map_err(|_| HostError::Journal)?,
        jit_state: row.get(18).map_err(|_| HostError::Journal)?,
        witness_request_id: row.get(19).map_err(|_| HostError::Journal)?,
        jit_started_request_id: row.get(20).map_err(|_| HostError::Journal)?,
        jit_runner_id: row.get(21).map_err(|_| HostError::Journal)?,
        jit_runner_name: row.get(22).map_err(|_| HostError::Journal)?,
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(state)
}

async fn read_attempt_state(path: &Path) -> Result<AttemptState, HostError> {
    let journal = Journal::open(path).await?;
    let connection = journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT (SELECT COUNT(*) FROM physical_attempt_lineages), (SELECT COUNT(*) FROM physical_attempts), (SELECT COUNT(*) FROM physical_attempt_current), a.generation, a.reservation_id, a.intent_id, a.witness_request_id FROM physical_attempts AS a",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    Ok(AttemptState {
        lineages: row.get(0).map_err(|_| HostError::Journal)?,
        attempts: row.get(1).map_err(|_| HostError::Journal)?,
        current_attempts: row.get(2).map_err(|_| HostError::Journal)?,
        generation: row.get(3).map_err(|_| HostError::Journal)?,
        reservation_id: row.get(4).map_err(|_| HostError::Journal)?,
        intent_id: row.get(5).map_err(|_| HostError::Journal)?,
        witness_request_id: row.get(6).map_err(|_| HostError::Journal)?,
    })
}
