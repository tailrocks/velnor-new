//! Build and store the durable decision after the source and Started event agree.

use crate::error::HostError;
use crate::journal::execution_authorization::records::digest;
use crate::launch::execution_authorization::{
    AuthorizedExecutionSource, PersistedExecutionDecision,
};
use serde::Serialize;

use super::super::policy_snapshot;
use super::super::proof_row::ProofRow;
use super::super::started::StartedExecution;
use super::PrepareContext;

#[cfg(test)]
mod tests;

const MAX_DECISION_BYTES: usize = 8 * 1024;

pub(super) struct PreparedDecision {
    document: PersistedExecutionDecision,
    json: String,
    digest: String,
    allow: bool,
    deny_code: Option<String>,
    role: Option<crate::config::JobAuthorityRole>,
    actual_request_id: i64,
    run_id: i64,
    run_attempt: i64,
    job_key: String,
    native_job_id: Option<String>,
}

pub(super) async fn build_decision(
    connection: &turso::Connection,
    context: &PrepareContext<'_>,
    proof: &ProofRow,
    started: &StartedExecution,
) -> Result<PreparedDecision, HostError> {
    let policy = policy_snapshot::load(
        connection,
        &context.connection_id,
        proof.generation,
        &proof.policy_digest,
        &proof.binding_digest,
    )
    .await?;
    let (role, deny_code) = determine_authority(context.source, proof, started, &policy)?;
    let allow = deny_code.is_none();
    let actual_request_id = started.request_id;
    let request = context.source.request();
    let native_job_id = started
        .native_job_id
        .as_deref()
        .filter(|value| valid_native_job_id(value))
        .map(str::to_owned);
    let document_bytes = serde_json::to_vec(&PreparedDecisionDocument {
        schema_version: 1,
        allow,
        challenge: proof.challenge.clone(),
        request_sha256: context.request_digest.clone(),
        reservation_id: proof.reservation_id,
        intent_id: context.operation_id,
        actual_request_id,
        run_id: request.run_id(),
        run_attempt: request.run_attempt(),
        job_key: request.job_key().to_owned(),
        runner_id: proof.runner_id,
        policy_generation: u64::try_from(proof.generation).map_err(|_| HostError::Journal)?,
        policy_digest: proof.policy_digest.clone(),
        native_job_id: native_job_id.clone(),
        rest_job_id: context.source.rest_job_id().map(str::to_owned),
        deny_code: deny_code.map(str::to_owned),
    })
    .map_err(|_| HostError::Journal)?;
    if document_bytes.len() > MAX_DECISION_BYTES {
        return Err(HostError::Journal);
    }
    let document: PersistedExecutionDecision =
        serde_json::from_slice(&document_bytes).map_err(|_| HostError::Journal)?;
    if document.canonical_json()?.as_slice() != document_bytes.as_slice() || !document.valid() {
        return Err(HostError::Journal);
    }
    Ok(PreparedDecision {
        document,
        json: String::from_utf8(document_bytes.clone()).map_err(|_| HostError::Journal)?,
        digest: digest(&document_bytes),
        allow,
        deny_code: deny_code.map(str::to_owned),
        role,
        actual_request_id,
        run_id: i64::try_from(request.run_id()).map_err(|_| HostError::Journal)?,
        run_attempt: i64::from(request.run_attempt()),
        job_key: request.job_key().to_owned(),
        native_job_id,
    })
}
fn determine_authority(
    source: &AuthorizedExecutionSource,
    proof: &ProofRow,
    started: &StartedExecution,
    policy: &policy_snapshot::PolicySnapshot,
) -> Result<
    (
        Option<crate::config::JobAuthorityRole>,
        Option<&'static str>,
    ),
    HostError,
> {
    if let Some(error) = association_error(source, proof, started)? {
        return Ok((None, Some(error)));
    }
    if !policy.is_active {
        return Ok((None, Some("policy_changed")));
    }
    if source.request().repository_id()
        != u64::try_from(policy.repository.repository_id).map_err(|_| HostError::Journal)?
        || source.request().repository_name() != policy.repository.repository
    {
        return Ok((None, Some("source_mismatch")));
    }
    let role = source.approved_role(&policy.policy, &policy.repository);
    let deny_code = role.is_none().then_some("workflow_not_approved");
    Ok((role, deny_code))
}
pub(super) async fn store_decision(
    connection: &turso::Connection,
    context: &PrepareContext<'_>,
    proof: &ProofRow,
    started: &StartedExecution,
    prepared: &PreparedDecision,
) -> Result<(), HostError> {
    let changed = connection
        .execute(
            "UPDATE connection_execution_proofs SET proof_state = ?1, actual_request_id = ?2, started_event_digest = ?3, run_id = ?4, run_attempt = ?5, workflow_job_key = ?6, authority_role = ?7, native_job_id = ?8, rest_job_id = ?9, deny_code = ?10 WHERE intent_id = ?11 AND reservation_id = ?12 AND connection_id = ?13 AND proof_state = 'pending' AND challenge_state = 'published' AND source_digest = ?14",
            turso::params![
                if prepared.allow { "approved" } else { "denied" },
                prepared.actual_request_id,
                started.event_digest.as_str(),
                prepared.run_id,
                prepared.run_attempt,
                prepared.job_key.as_str(),
                prepared.role.map(role_name),
                prepared.native_job_id.as_deref(),
                context.source.rest_job_id(),
                prepared.deny_code.as_deref(),
                context.operation_id,
                proof.reservation_id,
                context.connection_id.as_str(),
                context.source_digest.as_str(),
            ],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed != 1 {
        return Err(HostError::Journal);
    }
    connection
        .execute(
            "INSERT INTO connection_execution_decisions (intent_id, reservation_id, connection_id, scale_set_id, witness_request_id, actual_request_id, challenge, generation, policy_digest, request_digest, allow, decision_digest, decision_json, delivery_state) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 'prepared')",
            turso::params![
                context.operation_id,
                proof.reservation_id,
                context.connection_id.as_str(),
                proof.scale_set_id,
                proof.witness_request_id,
                prepared.actual_request_id,
                prepared.document.challenge(),
                proof.generation,
                proof.policy_digest.as_str(),
                context.request_digest.as_str(),
                i64::from(prepared.allow),
                prepared.digest.as_str(),
                prepared.json.as_str(),
            ],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}
fn association_error(
    source: &AuthorizedExecutionSource,
    proof: &ProofRow,
    started: &StartedExecution,
) -> Result<Option<&'static str>, HostError> {
    if started.jit_state == "completed"
        || started.jit_completed_request_id.is_some()
        || started.completed_event_digest.is_some()
        || started.runner_terminated
        || started.reservation_state != "reserved"
    {
        return Ok(Some("runner_terminated"));
    }
    if !started.matches_source_request(source)? {
        return Ok(Some("source_mismatch"));
    }
    if started.jit_witness_request_id != proof.witness_request_id
        || started.jit_started_request_id != Some(started.request_id)
        || started.jit_state != "started"
        || started.runner_id != proof.runner_id
        || started.runner_name != proof.runner_name
        || started.jit_runner_id != Some(proof.runner_id)
        || started.jit_runner_name.as_deref() != Some(proof.runner_name.as_str())
        || started.acquire_state != "acquired"
        || started.state_delivery_state != "delivered"
        || !started.state_resources_bound
        || started.state_instance_binding_digest.as_deref()
            != Some(proof.instance_binding_digest.as_str())
        || started.event_digest.len() != 64
        || started
            .native_job_id
            .as_deref()
            .is_none_or(|value| !valid_native_job_id(value))
    {
        return Ok(Some("association_mismatch"));
    }
    Ok(None)
}
fn role_name(role: crate::config::JobAuthorityRole) -> &'static str {
    match role {
        crate::config::JobAuthorityRole::Verification => "verification",
        crate::config::JobAuthorityRole::TrustedPublication => "trusted_publication",
    }
}

fn valid_native_job_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}

#[derive(Serialize)]
struct PreparedDecisionDocument {
    schema_version: u32,
    allow: bool,
    challenge: String,
    request_sha256: String,
    reservation_id: i64,
    intent_id: i64,
    actual_request_id: i64,
    run_id: u64,
    run_attempt: u32,
    job_key: String,
    runner_id: i64,
    policy_generation: u64,
    policy_digest: String,
    native_job_id: Option<String>,
    rest_job_id: Option<String>,
    deny_code: Option<String>,
}
