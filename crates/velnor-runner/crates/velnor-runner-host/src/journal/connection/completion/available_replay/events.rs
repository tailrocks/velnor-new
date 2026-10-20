use super::source::{
    canonical_stored_source, digest_hex, event_source, valid_digest, valid_optional_job_id,
    valid_text,
};
use super::{AUTHORITY_MODEL, RunnerEvent};
use crate::error::HostError;
use velnor_runner_github::{CompleteSourceMetadata, InnerJob, InnerKind};

pub(super) async fn event_fields_match(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    request_id: i64,
    message_id: i64,
    job: &InnerJob,
) -> Result<bool, HostError> {
    let (query, includes_result) = match job.kind {
        InnerKind::Started => (
            "SELECT started_message_id, started_job_id, started_runner_id, started_runner_name FROM connection_request_authorizations WHERE connection_id = ?1 AND scale_set_id = ?2 AND request_id = ?3",
            false,
        ),
        InnerKind::Completed => (
            "SELECT completed_message_id, completed_job_id, completed_runner_id, completed_runner_name, completed_result FROM connection_request_authorizations WHERE connection_id = ?1 AND scale_set_id = ?2 AND request_id = ?3",
            true,
        ),
        _ => return Ok(false),
    };
    let mut rows = connection
        .query(query, (connection_id, scale_set_id, request_id))
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let stored_message_id: Option<i64> = row.get(0).map_err(|_| HostError::Journal)?;
    let stored_job_id: Option<String> = row.get(1).map_err(|_| HostError::Journal)?;
    let stored_runner_id: Option<i64> = row.get(2).map_err(|_| HostError::Journal)?;
    let stored_runner_name: Option<String> = row.get(3).map_err(|_| HostError::Journal)?;
    let stored_result: Option<String> = if includes_result {
        row.get(4).map_err(|_| HostError::Journal)?
    } else {
        None
    };
    Ok(stored_message_id == Some(message_id)
        && stored_job_id == job.job_id
        && stored_runner_id == job.runner_id
        && stored_runner_name == job.runner_name
        && stored_result == job.result)
}

pub(super) async fn current_scale_set_matches(
    connection: &turso::Connection,
    connection_id: &str,
    expected: i64,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT scale_set_id FROM connection_runtime_bindings WHERE connection_id = ?1",
            [connection_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let actual: Option<i64> = row.get(0).map_err(|_| HostError::Journal)?;
    Ok(actual == Some(expected) && rows.next().await.map_err(|_| HostError::Journal)?.is_none())
}

pub(super) async fn started_event_matches(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    request_id: i64,
    binding_digest: &str,
    message_id: Option<i64>,
    digest: Option<&str>,
    job_id: Option<&str>,
    runner_id: Option<i64>,
    runner_name: Option<&str>,
    source: CompleteSourceMetadata<'_>,
    has_completed: bool,
) -> Result<bool, HostError> {
    let (Some(message_id), Some(digest), Some(runner_id), Some(runner_name)) =
        (message_id, digest, runner_id, runner_name)
    else {
        return Ok(false);
    };
    if message_id < 0
        || !valid_optional_job_id(job_id)
        || runner_id <= 0
        || !valid_text(runner_name, 128)
    {
        return Ok(false);
    }
    let expected = digest_hex(
        &serde_json::to_vec(&RunnerEvent {
            request_id,
            message_id,
            runner_id,
            runner_name,
            scale_set_id,
            job_id,
            source: event_source(source),
        })
        .map_err(|_| HostError::Journal)?,
    );
    if expected != digest {
        return Ok(false);
    }
    runner_link_matches(
        connection,
        connection_id,
        scale_set_id,
        request_id,
        binding_digest,
        runner_id,
        runner_name,
        0,
        digest,
        has_completed,
        false,
    )
    .await
}

pub(super) async fn runner_link_matches(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    request_id: i64,
    binding_digest: &str,
    runner_id: i64,
    runner_name: &str,
    expected_intent_id: i64,
    event_digest: &str,
    has_completed: bool,
    completed: bool,
) -> Result<bool, HostError> {
    let event_column = if completed {
        "completed_event_digest"
    } else {
        "started_event_digest"
    };
    let jit_column = if completed { "completed" } else { "started" };
    let jit_states = if completed {
        "'completed', 'cleaned'"
    } else if has_completed {
        "'started', 'completed', 'cleaned'"
    } else {
        "'started'"
    };
    let query = format!(
        "SELECT jit.witness_request_id FROM connection_request_authorizations AS auth JOIN connection_runner_jits AS jit ON jit.connection_id = auth.connection_id AND jit.scale_set_id = auth.scale_set_id AND jit.{jit_column}_request_id = auth.request_id JOIN connection_request_reservations AS link ON link.connection_id = jit.connection_id AND link.scale_set_id = jit.scale_set_id AND link.request_id = jit.witness_request_id JOIN capacity_reservations AS reservation ON reservation.id = link.reservation_id AND reservation.connection_id = link.connection_id JOIN connection_intents AS intent ON intent.id = reservation.intent_id AND intent.connection_id = reservation.connection_id AND intent.id = jit.intent_id AND intent.assignment_key = reservation.assignment_key WHERE auth.connection_id = ?1 AND auth.scale_set_id = ?2 AND auth.request_id = ?3 AND auth.binding_digest = ?4 AND link.binding_digest = ?4 AND reservation.binding_digest = ?4 AND auth.acquire_state = 'acquired' AND auth.{event_column} = ?5 AND auth.{jit_column}_runner_id = ?6 AND auth.{jit_column}_runner_name = ?7 AND intent.kind = 'launch' AND intent.scale_set_id = ?2 AND intent.runner_request_id = ?3 AND intent.runner_name = ?7 AND intent.github_runner_id = CAST(?6 AS TEXT) AND jit.runner_id = ?6 AND jit.runner_name = ?7 AND jit.requested_name = ?7 AND ((intent.cleanup_proven = 0 AND jit.state IN ({jit_states})) OR (?9 = 1 AND intent.cleanup_proven = 1 AND jit.state = 'cleaned' AND EXISTS (SELECT 1 FROM connection_completion_cleanup AS cleanup WHERE cleanup.intent_id = intent.id AND cleanup.connection_id = intent.connection_id AND cleanup.scale_set_id = intent.scale_set_id AND cleanup.runner_request_id = ?3 AND cleanup.runner_id = ?6 AND cleanup.runner_name = ?7 AND cleanup.runner_absent = 1))) AND (?8 = 0 OR intent.id = ?8)"
    );
    let mut rows = connection
        .query(
            &query,
            (
                connection_id,
                scale_set_id,
                request_id,
                binding_digest,
                event_digest,
                runner_id,
                runner_name,
                expected_intent_id,
                i64::from(has_completed || completed),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let witness_request_id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Ok(false);
    }
    witness_is_acquired(
        connection,
        connection_id,
        scale_set_id,
        witness_request_id,
        binding_digest,
    )
    .await
}

pub(super) async fn witness_is_acquired(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    request_id: i64,
    binding_digest: &str,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT authority_model, binding_digest, source_digest, source_json, acquire_state, acquire_response_json, acquire_response_digest FROM connection_request_authorizations WHERE connection_id = ?1 AND scale_set_id = ?2 AND request_id = ?3",
            (connection_id, scale_set_id, request_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let authority: String = row.get(0).map_err(|_| HostError::Journal)?;
    let stored_binding: String = row.get(1).map_err(|_| HostError::Journal)?;
    let source_digest: String = row.get(2).map_err(|_| HostError::Journal)?;
    let source_json: String = row.get(3).map_err(|_| HostError::Journal)?;
    let acquire_state: String = row.get(4).map_err(|_| HostError::Journal)?;
    let response_json: Option<String> = row.get(5).map_err(|_| HostError::Journal)?;
    let response_digest: Option<String> = row.get(6).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || request_id <= 0
        || authority != AUTHORITY_MODEL
        || stored_binding != binding_digest
        || acquire_state != "acquired"
        || !valid_digest(&source_digest)
        || digest_hex(source_json.as_bytes()) != source_digest
        || !canonical_stored_source(connection, connection_id, &source_json).await?
    {
        return Ok(false);
    }
    let expected = format!("{{\"kind\":\"returned\",\"request_ids\":[{request_id}]}}");
    Ok(response_json.as_deref() == Some(expected.as_str())
        && response_digest.as_deref().is_some_and(valid_digest)
        && response_json
            .as_deref()
            .zip(response_digest.as_deref())
            .is_some_and(|(json, digest)| digest_hex(json.as_bytes()) == digest))
}
