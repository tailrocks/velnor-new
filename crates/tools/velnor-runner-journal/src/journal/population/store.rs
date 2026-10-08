//! Atomic persistence implementation for historical Scale Set observations.

use std::time::{Duration, UNIX_EPOCH};

use velnor_runner_github::Statistics;

use crate::error::HostError;
use crate::journal::ScaleSetSessionIdentity;

use super::{PopulationSnapshotWrite, ScaleSetPopulationSnapshot, ScaleSetPopulationSource};

pub(super) async fn record_snapshot(
    conn: &turso::Connection,
    identity: &ScaleSetSessionIdentity,
    snapshot: &ScaleSetPopulationSnapshot,
) -> Result<PopulationSnapshotWrite, HostError> {
    let mut rows = conn
        .query(
            "SELECT i.subject, i.state, i.effect_state, s.session_id, s.state, s.close_attempted FROM intents AS i JOIN scale_set_sessions AS s ON s.intent_id = i.id WHERE i.id = ?1 AND i.kind = 'scale-set-session'",
            [snapshot.intent_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let subject: String = row.get(0).map_err(|_| HostError::Journal)?;
    let intent_state: String = row.get(1).map_err(|_| HostError::Journal)?;
    let effect_state: String = row.get(2).map_err(|_| HostError::Journal)?;
    let session_id: Option<String> = row.get(3).map_err(|_| HostError::Journal)?;
    let session_state: String = row.get(4).map_err(|_| HostError::Journal)?;
    let close_attempted: i64 = row.get(5).map_err(|_| HostError::Journal)?;
    if subject != identity.subject()
        || session_id.as_deref() != Some(snapshot.session_id())
        || intent_state != "pending"
        || effect_state != "may_have_effect"
        || session_state != "open"
        || close_attempted != 0
    {
        return Err(HostError::Journal);
    }
    drop(rows);

    let current = read_snapshot_row(conn, snapshot.intent_id).await?;
    let Some(current) = current else {
        insert_snapshot(conn, snapshot).await?;
        return Ok(PopulationSnapshotWrite::Stored);
    };
    if current.session_id != snapshot.session_id || current.scale_set_id != snapshot.scale_set_id {
        return Err(HostError::Journal);
    }
    match (current.source, snapshot.source) {
        (ScaleSetPopulationSource::SessionCreated, ScaleSetPopulationSource::SessionCreated)
            if current.same_payload(snapshot) =>
        {
            Ok(PopulationSnapshotWrite::Unchanged)
        }
        (
            ScaleSetPopulationSource::SessionCreated | ScaleSetPopulationSource::PollBatch,
            ScaleSetPopulationSource::SessionCreated,
        ) => Err(HostError::Journal),
        (ScaleSetPopulationSource::SessionCreated, ScaleSetPopulationSource::PollBatch) => {
            update_snapshot(conn, snapshot).await?;
            Ok(PopulationSnapshotWrite::Stored)
        }
        (ScaleSetPopulationSource::PollBatch, ScaleSetPopulationSource::PollBatch) => {
            let current_id = current.message_id.ok_or(HostError::Journal)?;
            let next_id = snapshot.message_id.ok_or(HostError::Journal)?;
            if next_id < current_id {
                Ok(PopulationSnapshotWrite::OlderMessageIgnored)
            } else if next_id == current_id && current.same_payload(snapshot) {
                Ok(PopulationSnapshotWrite::Unchanged)
            } else if next_id == current_id {
                Err(HostError::Journal)
            } else {
                update_snapshot(conn, snapshot).await?;
                Ok(PopulationSnapshotWrite::Stored)
            }
        }
    }
}

async fn read_snapshot_row(
    conn: &turso::Connection,
    intent_id: i64,
) -> Result<Option<ScaleSetPopulationSnapshot>, HostError> {
    let mut rows = conn
        .query(
            "SELECT session_id, scale_set_id, source, message_id, observed_at_ms, total_available_jobs, total_acquired_jobs, total_assigned_jobs, total_running_jobs, total_registered_runners, total_busy_runners, total_idle_runners FROM scale_set_population_observations WHERE intent_id = ?1",
            [intent_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let source =
        ScaleSetPopulationSource::parse(&row.get::<String>(2).map_err(|_| HostError::Journal)?)?;
    let observed_at_ms = row.get::<i64>(4).map_err(|_| HostError::Journal)?;
    let snapshot = ScaleSetPopulationSnapshot {
        intent_id,
        session_id: row.get(0).map_err(|_| HostError::Journal)?,
        scale_set_id: row.get(1).map_err(|_| HostError::Journal)?,
        source,
        message_id: row.get(3).map_err(|_| HostError::Journal)?,
        observed_at: UNIX_EPOCH
            .checked_add(Duration::from_millis(
                u64::try_from(observed_at_ms).map_err(|_| HostError::Journal)?,
            ))
            .ok_or(HostError::Journal)?,
        statistics: Statistics {
            total_available_jobs: row.get(5).map_err(|_| HostError::Journal)?,
            total_acquired_jobs: row.get(6).map_err(|_| HostError::Journal)?,
            total_assigned_jobs: row.get(7).map_err(|_| HostError::Journal)?,
            total_running_jobs: row.get(8).map_err(|_| HostError::Journal)?,
            total_registered_runners: row.get(9).map_err(|_| HostError::Journal)?,
            total_busy_runners: row.get(10).map_err(|_| HostError::Journal)?,
            total_idle_runners: row.get(11).map_err(|_| HostError::Journal)?,
        },
    };
    snapshot.validate()?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(Some(snapshot))
}

async fn insert_snapshot(
    conn: &turso::Connection,
    snapshot: &ScaleSetPopulationSnapshot,
) -> Result<(), HostError> {
    let statistics = snapshot.statistics();
    let changed = conn
        .execute(
            "INSERT INTO scale_set_population_observations (intent_id, session_id, scale_set_id, source, message_id, observed_at_ms, total_available_jobs, total_acquired_jobs, total_assigned_jobs, total_running_jobs, total_registered_runners, total_busy_runners, total_idle_runners) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            (snapshot.intent_id, snapshot.session_id.as_str(), snapshot.scale_set_id, snapshot.source.as_str(), snapshot.message_id, snapshot.observed_at_ms()?, statistics.total_available_jobs, statistics.total_acquired_jobs, statistics.total_assigned_jobs, statistics.total_running_jobs, statistics.total_registered_runners, statistics.total_busy_runners, statistics.total_idle_runners),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn update_snapshot(
    conn: &turso::Connection,
    snapshot: &ScaleSetPopulationSnapshot,
) -> Result<(), HostError> {
    let statistics = snapshot.statistics();
    let changed = conn
        .execute(
            "UPDATE scale_set_population_observations SET session_id = ?1, scale_set_id = ?2, source = ?3, message_id = ?4, observed_at_ms = ?5, total_available_jobs = ?6, total_acquired_jobs = ?7, total_assigned_jobs = ?8, total_running_jobs = ?9, total_registered_runners = ?10, total_busy_runners = ?11, total_idle_runners = ?12 WHERE intent_id = ?13",
            (snapshot.session_id.as_str(), snapshot.scale_set_id, snapshot.source.as_str(), snapshot.message_id, snapshot.observed_at_ms()?, statistics.total_available_jobs, statistics.total_acquired_jobs, statistics.total_assigned_jobs, statistics.total_running_jobs, statistics.total_registered_runners, statistics.total_busy_runners, statistics.total_idle_runners, snapshot.intent_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}
