//! Exact schema, guard, revision-trigger, and proof-row validation.

use crate::error::HostError;
use crate::journal::connection::reservation::jit_conflict::identity::{
    ConflictIdentity, Target, delete_started_digest_values, local_evidence,
    remote_absence_digest_values, remote_target_digest_values, valid_sha256,
};

use super::schema_objects;

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    for (kind, name, expected) in schema_objects() {
        validate_object(conn, kind, name, expected).await?;
    }
    validate_revision_triggers(conn).await?;
    validate_rows(conn).await?;
    Ok(())
}

pub(super) async fn validate_persisted_jit_conflict_proofs(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    let mut claims = connection
        .query(
            "SELECT claim.reservation_id,claim.claim_generation,claim.snapshot_sha256,claim.lineage_id,claim.generation,claim.intent_id,claim.connection_id,claim.scale_set_id,claim.witness_request_id,claim.physical_nonce,claim.binding_digest,claim.source_digest,claim.acquire_response_digest,claim.authority_generation,claim.authority_digest,claim.engine_id,claim.docker_root_digest,state.instance_binding_digest,claim.requested_name FROM physical_attempt_jit_conflict_recovery_claims claim LEFT JOIN capacity_reservation_provisioning_state state ON state.reservation_id=claim.reservation_id ORDER BY claim.reservation_id,claim.claim_generation",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = claims.next().await.map_err(|_| HostError::Journal)? {
        let claim = persisted_claim(&row)?;
        let snapshot = claim
            .identity
            .snapshot_sha256()
            .map_err(|_| HostError::Journal)?;
        if !valid_sha256(&claim.stored_snapshot) || claim.stored_snapshot != snapshot {
            return Err(HostError::Journal);
        }
        validate_claim_progress(connection, &claim, &snapshot).await?;
    }
    Ok(())
}

struct PersistedClaim {
    reservation_id: i64,
    claim_generation: i64,
    stored_snapshot: String,
    identity: ConflictIdentity,
}

fn persisted_claim(row: &turso::Row) -> Result<PersistedClaim, HostError> {
    let reservation_id = row.get(0).map_err(|_| HostError::Journal)?;
    Ok(PersistedClaim {
        reservation_id,
        claim_generation: row.get(1).map_err(|_| HostError::Journal)?,
        stored_snapshot: row.get(2).map_err(|_| HostError::Journal)?,
        identity: ConflictIdentity {
            lineage_id: row.get(3).map_err(|_| HostError::Journal)?,
            generation: row.get(4).map_err(|_| HostError::Journal)?,
            reservation_id,
            intent_id: row.get(5).map_err(|_| HostError::Journal)?,
            connection_id: row.get(6).map_err(|_| HostError::Journal)?,
            scale_set_id: row.get(7).map_err(|_| HostError::Journal)?,
            witness_request_id: row.get(8).map_err(|_| HostError::Journal)?,
            physical_nonce: row.get(9).map_err(|_| HostError::Journal)?,
            binding_digest: row.get(10).map_err(|_| HostError::Journal)?,
            source_digest: row.get(11).map_err(|_| HostError::Journal)?,
            acquire_response_digest: row.get(12).map_err(|_| HostError::Journal)?,
            authority_generation: row.get(13).map_err(|_| HostError::Journal)?,
            authority_digest: row.get(14).map_err(|_| HostError::Journal)?,
            engine_id: row.get(15).map_err(|_| HostError::Journal)?,
            docker_root_digest: row.get(16).map_err(|_| HostError::Journal)?,
            instance_binding_digest: row.get(17).map_err(|_| HostError::Journal)?,
            requested_name: row.get(18).map_err(|_| HostError::Journal)?,
        },
    })
}

async fn validate_claim_progress(
    connection: &turso::Connection,
    claim: &PersistedClaim,
    snapshot: &str,
) -> Result<(), HostError> {
    let mut progress = connection
        .query(
            "SELECT phase_index,phase,evidence_sha256,runner_id,runner_name,runner_scale_set_id,request_sha256,response_sha256,lookup_status,lookup_count,observed_at_unix_ms,remove_status FROM physical_attempt_jit_conflict_recovery_progress WHERE reservation_id=?1 AND claim_generation=?2 ORDER BY phase_index",
            (claim.reservation_id, claim.claim_generation),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut target_evidence = None;
    let mut phases = Vec::new();
    while let Some(row) = progress.next().await.map_err(|_| HostError::Journal)? {
        let evidence = progress_evidence_row(&row)?;
        phases.push((evidence.index, evidence.phase.clone()));
        let expected = expected_progress_digest(
            connection,
            &claim.identity,
            snapshot,
            &evidence,
            &mut target_evidence,
        )
        .await?;
        if evidence.evidence != expected {
            return Err(HostError::Journal);
        }
    }
    if !valid_phase_order(&phases) {
        return Err(HostError::Journal);
    }
    validate_observation_history(connection, claim.reservation_id).await
}

struct ProgressEvidenceRow {
    index: i64,
    phase: String,
    evidence: String,
    target_id: Option<i64>,
    target_name: Option<String>,
    target_scale_set_id: Option<i64>,
    request_sha256: Option<String>,
    response_sha256: Option<String>,
    lookup_status: Option<i64>,
    lookup_count: Option<i64>,
    observed_at: Option<i64>,
    remove_status: Option<i64>,
}

fn progress_evidence_row(row: &turso::Row) -> Result<ProgressEvidenceRow, HostError> {
    Ok(ProgressEvidenceRow {
        index: row.get(0).map_err(|_| HostError::Journal)?,
        phase: row.get(1).map_err(|_| HostError::Journal)?,
        evidence: row.get(2).map_err(|_| HostError::Journal)?,
        target_id: row.get(3).map_err(|_| HostError::Journal)?,
        target_name: row.get(4).map_err(|_| HostError::Journal)?,
        target_scale_set_id: row.get(5).map_err(|_| HostError::Journal)?,
        request_sha256: row.get(6).map_err(|_| HostError::Journal)?,
        response_sha256: row.get(7).map_err(|_| HostError::Journal)?,
        lookup_status: row.get(8).map_err(|_| HostError::Journal)?,
        lookup_count: row.get(9).map_err(|_| HostError::Journal)?,
        observed_at: row.get(10).map_err(|_| HostError::Journal)?,
        remove_status: row.get(11).map_err(|_| HostError::Journal)?,
    })
}

async fn expected_progress_digest(
    connection: &turso::Connection,
    identity: &ConflictIdentity,
    snapshot: &str,
    row: &ProgressEvidenceRow,
    target_evidence: &mut Option<String>,
) -> Result<String, HostError> {
    if !valid_sha256(&row.evidence) {
        return Err(HostError::Journal);
    }
    match (row.index, row.phase.as_str()) {
        (1, "local_absent") => local_progress_digest(connection, identity, row).await,
        (2, "remote_target" | "remote_initial_absent") | (4, "remote_absent") => {
            remote_progress_digest(identity, snapshot, row, target_evidence)
        }
        (3, "delete_started") => {
            delete_progress_digest(identity, snapshot, row, target_evidence.as_deref())
        }
        _ => Err(HostError::Journal),
    }
}

async fn local_progress_digest(
    connection: &turso::Connection,
    identity: &ConflictIdentity,
    row: &ProgressEvidenceRow,
) -> Result<String, HostError> {
    if row.target_id.is_some()
        || row.target_name.is_some()
        || row.target_scale_set_id.is_some()
        || row.request_sha256.is_some()
        || row.response_sha256.is_some()
        || row.lookup_status.is_some()
        || row.lookup_count.is_some()
        || row.remove_status.is_some()
    {
        return Err(HostError::Journal);
    }
    local_evidence(connection, identity).await
}

fn remote_progress_digest(
    identity: &ConflictIdentity,
    snapshot: &str,
    row: &ProgressEvidenceRow,
    target_evidence: &mut Option<String>,
) -> Result<String, HostError> {
    let request_sha256 = row.request_sha256.as_deref().ok_or(HostError::Journal)?;
    let response_sha256 = row.response_sha256.as_deref().ok_or(HostError::Journal)?;
    let observed_at = row.observed_at.ok_or(HostError::Journal)?;
    if !valid_sha256(request_sha256)
        || !valid_sha256(response_sha256)
        || row.lookup_status != Some(200)
        || row.lookup_count != Some(i64::from(row.phase == "remote_target"))
        || observed_at <= 0
        || row.target_name.as_deref() != Some(identity.requested_name.as_str())
        || row.target_scale_set_id != Some(identity.scale_set_id)
        || row.remove_status.is_some()
        || (row.phase == "remote_target" && row.target_id.is_none_or(|id| id <= 0))
        || (row.phase == "remote_initial_absent" && row.target_id.is_some())
        || (row.phase == "remote_absent" && row.target_id.is_some_and(|id| id <= 0))
    {
        return Err(HostError::Journal);
    }
    let target = Target {
        id: row.target_id,
        name: row.target_name.clone().ok_or(HostError::Journal)?,
        scale_set_id: row.target_scale_set_id.ok_or(HostError::Journal)?,
    };
    if row.phase == "remote_target" {
        *target_evidence = Some(row.evidence.clone());
        remote_target_digest_values(
            snapshot,
            &target,
            request_sha256,
            response_sha256,
            observed_at,
        )
    } else {
        remote_absence_digest_values(
            snapshot,
            &target,
            request_sha256,
            response_sha256,
            observed_at,
        )
    }
}

fn delete_progress_digest(
    identity: &ConflictIdentity,
    snapshot: &str,
    row: &ProgressEvidenceRow,
    target_evidence: Option<&str>,
) -> Result<String, HostError> {
    if row.target_id.is_none_or(|id| id <= 0)
        || row.target_name.as_deref() != Some(identity.requested_name.as_str())
        || row.target_scale_set_id != Some(identity.scale_set_id)
        || row.request_sha256.is_some()
        || row.response_sha256.is_some()
        || row.lookup_status.is_some()
        || row.lookup_count.is_some()
        || row.remove_status.is_some()
    {
        return Err(HostError::Journal);
    }
    let target_digest = target_evidence.ok_or(HostError::Journal)?;
    let target = Target {
        id: row.target_id,
        name: row.target_name.clone().ok_or(HostError::Journal)?,
        scale_set_id: row.target_scale_set_id.ok_or(HostError::Journal)?,
    };
    delete_started_digest_values(snapshot, &target, target_digest)
}

fn valid_phase_order(phases: &[(i64, String)]) -> bool {
    let indices: Vec<i64> = phases.iter().map(|(index, _)| *index).collect();
    let valid_order = matches!(
        indices.as_slice(),
        [] | [1] | [1, 2 | 4] | [1, 2, 3 | 4] | [1, 2, 3, 4]
    );
    let initial_absence_before_phase4 = indices.as_slice() != [1, 2, 4]
        || phases
            .get(1)
            .is_some_and(|(_, phase)| phase == "remote_initial_absent");
    let target_before_delete = !matches!(indices.as_slice(), [1, 2, 3] | [1, 2, 3, 4])
        || phases
            .get(1)
            .is_some_and(|(_, phase)| phase == "remote_target");
    valid_order && initial_absence_before_phase4 && target_before_delete
}

async fn validate_object(
    conn: &turso::Connection,
    kind: &str,
    name: &str,
    expected: &str,
) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT sql FROM sqlite_schema WHERE type=?1 AND name=?2",
            (kind, name),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let actual: String = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || super::super::sql_normalization::normalize_v40_guard_aliases(&actual)
            != super::super::sql_normalization::normalize_v40_guard_aliases(expected)
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn validate_observation_history(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress initial WHERE initial.reservation_id=?1 AND initial.phase='remote_initial_absent' AND (EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress target WHERE target.reservation_id=initial.reservation_id AND target.phase='remote_target') OR EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress deletion WHERE deletion.reservation_id=initial.reservation_id AND deletion.phase='delete_started'))) OR EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress first WHERE first.reservation_id=?1 AND first.phase_index=3 AND first.phase='delete_started' AND EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress replay WHERE replay.reservation_id=first.reservation_id AND replay.phase_index=3 AND replay.phase='delete_started' AND replay.claim_generation>first.claim_generation)) OR EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress target WHERE target.reservation_id=?1 AND target.phase_index=2 AND target.phase='remote_target' AND EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress deletion WHERE deletion.reservation_id=target.reservation_id AND deletion.phase_index=3 AND deletion.phase='delete_started' AND target.claim_generation>deletion.claim_generation))",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let invalid = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() || invalid != 0 {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn validate_revision_triggers(conn: &turso::Connection) -> Result<(), HostError> {
    for table in super::table_names() {
        for (suffix, action) in [
            ("insert", "AFTER INSERT"),
            ("update", "AFTER UPDATE"),
            ("delete", "AFTER DELETE"),
        ] {
            let name = format!("{table}_revision_{suffix}");
            let expected = super::revision_trigger_sql(table, suffix, action);
            let mut rows = conn
                .query(
                    "SELECT sql FROM sqlite_schema WHERE type='trigger' AND name=?1",
                    (name.as_str(),),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let actual: String = rows
                .next()
                .await
                .map_err(|_| HostError::Journal)?
                .ok_or(HostError::Journal)?
                .get(0)
                .map_err(|_| HostError::Journal)?;
            if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
                || super::super::sql_normalization::normalize(&actual)
                    != super::super::sql_normalization::normalize(&expected)
            {
                return Err(HostError::Journal);
            }
        }
    }
    Ok(())
}

async fn validate_rows(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT (SELECT COUNT(*) FROM physical_attempt_jit_conflict_recovery_claims claim LEFT JOIN physical_attempts attempt ON attempt.lineage_id=claim.lineage_id AND attempt.generation=claim.generation AND attempt.reservation_id=claim.reservation_id AND attempt.intent_id=claim.intent_id LEFT JOIN physical_attempt_jit_conflict_receipts receipt ON receipt.lineage_id=claim.lineage_id AND receipt.generation=claim.generation AND receipt.reservation_id=claim.reservation_id AND receipt.intent_id=claim.intent_id LEFT JOIN connection_request_authorizations auth ON auth.connection_id=claim.connection_id AND auth.scale_set_id=claim.scale_set_id AND auth.request_id=claim.witness_request_id WHERE attempt.lineage_id IS NULL OR receipt.intent_id IS NULL OR attempt.physical_nonce IS NOT claim.physical_nonce OR attempt.connection_id IS NOT claim.connection_id OR attempt.scale_set_id IS NOT claim.scale_set_id OR attempt.witness_request_id IS NOT claim.witness_request_id OR attempt.binding_digest IS NOT claim.binding_digest OR attempt.source_digest IS NOT claim.source_digest OR (attempt.acquire_response_digest IS NOT NULL AND attempt.acquire_response_digest IS NOT claim.acquire_response_digest) OR attempt.engine_id IS NOT claim.engine_id OR attempt.docker_root_digest IS NOT claim.docker_root_digest OR receipt.requested_name IS NOT claim.requested_name OR receipt.response_status IS NOT 409 OR receipt.authority_generation IS NOT claim.authority_generation OR receipt.authority_digest IS NOT claim.authority_digest OR auth.acquire_state IS NOT 'acquired' OR auth.acquire_response_digest IS NOT claim.acquire_response_digest OR claim.requested_name IS NOT ('v'||claim.physical_nonce)) + (SELECT COUNT(*) FROM physical_attempt_jit_conflict_recovery_progress progress LEFT JOIN physical_attempt_jit_conflict_recovery_claims claim ON claim.reservation_id=progress.reservation_id AND claim.claim_generation=progress.claim_generation WHERE claim.reservation_id IS NULL OR (progress.phase_index=2 AND (progress.runner_name IS NOT claim.requested_name OR progress.runner_scale_set_id IS NOT claim.scale_set_id OR (progress.phase='remote_target' AND progress.runner_id IS NULL) OR (progress.phase='remote_initial_absent' AND progress.runner_id IS NOT NULL))) OR (progress.phase_index=3 AND NOT EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress target WHERE target.reservation_id=progress.reservation_id AND target.claim_generation=progress.claim_generation AND target.phase_index=2 AND target.phase='remote_target' AND target.runner_id=progress.runner_id AND target.runner_name=progress.runner_name AND target.runner_scale_set_id=progress.runner_scale_set_id)) OR (progress.phase_index=4 AND NOT ((progress.runner_id IS NULL AND EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress observed WHERE observed.reservation_id=progress.reservation_id AND observed.claim_generation=progress.claim_generation AND observed.phase_index=2 AND observed.phase='remote_initial_absent' AND observed.runner_id IS NULL AND observed.runner_name=progress.runner_name AND observed.runner_scale_set_id=progress.runner_scale_set_id)) OR (progress.runner_id IS NOT NULL AND (EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress deletion WHERE deletion.reservation_id=progress.reservation_id AND deletion.claim_generation=progress.claim_generation AND deletion.phase_index=3 AND deletion.phase='delete_started' AND deletion.runner_id=progress.runner_id AND deletion.runner_name=progress.runner_name AND deletion.runner_scale_set_id=progress.runner_scale_set_id) OR EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress deletion JOIN physical_attempt_jit_conflict_recovery_progress target ON target.reservation_id=deletion.reservation_id AND target.claim_generation=deletion.claim_generation AND target.phase_index=2 WHERE deletion.reservation_id=progress.reservation_id AND deletion.claim_generation<progress.claim_generation AND deletion.phase_index=3 AND deletion.phase='delete_started' AND target.phase='remote_target' AND deletion.runner_id=progress.runner_id AND deletion.runner_name=progress.runner_name AND deletion.runner_scale_set_id=progress.runner_scale_set_id AND target.runner_id=progress.runner_id AND target.runner_name=progress.runner_name AND target.runner_scale_set_id=progress.runner_scale_set_id)))))) + (SELECT COUNT(*) FROM physical_attempt_jit_conflict_settlements settlement LEFT JOIN physical_attempt_jit_conflict_recovery_claims claim ON claim.reservation_id=settlement.reservation_id AND claim.claim_generation=settlement.claim_generation LEFT JOIN physical_attempts attempt ON attempt.lineage_id=settlement.lineage_id AND attempt.generation=settlement.generation AND attempt.reservation_id=settlement.reservation_id LEFT JOIN capacity_reservations reservation ON reservation.id=settlement.reservation_id LEFT JOIN connection_intents intent ON intent.id=attempt.intent_id LEFT JOIN connection_runner_jits jit ON jit.intent_id=attempt.intent_id LEFT JOIN capacity_reservation_metadata_settlements metadata ON metadata.reservation_id=settlement.reservation_id AND metadata.generation=settlement.metadata_generation LEFT JOIN physical_attempt_jit_conflict_recovery_progress local ON local.reservation_id=settlement.reservation_id AND local.claim_generation=settlement.claim_generation AND local.phase_index=1 LEFT JOIN physical_attempt_jit_conflict_recovery_progress remote ON remote.reservation_id=settlement.reservation_id AND remote.claim_generation=settlement.claim_generation AND remote.phase_index=4 WHERE claim.reservation_id IS NULL OR attempt.lineage_id IS NULL OR reservation.state IS NOT 'released' OR reservation.remote_cleanup_proven IS NOT 1 OR reservation.local_cleanup_proven IS NOT 1 OR intent.cleanup_proven IS NOT 1 OR jit.state IS NOT 'conflict_retired' OR jit.runner_id IS NOT NULL OR jit.runner_name IS NOT NULL OR metadata.outcome IS NOT 'settled' OR local.evidence_sha256 IS NULL OR remote.reservation_id IS NULL OR remote.evidence_sha256 IS NULL OR remote.runner_id IS NOT settlement.remote_runner_id OR remote.runner_name IS NOT settlement.remote_runner_name OR remote.runner_scale_set_id IS NOT settlement.remote_scale_set_id OR settlement.proof_digest IS NOT settlement.settlement_digest OR EXISTS(SELECT 1 FROM physical_attempt_current current WHERE current.lineage_id=settlement.lineage_id AND current.generation=settlement.generation AND current.reservation_id=settlement.reservation_id))",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let invalid = row.get::<i64>(0).map_err(|_| HostError::Journal)?;
    if invalid == 0 {
        validate_persisted_jit_conflict_proofs(conn).await?;
        crate::journal::connection::reservation::validate_jit_conflict_settlement_digests(conn)
            .await
    } else {
        Err(HostError::Journal)
    }
}
