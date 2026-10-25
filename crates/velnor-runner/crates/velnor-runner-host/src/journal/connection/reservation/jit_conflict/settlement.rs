//! Atomic terminal accounting for a fully proved JIT-name conflict recovery.

use serde_json::json;

use crate::error::HostError;

use super::identity::hex_digest;

pub(super) async fn has_claim(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<bool, HostError> {
    if reservation_id <= 0 {
        return Err(HostError::Identity);
    }
    let mut rows = connection
        .query(
            "SELECT EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_claims WHERE reservation_id=?1)",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let exists = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    match exists {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(HostError::Journal),
    }
}

pub(super) async fn settle_after_metadata(
    connection: &turso::Connection,
    reservation_id: i64,
    metadata_generation: i64,
) -> Result<bool, HostError> {
    if reservation_id <= 0 || metadata_generation <= 0 {
        return Err(HostError::Identity);
    }
    crate::journal::schema::validate_jit_conflict_progress_proofs(connection).await?;
    let row = read_settlement_input(connection, reservation_id, metadata_generation).await?;
    let Some(input) = row else {
        return Ok(false);
    };
    let digest = settlement_digest(&input)?;
    let changed = connection
        .execute(
            "INSERT INTO physical_attempt_jit_conflict_settlements (lineage_id,generation,reservation_id,claim_generation,metadata_generation,remote_runner_id,remote_runner_name,remote_scale_set_id,proof_digest,settlement_digest,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?9,unixepoch())",
            (input.lineage_id, input.generation, input.reservation_id, input.claim_generation, input.metadata_generation, input.runner_id, input.runner_name.as_str(), input.scale_set_id, digest.as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed != 1 {
        return Err(HostError::Journal);
    }
    if !crate::journal::connection::reservation::provisioning::release_ready(
        connection,
        reservation_id,
    )
    .await?
    {
        return Err(HostError::Identity);
    }
    transition_jit(connection, &input).await?;
    transition_intent(connection, &input).await?;
    release_reservation(connection, &input).await?;
    remove_current(connection, &input).await?;
    Ok(true)
}

struct SettlementInput {
    lineage_id: i64,
    generation: i64,
    reservation_id: i64,
    intent_id: i64,
    claim_generation: i64,
    metadata_generation: i64,
    connection_id: String,
    scale_set_id: i64,
    witness_request_id: i64,
    physical_nonce: String,
    binding_digest: String,
    source_digest: String,
    acquire_response_digest: String,
    authority_generation: i64,
    authority_digest: String,
    engine_id: String,
    docker_root_digest: String,
    requested_name: String,
    snapshot_sha256: String,
    local_evidence_sha256: String,
    remote_evidence_sha256: String,
    runner_id: Option<i64>,
    runner_name: String,
    remote_scale_set_id: i64,
    probe_operation_id: String,
    sample_generation_id: String,
    profile_generation_id: String,
    profile_digest: String,
    observed_at_unix_ms: i64,
}

async fn read_settlement_input(
    connection: &turso::Connection,
    reservation_id: i64,
    metadata_generation: i64,
) -> Result<Option<SettlementInput>, HostError> {
    let mut rows = connection
        .query(
            "SELECT claim.lineage_id,claim.generation,claim.reservation_id,claim.intent_id,claim.claim_generation,metadata.generation,claim.connection_id,claim.scale_set_id,claim.witness_request_id,claim.physical_nonce,claim.binding_digest,claim.source_digest,claim.acquire_response_digest,claim.authority_generation,claim.authority_digest,claim.engine_id,claim.docker_root_digest,claim.requested_name,claim.snapshot_sha256,local.evidence_sha256,remote.evidence_sha256,remote.runner_id,remote.runner_name,remote.runner_scale_set_id,metadata_claim.probe_operation_id,metadata.sample_generation_id,metadata.profile_generation_id,metadata.profile_digest,metadata.observed_at_unix_ms,metadata.outcome,metadata_claim.operation_id,metadata_claim.scale_set_id,metadata_claim.engine_id,metadata_claim.docker_root_digest FROM physical_attempt_jit_conflict_recovery_claims claim JOIN physical_attempt_current current ON current.lineage_id=claim.lineage_id AND current.generation=claim.generation AND current.reservation_id=claim.reservation_id AND current.intent_id=claim.intent_id JOIN physical_attempt_jit_conflict_recovery_progress local ON local.reservation_id=claim.reservation_id AND local.claim_generation=claim.claim_generation AND local.phase_index=1 AND local.phase='local_absent' JOIN physical_attempt_jit_conflict_recovery_progress remote ON remote.reservation_id=claim.reservation_id AND remote.claim_generation=claim.claim_generation AND remote.phase_index=4 AND remote.phase='remote_absent' JOIN capacity_reservation_metadata_settlement_claims metadata_claim ON metadata_claim.reservation_id=claim.reservation_id AND metadata_claim.generation=?2 JOIN capacity_reservation_metadata_settlements metadata ON metadata.reservation_id=metadata_claim.reservation_id AND metadata.generation=metadata_claim.generation WHERE claim.reservation_id=?1 AND claim.claim_generation=(SELECT MAX(latest.claim_generation) FROM physical_attempt_jit_conflict_recovery_claims latest WHERE latest.reservation_id=claim.reservation_id) AND claim.lease_until>unixepoch()",
            (reservation_id, metadata_generation),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    let input = settlement_input_from_row(&row)?;
    validate_settlement_input(&row, &input)?;
    Ok(Some(input))
}

fn settlement_input_from_row(row: &turso::Row) -> Result<SettlementInput, HostError> {
    Ok(SettlementInput {
        lineage_id: row.get(0).map_err(|_| HostError::Journal)?,
        generation: row.get(1).map_err(|_| HostError::Journal)?,
        reservation_id: row.get(2).map_err(|_| HostError::Journal)?,
        intent_id: row.get(3).map_err(|_| HostError::Journal)?,
        claim_generation: row.get(4).map_err(|_| HostError::Journal)?,
        metadata_generation: row.get(5).map_err(|_| HostError::Journal)?,
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
        requested_name: row.get(17).map_err(|_| HostError::Journal)?,
        snapshot_sha256: row.get(18).map_err(|_| HostError::Journal)?,
        local_evidence_sha256: row.get(19).map_err(|_| HostError::Journal)?,
        remote_evidence_sha256: row.get(20).map_err(|_| HostError::Journal)?,
        runner_id: row.get(21).map_err(|_| HostError::Journal)?,
        runner_name: row.get(22).map_err(|_| HostError::Journal)?,
        remote_scale_set_id: row.get(23).map_err(|_| HostError::Journal)?,
        probe_operation_id: row.get(24).map_err(|_| HostError::Journal)?,
        sample_generation_id: row.get(25).map_err(|_| HostError::Journal)?,
        profile_generation_id: row.get(26).map_err(|_| HostError::Journal)?,
        profile_digest: row.get(27).map_err(|_| HostError::Journal)?,
        observed_at_unix_ms: row.get(28).map_err(|_| HostError::Journal)?,
    })
}

fn validate_settlement_input(row: &turso::Row, input: &SettlementInput) -> Result<(), HostError> {
    let outcome: String = row.get(29).map_err(|_| HostError::Journal)?;
    let metadata_intent: i64 = row.get(30).map_err(|_| HostError::Journal)?;
    let metadata_scale_set: i64 = row.get(31).map_err(|_| HostError::Journal)?;
    let metadata_engine: String = row.get(32).map_err(|_| HostError::Journal)?;
    let metadata_root: String = row.get(33).map_err(|_| HostError::Journal)?;
    if outcome != "settled"
        || metadata_intent != input.intent_id
        || metadata_scale_set != input.scale_set_id
        || metadata_engine != input.engine_id
        || metadata_root != input.docker_root_digest
        || input.sample_generation_id != input.probe_operation_id
        || input.requested_name != input.runner_name
        || input.scale_set_id != input.remote_scale_set_id
        || input.runner_id.is_some_and(|runner_id| runner_id <= 0)
        || input.lineage_id <= 0
        || input.generation <= 0
        || input.intent_id <= 0
        || input.claim_generation <= 0
        || input.witness_request_id <= 0
        || input.metadata_generation <= 0
        || input.observed_at_unix_ms <= 0
        || [
            input.binding_digest.as_str(),
            input.source_digest.as_str(),
            input.acquire_response_digest.as_str(),
            input.authority_digest.as_str(),
            input.docker_root_digest.as_str(),
            input.snapshot_sha256.as_str(),
            input.local_evidence_sha256.as_str(),
            input.remote_evidence_sha256.as_str(),
            input.profile_digest.as_str(),
        ]
        .into_iter()
        .any(|value| !valid_sha256(value))
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn settlement_digest(input: &SettlementInput) -> Result<String, HostError> {
    let value = json!([
        "velnor-jit-conflict-settlement-v1",
        input.lineage_id,
        input.generation,
        input.reservation_id,
        input.intent_id,
        input.claim_generation,
        input.metadata_generation,
        input.connection_id,
        input.scale_set_id,
        input.witness_request_id,
        input.physical_nonce,
        input.binding_digest,
        input.source_digest,
        input.acquire_response_digest,
        input.authority_generation,
        input.authority_digest,
        input.engine_id,
        input.docker_root_digest,
        input.requested_name,
        input.snapshot_sha256,
        input.local_evidence_sha256,
        input.remote_evidence_sha256,
        input.runner_id,
        input.runner_name,
        input.remote_scale_set_id,
        input.probe_operation_id,
        input.sample_generation_id,
        input.profile_generation_id,
        input.profile_digest,
        input.observed_at_unix_ms,
    ]);
    let bytes = serde_json::to_vec(&value).map_err(|_| HostError::Journal)?;
    Ok(hex_digest(&bytes))
}

async fn transition_jit(
    connection: &turso::Connection,
    input: &SettlementInput,
) -> Result<(), HostError> {
    let changed = connection
        .execute(
            "UPDATE connection_runner_jits SET state='conflict_retired' WHERE intent_id=?1 AND connection_id=?2 AND scale_set_id=?3 AND witness_request_id=?4 AND requested_name=?5 AND state='uncertain' AND runner_id IS NULL AND runner_name IS NULL AND started_request_id IS NULL AND completed_request_id IS NULL",
            (input.intent_id, input.connection_id.as_str(), input.scale_set_id, input.witness_request_id, input.requested_name.as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

async fn transition_intent(
    connection: &turso::Connection,
    input: &SettlementInput,
) -> Result<(), HostError> {
    let changed = connection
        .execute(
            "UPDATE connection_intents SET cleanup_proven=1 WHERE id=?1 AND connection_id=?2 AND scale_set_id=?3 AND kind='launch' AND state='uncertain' AND launch_phase='jit_uncertain' AND jit_requested=1 AND runner_name=?4 AND github_runner_id IS NULL AND cleanup_proven=0",
            (input.intent_id, input.connection_id.as_str(), input.scale_set_id, input.requested_name.as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

async fn release_reservation(
    connection: &turso::Connection,
    input: &SettlementInput,
) -> Result<(), HostError> {
    let changed = connection
        .execute(
            "UPDATE capacity_reservations SET state='released' WHERE id=?1 AND connection_id=?2 AND intent_id=?3 AND state='uncertain' AND remote_cleanup_proven=1 AND local_cleanup_proven=1",
            (input.reservation_id, input.connection_id.as_str(), input.intent_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

async fn remove_current(
    connection: &turso::Connection,
    input: &SettlementInput,
) -> Result<(), HostError> {
    let changed = connection
        .execute(
            "DELETE FROM physical_attempt_current WHERE lineage_id=?1 AND generation=?2 AND reservation_id=?3 AND intent_id=?4",
            (input.lineage_id, input.generation, input.reservation_id, input.intent_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[path = "settlement/proofs.rs"]
mod proofs;
pub(super) use proofs::{metadata_claim_ready, validate_persisted_settlements};
