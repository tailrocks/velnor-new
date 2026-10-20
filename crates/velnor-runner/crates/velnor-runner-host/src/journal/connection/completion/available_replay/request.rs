use super::AUTHORITY_MODEL;
use super::events::{runner_link_matches, started_event_matches};
use super::source::{
    completion_event, event_digest, event_source, valid_acquire_response, valid_digest,
    valid_optional_job_id, valid_optional_text,
};
use crate::error::HostError;
use velnor_runner_github::CompleteSourceMetadata;

struct TerminalRequest {
    policy_mode: String,
    authority_model: String,
    binding_digest: String,
    source_digest: String,
    acquire_state: String,
    acquire_json: Option<String>,
    acquire_digest: Option<String>,
    started_message_id: Option<i64>,
    started_digest: Option<String>,
    started_job_id: Option<String>,
    started_runner_id: Option<i64>,
    started_runner_name: Option<String>,
    completed_message_id: Option<i64>,
    completed_digest: Option<String>,
    completed_job_id: Option<String>,
    completed_result: Option<String>,
    completed_runner_id: Option<i64>,
    completed_runner_name: Option<String>,
    completed_intent_id: Option<i64>,
}

impl TerminalRequest {
    fn has_started(&self) -> bool {
        self.started_message_id.is_some()
            && self.started_digest.is_some()
            && self.started_runner_id.is_some()
            && self.started_runner_name.is_some()
    }

    fn has_completed(&self) -> bool {
        self.completed_message_id.is_some() && self.completed_digest.is_some()
    }

    fn event_shape_is_valid(&self) -> bool {
        let started_evidence = self.started_message_id.is_some()
            || self.started_digest.is_some()
            || self.started_job_id.is_some()
            || self.started_runner_id.is_some()
            || self.started_runner_name.is_some();
        let completed_evidence = self.completed_message_id.is_some()
            || self.completed_digest.is_some()
            || self.completed_job_id.is_some()
            || self.completed_result.is_some()
            || self.completed_runner_id.is_some()
            || self.completed_runner_name.is_some()
            || self.completed_intent_id.is_some();
        (!started_evidence || self.has_started())
            && (!completed_evidence || self.has_completed())
            && (self.completed_runner_id.is_some() == self.completed_runner_name.is_some())
            && (self.completed_runner_id.is_some() == self.completed_intent_id.is_some())
    }

    fn job_id_matches(&self, expected: Option<&str>) -> bool {
        expected.is_none_or(|job_id| {
            self.started_job_id
                .as_deref()
                .is_none_or(|stored| stored == job_id)
                && self
                    .completed_job_id
                    .as_deref()
                    .is_none_or(|stored| stored == job_id)
        })
    }
}

pub(super) async fn request_is_terminal(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    request_id: i64,
    binding_digest: &str,
    source: CompleteSourceMetadata<'_>,
    available_job_id: Option<&str>,
) -> Result<bool, HostError> {
    let Some(record) =
        load_terminal_request(connection, connection_id, scale_set_id, request_id).await?
    else {
        return Ok(false);
    };
    if !request_identity_matches(
        connection,
        connection_id,
        scale_set_id,
        request_id,
        binding_digest,
        source,
        &record,
    )
    .await?
        || !record.event_shape_is_valid()
        || available_job_id.is_some_and(|job_id| !valid_optional_job_id(Some(job_id)))
        || !record.job_id_matches(available_job_id)
    {
        return Ok(false);
    }
    if record.has_completed() {
        completed_request_is_terminal(
            connection,
            connection_id,
            scale_set_id,
            request_id,
            binding_digest,
            source,
            &record,
        )
        .await
    } else {
        started_request_is_terminal(
            connection,
            connection_id,
            scale_set_id,
            request_id,
            binding_digest,
            source,
            &record,
        )
        .await
    }
}

async fn load_terminal_request(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    request_id: i64,
) -> Result<Option<TerminalRequest>, HostError> {
    let mut rows = connection
        .query(
            "SELECT policy_mode, authority_model, binding_digest, source_digest, acquire_state, acquire_response_json, acquire_response_digest, started_message_id, started_event_digest, started_job_id, started_runner_id, started_runner_name, completed_message_id, completed_event_digest, completed_job_id, completed_result, completed_runner_id, completed_runner_name, completed_intent_id FROM connection_request_authorizations WHERE connection_id = ?1 AND scale_set_id = ?2 AND request_id = ?3",
            (connection_id, scale_set_id, request_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Ok(None);
    }
    Ok(Some(TerminalRequest {
        policy_mode: row.get(0).map_err(|_| HostError::Journal)?,
        authority_model: row.get(1).map_err(|_| HostError::Journal)?,
        binding_digest: row.get(2).map_err(|_| HostError::Journal)?,
        source_digest: row.get(3).map_err(|_| HostError::Journal)?,
        acquire_state: row.get(4).map_err(|_| HostError::Journal)?,
        acquire_json: row.get(5).map_err(|_| HostError::Journal)?,
        acquire_digest: row.get(6).map_err(|_| HostError::Journal)?,
        started_message_id: row.get(7).map_err(|_| HostError::Journal)?,
        started_digest: row.get(8).map_err(|_| HostError::Journal)?,
        started_job_id: row.get(9).map_err(|_| HostError::Journal)?,
        started_runner_id: row.get(10).map_err(|_| HostError::Journal)?,
        started_runner_name: row.get(11).map_err(|_| HostError::Journal)?,
        completed_message_id: row.get(12).map_err(|_| HostError::Journal)?,
        completed_digest: row.get(13).map_err(|_| HostError::Journal)?,
        completed_job_id: row.get(14).map_err(|_| HostError::Journal)?,
        completed_result: row.get(15).map_err(|_| HostError::Journal)?,
        completed_runner_id: row.get(16).map_err(|_| HostError::Journal)?,
        completed_runner_name: row.get(17).map_err(|_| HostError::Journal)?,
        completed_intent_id: row.get(18).map_err(|_| HostError::Journal)?,
    }))
}

async fn request_identity_matches(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    request_id: i64,
    binding_digest: &str,
    source: CompleteSourceMetadata<'_>,
    record: &TerminalRequest,
) -> Result<bool, HostError> {
    Ok(matches!(
        record.policy_mode.as_str(),
        "repository" | "branch_restricted"
    ) && record.authority_model == AUTHORITY_MODEL
        && record.binding_digest == binding_digest
        && valid_digest(&record.source_digest)
        && crate::journal::source_authorization::event_source_matches(
            connection,
            connection_id,
            scale_set_id,
            request_id,
            binding_digest,
            event_source(source),
        )
        .await?
        && valid_acquire_response(
            &record.acquire_state,
            record.acquire_json.as_deref(),
            record.acquire_digest.as_deref(),
            request_id,
        )?)
}

async fn started_request_is_terminal(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    request_id: i64,
    binding_digest: &str,
    source: CompleteSourceMetadata<'_>,
    record: &TerminalRequest,
) -> Result<bool, HostError> {
    if !record.has_started()
        || !matches!(
            record.acquire_state.as_str(),
            "authorized" | "requested" | "acquired"
        )
    {
        return Ok(false);
    }
    started_event_matches(
        connection,
        connection_id,
        scale_set_id,
        request_id,
        binding_digest,
        record.started_message_id,
        record.started_digest.as_deref(),
        record.started_job_id.as_deref(),
        record.started_runner_id,
        record.started_runner_name.as_deref(),
        source,
        false,
    )
    .await
}

async fn completed_request_is_terminal(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    request_id: i64,
    binding_digest: &str,
    source: CompleteSourceMetadata<'_>,
    record: &TerminalRequest,
) -> Result<bool, HostError> {
    if !completed_state_is_valid(record)
        || !completion_record_matches(request_id, scale_set_id, source, record)?
    {
        return Ok(false);
    }
    if !completed_runner_matches(
        connection,
        connection_id,
        scale_set_id,
        request_id,
        binding_digest,
        record,
    )
    .await?
    {
        return Ok(false);
    }
    if record.has_started()
        && !started_event_matches(
            connection,
            connection_id,
            scale_set_id,
            request_id,
            binding_digest,
            record.started_message_id,
            record.started_digest.as_deref(),
            record.started_job_id.as_deref(),
            record.started_runner_id,
            record.started_runner_name.as_deref(),
            source,
            true,
        )
        .await?
    {
        return Ok(false);
    }
    Ok(true)
}

fn completed_state_is_valid(record: &TerminalRequest) -> bool {
    if record.completed_runner_id.is_some() {
        record.acquire_state == "acquired"
    } else {
        matches!(
            record.acquire_state.as_str(),
            "authorized" | "requested" | "acquired" | "already_acquired" | "uncertain"
        )
    }
}

fn completion_record_matches(
    request_id: i64,
    scale_set_id: i64,
    source: CompleteSourceMetadata<'_>,
    record: &TerminalRequest,
) -> Result<bool, HostError> {
    let (Some(message_id), Some(digest)) = (
        record.completed_message_id,
        record.completed_digest.as_deref(),
    ) else {
        return Ok(false);
    };
    Ok(valid_optional_job_id(record.completed_job_id.as_deref())
        && valid_optional_text(record.completed_result.as_deref(), 128)
        && event_digest(
            digest,
            &completion_event(
                request_id,
                message_id,
                record.completed_result.as_deref(),
                record.completed_runner_id,
                record.completed_runner_name.as_deref(),
                record.completed_job_id.as_deref(),
                scale_set_id,
                source,
            ),
        )
        .is_ok())
}

async fn completed_runner_matches(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    request_id: i64,
    binding_digest: &str,
    record: &TerminalRequest,
) -> Result<bool, HostError> {
    match (
        record.completed_runner_id,
        record.completed_runner_name.as_deref(),
        record.completed_intent_id,
        record.completed_digest.as_deref(),
    ) {
        (Some(runner_id), Some(runner_name), Some(intent_id), Some(digest)) => {
            runner_link_matches(
                connection,
                connection_id,
                scale_set_id,
                request_id,
                binding_digest,
                runner_id,
                runner_name,
                intent_id,
                digest,
                true,
                true,
            )
            .await
        }
        (None, None, None, _) => Ok(true),
        _ => Ok(false),
    }
}
