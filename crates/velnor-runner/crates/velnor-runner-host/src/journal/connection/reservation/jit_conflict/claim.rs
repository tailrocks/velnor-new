use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::{in_transaction, read_transaction};
use velnor_runner_github::RunnerReference;

use super::identity::{self, ConflictIdentity};

const MIN_LEASE_SECONDS: i64 = 10;
const MAX_LEASE_SECONDS: i64 = 300;

pub(crate) struct JitConflictRecoveryCandidate {
    identity: ConflictIdentity,
    snapshot_sha256: String,
}

pub(crate) struct JitConflictRecoveryClaim {
    identity: ConflictIdentity,
    claim_generation: i64,
    lease_until: i64,
    snapshot_sha256: String,
}

impl JitConflictRecoveryCandidate {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.identity.reservation_id
    }
}

impl JitConflictRecoveryClaim {
    pub(super) fn identity(&self) -> &ConflictIdentity {
        &self.identity
    }

    pub(super) fn snapshot_sha256(&self) -> &str {
        &self.snapshot_sha256
    }

    pub(crate) const fn reservation_id(&self) -> i64 {
        self.identity.reservation_id
    }

    pub(crate) const fn intent_id(&self) -> i64 {
        self.identity.intent_id
    }

    pub(crate) const fn scale_set_id(&self) -> i64 {
        self.identity.scale_set_id
    }

    pub(crate) const fn claim_generation(&self) -> i64 {
        self.claim_generation
    }

    pub(crate) const fn lease_until(&self) -> i64 {
        self.lease_until
    }

    pub(crate) fn requested_name(&self) -> &str {
        &self.identity.requested_name
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.identity.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.identity.docker_root_digest
    }

    pub(crate) fn physical_nonce(&self) -> &str {
        &self.identity.physical_nonce
    }
}

pub(super) async fn recoverable_candidates(
    scope: &ConnectionJournal,
) -> Result<Vec<JitConflictRecoveryCandidate>, HostError> {
    read_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        let mut rows = connection
            .query(
                "SELECT current.reservation_id FROM physical_attempt_current current JOIN physical_attempts attempt ON attempt.lineage_id=current.lineage_id AND attempt.generation=current.generation AND attempt.reservation_id=current.reservation_id AND attempt.intent_id=current.intent_id JOIN capacity_reservations reservation ON reservation.id=attempt.reservation_id JOIN connection_intents intent ON intent.id=attempt.intent_id JOIN connection_runner_jits jit ON jit.intent_id=attempt.intent_id JOIN physical_attempt_jit_conflict_receipts receipt ON receipt.lineage_id=attempt.lineage_id AND receipt.generation=attempt.generation AND receipt.reservation_id=attempt.reservation_id AND receipt.intent_id=attempt.intent_id WHERE attempt.connection_id=?1 AND attempt.binding_digest=?2 AND reservation.state='uncertain' AND intent.state='uncertain' AND intent.launch_phase='jit_uncertain' AND jit.state='uncertain' AND receipt.response_status=409 AND NOT EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_settlements settled WHERE settled.reservation_id=reservation.id) AND NOT EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_claims active WHERE active.reservation_id=reservation.id AND active.lease_until>unixepoch()) AND NOT EXISTS(SELECT 1 FROM connection_completion_inbox inbox WHERE inbox.connection_id=attempt.connection_id AND inbox.scale_set_id=attempt.scale_set_id) AND NOT EXISTS(SELECT 1 FROM connection_request_authorizations auth WHERE auth.connection_id=attempt.connection_id AND auth.scale_set_id=attempt.scale_set_id AND ((auth.started_event_digest IS NOT NULL AND auth.started_runner_name=jit.requested_name) OR (auth.completed_event_digest IS NOT NULL AND auth.completed_runner_name=jit.requested_name))) AND NOT EXISTS(SELECT 1 FROM connection_execution_decisions decision WHERE decision.intent_id=attempt.intent_id) AND NOT EXISTS(SELECT 1 FROM execution_publication_operations publication WHERE publication.reservation_id=reservation.id) AND NOT EXISTS(SELECT 1 FROM connection_execution_proofs proof WHERE proof.reservation_id=reservation.id) ORDER BY current.reservation_id",
                (scope.connection_id().as_str(), scope.binding_digest()),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let mut ids = Vec::new();
        while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
            ids.push(row.get::<i64>(0).map_err(|_| HostError::Journal)?);
        }
        drop(rows);
        let mut candidates = Vec::with_capacity(ids.len());
        for reservation_id in ids {
            let Some(identity) = load_identity(connection, scope, reservation_id).await? else {
                return Err(HostError::Identity);
            };
            let snapshot_sha256 = identity.snapshot_sha256().map_err(|_| HostError::Journal)?;
            candidates.push(JitConflictRecoveryCandidate {
                identity,
                snapshot_sha256,
            });
        }
        Ok(candidates)
    })
    .await
}

pub(super) async fn claim_candidate(
    scope: &ConnectionJournal,
    candidate: &JitConflictRecoveryCandidate,
    lease_seconds: i64,
) -> Result<Option<JitConflictRecoveryClaim>, HostError> {
    if !(MIN_LEASE_SECONDS..=MAX_LEASE_SECONDS).contains(&lease_seconds)
        || candidate.identity.connection_id != scope.connection_id().as_str()
        || candidate.identity.binding_digest != scope.binding_digest()
    {
        return Err(HostError::Identity);
    }
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        let Some(identity) = load_identity(connection, scope, candidate.reservation_id()).await?
        else {
            return Ok(None);
        };
        if identity.snapshot_sha256().map_err(|_| HostError::Journal)? != candidate.snapshot_sha256
        {
            return Ok(None);
        }
        create_claim(connection, identity, lease_seconds).await
    })
    .await
}

async fn create_claim(
    connection: &turso::Connection,
    identity: ConflictIdentity,
    lease_seconds: i64,
) -> Result<Option<JitConflictRecoveryClaim>, HostError> {
    let (claim_generation, now) = next_claim(connection, identity.reservation_id).await?;
    let lease_until = now.checked_add(lease_seconds).ok_or(HostError::Identity)?;
    let snapshot_sha256 = identity.snapshot_sha256().map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO physical_attempt_jit_conflict_recovery_claims (reservation_id,claim_generation,lineage_id,generation,intent_id,connection_id,scale_set_id,witness_request_id,physical_nonce,binding_digest,source_digest,acquire_response_digest,authority_generation,authority_digest,engine_id,docker_root_digest,requested_name,snapshot_sha256,lease_until,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,unixepoch())",
            turso::params![
                identity.reservation_id,
                claim_generation,
                identity.lineage_id,
                identity.generation,
                identity.intent_id,
                identity.connection_id.as_str(),
                identity.scale_set_id,
                identity.witness_request_id,
                identity.physical_nonce.as_str(),
                identity.binding_digest.as_str(),
                identity.source_digest.as_str(),
                identity.acquire_response_digest.as_str(),
                identity.authority_generation,
                identity.authority_digest.as_str(),
                identity.engine_id.as_str(),
                identity.docker_root_digest.as_str(),
                identity.requested_name.as_str(),
                snapshot_sha256.as_str(),
                lease_until,
            ],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(Some(JitConflictRecoveryClaim {
        identity,
        claim_generation,
        lease_until,
        snapshot_sha256,
    }))
}

pub(super) async fn has_delete_started(
    scope: &ConnectionJournal,
    claim: &JitConflictRecoveryClaim,
) -> Result<bool, HostError> {
    crate::journal::connection::storage::read_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        validate_claim_in_transaction(connection, scope, claim).await?;
        identity::has_delete_started(connection, claim).await
    })
    .await
}

pub(super) async fn delete_target(
    scope: &ConnectionJournal,
    claim: &JitConflictRecoveryClaim,
) -> Result<Option<RunnerReference>, HostError> {
    crate::journal::connection::storage::read_transaction(
        scope.journal_ref(),
        async |connection| {
            scope.validate_scope(connection).await?;
            validate_claim_in_transaction(connection, scope, claim).await?;
            let mut rows = connection
                .query(
                    "SELECT progress.runner_id,progress.runner_name,progress.runner_scale_set_id FROM physical_attempt_jit_conflict_recovery_progress progress JOIN physical_attempt_jit_conflict_recovery_progress target ON target.reservation_id=progress.reservation_id AND target.claim_generation=progress.claim_generation AND target.phase_index=2 AND target.phase='remote_target' AND target.runner_id=progress.runner_id AND target.runner_name=progress.runner_name AND target.runner_scale_set_id=progress.runner_scale_set_id WHERE progress.reservation_id=?1 AND progress.phase_index=3 AND progress.phase='delete_started' ORDER BY progress.claim_generation DESC LIMIT 1",
                    [claim.reservation_id()],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
                return Ok(None);
            };
            let runner = RunnerReference {
                id: row.get(0).map_err(|_| HostError::Journal)?,
                name: row.get(1).map_err(|_| HostError::Journal)?,
                runner_scale_set_id: row.get(2).map_err(|_| HostError::Journal)?,
            };
            if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
                || runner.id <= 0
                || runner.name != claim.requested_name()
                || runner.runner_scale_set_id != claim.scale_set_id()
            {
                return Err(HostError::Identity);
            }
            Ok(Some(runner))
        },
    )
    .await
}

pub(super) async fn current_observation(
    scope: &ConnectionJournal,
    claim: &JitConflictRecoveryClaim,
) -> Result<Option<Option<RunnerReference>>, HostError> {
    crate::journal::connection::storage::read_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        validate_claim_in_transaction(connection, scope, claim).await?;
        Ok(identity::current_observation(connection, claim)
            .await?
            .map(|target| {
                target.id.map(|id| RunnerReference {
                    id,
                    name: target.name,
                    runner_scale_set_id: target.scale_set_id,
                })
            }))
    })
    .await
}

async fn next_claim(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<(i64, i64), HostError> {
    let mut rows = connection
        .query(
            "SELECT COALESCE(MAX(claim_generation),0),COALESCE(MAX(lease_until),0),unixepoch() FROM physical_attempt_jit_conflict_recovery_claims WHERE reservation_id=?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let latest: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let prior_lease: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    let now: i64 = row.get(2).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    if latest > 0 && prior_lease > now {
        return Err(HostError::Journal);
    }
    Ok((latest.checked_add(1).ok_or(HostError::Journal)?, now))
}

async fn load_identity(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    reservation_id: i64,
) -> Result<Option<ConflictIdentity>, HostError> {
    let mut rows = connection
        .query(
            "SELECT attempt.lineage_id,attempt.generation,attempt.reservation_id,attempt.intent_id,attempt.connection_id,attempt.scale_set_id,attempt.witness_request_id,attempt.physical_nonce,attempt.binding_digest,attempt.source_digest,auth.acquire_response_digest,receipt.authority_generation,receipt.authority_digest,attempt.engine_id,attempt.docker_root_digest,state.instance_binding_digest,jit.requested_name FROM physical_attempt_current current JOIN physical_attempts attempt ON attempt.lineage_id=current.lineage_id AND attempt.generation=current.generation AND attempt.reservation_id=current.reservation_id AND attempt.intent_id=current.intent_id JOIN capacity_reservations reservation ON reservation.id=attempt.reservation_id AND reservation.intent_id=attempt.intent_id AND reservation.connection_id=attempt.connection_id JOIN connection_intents intent ON intent.id=attempt.intent_id AND intent.connection_id=attempt.connection_id JOIN capacity_reservation_provisioning_state state ON state.reservation_id=reservation.id JOIN capacity_reservation_provisioning provisioning ON provisioning.reservation_id=reservation.id JOIN connection_runner_jits jit ON jit.intent_id=intent.id AND jit.connection_id=intent.connection_id JOIN connection_request_authorizations auth ON auth.connection_id=attempt.connection_id AND auth.scale_set_id=attempt.scale_set_id AND auth.request_id=attempt.witness_request_id JOIN physical_attempt_jit_conflict_receipts receipt ON receipt.lineage_id=attempt.lineage_id AND receipt.generation=attempt.generation AND receipt.reservation_id=attempt.reservation_id AND receipt.intent_id=attempt.intent_id JOIN connection_active_job_authority active ON active.connection_id=attempt.connection_id JOIN connection_job_authority_policies policy ON policy.connection_id=active.connection_id AND policy.generation=active.generation AND policy.policy_digest=active.policy_digest WHERE current.reservation_id=?1 AND attempt.connection_id=?2 AND attempt.binding_digest=?3 AND current.reservation_id=reservation.id AND reservation.state='uncertain' AND reservation.binding_digest=attempt.binding_digest AND reservation.engine_id=attempt.engine_id AND reservation.docker_root_digest=attempt.docker_root_digest AND ((reservation.local_cleanup_proven=0 AND state.storage_cleanup_proven=0 AND reservation.remote_cleanup_proven=0 AND NOT EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress prior WHERE prior.reservation_id=reservation.id AND (prior.phase_index=1 OR prior.phase_index=4))) OR (reservation.local_cleanup_proven=1 AND state.storage_cleanup_proven=1 AND EXISTS(SELECT 1 FROM capacity_reservation_provisioning_cleanups cleanup JOIN capacity_reservation_cleanup_observer_witnesses witness ON witness.reservation_id=cleanup.reservation_id WHERE cleanup.reservation_id=reservation.id AND cleanup.instance_binding_digest=state.instance_binding_digest AND cleanup.role_mask=2047 AND cleanup.containers_absent=1 AND cleanup.volumes_absent=1 AND cleanup.mounts_detached=1 AND cleanup.cgroups_absent=1 AND witness.connection_id=attempt.connection_id AND witness.operation_id=attempt.intent_id AND witness.engine_id=attempt.engine_id AND witness.docker_root_digest=attempt.docker_root_digest AND witness.instance_binding_digest=state.instance_binding_digest) AND EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress local WHERE local.reservation_id=reservation.id AND local.phase_index=1 AND local.phase='local_absent') AND ((reservation.remote_cleanup_proven=0) OR (reservation.remote_cleanup_proven=1 AND EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress remote WHERE remote.reservation_id=reservation.id AND remote.phase_index=4 AND remote.phase='remote_absent' AND remote.runner_name=jit.requested_name AND remote.runner_scale_set_id=attempt.scale_set_id AND remote.lookup_status=200 AND remote.lookup_count=0))))) AND intent.kind='launch' AND intent.state='uncertain' AND intent.launch_phase='jit_uncertain' AND intent.scale_set_id=attempt.scale_set_id AND intent.runner_name=jit.requested_name AND intent.github_runner_id IS NULL AND intent.cleanup_proven=0 AND intent.jit_requested=1 AND state.resources_bound=1 AND state.jit_state='uncertain' AND state.runner_id IS NULL AND state.runner_name IS NULL AND state.runner_scale_set_id IS NULL AND provisioning.operation_id=attempt.intent_id AND provisioning.engine_id=attempt.engine_id AND provisioning.docker_root_digest=attempt.docker_root_digest AND jit.scale_set_id=attempt.scale_set_id AND jit.witness_request_id=attempt.witness_request_id AND jit.state='uncertain' AND jit.runner_id IS NULL AND jit.runner_name IS NULL AND jit.started_request_id IS NULL AND jit.completed_request_id IS NULL AND auth.acquire_state='acquired' AND auth.acquire_response_digest IS NOT NULL AND auth.binding_digest=attempt.binding_digest AND auth.source_digest=attempt.source_digest AND (attempt.acquire_response_digest IS NULL OR attempt.acquire_response_digest=auth.acquire_response_digest) AND receipt.response_status=409 AND receipt.connection_id=attempt.connection_id AND receipt.scale_set_id=attempt.scale_set_id AND receipt.witness_request_id=attempt.witness_request_id AND receipt.requested_name=jit.requested_name AND receipt.binding_digest=attempt.binding_digest AND receipt.source_digest=attempt.source_digest AND receipt.acquire_response_digest=auth.acquire_response_digest AND receipt.engine_id=attempt.engine_id AND receipt.docker_root_digest=attempt.docker_root_digest AND active.generation=receipt.authority_generation AND active.policy_digest=receipt.authority_digest AND policy.binding_digest=attempt.binding_digest AND jit.requested_name='v'||attempt.physical_nonce AND NOT EXISTS(SELECT 1 FROM connection_completion_inbox inbox WHERE inbox.connection_id=attempt.connection_id AND inbox.scale_set_id=attempt.scale_set_id) AND NOT EXISTS(SELECT 1 FROM connection_request_authorizations event_auth WHERE event_auth.connection_id=attempt.connection_id AND event_auth.scale_set_id=attempt.scale_set_id AND ((event_auth.started_event_digest IS NOT NULL AND event_auth.started_runner_name=jit.requested_name) OR (event_auth.completed_event_digest IS NOT NULL AND event_auth.completed_runner_name=jit.requested_name))) AND NOT EXISTS(SELECT 1 FROM connection_execution_decisions decision WHERE decision.intent_id=attempt.intent_id) AND NOT EXISTS(SELECT 1 FROM execution_publication_operations publication WHERE publication.reservation_id=reservation.id) AND NOT EXISTS(SELECT 1 FROM connection_execution_proofs proof WHERE proof.reservation_id=reservation.id) AND NOT EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_settlements settled WHERE settled.reservation_id=reservation.id) AND NOT EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_claims live_claim WHERE live_claim.reservation_id=reservation.id AND live_claim.lease_until>unixepoch())",
            (reservation_id, scope.connection_id().as_str(), scope.binding_digest()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let identity = ConflictIdentity {
        lineage_id: row.get(0).map_err(|_| HostError::Journal)?,
        generation: row.get(1).map_err(|_| HostError::Journal)?,
        reservation_id: row.get(2).map_err(|_| HostError::Journal)?,
        intent_id: row.get(3).map_err(|_| HostError::Journal)?,
        connection_id: row.get(4).map_err(|_| HostError::Journal)?,
        scale_set_id: row.get(5).map_err(|_| HostError::Journal)?,
        witness_request_id: row.get(6).map_err(|_| HostError::Journal)?,
        physical_nonce: row.get(7).map_err(|_| HostError::Journal)?,
        binding_digest: row.get(8).map_err(|_| HostError::Journal)?,
        source_digest: row.get(9).map_err(|_| HostError::Journal)?,
        acquire_response_digest: row.get(10).map_err(|_| HostError::Journal)?,
        authority_generation: row.get(11).map_err(|_| HostError::Journal)?,
        authority_digest: row.get(12).map_err(|_| HostError::Journal)?,
        engine_id: row.get(13).map_err(|_| HostError::Journal)?,
        docker_root_digest: row.get(14).map_err(|_| HostError::Journal)?,
        instance_binding_digest: row.get(15).map_err(|_| HostError::Journal)?,
        requested_name: row.get(16).map_err(|_| HostError::Journal)?,
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Identity);
    }
    Ok(Some(identity))
}

pub(super) async fn validate_claim_in_transaction(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    claim: &JitConflictRecoveryClaim,
) -> Result<(), HostError> {
    let identity = claim.identity();
    if identity.connection_id != scope.connection_id().as_str()
        || identity.binding_digest != scope.binding_digest()
        || identity.snapshot_sha256().map_err(|_| HostError::Journal)? != claim.snapshot_sha256()
    {
        return Err(HostError::Identity);
    }
    let mut rows = connection
        .query(
            "SELECT claim.snapshot_sha256,claim.lease_until FROM physical_attempt_jit_conflict_recovery_claims claim JOIN physical_attempt_current current ON current.lineage_id=claim.lineage_id AND current.generation=claim.generation AND current.reservation_id=claim.reservation_id AND current.intent_id=claim.intent_id JOIN physical_attempts attempt ON attempt.lineage_id=current.lineage_id AND attempt.generation=current.generation AND attempt.reservation_id=current.reservation_id AND attempt.intent_id=current.intent_id JOIN capacity_reservations reservation ON reservation.id=claim.reservation_id AND reservation.intent_id=claim.intent_id AND reservation.connection_id=claim.connection_id JOIN connection_intents intent ON intent.id=claim.intent_id AND intent.connection_id=claim.connection_id JOIN connection_runner_jits jit ON jit.intent_id=claim.intent_id AND jit.connection_id=claim.connection_id JOIN capacity_reservation_provisioning_state state ON state.reservation_id=claim.reservation_id JOIN physical_attempt_jit_conflict_receipts receipt ON receipt.lineage_id=claim.lineage_id AND receipt.generation=claim.generation AND receipt.reservation_id=claim.reservation_id AND receipt.intent_id=claim.intent_id JOIN connection_request_authorizations auth ON auth.connection_id=claim.connection_id AND auth.scale_set_id=claim.scale_set_id AND auth.request_id=claim.witness_request_id JOIN connection_active_job_authority active ON active.connection_id=claim.connection_id JOIN connection_job_authority_policies policy ON policy.connection_id=active.connection_id AND policy.generation=active.generation AND policy.policy_digest=active.policy_digest WHERE claim.reservation_id=?1 AND claim.claim_generation=?2 AND claim.lineage_id=?3 AND claim.generation=?4 AND claim.intent_id=?5 AND claim.connection_id=?6 AND claim.scale_set_id=?7 AND claim.witness_request_id=?8 AND claim.physical_nonce=?9 AND claim.binding_digest=?10 AND claim.source_digest=?11 AND claim.acquire_response_digest=?12 AND claim.authority_generation=?13 AND claim.authority_digest=?14 AND claim.engine_id=?15 AND claim.docker_root_digest=?16 AND claim.requested_name=?17 AND claim.snapshot_sha256=?18 AND claim.lease_until=?19 AND claim.claim_generation=(SELECT MAX(latest.claim_generation) FROM physical_attempt_jit_conflict_recovery_claims latest WHERE latest.reservation_id=claim.reservation_id) AND claim.lease_until>unixepoch() AND attempt.connection_id=claim.connection_id AND attempt.scale_set_id=claim.scale_set_id AND attempt.witness_request_id=claim.witness_request_id AND attempt.physical_nonce=claim.physical_nonce AND attempt.binding_digest=claim.binding_digest AND attempt.source_digest=claim.source_digest AND (attempt.acquire_response_digest IS NULL OR attempt.acquire_response_digest=claim.acquire_response_digest) AND attempt.engine_id=claim.engine_id AND attempt.docker_root_digest=claim.docker_root_digest AND reservation.state='uncertain' AND reservation.binding_digest=claim.binding_digest AND reservation.engine_id=claim.engine_id AND reservation.docker_root_digest=claim.docker_root_digest AND ((reservation.local_cleanup_proven=0 AND state.storage_cleanup_proven=0 AND reservation.remote_cleanup_proven=0 AND NOT EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress prior WHERE prior.reservation_id=reservation.id AND (prior.phase_index=1 OR prior.phase_index=4))) OR (reservation.local_cleanup_proven=1 AND state.storage_cleanup_proven=1 AND state.instance_binding_digest=?20 AND EXISTS(SELECT 1 FROM capacity_reservation_provisioning_cleanups cleanup JOIN capacity_reservation_cleanup_observer_witnesses witness ON witness.reservation_id=cleanup.reservation_id WHERE cleanup.reservation_id=reservation.id AND cleanup.instance_binding_digest=state.instance_binding_digest AND cleanup.role_mask=2047 AND cleanup.containers_absent=1 AND cleanup.volumes_absent=1 AND cleanup.mounts_detached=1 AND cleanup.cgroups_absent=1 AND witness.connection_id=claim.connection_id AND witness.operation_id=claim.intent_id AND witness.engine_id=claim.engine_id AND witness.docker_root_digest=claim.docker_root_digest AND witness.instance_binding_digest=state.instance_binding_digest) AND EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress local WHERE local.reservation_id=reservation.id AND local.phase_index=1 AND local.phase='local_absent') AND (reservation.remote_cleanup_proven=0 OR EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress remote WHERE remote.reservation_id=reservation.id AND remote.phase_index=4 AND remote.phase='remote_absent' AND remote.runner_name=jit.requested_name AND remote.runner_scale_set_id=attempt.scale_set_id AND remote.lookup_status=200 AND remote.lookup_count=0 AND remote.request_sha256 IS NOT NULL AND remote.response_sha256 IS NOT NULL)))) AND intent.kind='launch' AND intent.state='uncertain' AND intent.launch_phase='jit_uncertain' AND intent.scale_set_id=claim.scale_set_id AND intent.runner_name=claim.requested_name AND intent.github_runner_id IS NULL AND intent.cleanup_proven=0 AND intent.jit_requested=1 AND jit.scale_set_id=claim.scale_set_id AND jit.witness_request_id=claim.witness_request_id AND jit.requested_name=claim.requested_name AND jit.state='uncertain' AND jit.runner_id IS NULL AND jit.runner_name IS NULL AND jit.started_request_id IS NULL AND jit.completed_request_id IS NULL AND state.resources_bound=1 AND state.jit_state='uncertain' AND state.runner_id IS NULL AND state.runner_name IS NULL AND state.runner_scale_set_id IS NULL AND receipt.response_status=409 AND receipt.connection_id=claim.connection_id AND receipt.scale_set_id=claim.scale_set_id AND receipt.witness_request_id=claim.witness_request_id AND receipt.requested_name=claim.requested_name AND receipt.binding_digest=claim.binding_digest AND receipt.source_digest=claim.source_digest AND receipt.acquire_response_digest=auth.acquire_response_digest AND receipt.authority_generation=claim.authority_generation AND receipt.authority_digest=claim.authority_digest AND receipt.engine_id=claim.engine_id AND receipt.docker_root_digest=claim.docker_root_digest AND auth.acquire_state='acquired' AND auth.acquire_response_digest=claim.acquire_response_digest AND auth.binding_digest=claim.binding_digest AND auth.source_digest=claim.source_digest AND auth.started_event_digest IS NULL AND auth.completed_event_digest IS NULL AND active.generation=claim.authority_generation AND active.policy_digest=claim.authority_digest AND policy.binding_digest=claim.binding_digest AND claim.requested_name='v'||claim.physical_nonce AND NOT EXISTS(SELECT 1 FROM connection_completion_inbox inbox WHERE inbox.connection_id=claim.connection_id AND inbox.scale_set_id=claim.scale_set_id) AND NOT EXISTS(SELECT 1 FROM connection_request_authorizations event_auth WHERE event_auth.connection_id=claim.connection_id AND event_auth.scale_set_id=claim.scale_set_id AND ((event_auth.started_event_digest IS NOT NULL AND event_auth.started_runner_name=claim.requested_name) OR (event_auth.completed_event_digest IS NOT NULL AND event_auth.completed_runner_name=claim.requested_name))) AND NOT EXISTS(SELECT 1 FROM connection_execution_decisions decision WHERE decision.intent_id=claim.intent_id) AND NOT EXISTS(SELECT 1 FROM execution_publication_operations publication WHERE publication.reservation_id=claim.reservation_id) AND NOT EXISTS(SELECT 1 FROM connection_execution_proofs proof WHERE proof.reservation_id=claim.reservation_id) AND NOT EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_settlements settled WHERE settled.reservation_id=claim.reservation_id)",
            turso::params![
                identity.reservation_id,
                claim.claim_generation(),
                identity.lineage_id,
                identity.generation,
                identity.intent_id,
                identity.connection_id.as_str(),
                identity.scale_set_id,
                identity.witness_request_id,
                identity.physical_nonce.as_str(),
                identity.binding_digest.as_str(),
                identity.source_digest.as_str(),
                identity.acquire_response_digest.as_str(),
                identity.authority_generation,
                identity.authority_digest.as_str(),
                identity.engine_id.as_str(),
                identity.docker_root_digest.as_str(),
                identity.requested_name.as_str(),
                claim.snapshot_sha256(),
                claim.lease_until(),
                identity.instance_binding_digest.as_str(),
            ],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Err(HostError::Identity);
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || row.get::<String>(0).map_err(|_| HostError::Journal)? != claim.snapshot_sha256()
        || row.get::<i64>(1).map_err(|_| HostError::Journal)? != claim.lease_until()
    {
        return Err(HostError::Identity);
    }
    Ok(())
}
