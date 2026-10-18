//! Bounded reads of scoped assignment and source rows.

use crate::error::HostError;
use crate::journal::connection::AssignmentIdentity;
use crate::journal::source_authorization::storage::matches_intent;

pub(super) struct LaunchRow {
    pub(super) id: i64,
    pub(super) state: String,
    pub(super) request_id: Option<i64>,
    pub(super) runner_name: Option<String>,
    pub(super) acquire_attempted: bool,
    pub(super) acquire_resolved: bool,
    pub(super) acquired: bool,
    pub(super) jit_requested: bool,
    pub(super) reservation_state: String,
    pub(super) reservation_binding_digest: String,
}

pub(super) struct SourceRow {
    pub(super) intent_id: i64,
    pub(super) message_id: i64,
    pub(super) policy_mode: String,
    pub(super) authority_model: String,
    pub(super) binding_digest: String,
    pub(super) source_digest: String,
    pub(super) source_json: String,
}

pub(super) async fn assigned_backlog_is_closed(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    binding_digest: &str,
) -> Result<bool, HostError> {
    const MAX_ACTIVE_ASSIGNMENTS: usize = 4_096;
    let mut rows = connection
        .query(
            "SELECT id, scale_set_id, runner_request_id, state, acquire_attempted, acquire_resolved, acquired, jit_requested, docker_id, github_runner_id, dind_id, worker_volume, launch_phase, runner_completed FROM connection_intents WHERE connection_id = ?1 AND kind = 'launch' AND cleanup_proven = 0 ORDER BY id LIMIT 4097",
            [connection_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut seen = 0usize;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        seen += 1;
        if seen > MAX_ACTIVE_ASSIGNMENTS {
            return Ok(false);
        }
        if !backlog_row_is_closed(connection, row, connection_id, scale_set_id, binding_digest)
            .await?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn backlog_row_is_closed(
    connection: &turso::Connection,
    row: turso::Row,
    connection_id: &str,
    expected_scale_set_id: i64,
    binding_digest: &str,
) -> Result<bool, HostError> {
    let intent_id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let scale_set_id: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    let request_id: Option<i64> = row.get(2).map_err(|_| HostError::Journal)?;
    let state: String = row.get(3).map_err(|_| HostError::Journal)?;
    let attempted: bool = row.get(4).map_err(|_| HostError::Journal)?;
    let resolved: bool = row.get(5).map_err(|_| HostError::Journal)?;
    let acquired: bool = row.get(6).map_err(|_| HostError::Journal)?;
    let jit_requested: bool = row.get(7).map_err(|_| HostError::Journal)?;
    let docker_id: Option<String> = row.get(8).map_err(|_| HostError::Journal)?;
    let runner_id: Option<i64> = row.get(9).map_err(|_| HostError::Journal)?;
    let dind_id: Option<String> = row.get(10).map_err(|_| HostError::Journal)?;
    let worker_volume: Option<String> = row.get(11).map_err(|_| HostError::Journal)?;
    let launch_phase: Option<String> = row.get(12).map_err(|_| HostError::Journal)?;
    let runner_completed: bool = row.get(13).map_err(|_| HostError::Journal)?;
    let no_local_effect = docker_id.is_none()
        && runner_id.is_none()
        && dind_id.is_none()
        && worker_volume.is_none()
        && launch_phase.is_none()
        && !runner_completed;
    if intent_id <= 0 || scale_set_id != expected_scale_set_id {
        return Ok(false);
    }
    let Some(request_id) = request_id else {
        return Ok(state == "failed"
            && !attempted
            && !resolved
            && !acquired
            && !jit_requested
            && no_local_effect);
    };
    if request_id <= 0 || matches!(state.as_str(), "uncertain" | "recovering") {
        return Ok(false);
    }
    if attempted && !resolved
        || acquired && (!attempted || !resolved)
        || jit_requested && !acquired
        || runner_completed && !acquired
        || !attempted && (resolved || acquired || jit_requested)
        || resolved && !acquired && state != "failed"
    {
        return Ok(false);
    }
    if state == "failed" && !attempted && !acquired && !jit_requested && no_local_effect {
        return Ok(true);
    }
    matches_intent(
        connection,
        connection_id,
        scale_set_id,
        request_id,
        intent_id,
        binding_digest,
    )
    .await
}

pub(super) async fn launch_row_by_id(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    intent_id: i64,
) -> Result<Option<LaunchRow>, HostError> {
    let mut rows = connection
        .query(
            "SELECT i.id, i.state, i.runner_request_id, i.runner_name, i.acquire_attempted, i.acquire_resolved, i.acquired, i.jit_requested, r.state, r.binding_digest FROM connection_intents AS i JOIN capacity_reservations AS r ON r.intent_id = i.id WHERE i.id = ?1 AND i.connection_id = ?2 AND i.scale_set_id = ?3 AND r.connection_id = ?2 AND r.state != 'released' AND i.kind = 'launch' AND i.cleanup_proven = 0",
            (intent_id, connection_id, scale_set_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    Ok(Some(launch_from_row(row)?))
}

pub(super) async fn launch_row(
    connection: &turso::Connection,
    identity: &AssignmentIdentity,
    connection_id: &str,
) -> Result<LaunchRow, HostError> {
    let key = identity.storage_key();
    let mut rows = connection
        .query(
            "SELECT i.id, i.state, i.runner_request_id, i.runner_name, i.acquire_attempted, i.acquire_resolved, i.acquired, i.jit_requested, r.state, r.binding_digest FROM connection_intents AS i JOIN capacity_reservations AS r ON r.intent_id = i.id WHERE i.connection_id = ?1 AND i.scale_set_id = ?2 AND i.assignment_key = ?3 AND r.connection_id = ?1 AND r.assignment_key = ?3 AND r.state != 'released' AND i.cleanup_proven = 0 ORDER BY i.id DESC LIMIT 2",
            (connection_id, identity.scale_set_id(), key.as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let value = launch_from_row(row)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(value)
}

fn launch_from_row(row: turso::Row) -> Result<LaunchRow, HostError> {
    Ok(LaunchRow {
        id: row.get(0).map_err(|_| HostError::Journal)?,
        state: row.get(1).map_err(|_| HostError::Journal)?,
        request_id: row.get(2).map_err(|_| HostError::Journal)?,
        runner_name: row.get(3).map_err(|_| HostError::Journal)?,
        acquire_attempted: row.get(4).map_err(|_| HostError::Journal)?,
        acquire_resolved: row.get(5).map_err(|_| HostError::Journal)?,
        acquired: row.get(6).map_err(|_| HostError::Journal)?,
        jit_requested: row.get(7).map_err(|_| HostError::Journal)?,
        reservation_state: row.get(8).map_err(|_| HostError::Journal)?,
        reservation_binding_digest: row.get(9).map_err(|_| HostError::Journal)?,
    })
}

pub(super) async fn source_row(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    request_id: i64,
) -> Result<Option<SourceRow>, HostError> {
    let mut rows = connection
        .query(
            "SELECT intent_id, message_id, policy_mode, authority_model, binding_digest, source_digest, source_json FROM connection_source_authorizations WHERE connection_id = ?1 AND scale_set_id = ?2 AND request_id = ?3",
            (connection_id, scale_set_id, request_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    Ok(Some(SourceRow {
        intent_id: row.get(0).map_err(|_| HostError::Journal)?,
        message_id: row.get(1).map_err(|_| HostError::Journal)?,
        policy_mode: row.get(2).map_err(|_| HostError::Journal)?,
        authority_model: row.get(3).map_err(|_| HostError::Journal)?,
        binding_digest: row.get(4).map_err(|_| HostError::Journal)?,
        source_digest: row.get(5).map_err(|_| HostError::Journal)?,
        source_json: row.get(6).map_err(|_| HostError::Journal)?,
    }))
}

pub(super) async fn matching_launch_intent(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    request_id: i64,
    intent_id: i64,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM connection_intents WHERE id = ?1 AND connection_id = ?2 AND scale_set_id = ?3 AND runner_request_id = ?4 AND kind = 'launch'",
            (intent_id, connection_id, scale_set_id, request_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?
        == 1)
}

pub(super) async fn repository_id(
    connection: &turso::Connection,
    connection_id: &str,
) -> Result<i64, HostError> {
    let mut rows = connection
        .query(
            "SELECT identity FROM connection_scopes WHERE connection_id = ?1",
            [connection_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let identity: String = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    serde_json::from_str::<serde_json::Value>(&identity)
        .map_err(|_| HostError::Journal)?
        .get("repository_id")
        .and_then(serde_json::Value::as_i64)
        .filter(|value| *value > 0)
        .ok_or(HostError::Journal)
}

pub(super) async fn created_set_count(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    binding_digest: &str,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM connection_scale_set_intents AS s JOIN connection_runtime_bindings AS b ON b.connection_id = s.connection_id WHERE s.connection_id = ?1 AND s.binding_digest = ?2 AND s.state = 'created' AND s.ownership = 'created' AND s.remote_id = ?3 AND b.scale_set_id = s.remote_id AND b.scale_set_name = s.remote_name AND b.set_ownership = 'created'",
            (connection_id, binding_digest, scale_set_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let count: i64 = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    Ok(count == 1)
}

pub(super) async fn set_is_exclusive_to_connection(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM connection_runtime_bindings WHERE scale_set_id = ?1 AND connection_id != ?2",
            (scale_set_id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let count: i64 = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    Ok(count == 0)
}

pub(super) async fn scalar_count(
    connection: &turso::Connection,
    query: &str,
) -> Result<i64, HostError> {
    let mut rows = connection
        .query(query, ())
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}
