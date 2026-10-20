use super::{AcquireResponse, CompletionEvent, RunnerEvent};
use crate::error::HostError;
use crate::journal::source_authorization::EventSource;
use serde::Serialize;
use sha2::{Digest, Sha256};
use velnor_runner_github::CompleteSourceMetadata;

pub(super) async fn canonical_stored_source(
    connection: &turso::Connection,
    connection_id: &str,
    source_json: &str,
) -> Result<bool, HostError> {
    let Some(repository_id) = connection_repository_id(connection, connection_id).await? else {
        return Ok(false);
    };
    Ok(
        crate::journal::source_authorization::validate_stored_source(source_json, repository_id)
            .is_ok(),
    )
}

pub(super) async fn connection_repository_id(
    connection: &turso::Connection,
    connection_id: &str,
) -> Result<Option<i64>, HostError> {
    // The immutable canonical connection identity supplies the source repository ID.
    let mut rows = connection
        .query(
            "SELECT identity FROM connection_scopes WHERE connection_id = ?1",
            [connection_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let identity: String = row.get(0).map_err(|_| HostError::Journal)?;
    let identity: serde_json::Value =
        serde_json::from_str(&identity).map_err(|_| HostError::Journal)?;
    Ok(identity
        .get("repository_id")
        .and_then(serde_json::Value::as_i64)
        .filter(|id| *id > 0))
}

pub(super) fn completion_event<'a>(
    request_id: i64,
    message_id: i64,
    result: Option<&'a str>,
    runner_id: Option<i64>,
    runner_name: Option<&'a str>,
    job_id: Option<&'a str>,
    scale_set_id: i64,
    source: CompleteSourceMetadata<'a>,
) -> CompletionEvent<'a> {
    CompletionEvent {
        request_id,
        message_id,
        result,
        runner: runner_id
            .zip(runner_name)
            .map(|(runner_id, runner_name)| RunnerEvent {
                request_id,
                message_id,
                runner_id,
                runner_name,
                scale_set_id,
                job_id,
                source: event_source(source),
            }),
        job_id,
        source: event_source(source),
    }
}

pub(super) fn event_source(source: CompleteSourceMetadata<'_>) -> EventSource<'_> {
    EventSource {
        owner_name: Some(source.owner_name),
        repository_name: Some(source.repository_name),
        job_workflow_ref: Some(source.job_workflow_ref),
        workflow_run_id: Some(source.workflow_run_id),
        event_name: Some(source.event_name),
    }
}

pub(super) fn event_digest<T: Serialize>(stored: &str, event: &T) -> Result<(), HostError> {
    if !valid_digest(stored)
        || digest_hex(&serde_json::to_vec(event).map_err(|_| HostError::Journal)?) != stored
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

pub(super) fn valid_acquire_response(
    state: &str,
    json: Option<&str>,
    digest: Option<&str>,
    request_id: i64,
) -> Result<bool, HostError> {
    if matches!(state, "authorized" | "requested") {
        return Ok(json.is_none() && digest.is_none());
    }
    let (Some(json), Some(digest)) = (json, digest) else {
        return Ok(false);
    };
    if !valid_digest(digest) || digest_hex(json.as_bytes()) != digest {
        return Ok(false);
    }
    let response: AcquireResponse = serde_json::from_str(json).map_err(|_| HostError::Journal)?;
    if serde_json::to_string(&response).map_err(|_| HostError::Journal)? != json {
        return Ok(false);
    }
    Ok(match (state, response) {
        ("acquired", AcquireResponse::Returned { request_ids }) => request_ids == [request_id],
        ("already_acquired", AcquireResponse::AlreadyAcquired)
        | ("uncertain", AcquireResponse::Uncertain | AcquireResponse::Returned { .. })
        | ("no_effect", AcquireResponse::NoEffect)
        | ("rejected", AcquireResponse::Rejected) => true,
        _ => false,
    })
}

pub(super) fn digest_hex(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    let mut value = String::with_capacity(64);
    for byte in hash {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    value
}

pub(super) fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn valid_optional_job_id(value: Option<&str>) -> bool {
    value.is_none_or(|text| {
        !text.is_empty() && text.len() <= 256 && !text.chars().any(char::is_control)
    })
}

pub(super) fn valid_optional_text(value: Option<&str>, limit: usize) -> bool {
    value.is_none_or(|text| {
        !text.is_empty() && text.len() <= limit && !text.chars().any(char::is_control)
    })
}

pub(super) fn valid_text(value: &str, limit: usize) -> bool {
    !value.is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}
