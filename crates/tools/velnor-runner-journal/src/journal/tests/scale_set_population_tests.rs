//! Session statistics persist across Empty polls and process restarts.

use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use crate::journal::{
    PopulationSnapshotWrite, ReplayRoute, ScaleSetPopulationSnapshot, ScaleSetPopulationSource,
    ScaleSetSessionClaim, ScaleSetSessionIdentity,
};
use crate::{DrainSnapshot, Journal};
use velnor_runner_github::Statistics;

use super::Scratch;

fn identity(set_id: i64) -> Result<ScaleSetSessionIdentity, crate::HostError> {
    ScaleSetSessionIdentity::new(
        ReplayRoute {
            destination: "https://api.github.com",
            registration_scope: "repository",
            owner: "acme",
            repository: "widget",
            runner_group_id: 2,
            runner_group_name: "trusted",
            scale_set_id: set_id,
            scale_set_name: "linux",
        },
        829_618_808,
        "acme/widget",
    )
}

fn statistics(assigned: i64, running: i64) -> Statistics {
    Statistics {
        total_available_jobs: 0,
        total_acquired_jobs: assigned,
        total_assigned_jobs: assigned,
        total_running_jobs: running,
        total_registered_runners: 2,
        total_busy_runners: 1,
        total_idle_runners: 1,
    }
}

#[derive(Clone, Copy)]
struct Observation {
    source: ScaleSetPopulationSource,
    message_id: Option<i64>,
    seconds: u64,
    assigned: i64,
    running: i64,
}

fn snapshot(
    intent_id: i64,
    session_id: &str,
    scale_set_id: i64,
    observation: Observation,
) -> Result<ScaleSetPopulationSnapshot, crate::HostError> {
    ScaleSetPopulationSnapshot::test_only(
        intent_id,
        session_id,
        scale_set_id,
        observation.source,
        observation.message_id,
        UNIX_EPOCH + Duration::from_secs(observation.seconds),
        statistics(observation.assigned, observation.running),
    )
}

async fn reserved_session(
    journal: &Journal,
    route: &ScaleSetSessionIdentity,
    session_id: &str,
) -> Result<i64, crate::HostError> {
    let ScaleSetSessionClaim::Reserved(intent_id) = journal
        .reserve_scale_set_session_if_accepting(route)
        .await?
    else {
        return Err(crate::HostError::Journal);
    };
    journal
        .record_scale_set_session_created(intent_id, session_id)
        .await?;
    Ok(intent_id)
}

#[tokio::test]
async fn create_stats_empty_poll_redelivery_and_restart_keep_latest_snapshot() -> Result<(), String>
{
    let scratch = Scratch::new("population-reopen").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let route = identity(3).map_err(|error| error.to_string())?;
    let intent_id = persist_created_empty_and_poll(&path, &route).await?;
    assert_reopen_snapshot_is_historical(&path, &route, intent_id).await?;
    Ok(())
}

async fn persist_created_empty_and_poll(
    path: &Path,
    route: &ScaleSetSessionIdentity,
) -> Result<i64, String> {
    let journal = Journal::open(path)
        .await
        .map_err(|error| error.to_string())?;
    let intent_id = reserved_session(&journal, route, "session-population-1")
        .await
        .map_err(|error| error.to_string())?;
    let created = snapshot(
        intent_id,
        "session-population-1",
        3,
        Observation {
            source: ScaleSetPopulationSource::SessionCreated,
            message_id: None,
            seconds: 1,
            assigned: 2,
            running: 0,
        },
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(
        journal
            .record_scale_set_population_snapshot(route, &created)
            .await
            .map_err(|error| error.to_string())?,
        PopulationSnapshotWrite::Stored
    );
    let empty_poll = journal
        .scale_set_population_snapshot(intent_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Empty poll must not erase create-time statistics".to_owned())?;
    assert_eq!(
        empty_poll.source(),
        ScaleSetPopulationSource::SessionCreated
    );
    assert_eq!(empty_poll.statistics().total_assigned_jobs, 2);
    let first = snapshot(
        intent_id,
        "session-population-1",
        3,
        Observation {
            source: ScaleSetPopulationSource::PollBatch,
            message_id: Some(20),
            seconds: 2,
            assigned: 3,
            running: 1,
        },
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(
        journal
            .record_scale_set_population_snapshot(route, &first)
            .await
            .map_err(|error| error.to_string())?,
        PopulationSnapshotWrite::Stored
    );
    Ok(intent_id)
}

async fn assert_reopen_snapshot_is_historical(
    path: &Path,
    route: &ScaleSetSessionIdentity,
    intent_id: i64,
) -> Result<(), String> {
    let journal = Journal::open(path)
        .await
        .map_err(|error| error.to_string())?;
    let latest = snapshot(
        intent_id,
        "session-population-1",
        3,
        Observation {
            source: ScaleSetPopulationSource::PollBatch,
            message_id: Some(20),
            seconds: 9,
            assigned: 3,
            running: 1,
        },
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(
        journal
            .record_scale_set_population_snapshot(route, &latest)
            .await
            .map_err(|error| error.to_string())?,
        PopulationSnapshotWrite::Unchanged,
        "message redelivery is idempotent even when local receipt time differs"
    );
    assert_older_message_is_ignored(&journal, route, intent_id).await?;
    let persisted = journal
        .scale_set_population_snapshot(intent_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "latest poll snapshot missing after reopen".to_owned())?;
    assert_eq!(persisted.message_id(), Some(20));
    assert_eq!(persisted.statistics().total_assigned_jobs, 3);
    assert_eq!(
        journal
            .drain_snapshot()
            .await
            .map_err(|error| error.to_string())?,
        DrainSnapshot {
            draining: false,
            occupied_launches: 0,
            unresolved_intents: 1,
        },
        "population snapshots do not change worker capacity or session occupancy"
    );
    Ok(())
}

async fn assert_older_message_is_ignored(
    journal: &Journal,
    route: &ScaleSetSessionIdentity,
    intent_id: i64,
) -> Result<(), String> {
    let stale = snapshot(
        intent_id,
        "session-population-1",
        3,
        Observation {
            source: ScaleSetPopulationSource::PollBatch,
            message_id: Some(19),
            seconds: 10,
            assigned: 4,
            running: 1,
        },
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(
        journal
            .record_scale_set_population_snapshot(route, &stale)
            .await
            .map_err(|error| error.to_string())?,
        PopulationSnapshotWrite::OlderMessageIgnored
    );
    Ok(())
}

#[tokio::test]
async fn v10_migration_adds_snapshot_table_without_releasing_open_session() -> Result<(), String> {
    let scratch = Scratch::new("population-v10-migration").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let route = identity(3).map_err(|error| error.to_string())?;
    let intent_id = {
        let journal = Journal::open(&path)
            .await
            .map_err(|error| error.to_string())?;
        reserved_session(&journal, &route, "session-v10-open")
            .await
            .map_err(|error| error.to_string())?
    };
    let database = turso::Builder::new_local(
        path.to_str()
            .ok_or_else(|| "journal path was not UTF-8".to_owned())?,
    )
    .build()
    .await
    .map_err(|error| error.to_string())?;
    let conn = database.connect().map_err(|error| error.to_string())?;
    conn.execute("DROP TABLE scale_set_population_observations", ())
        .await
        .map_err(|error| error.to_string())?;
    conn.execute("PRAGMA user_version = 10", ())
        .await
        .map_err(|error| error.to_string())?;
    drop(conn);
    drop(database);

    let migrated = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        migrated
            .drain_snapshot()
            .await
            .map_err(|error| error.to_string())?
            .unresolved_intents,
        1
    );
    assert!(
        migrated
            .scale_set_population_snapshot(intent_id)
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn malformed_v11_population_schema_is_rejected_without_rewrite() -> Result<(), String> {
    let scratch = Scratch::new("population-v11-malformed").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    drop(journal);
    let database = turso::Builder::new_local(
        path.to_str()
            .ok_or_else(|| "journal path was not UTF-8".to_owned())?,
    )
    .build()
    .await
    .map_err(|error| error.to_string())?;
    let conn = database.connect().map_err(|error| error.to_string())?;
    conn.execute("DROP TABLE scale_set_population_observations", ())
        .await
        .map_err(|error| error.to_string())?;
    conn.execute(
        "CREATE TABLE scale_set_population_observations (intent_id INTEGER PRIMARY KEY, session_id TEXT NOT NULL, scale_set_id INTEGER NOT NULL, source TEXT NOT NULL, message_id INTEGER, observed_at_ms INTEGER NOT NULL, total_available_jobs INTEGER NOT NULL, total_acquired_jobs INTEGER NOT NULL, total_assigned_jobs INTEGER NOT NULL, total_running_jobs INTEGER NOT NULL, total_registered_runners INTEGER NOT NULL, total_busy_runners INTEGER NOT NULL, total_idle_runners INTEGER NOT NULL)",
        (),
    )
    .await
    .map_err(|error| error.to_string())?;
    drop(conn);
    drop(database);

    assert!(Journal::open(&path).await.is_err());
    let database = turso::Builder::new_local(
        path.to_str()
            .ok_or_else(|| "journal path was not UTF-8".to_owned())?,
    )
    .build()
    .await
    .map_err(|error| error.to_string())?;
    let conn = database.connect().map_err(|error| error.to_string())?;
    let mut rows = conn
        .query("PRAGMA user_version", ())
        .await
        .map_err(|error| error.to_string())?;
    let version = rows
        .next()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "user_version returned no row".to_owned())?
        .get::<i64>(0)
        .map_err(|error| error.to_string())?;
    assert_eq!(
        version, 11,
        "the current version is not silently downgraded"
    );
    Ok(())
}
