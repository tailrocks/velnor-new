use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::{in_transaction, read_transaction};

use super::claim::PostJitRetirementClaim;
use super::identity::{ClaimIdentity, hex_digest};

pub(super) async fn validate_claim(
    scope: &ConnectionJournal,
    claim: &PostJitRetirementClaim,
) -> Result<(), HostError> {
    read_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        validate_in_transaction(connection, scope, claim).await
    })
    .await
}

pub(super) async fn resume_network_absence(
    scope: &ConnectionJournal,
    claim: &PostJitRetirementClaim,
) -> Result<bool, HostError> {
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        validate_in_transaction(connection, scope, claim).await?;
        let identity = claim.identity();
        let mut rows = connection
            .query(
                "SELECT connection_id,operation_id,engine_id,docker_root_digest,create_state,remove_state FROM capacity_reservation_networks WHERE reservation_id=?1",
                [identity.reservation_id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Identity)?;
        let connection_id: String = row.get(0).map_err(|_| HostError::Journal)?;
        let operation_id: i64 = row.get(1).map_err(|_| HostError::Journal)?;
        let engine_id: String = row.get(2).map_err(|_| HostError::Journal)?;
        let root_digest: String = row.get(3).map_err(|_| HostError::Journal)?;
        let create_state: String = row.get(4).map_err(|_| HostError::Journal)?;
        let remove_state: String = row.get(5).map_err(|_| HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
            || connection_id != identity.connection_id
            || operation_id != identity.intent_id
            || engine_id != identity.engine_id
            || root_digest != identity.docker_root_digest
            || create_state != "created"
        {
            return Err(HostError::Identity);
        }
        if remove_state != "removed" {
            return if matches!(remove_state.as_str(), "not_requested" | "requested" | "uncertain") {
                Ok(false)
            } else {
                Err(HostError::Identity)
            };
        }
        let mut phase_rows = connection
            .query(
                "SELECT phase FROM physical_attempt_post_jit_retirement_progress WHERE reservation_id=?1 AND claim_generation=?2 AND phase_index=3",
                (identity.reservation_id, claim.claim_generation()),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let phase_row = phase_rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Identity)?;
        let phase: String = phase_row.get(0).map_err(|_| HostError::Journal)?;
        if phase != "storage_absent"
            || phase_rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        {
            return Err(HostError::Identity);
        }
        record_network_phase_in_transaction(connection, scope, claim).await?;
        Ok(true)
    })
    .await
}

pub(super) async fn validate_in_transaction(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    claim: &PostJitRetirementClaim,
) -> Result<(), HostError> {
    let identity = claim.identity();
    if identity.connection_id != scope.connection_id().as_str()
        || identity.binding_digest != scope.binding_digest()
    {
        return Err(HostError::Identity);
    }
    let mut rows = connection
        .query(
            "SELECT c.snapshot_sha256,c.lease_until FROM physical_attempt_post_jit_retirement_claims c JOIN physical_attempt_current cur ON cur.lineage_id=c.lineage_id AND cur.generation=c.generation AND cur.reservation_id=c.reservation_id AND cur.intent_id=c.intent_id JOIN physical_attempts a ON a.lineage_id=c.lineage_id AND a.generation=c.generation AND a.reservation_id=c.reservation_id AND a.intent_id=c.intent_id JOIN capacity_reservations r ON r.id=a.reservation_id AND r.intent_id=a.intent_id AND r.connection_id=a.connection_id JOIN connection_intents i ON i.id=a.intent_id AND i.connection_id=a.connection_id JOIN capacity_reservation_provisioning_state s ON s.reservation_id=r.id JOIN connection_runner_jits j ON j.intent_id=i.id AND j.connection_id=i.connection_id JOIN connection_request_authorizations auth ON auth.connection_id=a.connection_id AND auth.scale_set_id=a.scale_set_id AND auth.request_id=a.witness_request_id WHERE c.reservation_id=?1 AND c.claim_generation=?2 AND c.lineage_id=?3 AND c.generation=?4 AND c.intent_id=?5 AND c.connection_id=?6 AND c.scale_set_id=?7 AND c.witness_request_id=?8 AND c.binding_digest=?9 AND c.source_digest=?10 AND c.acquire_response_digest=?11 AND c.engine_id=?12 AND c.docker_root_digest=?13 AND c.instance_binding_digest=?14 AND c.runner_id=?15 AND c.runner_name=?16 AND c.snapshot_sha256=?17 AND c.lease_until=?18 AND c.claim_generation=(SELECT MAX(latest.claim_generation) FROM physical_attempt_post_jit_retirement_claims latest WHERE latest.reservation_id=c.reservation_id) AND c.lease_until>unixepoch() AND cur.reservation_id=?1 AND a.connection_id=?6 AND a.scale_set_id=?7 AND a.witness_request_id=?8 AND a.binding_digest=?9 AND a.source_digest=?10 AND a.engine_id=?12 AND a.docker_root_digest=?13 AND (a.acquire_response_digest IS NULL OR a.acquire_response_digest=?11) AND r.state IN ('reserved','uncertain','recovering') AND r.binding_digest=?9 AND r.engine_id=?12 AND r.docker_root_digest=?13 AND i.kind='launch' AND i.scale_set_id=?7 AND s.instance_binding_digest=?14 AND s.runner_id=?15 AND s.runner_name=?16 AND j.runner_id=?15 AND j.runner_name=?16 AND j.scale_set_id=?7 AND j.witness_request_id=?8 AND auth.binding_digest=?9 AND auth.source_digest=?10 AND auth.acquire_response_digest=?11 AND auth.acquire_state='acquired' AND NOT EXISTS(SELECT 1 FROM physical_attempt_terminal_proofs terminal WHERE terminal.lineage_id=a.lineage_id AND terminal.generation=a.generation AND terminal.reservation_id=a.reservation_id)",
            turso::params![
                identity.reservation_id,
                claim.claim_generation(),
                identity.lineage_id,
                identity.generation,
                identity.intent_id,
                identity.connection_id.as_str(),
                identity.scale_set_id,
                identity.witness_request_id,
                identity.binding_digest.as_str(),
                identity.source_digest.as_str(),
                identity.acquire_response_digest.as_str(),
                identity.engine_id.as_str(),
                identity.docker_root_digest.as_str(),
                identity.instance_binding_digest.as_str(),
                identity.runner_id,
                identity.runner_name.as_str(),
                claim.snapshot_sha256(),
                claim.lease_until(),
            ],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Err(HostError::Identity);
    };
    let snapshot: String = row.get(0).map_err(|_| HostError::Journal)?;
    let lease_until: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || snapshot != claim.snapshot_sha256()
        || lease_until != claim.lease_until()
        || identity.snapshot_digest().map_err(|_| HostError::Journal)? != snapshot
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

pub(super) async fn append_phase(
    connection: &turso::Connection,
    claim: &PostJitRetirementClaim,
    phase_index: i64,
    phase: &str,
    evidence_sha256: &str,
) -> Result<(), HostError> {
    if evidence_sha256.len() != 64
        || !evidence_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(HostError::Identity);
    }
    let identity = claim.identity();
    let mut rows = connection
        .query(
            "SELECT phase,evidence_sha256 FROM physical_attempt_post_jit_retirement_progress WHERE reservation_id=?1 AND claim_generation=?2 AND phase_index=?3",
            (identity.reservation_id, claim.claim_generation(), phase_index),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let stored_phase: String = row.get(0).map_err(|_| HostError::Journal)?;
        let stored_evidence: String = row.get(1).map_err(|_| HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
            || stored_phase != phase
            || stored_evidence != evidence_sha256
        {
            return Err(HostError::Identity);
        }
        return Ok(());
    }
    connection
        .execute(
            "INSERT INTO physical_attempt_post_jit_retirement_progress (reservation_id,claim_generation,phase_index,phase,evidence_sha256,created_at) VALUES (?1,?2,?3,?4,?5,unixepoch())",
            (
                identity.reservation_id,
                claim.claim_generation(),
                phase_index,
                phase,
                evidence_sha256,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(in crate::journal) async fn record_network_phase_in_transaction(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    claim: &PostJitRetirementClaim,
) -> Result<(), HostError> {
    validate_in_transaction(connection, scope, claim).await?;
    let identity = claim.identity();
    let mut rows = connection
        .query(
            "SELECT connection_id,operation_id,engine_id,docker_root_digest,network_name,network_id,labels_json,create_state,remove_state FROM capacity_reservation_networks WHERE reservation_id=?1",
            [identity.reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let connection_id: String = row.get(0).map_err(|_| HostError::Journal)?;
    let operation_id: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    let engine_id: String = row.get(2).map_err(|_| HostError::Journal)?;
    let root_digest: String = row.get(3).map_err(|_| HostError::Journal)?;
    let network_name: String = row.get(4).map_err(|_| HostError::Journal)?;
    let network_id: Option<String> = row.get(5).map_err(|_| HostError::Journal)?;
    let labels_json: String = row.get(6).map_err(|_| HostError::Journal)?;
    let create_state: String = row.get(7).map_err(|_| HostError::Journal)?;
    let remove_state: String = row.get(8).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || connection_id != identity.connection_id
        || operation_id != identity.intent_id
        || engine_id != identity.engine_id
        || root_digest != identity.docker_root_digest
        || create_state != "created"
        || remove_state != "removed"
        || network_id.is_none()
    {
        return Err(HostError::Identity);
    }
    let value = serde_json::json!([
        "velnor-post-jit-retirement-network-absence-v1",
        identity.reservation_id,
        identity.generation,
        identity.connection_id,
        identity.intent_id,
        claim.claim_generation(),
        engine_id,
        root_digest,
        network_name,
        network_id,
        labels_json,
        create_state,
        remove_state,
    ]);
    let bytes = serde_json::to_vec(&value).map_err(|_| HostError::Journal)?;
    append_phase(connection, claim, 4, "network_absent", &hex_digest(&bytes)).await?;
    crate::journal::request_lifecycle::physical::mark_runner_cleaned_after_cleanup(
        connection,
        &identity.connection_id,
        identity.intent_id,
    )
    .await?;
    validate_cleaned_jit_identity(connection, identity).await
}

async fn validate_cleaned_jit_identity(
    connection: &turso::Connection,
    identity: &ClaimIdentity,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT state.jit_state,state.runner_id,state.runner_name,state.runner_scale_set_id,jit.state,jit.runner_id,jit.runner_name,jit.scale_set_id,jit.witness_request_id FROM capacity_reservation_provisioning_state state JOIN capacity_reservations reservation ON reservation.id=state.reservation_id JOIN connection_runner_jits jit ON jit.intent_id=reservation.intent_id AND jit.connection_id=reservation.connection_id WHERE state.reservation_id=?1 AND reservation.connection_id=?2 AND reservation.intent_id=?3",
            (identity.reservation_id, identity.connection_id.as_str(), identity.intent_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Identity)?;
    let provisioning_state: String = row.get(0).map_err(|_| HostError::Journal)?;
    let provisioning_runner_id: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    let provisioning_runner_name: String = row.get(2).map_err(|_| HostError::Journal)?;
    let provisioning_scale_set_id: i64 = row.get(3).map_err(|_| HostError::Journal)?;
    let jit_state: String = row.get(4).map_err(|_| HostError::Journal)?;
    let jit_runner_id: i64 = row.get(5).map_err(|_| HostError::Journal)?;
    let jit_runner_name: String = row.get(6).map_err(|_| HostError::Journal)?;
    let jit_scale_set_id: i64 = row.get(7).map_err(|_| HostError::Journal)?;
    let witness_request_id: i64 = row.get(8).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || provisioning_state != "cleaned"
        || provisioning_runner_id != identity.runner_id
        || provisioning_runner_name != identity.runner_name
        || provisioning_scale_set_id != identity.scale_set_id
        || jit_state != "cleaned"
        || jit_runner_id != identity.runner_id
        || jit_runner_name != identity.runner_name
        || jit_scale_set_id != identity.scale_set_id
        || witness_request_id != identity.witness_request_id
    {
        return Err(HostError::Identity);
    }
    Ok(())
}
