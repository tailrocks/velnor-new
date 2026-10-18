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

async fn validate_auth_row(conn: &turso::Connection, row: &turso::Row) -> Result<(), HostError> {
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
    let response_json: Option<String> = row.get(23).map_err(|_| HostError::Journal)?;
    let identity: String = row.get(24).map_err(|_| HostError::Journal)?;
    let identity_json: serde_json::Value =
        serde_json::from_str(&identity).map_err(|_| HostError::Journal)?;
    let repository_id = identity_json
        .get("repository_id")
        .and_then(serde_json::Value::as_i64)
        .filter(|value| *value > 0)
        .ok_or(HostError::Journal)?;
    let has_start = started_digest.is_some();
    let has_completion = completed_digest.is_some();
    let event_state_allowed = matches!(
        acquire_state.as_str(),
        "acquired" | "already_acquired" | "uncertain" | "requested"
    ) || (acquire_state == "authorized"
        && !has_start
        && has_completion
        && completed_runner_id.is_none());
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
        || match (&response_json, &response_digest) {
            (Some(value), Some(expected)) => {
                value.len() > 4096 || &digest(value.as_bytes()) != expected
            }
            (None, None) => false,
            _ => true,
        }
        || !matches!(
            acquire_state.as_str(),
            "authorized"
                | "requested"
                | "acquired"
                | "no_effect"
                | "already_acquired"
                | "rejected"
                | "uncertain"
        )
        || (matches!(acquire_state.as_str(), "authorized" | "requested")
            && response_digest.is_some())
        || (!matches!(acquire_state.as_str(), "authorized" | "requested")
            && response_digest
                .as_deref()
                .is_none_or(|value| !valid_digest(value)))
        || has_start != started_message.is_some()
        || has_start != started_runner_id.is_some()
        || has_start != started_runner_name.is_some()
        || started_digest
            .as_deref()
            .is_some_and(|value| !valid_digest(value))
        || started_job_id
            .as_deref()
            .is_some_and(|value| !valid_event_text(value, 256))
        || started_runner_id.is_some_and(|value| value <= 0)
        || started_runner_name
            .as_deref()
            .is_some_and(|value| !valid_event_text(value, 128))
        || has_completion != completed_message.is_some()
        || completed_digest
            .as_deref()
            .is_some_and(|value| !valid_digest(value))
        || completed_job_id
            .as_deref()
            .is_some_and(|value| !valid_event_text(value, 256))
        || completed_result
            .as_deref()
            .is_some_and(|value| !valid_event_text(value, 128))
        || (completed_runner_id.is_some() != completed_runner_name.is_some())
        || (completed_runner_id.is_some() != completed_intent_id.is_some())
        || completed_runner_id.is_some_and(|value| value <= 0)
        || completed_runner_name
            .as_deref()
            .is_some_and(|value| !valid_event_text(value, 128))
        || !event_state_allowed && (has_start || has_completion)
    {
        return Err(HostError::Journal);
    }
    let mut scope_rows = conn
        .query(
            "SELECT identity FROM connection_scopes WHERE connection_id = ?1",
            [connection_id.as_str()],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let stored_identity: String = scope_rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    if stored_identity != identity || digest(stored_identity.as_bytes()) != binding_digest {
        return Err(HostError::Journal);
    }
    crate::journal::source_authorization::validate_stored_source(&source_json, repository_id)?;
    if let Some(intent_id) = completed_intent_id {
        let mut intent_rows = conn
            .query(
                "SELECT 1 FROM connection_runner_jits WHERE intent_id = ?1 AND connection_id = ?2 AND scale_set_id = ?3 AND completed_request_id = ?4",
                (intent_id, connection_id.as_str(), scale_set_id, request_id),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if intent_rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .is_none()
        {
            return Err(HostError::Journal);
        }
    }
    Ok(())
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
