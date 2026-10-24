use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;
use crate::launch::bootstrap::VerifiedStorageCleanup;
use velnor_runner_github::{RunnerReference, RunnerResponseReceipt};

use super::claim::{JitConflictRecoveryClaim, validate_claim_in_transaction};
use super::identity;
use super::identity::{
    Target, delete_started_digest_values, local_evidence, now_millis, remote_absence_digest,
    remote_target_digest, target_from_runner, timestamp, validate_lookup,
};

struct PhaseInput<'a> {
    index: i64,
    phase: &'a str,
    evidence_sha256: &'a str,
    target: Option<&'a Target>,
    request_sha256: Option<&'a str>,
    response_sha256: Option<&'a str>,
    lookup_status: Option<i64>,
    lookup_count: Option<i64>,
    observed_at_unix_ms: i64,
}

pub(super) async fn validate_claim(
    scope: &ConnectionJournal,
    claim: &JitConflictRecoveryClaim,
) -> Result<(), HostError> {
    crate::journal::connection::storage::read_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        validate_claim_in_transaction(connection, scope, claim).await
    })
    .await
}

pub(super) async fn record_local_absence(
    scope: &ConnectionJournal,
    claim: &JitConflictRecoveryClaim,
    proof: VerifiedStorageCleanup,
) -> Result<(), HostError> {
    let identity = claim.identity();
    if proof.reservation_id() != identity.reservation_id
        || proof.operation_id() != identity.intent_id
        || proof.engine_id() != identity.engine_id
        || proof.docker_root_digest() != identity.docker_root_digest
        || proof.instance_binding_digest() != identity.instance_binding_digest
        || !proof.all_owned_refs_absent()
    {
        return Err(HostError::Identity);
    }
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        validate_claim_in_transaction(connection, scope, claim).await?;
        crate::journal::connection::reservation::provisioning::record_retirement_cleanup_evidence(
            connection, scope, &proof,
        )
        .await?;
        mark_local_cleanup(connection, claim).await?;
        let evidence = local_evidence(connection, identity).await?;
        insert_phase(
            connection,
            claim,
            PhaseInput {
                index: 1,
                phase: "local_absent",
                evidence_sha256: &evidence,
                target: None,
                request_sha256: None,
                response_sha256: None,
                lookup_status: None,
                lookup_count: None,
                observed_at_unix_ms: now_millis(connection).await?,
            },
        )
        .await
    })
    .await
}

pub(super) async fn resume_local_absence(
    scope: &ConnectionJournal,
    claim: &JitConflictRecoveryClaim,
) -> Result<bool, HostError> {
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        validate_claim_in_transaction(connection, scope, claim).await?;
        let mut rows = connection
            .query(
                "SELECT reservation.local_cleanup_proven,EXISTS(SELECT 1 FROM capacity_reservation_provisioning_cleanups cleanup WHERE cleanup.reservation_id=reservation.id),EXISTS(SELECT 1 FROM capacity_reservation_cleanup_observer_witnesses witness WHERE witness.reservation_id=reservation.id AND witness.connection_id=?2 AND witness.operation_id=?3) FROM capacity_reservations reservation WHERE reservation.id=?1 AND reservation.connection_id=?2 AND reservation.intent_id=?3 AND reservation.state='uncertain'",
                (claim.reservation_id(), scope.connection_id().as_str(), claim.intent_id()),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Identity)?;
        let local = row.get::<i64>(0).map_err(|_| HostError::Journal)?;
        let cleanup = row.get::<i64>(1).map_err(|_| HostError::Journal)?;
        let witness = row.get::<i64>(2).map_err(|_| HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
            return Err(HostError::Identity);
        }
        if local == 0 && cleanup == 0 && witness == 0 {
            return Ok(false);
        }
        if local != 1 || cleanup != 1 || witness != 1 {
            return Err(HostError::Identity);
        }
        let evidence = local_evidence(connection, claim.identity()).await?;
        insert_phase(
            connection,
            claim,
            PhaseInput {
                index: 1,
                phase: "local_absent",
                evidence_sha256: &evidence,
                target: None,
                request_sha256: None,
                response_sha256: None,
                lookup_status: None,
                lookup_count: None,
                observed_at_unix_ms: now_millis(connection).await?,
            },
        )
        .await?;
        Ok(true)
    })
    .await
}

async fn mark_local_cleanup(
    connection: &turso::Connection,
    claim: &JitConflictRecoveryClaim,
) -> Result<(), HostError> {
    let identity = claim.identity();
    let changed = connection
        .execute(
            "UPDATE capacity_reservations SET local_cleanup_proven=1 WHERE id=?1 AND connection_id=?2 AND intent_id=?3 AND state='uncertain' AND EXISTS(SELECT 1 FROM physical_attempt_current current JOIN physical_attempt_jit_conflict_recovery_claims claim ON claim.lineage_id=current.lineage_id AND claim.generation=current.generation AND claim.reservation_id=current.reservation_id AND claim.intent_id=current.intent_id WHERE current.reservation_id=?1 AND claim.claim_generation=?4 AND claim.snapshot_sha256=?5 AND claim.lease_until>unixepoch())",
            (identity.reservation_id, identity.connection_id.as_str(), identity.intent_id, claim.claim_generation(), claim.snapshot_sha256()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

pub(super) async fn record_remote_target(
    scope: &ConnectionJournal,
    claim: &JitConflictRecoveryClaim,
    runner: &RunnerReference,
    receipt: &RunnerResponseReceipt,
) -> Result<(), HostError> {
    validate_lookup(receipt, true)?;
    let identity = claim.identity();
    if runner.id <= 0
        || runner.name != identity.requested_name
        || runner.runner_scale_set_id != identity.scale_set_id
    {
        return Err(HostError::Ownership);
    }
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        validate_claim_in_transaction(connection, scope, claim).await?;
        if identity::has_delete_started(connection, claim).await? {
            return Err(HostError::Identity);
        }
        let target = target_from_runner(runner);
        let evidence = remote_target_digest(claim, &target, receipt)?;
        insert_phase(
            connection,
            claim,
            PhaseInput {
                index: 2,
                phase: "remote_target",
                evidence_sha256: &evidence,
                target: Some(&target),
                request_sha256: Some(receipt.request_sha256()),
                response_sha256: Some(receipt.response_sha256().ok_or(HostError::Identity)?),
                lookup_status: Some(200),
                lookup_count: Some(1),
                observed_at_unix_ms: timestamp(receipt.observed_at_unix_ms())?,
            },
        )
        .await
    })
    .await
}

pub(super) async fn record_remote_initial_absence(
    scope: &ConnectionJournal,
    claim: &JitConflictRecoveryClaim,
    receipt: &RunnerResponseReceipt,
) -> Result<(), HostError> {
    validate_lookup(receipt, false)?;
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        validate_claim_in_transaction(connection, scope, claim).await?;
        let target = Target {
            id: None,
            name: claim.requested_name().to_owned(),
            scale_set_id: claim.scale_set_id(),
        };
        let evidence = remote_absence_digest(claim, &target, receipt)?;
        insert_phase(
            connection,
            claim,
            PhaseInput {
                index: 2,
                phase: "remote_initial_absent",
                evidence_sha256: &evidence,
                target: Some(&target),
                request_sha256: Some(receipt.request_sha256()),
                response_sha256: Some(receipt.response_sha256().ok_or(HostError::Identity)?),
                lookup_status: Some(200),
                lookup_count: Some(0),
                observed_at_unix_ms: timestamp(receipt.observed_at_unix_ms())?,
            },
        )
        .await
    })
    .await
}

pub(super) async fn record_delete_started(
    scope: &ConnectionJournal,
    claim: &JitConflictRecoveryClaim,
    runner: &RunnerReference,
) -> Result<(), HostError> {
    let identity = claim.identity();
    let target = target_from_runner(runner);
    let runner_id = target.id.ok_or(HostError::Identity)?;
    if runner_id <= 0
        || target.name != identity.requested_name
        || target.scale_set_id != identity.scale_set_id
    {
        return Err(HostError::Ownership);
    }
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        validate_claim_in_transaction(connection, scope, claim).await?;
        if identity::has_delete_started(connection, claim).await? {
            return Err(HostError::Identity);
        }
        let mut rows = connection
            .query(
                "SELECT evidence_sha256 FROM physical_attempt_jit_conflict_recovery_progress WHERE reservation_id=?1 AND claim_generation=?2 AND phase_index=2 AND phase='remote_target' AND runner_id=?3 AND runner_name=?4 AND runner_scale_set_id=?5",
                (claim.reservation_id(), claim.claim_generation(), runner_id, target.name.as_str(), target.scale_set_id),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let target_evidence: String = rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Identity)?.get(0).map_err(|_| HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
            return Err(HostError::Identity);
        }
        let evidence = delete_started_digest_values(
            claim.snapshot_sha256(),
            &target,
            &target_evidence,
        )?;
        insert_phase(
            connection,
            claim,
            PhaseInput {
                index: 3,
                phase: "delete_started",
                evidence_sha256: &evidence,
                target: Some(&target),
                request_sha256: None,
                response_sha256: None,
                lookup_status: None,
                lookup_count: None,
                observed_at_unix_ms: now_millis(connection).await?,
            },
        )
        .await
    })
    .await
}

pub(super) async fn record_remote_absence(
    scope: &ConnectionJournal,
    claim: &JitConflictRecoveryClaim,
    receipt: &RunnerResponseReceipt,
) -> Result<(), HostError> {
    validate_lookup(receipt, false)?;
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        validate_claim_in_transaction(connection, scope, claim).await?;
        let deletion_started = identity::has_delete_started(connection, claim).await?;
        let target = if deletion_started {
            identity::latest_delete_target(connection, claim).await?
        } else {
            let observed = identity::latest_current_observation(connection, claim).await?;
            if observed.id.is_some() {
                return Err(HostError::Identity);
            }
            observed
        };
        let evidence = remote_absence_digest(claim, &target, receipt)?;
        insert_phase(
            connection,
            claim,
            PhaseInput {
                index: 4,
                phase: "remote_absent",
                evidence_sha256: &evidence,
                target: Some(&target),
                request_sha256: Some(receipt.request_sha256()),
                response_sha256: Some(receipt.response_sha256().ok_or(HostError::Identity)?),
                lookup_status: Some(200),
                lookup_count: Some(0),
                observed_at_unix_ms: timestamp(receipt.observed_at_unix_ms())?,
            },
        )
        .await?;
        let changed = connection
            .execute(
                "UPDATE capacity_reservations SET remote_cleanup_proven=1 WHERE id=?1 AND connection_id=?2 AND intent_id=?3 AND state='uncertain' AND EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_claims claim WHERE claim.reservation_id=?1 AND claim.claim_generation=?4 AND claim.lease_until>unixepoch())",
                (claim.reservation_id(), scope.connection_id().as_str(), claim.intent_id(), claim.claim_generation()),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed == 1 { Ok(()) } else { Err(HostError::Identity) }
    })
    .await
}

async fn insert_phase(
    connection: &turso::Connection,
    claim: &JitConflictRecoveryClaim,
    input: PhaseInput<'_>,
) -> Result<(), HostError> {
    let identity = claim.identity();
    let (runner_id, runner_name, runner_scale_set_id) = match input.target {
        Some(target) => (
            target.id,
            Some(target.name.as_str()),
            Some(target.scale_set_id),
        ),
        None => (None, None, None),
    };
    connection
        .execute(
            "INSERT INTO physical_attempt_jit_conflict_recovery_progress (reservation_id,claim_generation,phase_index,phase,evidence_sha256,runner_id,runner_name,runner_scale_set_id,request_sha256,response_sha256,lookup_status,lookup_count,observed_at_unix_ms) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            turso::params![identity.reservation_id, claim.claim_generation(), input.index, input.phase, input.evidence_sha256, runner_id, runner_name, runner_scale_set_id, input.request_sha256, input.response_sha256, input.lookup_status, input.lookup_count, input.observed_at_unix_ms],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}
