//! Saved state for an acquired request that can continue before JIT.

use crate::error::HostError;
use crate::journal::connection::storage::in_transaction;
use crate::journal::connection::{AssignmentIdentity, ConnectionJournal};

#[path = "recovery/row.rs"]
mod row;

/// Journal record that links an acquired request to its reservation.
pub(crate) struct AcquiredRecoveryCandidate {
    request: AssignmentIdentity,
    #[cfg(test)]
    physical_assignment: AssignmentIdentity,
    reservation_id: i64,
    intent_id: i64,
    runner_name: String,
    engine_id: String,
    docker_root_digest: String,
    sample_generation_id: String,
    storage_profile_generation_id: String,
    storage_profile_digest: String,
    policy_generation: u64,
    policy_digest: String,
    charge: (u64, u64),
}

struct AcquiredRecoveryLookup<'a> {
    request: &'a AssignmentIdentity,
    request_id: i64,
    reservation_id: i64,
    connection_id: String,
    binding_digest: String,
    generation: i64,
    policy_generation: u64,
    policy_digest: String,
    scale_set_id: i64,
}

impl AcquiredRecoveryCandidate {
    pub(crate) fn into_request(self) -> AssignmentIdentity {
        self.request
    }

    pub(crate) fn request(&self) -> &AssignmentIdentity {
        &self.request
    }
    #[cfg(test)]
    pub(crate) fn physical_assignment(&self) -> &AssignmentIdentity {
        &self.physical_assignment
    }
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }
    pub(crate) const fn intent_id(&self) -> i64 {
        self.intent_id
    }
    pub(crate) fn runner_name(&self) -> &str {
        &self.runner_name
    }
    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }
    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }
    pub(crate) fn sample_generation_id(&self) -> &str {
        &self.sample_generation_id
    }
    pub(crate) fn storage_profile_generation_id(&self) -> &str {
        &self.storage_profile_generation_id
    }
    pub(crate) fn storage_profile_digest(&self) -> &str {
        &self.storage_profile_digest
    }
    pub(crate) const fn policy_generation(&self) -> u64 {
        self.policy_generation
    }
    pub(crate) fn policy_digest(&self) -> &str {
        &self.policy_digest
    }
    pub(crate) const fn charge(&self) -> (u64, u64) {
        self.charge
    }
}

impl ConnectionJournal {
    /// Check this acquired request and its reservation before recovery.
    pub(crate) async fn acquired_recovery_candidate(
        &self,
        request: &AssignmentIdentity,
        reservation_id: i64,
        policy_generation: u64,
        policy_digest: &str,
    ) -> Result<Option<AcquiredRecoveryCandidate>, HostError> {
        let Some(request_id) = request.request_id() else {
            return Ok(None);
        };
        if reservation_id <= 0
            || policy_generation == 0
            || policy_digest.len() != 64
            || !policy_digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            || request.connection_id() != self.connection_id()
        {
            return Ok(None);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        let binding_digest = self.binding_digest().to_owned();
        let generation = i64::try_from(policy_generation).map_err(|_| HostError::Identity)?;
        let lookup = AcquiredRecoveryLookup {
            request,
            request_id,
            reservation_id,
            connection_id,
            binding_digest,
            generation,
            policy_generation,
            policy_digest: policy_digest.to_owned(),
            scale_set_id: request.scale_set_id(),
        };

        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            load_candidate(self, connection, &lookup).await
        })
        .await
    }
}

async fn load_candidate(
    scoped: &ConnectionJournal,
    connection: &turso::Connection,
    lookup: &AcquiredRecoveryLookup<'_>,
) -> Result<Option<AcquiredRecoveryCandidate>, HostError> {
    if !super::created_set_matches(
        connection,
        &lookup.connection_id,
        lookup.scale_set_id,
        &lookup.binding_digest,
    )
    .await?
        || !active_policy_matches(
            connection,
            &lookup.connection_id,
            &lookup.binding_digest,
            lookup.generation,
            &lookup.policy_digest,
        )
        .await?
        || !acquired_source_matches(
            connection,
            &lookup.connection_id,
            lookup.scale_set_id,
            lookup.request_id,
            &lookup.binding_digest,
        )
        .await?
    {
        return Ok(None);
    }
    let Some(row) = row::load_candidate_row(connection, lookup).await? else {
        return Ok(None);
    };
    row::candidate_from_row(scoped, connection, lookup, &row).await
}

async fn active_policy_matches(
    connection: &turso::Connection,
    connection_id: &str,
    binding_digest: &str,
    generation: i64,
    policy_digest: &str,
) -> Result<bool, HostError> {
    let mut rows = connection.query(
        "SELECT 1 FROM connection_active_job_authority AS active JOIN connection_job_authority_policies AS policy ON policy.connection_id = active.connection_id AND policy.generation = active.generation AND policy.policy_digest = active.policy_digest WHERE active.connection_id = ?1 AND active.generation = ?2 AND active.policy_digest = ?3 AND policy.binding_digest = ?4",
        (connection_id, generation, policy_digest, binding_digest),
    ).await.map_err(|_| HostError::Journal)?;
    Ok(rows.next().await.map_err(|_| HostError::Journal)?.is_some())
}

async fn acquired_source_matches(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    request_id: i64,
    binding_digest: &str,
) -> Result<bool, HostError> {
    let mut rows = connection.query(
        "SELECT auth.policy_mode, auth.authority_model, auth.binding_digest, auth.source_digest, auth.source_json, auth.acquire_response_json, auth.acquire_response_digest, auth.started_event_digest, auth.completed_event_digest, scope.identity FROM connection_request_authorizations AS auth JOIN connection_scopes AS scope ON scope.connection_id = auth.connection_id WHERE auth.connection_id = ?1 AND auth.scale_set_id = ?2 AND auth.request_id = ?3 AND auth.acquire_state = 'acquired' AND auth.binding_digest = ?4",
        (connection_id, scale_set_id, request_id, binding_digest),
    ).await.map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let policy_mode: String = row.get(0).map_err(|_| HostError::Journal)?;
    let authority_model: String = row.get(1).map_err(|_| HostError::Journal)?;
    let stored_binding: String = row.get(2).map_err(|_| HostError::Journal)?;
    let source_digest: String = row.get(3).map_err(|_| HostError::Journal)?;
    let source_json: String = row.get(4).map_err(|_| HostError::Journal)?;
    let response_json: String = row.get(5).map_err(|_| HostError::Journal)?;
    let response_digest: String = row.get(6).map_err(|_| HostError::Journal)?;
    let started: Option<String> = row.get(7).map_err(|_| HostError::Journal)?;
    let completed: Option<String> = row.get(8).map_err(|_| HostError::Journal)?;
    let identity: String = row.get(9).map_err(|_| HostError::Journal)?;
    let (repository_id, current_mode) = scope_source_identity(&identity, binding_digest)?;
    let expected_response = serde_json::to_string(&super::AcquireResponse::Returned {
        request_ids: &[request_id],
    })
    .map_err(|_| HostError::Journal)?;
    Ok(started.is_none()
        && completed.is_none()
        && policy_mode == current_mode
        && authority_model == super::AUTHORITY_MODEL
        && stored_binding == binding_digest
        && super::digest(source_json.as_bytes()) == source_digest
        && crate::journal::source_authorization::validate_stored_source(
            &source_json,
            repository_id,
        )
        .is_ok()
        && response_json == expected_response
        && super::digest(response_json.as_bytes()) == response_digest)
}

async fn pristine_rows(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<bool, HostError> {
    let queries = [
        "SELECT COUNT(*) FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1 AND create_state != 'unclaimed'",
        "SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers WHERE reservation_id = ?1 AND (create_state != 'unclaimed' OR start_state != 'not_requested')",
        "SELECT COUNT(*) FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1",
        "SELECT COUNT(*) FROM capacity_reservation_networks WHERE reservation_id = ?1 AND create_state != 'unclaimed'",
    ];
    for query in queries {
        let mut rows = connection
            .query(query, [reservation_id])
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        let count: i64 = row.get(0).map_err(|_| HostError::Journal)?;
        if count != 0 {
            return Ok(false);
        }
    }
    Ok(true)
}

fn scope_source_identity(identity: &str, binding_digest: &str) -> Result<(i64, String), HostError> {
    if super::digest(identity.as_bytes()) != binding_digest {
        return Err(HostError::Identity);
    }
    let value: serde_json::Value =
        serde_json::from_str(identity).map_err(|_| HostError::Journal)?;
    let repository_id = value
        .get("repository_id")
        .and_then(serde_json::Value::as_i64)
        .filter(|value| *value > 0)
        .ok_or(HostError::Identity)?;
    let mode = value
        .get("source_policy")
        .and_then(|policy| policy.get("mode"))
        .and_then(serde_json::Value::as_str)
        .ok_or(HostError::Identity)?
        .to_owned();
    if !matches!(mode.as_str(), "repository" | "branch_restricted") {
        return Err(HostError::Identity);
    }
    Ok((repository_id, mode))
}
