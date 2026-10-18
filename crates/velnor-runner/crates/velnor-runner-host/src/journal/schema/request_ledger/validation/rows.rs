//! Validation of persisted request and physical runner records.

use sha2::{Digest, Sha256};

use crate::error::HostError;

pub(super) async fn validate_auth_rows(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT connection_id, scale_set_id, request_id, message_id, policy_mode, authority_model, binding_digest, source_digest, source_json, acquire_state, acquire_response_digest, started_message_id, started_event_digest, started_job_id, started_runner_id, started_runner_name, completed_message_id, completed_event_digest, completed_job_id, completed_result, completed_runner_id, completed_runner_name, completed_intent_id, acquire_response_json, (SELECT identity FROM connection_scopes AS s WHERE s.connection_id = connection_request_authorizations.connection_id) FROM connection_request_authorizations",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        validate_auth_row(conn, &row).await?;
    }
    Ok(())
}

struct AuthorizationIdentity {
    connection_id: String,
    scale_set_id: i64,
    request_id: i64,
    binding_digest: String,
    source_json: String,
    acquire_state: String,
    repository_id: i64,
    identity: String,
}

struct AuthorizationEvents {
    completed_intent_id: Option<i64>,
}

async fn validate_auth_row(conn: &turso::Connection, row: &turso::Row) -> Result<(), HostError> {
    let identity = read_authorization_identity(row)?;
    let events = read_authorization_events(row, &identity.acquire_state)?;
    validate_scope_identity(conn, &identity).await?;
    crate::journal::source_authorization::validate_stored_source(
        &identity.source_json,
        identity.repository_id,
    )?;
    validate_completed_jit(conn, &identity, events.completed_intent_id).await
}

fn read_authorization_identity(row: &turso::Row) -> Result<AuthorizationIdentity, HostError> {
    let connection_id: String = row.get(0).map_err(|_| HostError::Journal)?;
    let scale_set_id: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    let request_id: i64 = row.get(2).map_err(|_| HostError::Journal)?;
    let message_id: i64 = row.get(3).map_err(|_| HostError::Journal)?;
    let policy_mode: String = row.get(4).map_err(|_| HostError::Journal)?;
    let authority: String = row.get(5).map_err(|_| HostError::Journal)?;
    let binding_digest: String = row.get(6).map_err(|_| HostError::Journal)?;
    let source_digest: String = row.get(7).map_err(|_| HostError::Journal)?;
    let source_json: String = row.get(8).map_err(|_| HostError::Journal)?;
    let acquire_state: String = row.get(9).map_err(|_| HostError::Journal)?;
    let response_digest: Option<String> = row.get(10).map_err(|_| HostError::Journal)?;
    let response_json: Option<String> = row.get(23).map_err(|_| HostError::Journal)?;
    let identity: String = row.get(24).map_err(|_| HostError::Journal)?;
    let identity_json: serde_json::Value =
        serde_json::from_str(&identity).map_err(|_| HostError::Journal)?;
    let repository_id = identity_json
        .get("repository_id")
        .and_then(serde_json::Value::as_i64)
        .filter(|value| *value > 0)
        .ok_or(HostError::Journal)?;
    let valid_response = match (&response_json, &response_digest) {
        (Some(value), Some(expected)) => {
            value.len() <= 4096 && &digest(value.as_bytes()) == expected
        }
        (None, None) => true,
        _ => false,
    };
    let state_is_valid = matches!(
        acquire_state.as_str(),
        "authorized"
            | "requested"
            | "acquired"
            | "no_effect"
            | "already_acquired"
            | "rejected"
            | "uncertain"
    );
    let response_matches_state = if matches!(acquire_state.as_str(), "authorized" | "requested") {
        response_digest.is_none()
    } else {
        response_digest.as_deref().is_some_and(valid_digest)
    };
    if connection_id.is_empty()
        || scale_set_id <= 0
        || request_id <= 0
        || message_id < 0
        || !matches!(policy_mode.as_str(), "repository" | "branch_restricted")
        || authority != "created_empty_exclusive_admin_session_v1"
        || !valid_digest(&binding_digest)
        || !valid_digest(&source_digest)
        || digest(source_json.as_bytes()) != source_digest
        || digest(identity.as_bytes()) != binding_digest
        || !valid_response
        || !state_is_valid
        || !response_matches_state
    {
        return Err(HostError::Journal);
    }
    Ok(AuthorizationIdentity {
        connection_id,
        scale_set_id,
        request_id,
        binding_digest,
        source_json,
        acquire_state,
        repository_id,
        identity,
    })
}

fn read_authorization_events(
    row: &turso::Row,
    acquire_state: &str,
) -> Result<AuthorizationEvents, HostError> {
    let started_message: Option<i64> = row.get(11).map_err(|_| HostError::Journal)?;
    let started_digest: Option<String> = row.get(12).map_err(|_| HostError::Journal)?;
    let started_job_id: Option<String> = row.get(13).map_err(|_| HostError::Journal)?;
    let started_runner_id: Option<i64> = row.get(14).map_err(|_| HostError::Journal)?;
    let started_runner_name: Option<String> = row.get(15).map_err(|_| HostError::Journal)?;
    let completed_message: Option<i64> = row.get(16).map_err(|_| HostError::Journal)?;
    let completed_digest: Option<String> = row.get(17).map_err(|_| HostError::Journal)?;
    let completed_job_id: Option<String> = row.get(18).map_err(|_| HostError::Journal)?;
    let completed_result: Option<String> = row.get(19).map_err(|_| HostError::Journal)?;
    let completed_runner_id: Option<i64> = row.get(20).map_err(|_| HostError::Journal)?;
    let completed_runner_name: Option<String> = row.get(21).map_err(|_| HostError::Journal)?;
    let completed_intent_id: Option<i64> = row.get(22).map_err(|_| HostError::Journal)?;
    let has_start = started_digest.is_some();
    let has_completion = completed_digest.is_some();
    if !valid_started_event(
        has_start,
        started_message,
        started_digest.as_deref(),
        started_job_id.as_deref(),
        started_runner_id,
        started_runner_name.as_deref(),
    ) || !valid_completed_event(
        has_completion,
        completed_message,
        completed_digest.as_deref(),
        completed_job_id.as_deref(),
        completed_result.as_deref(),
        completed_runner_id,
        completed_runner_name.as_deref(),
        completed_intent_id,
    ) {
        return Err(HostError::Journal);
    }
    let event_state_allowed = matches!(
        acquire_state,
        "acquired" | "already_acquired" | "uncertain" | "requested"
    ) || (acquire_state == "authorized"
        && !has_start
        && has_completion
        && completed_runner_id.is_none());
    if !event_state_allowed && (has_start || has_completion) {
        return Err(HostError::Journal);
    }
    Ok(AuthorizationEvents {
        completed_intent_id,
    })
}

fn valid_started_event(
    present: bool,
    message_id: Option<i64>,
    digest_value: Option<&str>,
    job_id: Option<&str>,
    runner_id: Option<i64>,
    runner_name: Option<&str>,
) -> bool {
    present == message_id.is_some()
        && present == runner_id.is_some()
        && present == runner_name.is_some()
        && digest_value.is_none_or(valid_digest)
        && job_id.is_none_or(|value| valid_event_text(value, 256))
        && runner_id.is_none_or(|value| value > 0)
        && runner_name.is_none_or(|value| valid_event_text(value, 128))
}

fn valid_completed_event(
    present: bool,
    message_id: Option<i64>,
    digest_value: Option<&str>,
    job_id: Option<&str>,
    result: Option<&str>,
    runner_id: Option<i64>,
    runner_name: Option<&str>,
    intent_id: Option<i64>,
) -> bool {
    present == message_id.is_some()
        && digest_value.is_none_or(valid_digest)
        && job_id.is_none_or(|value| valid_event_text(value, 256))
        && result.is_none_or(|value| valid_event_text(value, 128))
        && runner_id.is_some() == runner_name.is_some()
        && runner_id.is_some() == intent_id.is_some()
        && runner_id.is_none_or(|value| value > 0)
        && runner_name.is_none_or(|value| valid_event_text(value, 128))
}

async fn validate_scope_identity(
    conn: &turso::Connection,
    authorization: &AuthorizationIdentity,
) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT identity FROM connection_scopes WHERE connection_id = ?1",
            [authorization.connection_id.as_str()],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let stored_identity: String = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    if stored_identity == authorization.identity
        && digest(stored_identity.as_bytes()) == authorization.binding_digest
    {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn validate_completed_jit(
    conn: &turso::Connection,
    authorization: &AuthorizationIdentity,
    intent_id: Option<i64>,
) -> Result<(), HostError> {
    let Some(intent_id) = intent_id else {
        return Ok(());
    };
    let mut rows = conn
        .query(
            "SELECT 1 FROM connection_runner_jits WHERE intent_id = ?1 AND connection_id = ?2 AND scale_set_id = ?3 AND completed_request_id = ?4",
            (intent_id, authorization.connection_id.as_str(), authorization.scale_set_id, authorization.request_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

pub(super) async fn validate_jit_rows(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT j.intent_id, j.connection_id, j.scale_set_id, j.witness_request_id, j.requested_name, j.state, j.runner_id, j.runner_name, j.started_request_id, j.completed_request_id, i.assignment_key, i.github_runner_id, i.runner_name, i.jit_requested, i.cleanup_proven, r.acquire_state, j.witness_request_id, j.started_request_id, j.completed_request_id, (SELECT COUNT(*) FROM capacity_reservations AS capacity WHERE capacity.intent_id = j.intent_id AND capacity.connection_id = j.connection_id AND capacity.state = 'released' AND capacity.remote_cleanup_proven = 1 AND capacity.local_cleanup_proven = 1) FROM connection_runner_jits AS j JOIN connection_intents AS i ON i.id = j.intent_id AND i.connection_id = j.connection_id JOIN connection_request_authorizations AS r ON r.connection_id = j.connection_id AND r.scale_set_id = j.scale_set_id AND r.request_id = j.witness_request_id",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
        let connection_id: String = row.get(1).map_err(|_| HostError::Journal)?;
        let set_id: i64 = row.get(2).map_err(|_| HostError::Journal)?;
        let witness: i64 = row.get(3).map_err(|_| HostError::Journal)?;
        let requested: String = row.get(4).map_err(|_| HostError::Journal)?;
        let state: String = row.get(5).map_err(|_| HostError::Journal)?;
        let runner_id: Option<i64> = row.get(6).map_err(|_| HostError::Journal)?;
        let runner_name: Option<String> = row.get(7).map_err(|_| HostError::Journal)?;
        let assignment_key: String = row.get(10).map_err(|_| HostError::Journal)?;
        let stored_id: Option<String> = row.get(11).map_err(|_| HostError::Journal)?;
        let stored_name: Option<String> = row.get(12).map_err(|_| HostError::Journal)?;
        let jit_requested: bool = row.get(13).map_err(|_| HostError::Journal)?;
        let cleaned: bool = row.get(14).map_err(|_| HostError::Journal)?;
        let witness_state: String = row.get(15).map_err(|_| HostError::Journal)?;
        let stored_witness: i64 = row.get(16).map_err(|_| HostError::Journal)?;
        let started_request: Option<i64> = row.get(17).map_err(|_| HostError::Journal)?;
        let completed_request: Option<i64> = row.get(18).map_err(|_| HostError::Journal)?;
        let released_cleanup_proofs: i64 = row.get(19).map_err(|_| HostError::Journal)?;
        if id <= 0
            || connection_id.is_empty()
            || set_id <= 0
            || witness <= 0
            || requested.is_empty()
            || requested.len() > 128
            || !matches!(
                state.as_str(),
                "claimed"
                    | "uncertain"
                    | "bound"
                    | "start_requested"
                    | "start_uncertain"
                    | "ready"
                    | "started"
                    | "completed"
                    | "cleaned"
            )
            || !jit_requested
            || (state == "cleaned" && !cleaned)
            || (cleaned && completed_request.is_some() && state != "cleaned")
            || witness_state != "acquired"
            || stored_witness != witness
            || !assignment_key.starts_with(&format!("v1|{connection_id}|{set_id}|remote|6:runner|"))
            || runner_id.map(|value| value.to_string()) != stored_id
            || runner_name != stored_name
            || runner_name
                .as_deref()
                .is_some_and(|value| value != requested)
            || (matches!(state.as_str(), "claimed" | "uncertain") && runner_id.is_some())
            || (matches!(
                state.as_str(),
                "bound"
                    | "start_requested"
                    | "start_uncertain"
                    | "ready"
                    | "started"
                    | "completed"
                    | "cleaned"
            ) && runner_id.is_none())
            || (state == "started" && started_request.is_none())
            || (state == "completed" && completed_request.is_none())
            || (state == "cleaned" && completed_request.is_none() && released_cleanup_proofs != 1)
            || (matches!(
                state.as_str(),
                "claimed" | "uncertain" | "bound" | "start_requested" | "start_uncertain" | "ready"
            )) && (started_request.is_some() || completed_request.is_some())
        {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

fn digest(value: &[u8]) -> String {
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(value) {
        use std::fmt::Write as _;
        let _written = write!(output, "{byte:02x}");
    }
    output
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_event_text(value: &str, maximum: usize) -> bool {
    value.len() <= maximum && !value.chars().any(char::is_control)
}
