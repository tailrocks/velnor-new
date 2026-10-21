use serde_json::json;

use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;
use crate::launch::bootstrap::VerifiedStorageCleanup;

use super::claim::PostJitRetirementClaim;
use super::identity::hex_digest;
use super::progress::{append_phase, validate_in_transaction};

pub(super) async fn record_local_absence(
    scope: &ConnectionJournal,
    claim: &PostJitRetirementClaim,
    proof: VerifiedStorageCleanup,
) -> Result<(), HostError> {
    let identity = claim.identity();
    if proof.reservation_id() != identity.reservation_id
        || proof.operation_id() != identity.intent_id
        || proof.engine_id() != identity.engine_id
        || proof.docker_root_digest() != identity.docker_root_digest
        || proof.instance_binding_digest() != identity.instance_binding_digest
    {
        return Err(HostError::Identity);
    }
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        validate_in_transaction(connection, scope, claim).await?;
        crate::journal::connection::reservation::provisioning::record_retirement_cleanup_evidence(
            connection, scope, &proof,
        )
        .await?;
        record_local_cleanup_flag(connection, claim).await?;
        let evidence = local_evidence(connection, claim).await?;
        append_phase(connection, claim, 1, "runner_stopped", &evidence).await
    })
    .await
}

pub(super) async fn resume_local_absence(
    scope: &ConnectionJournal,
    claim: &PostJitRetirementClaim,
) -> Result<bool, HostError> {
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        validate_in_transaction(connection, scope, claim).await?;
        let identity = claim.identity();
        let mut rows = connection
            .query(
                "SELECT r.local_cleanup_proven,EXISTS(SELECT 1 FROM capacity_reservation_provisioning_cleanups cleanup WHERE cleanup.reservation_id=r.id),EXISTS(SELECT 1 FROM capacity_reservation_cleanup_observer_witnesses witness WHERE witness.reservation_id=r.id AND witness.connection_id=?2 AND witness.operation_id=?3),EXISTS(SELECT 1 FROM physical_attempt_post_jit_retirement_progress phase WHERE phase.reservation_id=r.id AND phase.claim_generation=?4 AND phase.phase_index=1) FROM capacity_reservations r WHERE r.id=?1 AND r.connection_id=?2 AND r.intent_id=?3 AND r.state IN ('reserved','uncertain','recovering')",
                (
                    identity.reservation_id,
                    identity.connection_id.as_str(),
                    identity.intent_id,
                    claim.claim_generation(),
                ),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Identity)?;
        let local_flag: i64 = row.get(0).map_err(|_| HostError::Journal)?;
        let cleanup_exists: i64 = row.get(1).map_err(|_| HostError::Journal)?;
        let witness_exists: i64 = row.get(2).map_err(|_| HostError::Journal)?;
        let phase_exists: i64 = row.get(3).map_err(|_| HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
            return Err(HostError::Identity);
        }
        if local_flag == 0 && phase_exists == 0 {
            return Ok(false);
        }
        if local_flag != 1 || cleanup_exists != 1 || witness_exists != 1 {
            return Err(HostError::Identity);
        }
        let evidence = local_evidence(connection, claim).await?;
        append_phase(connection, claim, 1, "runner_stopped", &evidence).await?;
        Ok(true)
    })
    .await
}

async fn record_local_cleanup_flag(
    connection: &turso::Connection,
    claim: &PostJitRetirementClaim,
) -> Result<(), HostError> {
    let identity = claim.identity();
    let changed = connection
        .execute(
            "UPDATE capacity_reservations SET local_cleanup_proven=1 WHERE id=?1 AND connection_id=?2 AND intent_id=?3 AND state IN ('reserved','uncertain','recovering') AND EXISTS(SELECT 1 FROM physical_attempt_current cur JOIN physical_attempts a ON a.lineage_id=cur.lineage_id AND a.generation=cur.generation AND a.reservation_id=cur.reservation_id AND a.intent_id=cur.intent_id JOIN physical_attempt_post_jit_retirement_claims c ON c.lineage_id=a.lineage_id AND c.generation=a.generation AND c.reservation_id=a.reservation_id AND c.intent_id=a.intent_id WHERE cur.reservation_id=?1 AND c.claim_generation=?4 AND c.lease_until=?5 AND c.lease_until>unixepoch() AND c.connection_id=?2 AND c.binding_digest=?6 AND c.snapshot_sha256=?7)",
            (
                identity.reservation_id,
                identity.connection_id.as_str(),
                identity.intent_id,
                claim.claim_generation(),
                claim.lease_until(),
                identity.binding_digest.as_str(),
                claim.snapshot_sha256(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        return Ok(());
    }
    let mut rows = connection
        .query(
            "SELECT local_cleanup_proven FROM capacity_reservations WHERE id=?1 AND connection_id=?2 AND intent_id=?3 AND state IN ('reserved','uncertain','recovering')",
            (
                identity.reservation_id,
                identity.connection_id.as_str(),
                identity.intent_id,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let already_proven: i64 = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() || already_proven != 1 {
        return Err(HostError::Journal);
    }
    Ok(())
}

pub(super) async fn record_storage_phase(
    scope: &ConnectionJournal,
    claim: &PostJitRetirementClaim,
) -> Result<(), HostError> {
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        validate_in_transaction(connection, scope, claim).await?;
        let local = local_evidence(connection, claim).await?;
        append_phase(connection, claim, 1, "runner_stopped", &local).await?;
        let value = json!([
            "velnor-post-jit-retirement-storage-absence-v1",
            claim.reservation_id(),
            claim.generation(),
            claim.claim_generation(),
            local,
        ]);
        let bytes = serde_json::to_vec(&value).map_err(|_| HostError::Journal)?;
        append_phase(connection, claim, 3, "storage_absent", &hex_digest(&bytes)).await
    })
    .await
}

pub(super) async fn local_evidence(
    connection: &turso::Connection,
    claim: &PostJitRetirementClaim,
) -> Result<String, HostError> {
    let identity = claim.identity();
    let mut rows = connection
        .query(
            "SELECT cleanup.runner_container_id,cleanup.dind_container_id,cleanup.shared_run_volume,cleanup.runner_home_volume,cleanup.runner_work_volume,cleanup.dind_data_volume,cleanup.metadata_volume,cleanup.instance_binding_digest,cleanup.role_mask,cleanup.containers_absent,cleanup.volumes_absent,cleanup.mounts_detached,cleanup.cgroups_absent,w.operation_id,w.engine_id,w.docker_root_digest,w.runner_container_id,w.dind_container_id,w.instance_binding_digest,w.guest_boot_id,w.binding_sha256 FROM capacity_reservation_provisioning_cleanups cleanup JOIN capacity_reservation_cleanup_observer_witnesses w ON w.reservation_id=cleanup.reservation_id AND w.connection_id=?2 AND w.operation_id=?3 WHERE cleanup.reservation_id=?1",
            (
                identity.reservation_id,
                identity.connection_id.as_str(),
                identity.intent_id,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Identity);
    }
    local_evidence_digest(&row, claim)
}

fn local_evidence_digest(
    row: &turso::Row,
    claim: &PostJitRetirementClaim,
) -> Result<String, HostError> {
    let identity = claim.identity();
    let cleanup_runner: String = row.get(0).map_err(|_| HostError::Journal)?;
    let cleanup_dind: String = row.get(1).map_err(|_| HostError::Journal)?;
    let volumes = [
        Some(row.get::<String>(2).map_err(|_| HostError::Journal)?),
        Some(row.get::<String>(3).map_err(|_| HostError::Journal)?),
        Some(row.get::<String>(4).map_err(|_| HostError::Journal)?),
        Some(row.get::<String>(5).map_err(|_| HostError::Journal)?),
        row.get::<Option<String>>(6)
            .map_err(|_| HostError::Journal)?,
    ];
    let instance_binding: String = row.get(7).map_err(|_| HostError::Journal)?;
    let role_mask: i64 = row.get(8).map_err(|_| HostError::Journal)?;
    let flags = [
        row.get::<i64>(9).map_err(|_| HostError::Journal)?,
        row.get::<i64>(10).map_err(|_| HostError::Journal)?,
        row.get::<i64>(11).map_err(|_| HostError::Journal)?,
        row.get::<i64>(12).map_err(|_| HostError::Journal)?,
    ];
    let witness_operation: i64 = row.get(13).map_err(|_| HostError::Journal)?;
    let witness_engine: String = row.get(14).map_err(|_| HostError::Journal)?;
    let witness_root: String = row.get(15).map_err(|_| HostError::Journal)?;
    let witness_runner: String = row.get(16).map_err(|_| HostError::Journal)?;
    let witness_dind: String = row.get(17).map_err(|_| HostError::Journal)?;
    let witness_binding: String = row.get(18).map_err(|_| HostError::Journal)?;
    let guest_boot_id: String = row.get(19).map_err(|_| HostError::Journal)?;
    let witness_digest: String = row.get(20).map_err(|_| HostError::Journal)?;
    if cleanup_runner != witness_runner
        || cleanup_dind != witness_dind
        || cleanup_runner == cleanup_dind
        || instance_binding != identity.instance_binding_digest
        || witness_binding != identity.instance_binding_digest
        || flags != [1, 1, 1, 1]
        || witness_operation != identity.intent_id
        || witness_engine != identity.engine_id
        || witness_root != identity.docker_root_digest
        || role_mask != 2047
    {
        return Err(HostError::Identity);
    }
    let value = json!([
        "velnor-post-jit-retirement-local-absence-v1",
        identity.reservation_id,
        identity.generation,
        identity.intent_id,
        claim.claim_generation(),
        identity.connection_id,
        identity.scale_set_id,
        identity.binding_digest,
        identity.engine_id,
        identity.docker_root_digest,
        identity.instance_binding_digest,
        cleanup_runner,
        cleanup_dind,
        volumes,
        role_mask,
        flags,
        guest_boot_id,
        witness_digest,
    ]);
    let bytes = serde_json::to_vec(&value).map_err(|_| HostError::Journal)?;
    Ok(hex_digest(&bytes))
}
