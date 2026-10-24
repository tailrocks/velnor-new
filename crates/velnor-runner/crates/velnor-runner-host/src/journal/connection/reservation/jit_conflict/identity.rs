use serde_json::json;
use sha2::{Digest, Sha256};

use crate::error::HostError;

use super::claim::JitConflictRecoveryClaim;

#[derive(Clone)]
pub(in crate::journal) struct Target {
    pub(in crate::journal) id: Option<i64>,
    pub(in crate::journal) name: String,
    pub(in crate::journal) scale_set_id: i64,
}

pub(in crate::journal) struct ConflictIdentity {
    pub(in crate::journal) lineage_id: i64,
    pub(in crate::journal) generation: i64,
    pub(in crate::journal) reservation_id: i64,
    pub(in crate::journal) intent_id: i64,
    pub(in crate::journal) connection_id: String,
    pub(in crate::journal) scale_set_id: i64,
    pub(in crate::journal) witness_request_id: i64,
    pub(in crate::journal) physical_nonce: String,
    pub(in crate::journal) binding_digest: String,
    pub(in crate::journal) source_digest: String,
    pub(in crate::journal) acquire_response_digest: String,
    pub(in crate::journal) authority_generation: i64,
    pub(in crate::journal) authority_digest: String,
    pub(in crate::journal) engine_id: String,
    pub(in crate::journal) docker_root_digest: String,
    pub(in crate::journal) instance_binding_digest: String,
    pub(in crate::journal) requested_name: String,
}

impl ConflictIdentity {
    pub(in crate::journal) fn snapshot_sha256(&self) -> Result<String, serde_json::Error> {
        let value = json!([
            "velnor-jit-conflict-recovery-identity-v1",
            self.lineage_id,
            self.generation,
            self.reservation_id,
            self.intent_id,
            self.connection_id,
            self.scale_set_id,
            self.witness_request_id,
            self.physical_nonce,
            self.binding_digest,
            self.source_digest,
            self.acquire_response_digest,
            self.authority_generation,
            self.authority_digest,
            self.engine_id,
            self.docker_root_digest,
            self.instance_binding_digest,
            self.requested_name,
        ]);
        Ok(hex_digest(&serde_json::to_vec(&value)?))
    }
}

pub(super) fn hex_digest(bytes: &[u8]) -> String {
    crate::digest_hex::lowercase_hex(&Sha256::digest(bytes))
}

pub(in crate::journal) async fn local_evidence(
    connection: &turso::Connection,
    identity: &ConflictIdentity,
) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT cleanup.runner_container_id,cleanup.dind_container_id,cleanup.shared_run_volume,cleanup.runner_home_volume,cleanup.runner_work_volume,cleanup.dind_data_volume,cleanup.metadata_volume,cleanup.instance_binding_digest,cleanup.role_mask,cleanup.containers_absent,cleanup.volumes_absent,cleanup.mounts_detached,cleanup.cgroups_absent,w.operation_id,w.engine_id,w.docker_root_digest,w.runner_container_id,w.dind_container_id,w.instance_binding_digest,w.guest_boot_id,w.binding_sha256 FROM capacity_reservation_provisioning_cleanups cleanup JOIN capacity_reservation_cleanup_observer_witnesses w ON w.reservation_id=cleanup.reservation_id AND w.connection_id=?2 AND w.operation_id=?3 WHERE cleanup.reservation_id=?1",
            (identity.reservation_id, identity.connection_id.as_str(), identity.intent_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Identity)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Identity);
    }
    let runner: String = row.get(0).map_err(|_| HostError::Journal)?;
    let dind: String = row.get(1).map_err(|_| HostError::Journal)?;
    let volumes = [
        Some(row.get::<String>(2).map_err(|_| HostError::Journal)?),
        Some(row.get::<String>(3).map_err(|_| HostError::Journal)?),
        Some(row.get::<String>(4).map_err(|_| HostError::Journal)?),
        Some(row.get::<String>(5).map_err(|_| HostError::Journal)?),
        row.get::<Option<String>>(6)
            .map_err(|_| HostError::Journal)?,
    ];
    let flags = [
        row.get::<i64>(9).map_err(|_| HostError::Journal)?,
        row.get::<i64>(10).map_err(|_| HostError::Journal)?,
        row.get::<i64>(11).map_err(|_| HostError::Journal)?,
        row.get::<i64>(12).map_err(|_| HostError::Journal)?,
    ];
    let operation: i64 = row.get(13).map_err(|_| HostError::Journal)?;
    let engine: String = row.get(14).map_err(|_| HostError::Journal)?;
    let root: String = row.get(15).map_err(|_| HostError::Journal)?;
    let witness_runner: String = row.get(16).map_err(|_| HostError::Journal)?;
    let witness_dind: String = row.get(17).map_err(|_| HostError::Journal)?;
    let binding: String = row.get(18).map_err(|_| HostError::Journal)?;
    let boot_id: String = row.get(19).map_err(|_| HostError::Journal)?;
    let binding_sha256: String = row.get(20).map_err(|_| HostError::Journal)?;
    let cleanup_binding: String = row.get(7).map_err(|_| HostError::Journal)?;
    let role_mask: i64 = row.get(8).map_err(|_| HostError::Journal)?;
    if runner != witness_runner
        || dind != witness_dind
        || runner == dind
        || operation != identity.intent_id
        || engine != identity.engine_id
        || root != identity.docker_root_digest
        || binding != identity.instance_binding_digest
        || cleanup_binding != identity.instance_binding_digest
        || role_mask != 2047
        || flags != [1, 1, 1, 1]
    {
        return Err(HostError::Identity);
    }
    let snapshot_sha256 = identity.snapshot_sha256().map_err(|_| HostError::Journal)?;
    let bytes = serde_json::to_vec(&json!([
        "velnor-jit-conflict-local-cleanup-v1",
        snapshot_sha256,
        runner,
        dind,
        volumes,
        cleanup_binding,
        role_mask,
        operation,
        engine,
        root,
        binding,
        boot_id,
        binding_sha256,
        flags,
    ]))
    .map_err(|_| HostError::Journal)?;
    Ok(hex_digest(&bytes))
}

pub(super) fn validate_lookup(
    receipt: &velnor_runner_github::RunnerResponseReceipt,
    found: bool,
) -> Result<(), HostError> {
    let (count, entries) = if found { (1, 1) } else { (0, 0) };
    if receipt.operation() != velnor_runner_github::RunnerObservationOperation::LookupByName
        || receipt.status() != Some(200)
        || !receipt.status_verified()
        || receipt.count() != Some(count)
        || receipt.value_count() != Some(entries)
        || !valid_sha256(receipt.request_sha256())
        || receipt
            .response_sha256()
            .is_none_or(|value| !valid_sha256(value))
        || receipt.observed_at_unix_ms() == 0
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

pub(super) fn target_from_runner(runner: &velnor_runner_github::RunnerReference) -> Target {
    Target {
        id: Some(runner.id),
        name: runner.name.clone(),
        scale_set_id: runner.runner_scale_set_id,
    }
}

pub(super) fn remote_target_digest(
    claim: &JitConflictRecoveryClaim,
    target: &Target,
    receipt: &velnor_runner_github::RunnerResponseReceipt,
) -> Result<String, HostError> {
    remote_target_digest_values(
        claim.snapshot_sha256(),
        target,
        receipt.request_sha256(),
        receipt.response_sha256().ok_or(HostError::Identity)?,
        timestamp(receipt.observed_at_unix_ms())?,
    )
}

pub(super) fn remote_absence_digest(
    claim: &JitConflictRecoveryClaim,
    target: &Target,
    receipt: &velnor_runner_github::RunnerResponseReceipt,
) -> Result<String, HostError> {
    remote_absence_digest_values(
        claim.snapshot_sha256(),
        target,
        receipt.request_sha256(),
        receipt.response_sha256().ok_or(HostError::Identity)?,
        timestamp(receipt.observed_at_unix_ms())?,
    )
}

pub(in crate::journal) fn remote_target_digest_values(
    snapshot_sha256: &str,
    target: &Target,
    request_sha256: &str,
    response_sha256: &str,
    observed_at_unix_ms: i64,
) -> Result<String, HostError> {
    digest_json(&json!([
        "velnor-jit-conflict-remote-target-v1",
        snapshot_sha256,
        target.id,
        target.name,
        target.scale_set_id,
        request_sha256,
        response_sha256,
        observed_at_unix_ms,
    ]))
}

pub(in crate::journal) fn remote_absence_digest_values(
    snapshot_sha256: &str,
    target: &Target,
    request_sha256: &str,
    response_sha256: &str,
    observed_at_unix_ms: i64,
) -> Result<String, HostError> {
    digest_json(&json!([
        "velnor-jit-conflict-remote-absence-v1",
        snapshot_sha256,
        target.id,
        target.name,
        target.scale_set_id,
        request_sha256,
        response_sha256,
        observed_at_unix_ms,
    ]))
}

pub(in crate::journal) fn delete_started_digest_values(
    snapshot_sha256: &str,
    target: &Target,
    target_digest: &str,
) -> Result<String, HostError> {
    digest_json(&json!([
        "velnor-jit-conflict-delete-started-v1",
        snapshot_sha256,
        target.id,
        target.name,
        target.scale_set_id,
        target_digest,
    ]))
}

fn digest_json(value: &serde_json::Value) -> Result<String, HostError> {
    let bytes = serde_json::to_vec(value).map_err(|_| HostError::Journal)?;
    Ok(hex_digest(&bytes))
}

pub(super) fn timestamp(value: u128) -> Result<i64, HostError> {
    i64::try_from(value).map_err(|_| HostError::Identity)
}

pub(super) async fn now_millis(connection: &turso::Connection) -> Result<i64, HostError> {
    let mut rows = connection
        .query(
            "SELECT CAST((julianday('now')-2440587.5)*86400000 AS INTEGER)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let value: i64 = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() || value <= 0 {
        return Err(HostError::Journal);
    }
    Ok(value)
}

pub(in crate::journal) fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[path = "identity/observations.rs"]
mod observations;
pub(in crate::journal::connection::reservation::jit_conflict) use observations::{
    current_observation, has_delete_started, latest_current_observation, latest_delete_target,
};
