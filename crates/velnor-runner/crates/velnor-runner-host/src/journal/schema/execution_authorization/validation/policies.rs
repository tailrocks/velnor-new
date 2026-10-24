//! Policy snapshot validation.

use crate::config::{ApprovedJobAuthorityPolicy, RepositoryConnection};
use crate::error::HostError;

use super::{canonical_json, count, digest};

const MAX_POLICY_BYTES: usize = 256 * 1024;

pub(super) async fn validate(connection: &turso::Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT policy.connection_id, policy.generation, policy.binding_digest, policy.policy_digest, policy.policy_json, scope.identity FROM connection_job_authority_policies AS policy JOIN connection_scopes AS scope ON scope.connection_id = policy.connection_id ORDER BY policy.connection_id, policy.generation",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let connection_id: String = row.get(0).map_err(|_| HostError::Journal)?;
        let generation: i64 = row.get(1).map_err(|_| HostError::Journal)?;
        let binding_digest: String = row.get(2).map_err(|_| HostError::Journal)?;
        let policy_digest: String = row.get(3).map_err(|_| HostError::Journal)?;
        let policy_json: String = row.get(4).map_err(|_| HostError::Journal)?;
        let identity: String = row.get(5).map_err(|_| HostError::Journal)?;
        validate_policy_row(
            &connection_id,
            generation,
            &binding_digest,
            &policy_digest,
            &policy_json,
            &identity,
        )?;
    }
    let invalid = count(
        connection,
        "SELECT COUNT(*) FROM connection_active_job_authority AS active LEFT JOIN connection_job_authority_policies AS policy ON policy.connection_id = active.connection_id AND policy.generation = active.generation AND policy.policy_digest = active.policy_digest WHERE policy.connection_id IS NULL",
    )
    .await?;
    if invalid == 0 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

fn validate_policy_row(
    connection_id: &str,
    generation: i64,
    binding_digest: &str,
    policy_digest: &str,
    policy_json: &str,
    identity: &str,
) -> Result<(), HostError> {
    if policy_json.len() > MAX_POLICY_BYTES {
        return Err(HostError::Journal);
    }
    let policy: ApprovedJobAuthorityPolicy =
        serde_json::from_str(policy_json).map_err(|_| HostError::Journal)?;
    let snapshot = canonical_json(&policy)?;
    let content = policy.canonical_json().map_err(|_| HostError::Journal)?;
    let connection: RepositoryConnection =
        serde_json::from_str(identity).map_err(|_| HostError::Journal)?;
    let canonical_identity = connection
        .canonical_identity_bytes()
        .map_err(|_| HostError::Journal)?;
    if snapshot != policy_json.as_bytes()
        || canonical_identity != identity.as_bytes()
        || policy.connection_id().as_str() != connection_id
        || i64::try_from(policy.generation()).ok() != Some(generation)
        || policy.binding_sha256() != binding_digest
        || policy.policy_sha256() != policy_digest
        || digest(&content) != policy_digest
    {
        return Err(HostError::Journal);
    }
    policy
        .validate_for(&connection)
        .map_err(|_| HostError::Journal)
}
