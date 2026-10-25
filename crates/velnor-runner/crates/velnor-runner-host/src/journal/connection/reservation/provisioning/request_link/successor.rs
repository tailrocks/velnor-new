use crate::config::ApprovedJobAuthorityPolicy;
use crate::error::HostError;
use crate::journal::connection::AssignmentIdentity;
use crate::journal::connection::storage::in_transaction;
use crate::launch::StartPermit;

use super::super::super::admission::{ReservationRequest, ReservationResult};
use super::super::super::storage_budget::StorageProvisioningAdmission;
use super::super::ConnectionJournal;
use super::super::admission::reserve_in_transaction;
use super::PhysicalSuccessorReservationResult;
use super::verification::{physical_nonce, reservation_intent_id};

impl ConnectionJournal {
    /// Check whether a fully settled terminal attempt permits a fresh physical generation.
    pub(crate) async fn acquired_successor_ready(
        &self,
        request: &AssignmentIdentity,
        policy_generation: u64,
        policy_digest: &str,
    ) -> Result<bool, HostError> {
        if request.request_id().is_none() {
            return Err(HostError::Identity);
        }
        if request.connection_id() != self.connection_id()
            || policy_generation == 0
            || policy_digest.len() != 64
            || !policy_digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(HostError::Identity);
        }
        let generation = i64::try_from(policy_generation).map_err(|_| HostError::Identity)?;
        let policy_digest = policy_digest.to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            successor_authorized(connection, self, request, generation, &policy_digest).await
        })
        .await
    }

    /// Reserve fresh physical capacity for an acquired request without another `AcquireJobs` call.
    pub(crate) async fn try_reserve_successor_for_acquired_request(
        &self,
        physical: ReservationRequest<'_>,
        permit: StartPermit,
        admission: StorageProvisioningAdmission,
        request: &AssignmentIdentity,
        policy: &ApprovedJobAuthorityPolicy,
        assigned_total: u32,
    ) -> Result<PhysicalSuccessorReservationResult, HostError> {
        let nonce = physical_nonce(physical.assignment.key())?.to_owned();
        let request_id = request.request_id().ok_or(HostError::Identity)?;
        let policy_generation = policy.generation();
        let policy_digest = policy.policy_sha256();
        if request.connection_id() != self.connection_id()
            || physical.assignment.connection_id() != self.connection_id()
            || request.scale_set_id() != physical.assignment.scale_set_id()
            || request_id <= 0
            || policy_generation == 0
            || assigned_total == 0
            || policy_digest.len() != 64
            || !policy_digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(HostError::Identity);
        }
        let generation = i64::try_from(policy_generation).map_err(|_| HostError::Identity)?;
        let policy_digest = policy_digest.to_owned();
        in_transaction(self.journal_ref(), async move |connection| {
            self.validate_scope(connection).await?;
            if !successor_authorized(connection, self, request, generation, &policy_digest).await? {
                return Err(HostError::Journal);
            }
            if !successor_population_shortfall(
                connection,
                self.connection_id().as_str(),
                request.scale_set_id(),
                assigned_total,
            )
            .await?
            {
                return Ok(PhysicalSuccessorReservationResult::AtCapacity);
            }
            let mut permit = Some(permit);
            let reserved =
                reserve_in_transaction(connection, self, physical, &mut permit, &admission).await?;
            let reservation_id = match reserved {
                ReservationResult::Reserved { reservation_id } => reservation_id,
                ReservationResult::AtCapacity => {
                    return Ok(PhysicalSuccessorReservationResult::AtCapacity);
                }
                #[cfg(test)]
                ReservationResult::Resumed { reservation_id } => {
                    reservation_intent_id(connection, reservation_id).await?;
                    return Ok(PhysicalSuccessorReservationResult::ReconcileRequired);
                }
                ReservationResult::ReconcileRequired { reservation_id }
                | ReservationResult::Released { reservation_id } => {
                    reservation_intent_id(connection, reservation_id).await?;
                    return Ok(PhysicalSuccessorReservationResult::ReconcileRequired);
                }
            };
            let intent_id = reservation_intent_id(connection, reservation_id).await?;
            super::super::super::physical_attempt::record_in_transaction(
                connection,
                self,
                reservation_id,
                intent_id,
                request_id,
                &nonce,
            )
            .await?;
            Ok(PhysicalSuccessorReservationResult::Reserved { reservation_id })
        })
        .await
    }
}

async fn successor_population_shortfall(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    assigned_total: u32,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM physical_attempt_current AS current JOIN physical_attempts AS attempt ON attempt.lineage_id = current.lineage_id AND attempt.generation = current.generation AND attempt.reservation_id = current.reservation_id JOIN capacity_reservations AS reservation ON reservation.id = current.reservation_id WHERE attempt.connection_id = ?1 AND attempt.scale_set_id = ?2 AND reservation.state != 'released'",
            (connection_id, scale_set_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let count: i64 = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(count >= 0
        && u64::try_from(count).map_err(|_| HostError::Journal)? < u64::from(assigned_total))
}

async fn successor_authorized(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    request: &AssignmentIdentity,
    policy_generation: i64,
    policy_digest: &str,
) -> Result<bool, HostError> {
    let request_id = request.request_id().ok_or(HostError::Identity)?;
    let mut rows = connection
        .query(
            "SELECT 1 FROM connection_request_authorizations AS auth JOIN connection_active_job_authority AS active ON active.connection_id = auth.connection_id JOIN connection_job_authority_policies AS policy ON policy.connection_id = active.connection_id AND policy.generation = active.generation AND policy.policy_digest = active.policy_digest WHERE auth.connection_id = ?1 AND auth.scale_set_id = ?2 AND auth.request_id = ?3 AND auth.binding_digest = ?4 AND auth.acquire_state = 'acquired' AND auth.started_event_digest IS NULL AND auth.completed_event_digest IS NULL AND active.generation = ?5 AND active.policy_digest = ?6 AND policy.binding_digest = ?4 AND NOT EXISTS (SELECT 1 FROM connection_completion_inbox AS inbox WHERE inbox.connection_id = auth.connection_id AND inbox.scale_set_id = auth.scale_set_id) AND NOT EXISTS (SELECT 1 FROM connection_request_authorizations AS event_auth WHERE event_auth.connection_id = auth.connection_id AND event_auth.scale_set_id = auth.scale_set_id AND ((event_auth.started_event_digest IS NOT NULL) OR (event_auth.completed_event_digest IS NOT NULL))) AND (EXISTS (SELECT 1 FROM physical_attempts AS prior JOIN physical_attempt_terminal_proofs AS outcome ON outcome.lineage_id = prior.lineage_id AND outcome.generation = prior.generation AND outcome.reservation_id = prior.reservation_id WHERE prior.connection_id = auth.connection_id AND prior.scale_set_id = auth.scale_set_id AND prior.witness_request_id = auth.request_id AND outcome.outcome IN ('pre_jit_capacity_available', 'normal_completion', 'post_jit_retired')) OR EXISTS (SELECT 1 FROM physical_attempt_jit_conflict_settlements settlement JOIN physical_attempt_jit_conflict_recovery_claims claim ON claim.reservation_id = settlement.reservation_id AND claim.claim_generation = settlement.claim_generation JOIN physical_attempts prior ON prior.lineage_id = settlement.lineage_id AND prior.generation = settlement.generation AND prior.reservation_id = settlement.reservation_id JOIN capacity_reservations reservation ON reservation.id = settlement.reservation_id AND reservation.intent_id = claim.intent_id AND reservation.connection_id = claim.connection_id JOIN connection_intents intent ON intent.id = claim.intent_id AND intent.connection_id = claim.connection_id JOIN connection_runner_jits jit ON jit.intent_id = claim.intent_id AND jit.connection_id = claim.connection_id JOIN capacity_reservation_metadata_settlements metadata ON metadata.reservation_id = settlement.reservation_id AND metadata.generation = settlement.metadata_generation JOIN physical_attempt_jit_conflict_recovery_progress local ON local.reservation_id = claim.reservation_id AND local.claim_generation = claim.claim_generation AND local.phase_index = 1 AND local.phase = 'local_absent' JOIN physical_attempt_jit_conflict_recovery_progress remote ON remote.reservation_id = claim.reservation_id AND remote.claim_generation = claim.claim_generation AND remote.phase_index = 4 AND remote.phase = 'remote_absent' WHERE prior.connection_id = auth.connection_id AND prior.scale_set_id = auth.scale_set_id AND prior.witness_request_id = auth.request_id AND settlement.proof_digest = settlement.settlement_digest AND settlement.remote_runner_id IS remote.runner_id AND settlement.remote_runner_name = remote.runner_name AND settlement.remote_scale_set_id = remote.runner_scale_set_id AND claim.connection_id = auth.connection_id AND claim.scale_set_id = auth.scale_set_id AND reservation.state = 'released' AND reservation.remote_cleanup_proven = 1 AND reservation.local_cleanup_proven = 1 AND intent.state = 'uncertain' AND intent.launch_phase = 'jit_uncertain' AND intent.cleanup_proven = 1 AND jit.state = 'conflict_retired' AND jit.runner_id IS NULL AND jit.runner_name IS NULL AND jit.requested_name = claim.requested_name AND metadata.outcome = 'settled' AND length(local.evidence_sha256) = 64 AND length(remote.evidence_sha256) = 64 AND NOT EXISTS (SELECT 1 FROM physical_attempt_current current WHERE current.lineage_id = prior.lineage_id)) OR EXISTS (SELECT 1 FROM physical_attempt_current AS current JOIN physical_attempts AS serving ON serving.lineage_id = current.lineage_id AND serving.generation = current.generation AND serving.reservation_id = current.reservation_id JOIN connection_intents AS intent ON intent.id = serving.intent_id JOIN connection_runner_jits AS jit ON jit.intent_id = intent.id AND jit.connection_id = intent.connection_id WHERE serving.connection_id = ?1 AND serving.scale_set_id = ?2 AND serving.witness_request_id = ?3 AND intent.runner_request_id IS NOT NULL AND intent.runner_request_id != ?3 AND jit.state IN ('started', 'completed', 'cleaned')))",
            (
                scope.connection_id().as_str(),
                request.scale_set_id(),
                request_id,
                scope.binding_digest(),
                policy_generation,
                policy_digest,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let authorized = rows.next().await.map_err(|_| HostError::Journal)?.is_some();
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(authorized)
}
