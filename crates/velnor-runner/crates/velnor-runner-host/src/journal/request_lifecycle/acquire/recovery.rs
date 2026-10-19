//! Exact durable state for one acquired request that can resume before JIT.

use crate::error::HostError;
use crate::journal::connection::storage::in_transaction;
use crate::journal::connection::{AssignmentIdentity, AssignmentKey, ConnectionJournal};

/// Journal proof for one acquired request and its existing physical charge.
pub(crate) struct AcquiredRecoveryCandidate {
    request: AssignmentIdentity,
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

impl AcquiredRecoveryCandidate {
    pub(crate) fn request(&self) -> &AssignmentIdentity {
        &self.request
    }
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
    /// Verify one exact acquired request and its still-pristine physical reservation.
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
        let policy_digest = policy_digest.to_owned();
        let scale_set_id = request.scale_set_id();

        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            if !super::created_set_matches(connection, &connection_id, scale_set_id, &binding_digest).await?
                || !active_policy_matches(connection, &connection_id, &binding_digest, generation, &policy_digest).await?
                || !acquired_source_matches(connection, &connection_id, scale_set_id, request_id, &binding_digest).await?
            {
                return Ok(None);
            }
            let mut rows = connection.query(
                "SELECT r.id, r.intent_id, r.assignment_key, r.engine_id, r.docker_root_digest, r.sample_generation_id, r.cpu_millicores, r.memory_bytes, i.assignment_key, i.runner_name, i.state, i.cleanup_proven, i.scale_set_id, i.runner_request_id, i.acquire_attempted, i.acquire_resolved, i.acquired, i.jit_requested, i.github_runner_id, i.docker_id, i.dind_id, i.worker_volume, i.launch_phase, p.operation_id, p.engine_id, p.docker_root_digest, p.sample_generation_id, s.bootstrap_start_state, s.resources_bound, s.jit_state, s.publication_state, s.delivery_state, s.storage_cleanup_proven, link.physical_nonce, p.profile_generation_id, p.profile_digest FROM capacity_reservations AS r JOIN connection_intents AS i ON i.id = r.intent_id AND i.connection_id = r.connection_id JOIN capacity_reservation_provisioning AS p ON p.reservation_id = r.id AND p.operation_id = i.id JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = r.id JOIN connection_request_reservations AS link ON link.reservation_id = r.id AND link.connection_id = r.connection_id AND link.scale_set_id = i.scale_set_id AND link.request_id = ?1 WHERE r.id = ?2 AND r.connection_id = ?3 AND r.binding_digest = ?4 AND r.state = 'reserved' AND i.scale_set_id = ?5 AND link.binding_digest = ?4",
                (request_id, reservation_id, connection_id.as_str(), binding_digest.as_str(), scale_set_id),
            ).await.map_err(|_| HostError::Journal)?;
            let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else { return Ok(None) };
            if rows.next().await.map_err(|_| HostError::Journal)?.is_some() { return Err(HostError::Journal) }

            let intent_id: i64 = row.get(1).map_err(|_| HostError::Journal)?;
            let reservation_key: String = row.get(2).map_err(|_| HostError::Journal)?;
            let engine_id: String = row.get(3).map_err(|_| HostError::Journal)?;
            let docker_root_digest: String = row.get(4).map_err(|_| HostError::Journal)?;
            let sample_generation_id: String = row.get(5).map_err(|_| HostError::Journal)?;
            let cpu = read_charge(row.get(6).map_err(|_| HostError::Journal)?)?;
            let memory = read_charge(row.get(7).map_err(|_| HostError::Journal)?)?;
            let intent_key: String = row.get(8).map_err(|_| HostError::Journal)?;
            let runner_name: String = row.get(9).map_err(|_| HostError::Journal)?;
            let intent_state: String = row.get(10).map_err(|_| HostError::Journal)?;
            let cleanup_proven: bool = row.get(11).map_err(|_| HostError::Journal)?;
            let intent_set_id: i64 = row.get(12).map_err(|_| HostError::Journal)?;
            let intent_request_id: Option<i64> = row.get(13).map_err(|_| HostError::Journal)?;
            let acquire_attempted: bool = row.get(14).map_err(|_| HostError::Journal)?;
            let acquire_resolved: bool = row.get(15).map_err(|_| HostError::Journal)?;
            let acquired: bool = row.get(16).map_err(|_| HostError::Journal)?;
            let jit_requested: bool = row.get(17).map_err(|_| HostError::Journal)?;
            let github_runner_id: Option<String> = row.get(18).map_err(|_| HostError::Journal)?;
            let docker_id: Option<String> = row.get(19).map_err(|_| HostError::Journal)?;
            let dind_id: Option<String> = row.get(20).map_err(|_| HostError::Journal)?;
            let worker_volume: Option<String> = row.get(21).map_err(|_| HostError::Journal)?;
            let launch_phase: Option<String> = row.get(22).map_err(|_| HostError::Journal)?;
            let provisioning_intent_id: i64 = row.get(23).map_err(|_| HostError::Journal)?;
            let provisioning_engine: String = row.get(24).map_err(|_| HostError::Journal)?;
            let provisioning_root: String = row.get(25).map_err(|_| HostError::Journal)?;
            let provisioning_sample: String = row.get(26).map_err(|_| HostError::Journal)?;
            let bootstrap_state: String = row.get(27).map_err(|_| HostError::Journal)?;
            let resources_bound: bool = row.get(28).map_err(|_| HostError::Journal)?;
            let jit_state: String = row.get(29).map_err(|_| HostError::Journal)?;
            let publication_state: String = row.get(30).map_err(|_| HostError::Journal)?;
            let delivery_state: String = row.get(31).map_err(|_| HostError::Journal)?;
            let storage_cleanup_proven: bool = row.get(32).map_err(|_| HostError::Journal)?;
            let nonce: String = row.get(33).map_err(|_| HostError::Journal)?;
            let storage_profile_generation_id: String = row.get(34).map_err(|_| HostError::Journal)?;
            let storage_profile_digest: String = row.get(35).map_err(|_| HostError::Journal)?;
            let physical_assignment = AssignmentIdentity::new(
                self.connection_id(),
                scale_set_id,
                AssignmentKey::Remote {
                    kind: "runner".to_owned(),
                    subject: nonce.clone(),
                },
            )?;

            if row.get::<i64>(0).map_err(|_| HostError::Journal)? != reservation_id
                || intent_id != provisioning_intent_id
                || reservation_key != intent_key
                || reservation_key != physical_assignment.storage_key()
                || intent_state != "pending"
                || cleanup_proven
                || intent_set_id != scale_set_id
                || intent_request_id.is_some() || acquire_attempted || acquire_resolved || acquired || jit_requested
                || github_runner_id.is_some() || docker_id.is_some() || dind_id.is_some()
                || worker_volume.is_some() || launch_phase.as_deref() != Some("provisioning_held")
                || provisioning_engine != engine_id || provisioning_root != docker_root_digest
                || provisioning_sample != sample_generation_id
                || bootstrap_state != "not_requested" || resources_bound
                || jit_state != "not_requested" || publication_state != "not_requested"
                || delivery_state != "not_requested" || storage_cleanup_proven
                || !nonce_valid(&nonce) || runner_name != format!("v{nonce}")
                || !pristine_rows(connection, reservation_id).await?
            {
                return Ok(None);
            }
            Ok(Some(AcquiredRecoveryCandidate {
                request: request.clone(), physical_assignment, reservation_id, intent_id,
                runner_name, engine_id, docker_root_digest, sample_generation_id,
                storage_profile_generation_id, storage_profile_digest,
                policy_generation, policy_digest,
                charge: (cpu, memory),
            }))
        }).await
    }
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

fn read_charge(value: i64) -> Result<u64, HostError> {
    u64::try_from(value).map_err(|_| HostError::Journal)
}

fn nonce_valid(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
