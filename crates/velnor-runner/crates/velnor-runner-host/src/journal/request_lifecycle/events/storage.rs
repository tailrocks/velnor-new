//! Scoped event correlation queries and durable completion writes.

use crate::error::HostError;

pub(super) struct RequestEventState {
    pub(super) acquire_state: String,
    pub(super) started_digest: Option<String>,
    pub(super) completed_digest: Option<String>,
    pub(super) intent_id: Option<i64>,
}

pub(super) async fn request_event_state(
    connection: &turso::Connection,
    connection_id: &str,
    set_id: i64,
    request_id: i64,
) -> Result<Option<RequestEventState>, HostError> {
    let mut rows = connection
        .query(
            "SELECT acquire_state, started_event_digest, completed_event_digest, completed_intent_id FROM connection_request_authorizations WHERE connection_id = ?1 AND scale_set_id = ?2 AND request_id = ?3 AND authority_model = 'created_empty_exclusive_admin_session_v1'",
            (connection_id, set_id, request_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let state: String = row.get(0).map_err(|_| HostError::Journal)?;
    let started_digest: Option<String> = row.get(1).map_err(|_| HostError::Journal)?;
    let completed_digest: Option<String> = row.get(2).map_err(|_| HostError::Journal)?;
    let intent_id: Option<i64> = row.get(3).map_err(|_| HostError::Journal)?;
    Ok(Some(RequestEventState {
        acquire_state: state,
        started_digest,
        completed_digest,
        intent_id,
    }))
}

pub(super) async fn matching_runner_intent(
    connection: &turso::Connection,
    connection_id: &str,
    set_id: i64,
    runner_id: i64,
    runner_name: &str,
    request_id: i64,
    started: bool,
) -> Result<i64, HostError> {
    let allowed = if started {
        "'ready', 'started', 'completed'"
    } else {
        "'bound', 'start_requested', 'start_uncertain', 'ready', 'started', 'completed'"
    };
    let query = format!(
        "SELECT j.intent_id, j.started_request_id, j.completed_request_id FROM connection_runner_jits AS j JOIN connection_intents AS i ON i.id = j.intent_id AND i.connection_id = j.connection_id WHERE j.connection_id = ?1 AND j.scale_set_id = ?2 AND j.runner_id = ?3 AND j.runner_name = ?4 AND j.state IN ({allowed}) AND i.kind = 'launch' AND i.cleanup_proven = 0"
    );
    let mut rows = connection
        .query(&query, (connection_id, set_id, runner_id, runner_name))
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Err(HostError::Identity);
    };
    let intent_id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let started_request: Option<i64> = row.get(1).map_err(|_| HostError::Journal)?;
    let completed_request: Option<i64> = row.get(2).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || started_request.is_some_and(|value| value != request_id)
        || completed_request.is_some_and(|value| value != request_id)
    {
        return Err(HostError::Identity);
    }
    Ok(intent_id)
}

pub(super) async fn exact_started_intent(
    connection: &turso::Connection,
    connection_id: &str,
    set_id: i64,
    request_id: i64,
    runner: &velnor_runner_github::RunnerReference,
) -> Result<i64, HostError> {
    let mut rows = connection
        .query(
            "SELECT j.intent_id, i.assignment_key FROM connection_runner_jits AS j JOIN connection_intents AS i ON i.id = j.intent_id AND i.connection_id = j.connection_id WHERE j.connection_id = ?1 AND j.scale_set_id = ?2 AND j.started_request_id = ?3 AND j.runner_id = ?4 AND j.runner_name = ?5 AND i.scale_set_id = ?2 AND i.github_runner_id = ?6 AND i.runner_name = ?5 LIMIT 2",
            (connection_id, set_id, request_id, runner.id, runner.name.as_str(), runner.id.to_string()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Err(HostError::Identity);
    };
    let intent_id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let assignment_key: String = row.get(1).map_err(|_| HostError::Journal)?;
    let expected_prefix = format!("v1|{connection_id}|{set_id}|remote|6:runner|32:");
    let nonce = assignment_key.strip_prefix(&expected_prefix);
    if intent_id <= 0
        || nonce.is_none_or(|value| {
            value.len() != 32
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        || rows.next().await.map_err(|_| HostError::Journal)?.is_some()
    {
        return Err(HostError::Identity);
    }
    Ok(intent_id)
}

pub(super) async fn bind_completed_pair(
    connection: &turso::Connection,
    connection_id: &str,
    set_id: i64,
    request_id: i64,
    intent_id: i64,
    runner_id: i64,
    runner_name: &str,
) -> Result<(), HostError> {
    let changed = connection
        .execute(
            "UPDATE connection_intents SET runner_request_id = COALESCE(runner_request_id, ?1), runner_completed = 1, launch_phase = 'runner_completed' WHERE id = ?2 AND connection_id = ?3 AND scale_set_id = ?4 AND kind = 'launch' AND cleanup_proven = 0 AND (runner_request_id IS NULL OR runner_request_id = ?1) AND runner_name = ?5 AND github_runner_id = ?6",
            (request_id, intent_id, connection_id, set_id, runner_name, runner_id.to_string()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed != 1 {
        return Err(HostError::Identity);
    }
    Ok(())
}

pub(super) async fn insert_completion_cleanup(
    connection: &turso::Connection,
    connection_id: &str,
    set_id: i64,
    request_id: i64,
    intent_id: i64,
    runner_id: i64,
    runner_name: &str,
) -> Result<(), HostError> {
    connection
        .execute(
            "INSERT INTO connection_completion_cleanup (intent_id, connection_id, scale_set_id, runner_request_id, runner_id, runner_name) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (intent_id, connection_id, set_id, request_id, runner_id, runner_name),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn request_completed_runnerless(
    connection: &turso::Connection,
    connection_id: &str,
    set_id: i64,
    request_id: i64,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM connection_request_authorizations WHERE connection_id = ?1 AND scale_set_id = ?2 AND request_id = ?3 AND completed_event_digest IS NOT NULL AND completed_runner_id IS NULL",
            (connection_id, set_id, request_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(rows.next().await.map_err(|_| HostError::Journal)?.is_some())
}
