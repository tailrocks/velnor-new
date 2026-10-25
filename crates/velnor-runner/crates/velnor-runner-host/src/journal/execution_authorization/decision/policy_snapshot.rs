//! Load a journal-bound, validated policy snapshot.

use crate::config::{ApprovedJobAuthorityPolicy, RepositoryConnection};
use crate::error::HostError;

use super::super::records::digest;

pub(super) struct PolicySnapshot {
    pub(super) policy: ApprovedJobAuthorityPolicy,
    pub(super) repository: RepositoryConnection,
    pub(super) is_active: bool,
}

pub(super) async fn load(
    connection: &turso::Connection,
    connection_id: &str,
    generation: i64,
    policy_digest: &str,
    expected_binding_digest: &str,
) -> Result<PolicySnapshot, HostError> {
    let mut rows = connection
        .query(
            "SELECT policy.binding_digest, policy.policy_json, scope.identity, active.generation, active.policy_digest FROM connection_job_authority_policies AS policy JOIN connection_scopes AS scope ON scope.connection_id = policy.connection_id LEFT JOIN connection_active_job_authority AS active ON active.connection_id = policy.connection_id WHERE policy.connection_id = ?1 AND policy.generation = ?2 AND policy.policy_digest = ?3 AND scope.retired = 0",
            (connection_id, generation, policy_digest),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let stored_binding: String = row.get(0).map_err(|_| HostError::Journal)?;
    let policy_json: String = row.get(1).map_err(|_| HostError::Journal)?;
    let identity_json: String = row.get(2).map_err(|_| HostError::Journal)?;
    let active_generation: Option<i64> = row.get(3).map_err(|_| HostError::Journal)?;
    let active_digest: Option<String> = row.get(4).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }

    let policy: ApprovedJobAuthorityPolicy =
        serde_json::from_str(&policy_json).map_err(|_| HostError::Journal)?;
    let repository: RepositoryConnection =
        serde_json::from_str(&identity_json).map_err(|_| HostError::Journal)?;
    let full_policy = serde_json::to_vec(&policy).map_err(|_| HostError::Journal)?;
    let canonical_identity = repository
        .canonical_identity_bytes()
        .map_err(|_| HostError::Journal)?;
    let content = policy.canonical_json().map_err(|_| HostError::Journal)?;
    if full_policy != policy_json.as_bytes()
        || canonical_identity != identity_json.as_bytes()
        || stored_binding != expected_binding_digest
        || digest(&canonical_identity) != stored_binding
        || digest(&canonical_identity) != expected_binding_digest
        || policy.connection_id().as_str() != connection_id
        || policy.binding_sha256() != expected_binding_digest
        || i64::try_from(policy.generation()).ok() != Some(generation)
        || policy.policy_sha256() != policy_digest
        || digest(&content) != policy_digest
    {
        return Err(HostError::Journal);
    }
    policy
        .validate_for(&repository)
        .map_err(|_| HostError::Journal)?;
    let is_active =
        active_generation == Some(generation) && active_digest.as_deref() == Some(policy_digest);
    Ok(PolicySnapshot {
        policy,
        repository,
        is_active,
    })
}
