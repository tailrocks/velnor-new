use bollard::Docker;

use crate::{
    error::HostError,
    journal::{
        ConnectionJournal, Journal, PreJitCapacityProbeClaim, PreJitCleanupClaim, ProbeSeed,
        ResourceProbePurpose, StorageAdmissionSample,
    },
    launch::{SelectedEngineContext, resource_probe::ImageProvider},
};

use super::super::{
    execute,
    lifecycle::{guest_capacity, info, same_guest},
    projection::ProbeProjection,
};
use super::setup::prepare_settlement_setup;

/// This value contains a fresh capacity sample. The journal has retired its Capacity probe.
pub(in crate::launch) struct PreJitCapacityObservation {
    cleanup: PreJitCleanupClaim,
    probe_operation_id: String,
    probe_operation_name: String,
    probe_container_id: String,
    profile_generation_id: String,
    profile_digest: String,
    sample: StorageAdmissionSample,
}

impl PreJitCapacityObservation {
    pub(in crate::launch) fn into_parts(
        self,
    ) -> (
        PreJitCleanupClaim,
        String,
        String,
        String,
        String,
        String,
        StorageAdmissionSample,
    ) {
        (
            self.cleanup,
            self.probe_operation_id,
            self.probe_operation_name,
            self.probe_container_id,
            self.profile_generation_id,
            self.profile_digest,
            self.sample,
        )
    }
}

pub(in crate::launch) async fn measure_pre_jit_capacity<P: ImageProvider>(
    docker: &Docker,
    journal: &Journal,
    scope: &ConnectionJournal,
    claim: &PreJitCleanupClaim,
    selected: &SelectedEngineContext,
    provider: &P,
) -> Result<PreJitCapacityObservation, HostError> {
    let _operation = super::super::readiness::lock_operation(journal).await?;
    if !scope.validate_pre_jit_cleanup_claim(claim).await? {
        return Err(HostError::Ownership);
    }
    let prepared =
        prepare_pre_jit_capacity_probe(docker, journal, scope, claim, selected, provider).await?;
    observe_pre_jit_capacity(docker, journal, prepared).await
}

struct PreparedCapacityProbe {
    cleanup: PreJitCleanupClaim,
    profile: crate::journal::StorageProfile,
    evidence: super::super::provider::ProbeImagePreparationEvidence,
    engine_id: String,
    root: super::super::projection::DockerRoot,
    architecture: String,
    cpu_count: u32,
    memory_total: u64,
    deadline: super::super::lifecycle::Deadline,
    projection: ProbeProjection,
    probe_claim: PreJitCapacityProbeClaim,
}

async fn prepare_pre_jit_capacity_probe<P: ImageProvider>(
    docker: &Docker,
    journal: &Journal,
    scope: &ConnectionJournal,
    claim: &PreJitCleanupClaim,
    selected: &SelectedEngineContext,
    provider: &P,
) -> Result<PreparedCapacityProbe, HostError> {
    let setup = Box::pin(prepare_settlement_setup(
        docker, journal, scope, selected, provider,
    ))
    .await?;
    let super::setup::SettlementSetup {
        profile,
        image,
        evidence,
        engine_id,
        root,
        architecture,
        cpu_count,
        memory_total,
        deadline,
    } = setup;
    if engine_id != claim.engine_id() || root.digest() != claim.docker_root_digest() {
        return Err(HostError::Ownership);
    }

    let operation_id = uuid::Uuid::new_v4().simple().to_string();
    let instance_id = deadline.within(journal.instance_id()).await??;
    let projection = ProbeProjection::build(
        operation_id,
        instance_id.clone(),
        engine_id.clone(),
        root.path(),
        image,
    )?;
    let seed = ProbeSeed {
        operation_id: projection.operation_id.clone(),
        instance_id,
        engine_id: projection.engine_id.clone(),
        docker_root_digest: projection.root.digest().to_owned(),
        source_revision: projection.image.source_revision().to_owned(),
        runtime_image_id: projection.image.runtime_id().to_owned(),
        image_binding_digest: projection.image.binding_fingerprint().to_owned(),
        operation_name: projection.name.clone(),
        projection_digest: projection.projection_digest.clone(),
        purpose: ResourceProbePurpose::Capacity,
        request_sha256: crate::journal::capacity_request_sha256(),
    };
    let probe_claim = deadline
        .within(scope.prepare_pre_jit_capacity_probe(claim, seed))
        .await??;
    validate_probe_claim(claim, &probe_claim, &projection, &profile)?;
    Ok(PreparedCapacityProbe {
        cleanup: claim.clone(),
        profile,
        evidence,
        engine_id,
        root,
        architecture,
        cpu_count,
        memory_total,
        deadline,
        projection,
        probe_claim,
    })
}

async fn observe_pre_jit_capacity(
    docker: &Docker,
    journal: &Journal,
    prepared: PreparedCapacityProbe,
) -> Result<PreJitCapacityObservation, HostError> {
    let PreparedCapacityProbe {
        cleanup: claim,
        profile,
        evidence,
        engine_id,
        root,
        architecture,
        cpu_count,
        memory_total,
        deadline,
        projection,
        probe_claim,
    } = prepared;
    let output = Box::pin(execute::run(
        docker,
        journal,
        &projection,
        evidence,
        memory_total,
        &deadline,
    ))
    .await?;
    let startup = output
        .startup_evidence
        .as_ref()
        .ok_or(HostError::Ownership)?;
    let cleanup = startup.cleanup();
    let image = startup.image();
    if cleanup.operation_id() != probe_claim.probe_operation_id()
        || cleanup.operation_name() != probe_claim.probe_operation_name()
        || cleanup.engine_id() != claim.engine_id()
        || cleanup.docker_root_digest() != claim.docker_root_digest()
        || image.engine_id() != claim.engine_id()
        || image.docker_root_digest() != claim.docker_root_digest()
        || image.platform() != "linux/amd64"
    {
        return Err(HostError::Ownership);
    }
    let container_id = cleanup.container_id().to_owned();
    if container_id.is_empty() {
        return Err(HostError::Ownership);
    }
    let after = info(docker, &deadline).await?;
    if !same_guest(&after, &engine_id, &root, &architecture)?
        || guest_capacity(&after)? != (cpu_count, memory_total)
    {
        return Err(HostError::Ownership);
    }
    let sample = super::sample::settlement_sample_for_identity(
        &output,
        &profile,
        probe_claim.probe_operation_id(),
        probe_claim.profile_generation_id(),
        probe_claim.profile_digest(),
        claim.engine_id(),
        claim.docker_root_digest(),
    )?;
    Ok(PreJitCapacityObservation {
        cleanup: claim.clone(),
        probe_operation_id: probe_claim.probe_operation_id().to_owned(),
        probe_operation_name: cleanup.operation_name().to_owned(),
        probe_container_id: container_id,
        profile_generation_id: probe_claim.profile_generation_id().to_owned(),
        profile_digest: probe_claim.profile_digest().to_owned(),
        sample,
    })
}

fn validate_probe_claim(
    cleanup: &PreJitCleanupClaim,
    probe: &PreJitCapacityProbeClaim,
    projection: &ProbeProjection,
    profile: &crate::journal::StorageProfile,
) -> Result<(), HostError> {
    let seed = probe.seed();
    if probe.cleanup_claim().connection_id() != cleanup.connection_id()
        || probe.cleanup_claim().reservation_id() != cleanup.reservation_id()
        || probe.cleanup_claim().operation_id() != cleanup.operation_id()
        || probe.cleanup_claim().engine_id() != cleanup.engine_id()
        || probe.cleanup_claim().docker_root_digest() != cleanup.docker_root_digest()
        || probe.cleanup_claim().generation() != cleanup.generation()
        || probe.cleanup_claim().lease_until() != cleanup.lease_until()
        || probe.cleanup_claim().snapshot_sha256() != cleanup.snapshot_sha256()
        || probe.profile_generation_id() != profile.generation_id()
        || probe.profile_digest() != profile.profile_digest()
        || probe.probe_operation_id() != projection.operation_id
        || probe.probe_operation_name() != projection.name
        || seed.purpose != ResourceProbePurpose::Capacity
        || seed.operation_id != projection.operation_id
        || seed.operation_name != projection.name
        || seed.engine_id != cleanup.engine_id()
        || seed.docker_root_digest != cleanup.docker_root_digest()
        || seed.projection_digest != projection.projection_digest
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}
