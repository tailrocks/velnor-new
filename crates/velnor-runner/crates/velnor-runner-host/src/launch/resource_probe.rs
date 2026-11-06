// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::{Docker, models::SystemInfo};

use crate::journal::{Journal, ProbePhase, ProbeRow};
use crate::launch::{SelectedEngineContext, VerifiedWorkerImages};
use crate::worker::ResourceBudget;

pub(crate) use self::provider::ImageProvider;
use self::provider::ReleaseImageProvider;
pub use self::provider::{
    WorkerImage, verify_worker_attestation_manifest, verify_worker_build_source,
    verify_worker_runtime_config,
};

pub(crate) use self::sample::FreshPressureSample;
pub(crate) use self::sample::RecoveryObservation;
pub(crate) use self::sample::{StartObservationWitness, StartPermit};
pub(crate) use readiness::ReadinessSample;
pub(in crate::launch) use sample::{FilesystemInstanceDigestInput, filesystem_instance_digest};

mod cleanup_observer;
mod doctor;
mod execute;
pub(in crate::launch) use execute::ProbeStartupEvidence;
mod inspect;

mod lifecycle;
mod metadata_settlement;
mod ops;
mod owned_cgroups;
mod projection;
mod provider;
pub(crate) mod publication;
mod readiness;
pub(in crate::launch) use provider::VerifiedWorkspaceCleanup;
pub(crate) use readiness::collect as collect_readiness_sample;

mod release_secret;

pub(in crate::launch) use provider::VerifiedWorkerImageIdentityProof;

mod record;

mod recover;
mod sample;

pub(in crate::launch) use sample::retained_tmpfs_bytes as retained_tmpfs_memory_bytes;

pub(in crate::launch) use cleanup_observer::{
    CleanupObserverContext, VerifiedCleanupObserverRecord, observe_cleanup_binding,
};

pub(in crate::launch) use metadata_settlement::settle_storage_metadata_after_cleanup;

pub(in crate::launch) use metadata_settlement::{
    PreJitCapacityObservation, measure_pre_jit_capacity,
};

pub(crate) use owned_cgroups::{OwnedCgroupSubjects, OwnedContainerSubjects, VerifiedOwnedCgroups};
pub(crate) use release_secret::ReleaseReadSecret;

pub(in crate::launch::resource_probe) struct VerifiedPublicationProbeImage {
    image: projection::VerifiedProbeImage,
    evidence: provider::ProbeImagePreparationEvidence,
}

impl VerifiedPublicationProbeImage {
    pub(in crate::launch::resource_probe) fn image(&self) -> &projection::VerifiedProbeImage {
        &self.image
    }

    pub(in crate::launch::resource_probe) fn evidence(
        &self,
    ) -> &provider::ProbeImagePreparationEvidence {
        &self.evidence
    }
}

pub(in crate::launch::resource_probe) async fn verify_publication_probe_image_until(
    docker: &Docker,
    journal: &Journal,
    selected: &SelectedEngineContext,
    release_read_secret: &ReleaseReadSecret,
    deadline: std::time::Instant,
) -> Result<VerifiedPublicationProbeImage, crate::error::HostError> {
    let provider = release_image_provider(release_read_secret, journal)?;
    verify_publication_probe_image_until_with_provider(
        docker, journal, selected, &provider, deadline,
    )
    .await
}

pub(in crate::launch::resource_probe) async fn verify_publication_probe_image_until_with_provider<
    P: ImageProvider,
>(
    docker: &Docker,
    journal: &Journal,
    selected: &SelectedEngineContext,
    provider: &P,
    deadline: std::time::Instant,
) -> Result<VerifiedPublicationProbeImage, crate::error::HostError> {
    let (image, evidence) =
        lifecycle::verify_publication_image_until(docker, journal, provider, selected, deadline)
            .await?;
    Ok(VerifiedPublicationProbeImage { image, evidence })
}

pub(crate) struct VerifiedProbeCleanup {
    operation_id: String,
    engine_id: String,
    docker_root_digest: String,
    phase: ProbePhase,
    operation_name: String,
    container_id: Option<String>,
}

impl VerifiedProbeCleanup {
    fn after_verified_removal(
        row: &ProbeRow,
        projection: &projection::ProbeProjection,
        container_id: &str,
    ) -> Result<Self, crate::error::HostError> {
        let purpose = match row.work() {
            crate::journal::ProbeWork::ResourceProbe { purpose, .. } => *purpose,
            crate::journal::ProbeWork::ExecutionPublication => {
                return Err(crate::error::HostError::Ownership);
            }
        };
        if row.phase != ProbePhase::RemoveRequested
            || row.container_id.as_deref() != Some(container_id)
            || row.operation_id != projection.operation_id
            || row.engine_id != projection.engine_id
            || row.docker_root_digest != projection.root.digest()
            || purpose != projection.purpose
            || row.operation_name != projection.name
            || row.projection_digest != projection.projection_digest
        {
            return Err(crate::error::HostError::Ownership);
        }
        Ok(Self {
            operation_id: row.operation_id.clone(),
            engine_id: row.engine_id.clone(),
            docker_root_digest: row.docker_root_digest.clone(),
            phase: row.phase,
            operation_name: row.operation_name.clone(),
            container_id: row.container_id.clone(),
        })
    }

    pub(crate) fn operation_id(&self) -> &str {
        &self.operation_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) const fn phase(&self) -> ProbePhase {
        self.phase
    }

    pub(crate) fn operation_name(&self) -> &str {
        &self.operation_name
    }

    pub(crate) fn container_id(&self) -> Option<&str> {
        self.container_id.as_deref()
    }
}

pub(crate) fn build_pin_status() -> Result<(), &'static str> {
    provider::build_pin_status()
}

pub(crate) async fn verify_worker_images_for_engine(
    docker: &Docker,
    expected: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
) -> Result<(), crate::error::HostError> {
    provider::verify_worker_images_for_engine(docker, expected, images).await
}

pub(crate) async fn prepare_worker_images(
    docker: &Docker,
    journal: &Journal,
    expected: &SelectedEngineContext,
    release_read_secret: &ReleaseReadSecret,
) -> Result<VerifiedWorkerImages, crate::error::HostError> {
    let token = release_read_secret.as_provider_token()?;
    provider::prepare_worker_images(docker, journal, expected, token).await
}

pub(crate) async fn run_doctor_probe(
    docker: &Docker,
    journal: &Journal,
    release_read_secret: &ReleaseReadSecret,
    ticket: &crate::journal::DoctorProbeTicket,
) -> Result<Option<serde_json::Value>, crate::error::HostError> {
    Box::pin(doctor::run(docker, journal, release_read_secret, ticket)).await
}

pub(in crate::launch) fn release_image_provider<'a>(
    secret: &'a ReleaseReadSecret,
    journal: &'a Journal,
) -> Result<impl ImageProvider + 'a, crate::error::HostError> {
    let token = secret.as_provider_token()?;
    ReleaseImageProvider::from_compiled_source(token, journal)
        .map_err(|_| crate::error::HostError::Docker)
}

pub(in crate::launch) async fn observe_owned_cgroups_with_provider<P: ImageProvider>(
    docker: &Docker,
    journal: &Journal,
    provider: &P,
    subjects: &OwnedCgroupSubjects,
) -> Result<VerifiedOwnedCgroups, crate::error::HostError> {
    lifecycle::collect_owned_cgroups(docker, journal, provider, subjects)
        .await?
        .ok_or(crate::error::HostError::Docker)
}

pub(crate) async fn start_permit(
    docker: &Docker,
    journal: &Journal,
    budget: ResourceBudget,
    release_read_secret: &ReleaseReadSecret,
) -> Option<StartPermit> {
    let token = release_read_secret.as_provider_token().ok()?;
    let provider = match ReleaseImageProvider::from_compiled_source(token, journal) {
        Ok(provider) => provider,
        Err(error) => {
            eprintln!("resource_probe=unavailable reason={}", error.code());
            return None;
        }
    };
    match Box::pin(lifecycle::collect(docker, journal, &provider)).await {
        Ok(Some(sample)) => sample.into_start_permit(budget),
        Ok(None) | Err(_) => None,
    }
}

pub(crate) async fn recovery_observation(
    docker: &Docker,
    journal: &Journal,
    budget: ResourceBudget,
    charge: (u64, u64),
    engine: &SelectedEngineContext,
    release_read_secret: &ReleaseReadSecret,
) -> Option<RecoveryObservation> {
    let token = release_read_secret.as_provider_token().ok()?;
    let provider = ReleaseImageProvider::from_compiled_source(token, journal).ok()?;
    let sample = Box::pin(lifecycle::collect(docker, journal, &provider))
        .await
        .ok()??;
    sample.into_recovery_observation(
        budget,
        charge,
        engine.engine_id(),
        engine.docker_root_digest(),
    )
}

pub(super) fn docker_root_digest(path: &str) -> Result<String, crate::error::HostError> {
    projection::DockerRoot::parse(path).map(|root| root.digest().to_owned())
}

pub(super) fn daemon_architecture(info: &SystemInfo) -> Option<&'static str> {
    provider::daemon_architecture(info)
}

pub(super) fn daemon_platform_matches(info: &SystemInfo) -> bool {
    provider::daemon_platform_matches(info)
}
