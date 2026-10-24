//! Challenge and source-proof validation.

mod rows;

use crate::config::{ApprovedJobAuthorityPolicy, RepositoryConnection};
use crate::error::HostError;

use super::{count, digest};

pub(super) async fn validate(connection: &turso::Connection) -> Result<(), HostError> {
    let invalid = count(
        connection,
        "SELECT COUNT(*) FROM connection_execution_proofs AS proof WHERE (proof.challenge_state = 'not_requested' AND proof.challenge_file_inode IS NOT NULL) OR (proof.challenge_state = 'published' AND proof.challenge_file_inode IS NULL) OR (proof.challenge_file_inode IS NOT NULL AND proof.challenge_file_bytes != length(CAST(proof.challenge_json AS BLOB))) OR (proof.challenge_state IN ('requested', 'uncertain') AND proof.challenge_file_inode IS NOT NULL) OR (proof.proof_state = 'approved' AND (proof.actual_request_id IS NULL OR proof.started_event_digest IS NULL OR proof.run_id IS NULL OR proof.run_attempt IS NULL OR proof.workflow_job_key IS NULL OR proof.authority_role IS NULL OR proof.native_job_id IS NULL OR proof.source_json IS NULL)) OR (proof.proof_state = 'pending' AND (proof.actual_request_id IS NOT NULL OR proof.started_event_digest IS NOT NULL OR proof.run_id IS NOT NULL OR proof.run_attempt IS NOT NULL OR proof.workflow_job_key IS NOT NULL OR proof.authority_role IS NOT NULL OR proof.deny_code IS NOT NULL)) OR (proof.proof_state = 'denied' AND proof.deny_code IS NULL)",
    )
    .await?;
    if invalid != 0 {
        return Err(HostError::Journal);
    }
    validate_persisted_bindings(connection).await?;
    rows::validate_proof_rows(connection).await?;
    validate_actual_requests(connection).await
}

async fn validate_persisted_bindings(connection: &turso::Connection) -> Result<(), HostError> {
    let invalid = count(
        connection,
        "SELECT COUNT(*) FROM connection_execution_proofs AS proof WHERE NOT EXISTS (SELECT 1 FROM capacity_reservations AS reservation WHERE reservation.id = proof.reservation_id AND reservation.intent_id = proof.intent_id AND reservation.connection_id = proof.connection_id AND reservation.binding_digest = proof.binding_digest) OR NOT EXISTS (SELECT 1 FROM connection_job_authority_policies AS policy WHERE policy.connection_id = proof.connection_id AND policy.generation = proof.generation AND policy.policy_digest = proof.policy_digest AND policy.binding_digest = proof.binding_digest)",
    )
    .await?;
    if invalid == 0 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn policy_snapshot(
    connection: &turso::Connection,
    connection_id: &str,
    generation: i64,
    policy_digest: &str,
) -> Result<(ApprovedJobAuthorityPolicy, RepositoryConnection), HostError> {
    let mut rows = connection
        .query(
            "SELECT policy.policy_json, policy.binding_digest, scope.identity FROM connection_job_authority_policies AS policy JOIN connection_scopes AS scope ON scope.connection_id = policy.connection_id WHERE policy.connection_id = ?1 AND policy.generation = ?2 AND policy.policy_digest = ?3",
            (connection_id, generation, policy_digest),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let policy_json: String = row.get(0).map_err(|_| HostError::Journal)?;
    let binding_digest: String = row.get(1).map_err(|_| HostError::Journal)?;
    let identity: String = row.get(2).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    let policy: ApprovedJobAuthorityPolicy =
        serde_json::from_str(&policy_json).map_err(|_| HostError::Journal)?;
    let repository: RepositoryConnection =
        serde_json::from_str(&identity).map_err(|_| HostError::Journal)?;
    let full_policy = serde_json::to_vec(&policy).map_err(|_| HostError::Journal)?;
    let canonical_identity = repository
        .canonical_identity_bytes()
        .map_err(|_| HostError::Journal)?;
    if full_policy != policy_json.as_bytes()
        || canonical_identity != identity.as_bytes()
        || digest(&canonical_identity) != binding_digest
        || policy.connection_id().as_str() != connection_id
        || i64::try_from(policy.generation()).ok() != Some(generation)
        || policy.binding_sha256() != binding_digest
        || policy.policy_sha256() != policy_digest
        || digest(&policy.canonical_json().map_err(|_| HostError::Journal)?) != policy_digest
    {
        return Err(HostError::Journal);
    }
    policy
        .validate_for(&repository)
        .map_err(|_| HostError::Journal)?;
    Ok((policy, repository))
}

fn role_name(role: crate::config::JobAuthorityRole) -> &'static str {
    match role {
        crate::config::JobAuthorityRole::Verification => "verification",
        crate::config::JobAuthorityRole::TrustedPublication => "trusted_publication",
    }
}

async fn validate_actual_requests(connection: &turso::Connection) -> Result<(), HostError> {
    let invalid = count(
        connection,
        "SELECT COUNT(*) FROM connection_execution_proofs AS proof LEFT JOIN capacity_reservations AS reservation ON reservation.id = proof.reservation_id LEFT JOIN connection_intents AS intent ON intent.id = proof.intent_id AND intent.connection_id = proof.connection_id LEFT JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = proof.reservation_id LEFT JOIN connection_runner_jits AS jit ON jit.intent_id = proof.intent_id AND jit.connection_id = proof.connection_id LEFT JOIN connection_request_authorizations AS auth ON auth.connection_id = proof.connection_id AND auth.scale_set_id = proof.scale_set_id AND auth.request_id = proof.actual_request_id WHERE proof.proof_state = 'approved' AND (reservation.id IS NULL OR intent.id IS NULL OR state.reservation_id IS NULL OR jit.intent_id IS NULL OR auth.request_id IS NULL OR reservation.connection_id != proof.connection_id OR reservation.intent_id != proof.intent_id OR reservation.state != 'reserved' OR reservation.remote_cleanup_proven != 0 OR reservation.local_cleanup_proven != 0 OR intent.runner_request_id != proof.actual_request_id OR intent.cleanup_proven != 0 OR intent.runner_completed != 0 OR jit.witness_request_id != proof.witness_request_id OR jit.started_request_id != proof.actual_request_id OR jit.runner_id != proof.runner_id OR jit.runner_name != proof.runner_name OR jit.state != 'started' OR jit.completed_request_id IS NOT NULL OR auth.acquire_state != 'acquired' OR auth.started_event_digest != proof.started_event_digest OR auth.started_runner_id != proof.runner_id OR auth.started_runner_name != proof.runner_name OR auth.started_job_id IS NOT proof.native_job_id OR auth.completed_event_digest IS NOT NULL OR state.resources_bound != 1 OR state.instance_binding_digest != proof.instance_binding_digest OR state.delivery_state != 'delivered' OR state.storage_cleanup_proven != 0)",
    )
    .await?;
    if invalid == 0 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_hex16(value: &str) -> bool {
    value.len() == 16
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
