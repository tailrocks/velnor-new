//! This module removes owned resources after an exact pre-JIT create failure.

use sha2::{Digest, Sha256};

use crate::{error::HostError, journal::PreJitCleanupClaim};

#[path = "pre_jit_recovery/bootstrap.rs"]
mod bootstrap;
#[path = "pre_jit_recovery/network.rs"]
mod network;
#[path = "pre_jit_recovery/storage.rs"]
mod storage;

pub(crate) async fn remove_bootstrap_after_terminal_failure<
    P: crate::launch::resource_probe::ImageProvider,
>(
    docker: &bollard::Docker,
    journal: &crate::journal::ConnectionJournal,
    host: &crate::journal::Journal,
    engine: &crate::launch::SelectedEngineContext,
    images: &crate::launch::VerifiedWorkerImages,
    reservation_id: i64,
    provider: &P,
) -> Result<bool, HostError> {
    let Some(claim) = journal.claim_pre_jit_cleanup(reservation_id).await? else {
        return Ok(false);
    };
    Box::pin(remove_bootstrap_with_claim(
        docker, journal, host, engine, images, &claim, provider,
    ))
    .await
}

pub(crate) async fn remove_bootstrap_with_claim<P: crate::launch::resource_probe::ImageProvider>(
    docker: &bollard::Docker,
    journal: &crate::journal::ConnectionJournal,
    host: &crate::journal::Journal,
    engine: &crate::launch::SelectedEngineContext,
    images: &crate::launch::VerifiedWorkerImages,
    claim: &PreJitCleanupClaim,
    provider: &P,
) -> Result<bool, HostError> {
    if !journal.validate_pre_jit_cleanup_claim(claim).await? {
        return Ok(false);
    }
    let Some(inventory) = journal.pre_jit_cleanup_inventory(claim).await? else {
        return Ok(false);
    };
    if claim.engine_id() != engine.engine_id()
        || claim.docker_root_digest() != engine.docker_root_digest()
        || !engine.matches_images(images)
    {
        return Err(HostError::Ownership);
    }
    Box::pin(bootstrap::remove_pair_if_claimed(
        docker, journal, engine, images, claim, &inventory,
    ))
    .await?;
    if journal.pre_jit_cleanup_phase(claim).await? != Some(PreJitCleanupPhase::NetworkAbsent) {
        return Err(HostError::Journal);
    }
    let observation =
        crate::launch::measure_pre_jit_capacity(docker, host, journal, claim, engine, provider)
            .await?;
    let proof = PreJitCleanupSettlementProof::after_retired_capacity_probe(observation)?;
    let outcome = journal
        .record_pre_jit_cleanup_settlement(claim, &proof)
        .await?;
    if outcome == crate::journal::PreJitCleanupSettlementResult::CapacityAvailable
        && !journal.release_pre_jit_attempt(claim).await?
    {
        return Err(HostError::Journal);
    }
    Ok(true)
}

pub(crate) async fn recover_pre_jit_reservation(
    docker: &bollard::Docker,
    journal: &crate::journal::Journal,
    scope: &crate::journal::ConnectionJournal,
    engine: &crate::launch::SelectedEngineContext,
    images: &crate::launch::VerifiedWorkerImages,
    reservation_id: i64,
    release_secret: &crate::launch::ReleaseReadSecret,
) -> Result<bool, HostError> {
    let provider = crate::launch::resource_probe::release_image_provider(release_secret, journal)?;
    Box::pin(remove_bootstrap_after_terminal_failure(
        docker,
        scope,
        journal,
        engine,
        images,
        reservation_id,
        &provider,
    ))
    .await
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PreJitCleanupPhase {
    Claimed,
    BootstrapAbsent,
    InitializerAbsent,
    StorageAbsent,
    NetworkAbsent,
    Settled,
}

impl PreJitCleanupPhase {
    const fn successor(self) -> Option<Self> {
        match self {
            Self::Claimed => Some(Self::BootstrapAbsent),
            Self::BootstrapAbsent => Some(Self::InitializerAbsent),
            Self::InitializerAbsent => Some(Self::StorageAbsent),
            Self::StorageAbsent => Some(Self::NetworkAbsent),
            Self::NetworkAbsent | Self::Settled => None,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Claimed => "claimed",
            Self::BootstrapAbsent => "bootstrap_absent",
            Self::InitializerAbsent => "initializer_absent",
            Self::StorageAbsent => "storage_absent",
            Self::NetworkAbsent => "network_absent",
            Self::Settled => "settled",
        }
    }
}

/// This record stores one observed transition under the exact pre-JIT cleanup lease.
pub(crate) struct PreJitCleanupPhaseProof {
    connection_id: String,
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    snapshot_sha256: String,
    generation: i64,
    from_phase: PreJitCleanupPhase,
    to_phase: PreJitCleanupPhase,
    evidence_sha256: String,
}

impl PreJitCleanupPhaseProof {
    fn after_observed_phase(
        claim: &PreJitCleanupClaim,
        from_phase: PreJitCleanupPhase,
        to_phase: PreJitCleanupPhase,
        canonical_evidence: &[u8],
    ) -> Result<Self, HostError> {
        if from_phase.successor() != Some(to_phase)
            || canonical_evidence.is_empty()
            || canonical_evidence.len() > 32 * 1024
        {
            return Err(HostError::Identity);
        }
        let evidence_sha256 = lower_hex(&Sha256::digest(canonical_evidence));
        Ok(Self {
            connection_id: claim.connection_id().to_owned(),
            reservation_id: claim.reservation_id(),
            operation_id: claim.operation_id(),
            engine_id: claim.engine_id().to_owned(),
            docker_root_digest: claim.docker_root_digest().to_owned(),
            snapshot_sha256: claim.snapshot_sha256().to_owned(),
            generation: claim.generation(),
            from_phase,
            to_phase,
            evidence_sha256,
        })
    }

    pub(crate) fn connection_id(&self) -> &str {
        &self.connection_id
    }

    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn snapshot_sha256(&self) -> &str {
        &self.snapshot_sha256
    }

    pub(crate) const fn generation(&self) -> i64 {
        self.generation
    }

    pub(crate) const fn source_phase(&self) -> PreJitCleanupPhase {
        self.from_phase
    }

    pub(crate) const fn to_phase(&self) -> PreJitCleanupPhase {
        self.to_phase
    }

    pub(crate) fn evidence_sha256(&self) -> &str {
        &self.evidence_sha256
    }
}

/// The storage evidence is fresh. The journal has retired its Capacity probe.
pub(crate) struct PreJitCleanupSettlementProof {
    connection_id: String,
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    snapshot_sha256: String,
    generation: i64,
    probe_operation_id: String,
    profile_generation_id: String,
    profile_digest: String,
    probe_operation_name: String,
    probe_container_id: String,
    sample_evidence_sha256: String,
    sample: crate::journal::StorageAdmissionSample,
}

impl PreJitCleanupSettlementProof {
    /// Seal the sample after the journal retires the Capacity probe.
    pub(in crate::launch) fn after_retired_capacity_probe(
        observation: crate::launch::PreJitCapacityObservation,
    ) -> Result<Self, HostError> {
        let (
            claim,
            probe_operation_id,
            probe_operation_name,
            probe_container_id,
            profile_generation_id,
            profile_digest,
            sample,
        ) = observation.into_parts();
        let operation_id = probe_operation_id.as_str();
        let operation_name = probe_operation_name.as_str();
        let container_id = probe_container_id.as_str();
        let identity = sample.identity();
        if operation_id != identity.sample_generation_id()
            || identity.engine_id() != claim.engine_id()
            || identity.docker_root_digest() != claim.docker_root_digest()
            || identity.profile_generation_id() != profile_generation_id
            || identity.profile_digest() != profile_digest
            || operation_name != format!("velnor-resource-probe-{operation_id}")
            || container_id.is_empty()
        {
            return Err(HostError::Ownership);
        }
        let sample_evidence_sha256 = sample_digest(&sample, operation_name, container_id)?;
        let profile_generation_id = identity.profile_generation_id().to_owned();
        let profile_digest = identity.profile_digest().to_owned();
        Ok(Self {
            connection_id: claim.connection_id().to_owned(),
            reservation_id: claim.reservation_id(),
            operation_id: claim.operation_id(),
            engine_id: claim.engine_id().to_owned(),
            docker_root_digest: claim.docker_root_digest().to_owned(),
            snapshot_sha256: claim.snapshot_sha256().to_owned(),
            generation: claim.generation(),
            probe_operation_id,
            profile_generation_id,
            profile_digest,
            probe_operation_name,
            probe_container_id,
            sample_evidence_sha256,
            sample,
        })
    }

    pub(crate) fn sample(&self) -> &crate::journal::StorageAdmissionSample {
        &self.sample
    }
    pub(crate) fn connection_id(&self) -> &str {
        &self.connection_id
    }
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }
    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }
    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }
    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }
    pub(crate) fn snapshot_sha256(&self) -> &str {
        &self.snapshot_sha256
    }
    pub(crate) const fn generation(&self) -> i64 {
        self.generation
    }
    pub(crate) fn probe_operation_id(&self) -> &str {
        &self.probe_operation_id
    }
    pub(crate) fn profile_generation_id(&self) -> &str {
        &self.profile_generation_id
    }
    pub(crate) fn profile_digest(&self) -> &str {
        &self.profile_digest
    }
    pub(crate) fn probe_operation_name(&self) -> &str {
        &self.probe_operation_name
    }
    pub(crate) fn probe_container_id(&self) -> &str {
        &self.probe_container_id
    }
    pub(crate) fn sample_evidence_sha256(&self) -> &str {
        &self.sample_evidence_sha256
    }
}

fn sample_digest(
    sample: &crate::journal::StorageAdmissionSample,
    operation_name: &str,
    container_id: &str,
) -> Result<String, HostError> {
    let identity = sample.identity();
    let pools = sample
        .pools()
        .iter()
        .map(|pool| {
            serde_json::json!({
                "pool_id": pool.pool_id(),
                "filesystem_instance_digest": pool.filesystem_instance_digest(),
                "medium": pool.medium().as_str(),
                "filesystem_kind": pool.filesystem_kind().as_str(),
                "total_bytes": pool.total_bytes(),
                "available_bytes": pool.available_bytes(),
                "total_inodes": pool.total_inodes(),
                "available_inodes": pool.available_inodes(),
            })
        })
        .collect::<Vec<_>>();
    let memory = sample.memory().map(|value| {
        serde_json::json!({
            "guest_available_bytes": value.guest_available_bytes(),
            "ancestor_memory_max_bytes": value.ancestor_memory_max_bytes(),
            "ancestor_memory_current_bytes": value.ancestor_memory_current_bytes(),
            "ancestor_swap_max_bytes": value.ancestor_swap_max_bytes(),
            "ancestor_swap_current_bytes": value.ancestor_swap_current_bytes(),
        })
    });
    let bytes = serde_json::to_vec(&serde_json::json!({
        "sample_generation_id": identity.sample_generation_id(),
        "profile_generation_id": identity.profile_generation_id(),
        "profile_digest": identity.profile_digest(),
        "engine_id": identity.engine_id(),
        "docker_root_digest": identity.docker_root_digest(),
        "observed_at_unix_ms": sample.observed_at_unix_ms(),
        "pools": pools,
        "memory": memory,
        "probe_operation_name": operation_name,
        "probe_container_id": container_id,
    }))
    .map_err(|_| HostError::Identity)?;
    Ok(lower_hex(&Sha256::digest(bytes)))
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    value
}

#[cfg(test)]
#[path = "pre_jit_recovery/tests.rs"]
mod tests;
