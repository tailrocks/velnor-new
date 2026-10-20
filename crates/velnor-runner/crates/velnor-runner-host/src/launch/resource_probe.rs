//! Controller-only guest resource probe. Runner projections stay untouched.

use bollard::{Docker, models::SystemInfo};

use crate::journal::{Journal, ProbePhase, ProbeRow};
use crate::launch::{SelectedEngineContext, VerifiedWorkerImages};
use crate::scale_set::EnsureError;
use crate::worker::ResourceBudget;

pub(crate) use self::provider::ImageProvider;
use self::provider::ReleaseImageProvider;

pub(crate) use self::sample::RecoveryObservation;
pub(crate) use self::sample::{StartObservationWitness, StartPermit};
pub(in crate::launch) use sample::filesystem_instance_digest;

mod cleanup_observer;
mod doctor;
mod execute;
pub(in crate::launch) use execute::ProbeStartupEvidence;
mod inspect;
#[cfg(test)]
mod intake_composition;
mod lifecycle;
mod metadata_settlement;
mod ops;
mod owned_cgroups;
mod projection;
mod provider;
pub(crate) mod publication;
pub(in crate::launch) use provider::VerifiedWorkspaceCleanup;
mod release_secret;

pub(in crate::launch) use provider::VerifiedWorkerImageIdentityProof;

mod record;
#[cfg(test)]
pub(in crate::launch) use record::http_fixture::canonical_logs_response;
mod recover;
mod sample;
#[cfg(test)]
mod test_provider;
#[cfg(test)]
pub(crate) use intake_composition::IntakeCompositionProvider;
#[cfg(test)]
pub(in crate::launch) use intake_composition::intake_composition_image_inspect;
pub(in crate::launch) use sample::retained_tmpfs_bytes as retained_tmpfs_memory_bytes;
#[cfg(test)]
pub(crate) use test_provider::{
    recovery_observation_with_provider, start_permit_with_test_provider,
};

pub(in crate::launch) use cleanup_observer::{
    CleanupObserverContext, VerifiedCleanupObserverRecord, observe_cleanup_binding,
};
#[cfg(test)]
pub(in crate::launch) use cleanup_observer::{
    CleanupObserverTestProvider, observe_cleanup_binding_with_test_provider,
};
pub(in crate::launch) use metadata_settlement::settle_storage_metadata_after_cleanup;
#[cfg(test)]
pub(in crate::launch) use metadata_settlement::settle_storage_metadata_after_cleanup_with_test_provider;
pub(in crate::launch) use metadata_settlement::{
    PreJitCapacityObservation, measure_pre_jit_capacity,
};

pub(crate) use owned_cgroups::{OwnedCgroupSubjects, OwnedContainerSubjects, VerifiedOwnedCgroups};
pub(crate) use release_secret::ReleaseReadSecret;

/// An attested probe image bound to the current selected daemon and root.
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

/// Verify the release image and bind it to fresh selected-engine information.
pub(in crate::launch::resource_probe) async fn verify_publication_probe_image(
    docker: &Docker,
    journal: &Journal,
    selected: &SelectedEngineContext,
    release_read_secret: &ReleaseReadSecret,
) -> Result<VerifiedPublicationProbeImage, crate::error::HostError> {
    let provider = release_image_provider(release_read_secret, journal)?;
    verify_publication_probe_image_with_provider(docker, journal, selected, &provider).await
}

/// Verify the release image before the caller's absolute deadline.
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

async fn verify_publication_probe_image_with_provider<P: ImageProvider>(
    docker: &Docker,
    journal: &Journal,
    selected: &SelectedEngineContext,
    provider: &P,
) -> Result<VerifiedPublicationProbeImage, crate::error::HostError> {
    let (image, evidence) =
        lifecycle::verify_publication_image(docker, journal, provider, selected).await?;
    Ok(VerifiedPublicationProbeImage { image, evidence })
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

/// Proof that one bound probe is absent from its exact selected daemon.
///
/// Only the probe recovery path can construct this after checking both the
/// operation name and immutable container ID on the same engine and root.
pub(crate) struct VerifiedProbeCleanup {
    operation_id: String,
    engine_id: String,
    docker_root_digest: String,
    purpose: crate::journal::ResourceProbePurpose,
    phase: ProbePhase,
    operation_name: String,
    container_id: Option<String>,
}

impl VerifiedProbeCleanup {
    // Private; only recovery code in this module tree can mint this capability.
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
            purpose,
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

    pub(crate) const fn purpose(&self) -> crate::journal::ResourceProbePurpose {
        self.purpose
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

    #[cfg(test)]
    pub(crate) fn test_only(
        operation_id: String,
        engine_id: String,
        docker_root_digest: String,
        phase: ProbePhase,
        operation_name: String,
        container_id: Option<String>,
    ) -> Self {
        Self {
            operation_id,
            engine_id,
            docker_root_digest,
            purpose: crate::journal::ResourceProbePurpose::Capacity,
            phase,
            operation_name,
            container_id,
        }
    }
}

/// Report only whether this binary has a valid trusted release source pin.
/// This does not claim that GitHub, the release, or any Docker image is ready.
pub(crate) fn build_pin_status() -> Result<(), &'static str> {
    provider::build_pin_status()
}

/// Re-read the selected engine and inspect both prepared worker images.
pub(crate) async fn verify_worker_images_for_engine(
    docker: &Docker,
    expected: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
) -> Result<(), crate::error::HostError> {
    provider::verify_worker_images_for_engine(docker, expected, images).await
}

/// Prepare the attested Runner and `DinD` images on the selected daemon.
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

pub(crate) async fn observe_owned_cgroups(
    docker: &Docker,
    journal: &Journal,
    release_read_secret: &ReleaseReadSecret,
    subjects: &OwnedCgroupSubjects,
) -> Result<VerifiedOwnedCgroups, crate::error::HostError> {
    let token = release_read_secret.as_provider_token()?;
    let provider = ReleaseImageProvider::from_compiled_source(token, journal)
        .map_err(|_| crate::error::HostError::Docker)?;
    Box::pin(observe_owned_cgroups_with_provider(
        docker, journal, &provider, subjects,
    ))
    .await
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

#[cfg(test)]
mod tests;

/// Obtain a fresh one-use admission permit immediately before acquire/JIT/start.
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

/// Get a new guest resource sample for one existing reservation.
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

pub(super) fn consume_permit(permit: StartPermit, engine_id: &str, root_digest: &str) -> bool {
    permit.consume(engine_id, root_digest)
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

pub(super) async fn engine_binding(docker: &Docker) -> Result<(String, String), EnsureError> {
    let info = crate::docker_client::docker_deadline(docker.info())
        .await
        .map_err(|_| docker_identity_error())?
        .map_err(|_| docker_identity_error())?;
    let engine_id = info
        .id
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(docker_identity_error)?;
    let docker_root = info
        .docker_root_dir
        .as_deref()
        .ok_or_else(docker_identity_error)?;
    let root_digest = docker_root_digest(docker_root).map_err(|_| docker_identity_error())?;
    Ok((engine_id, root_digest))
}

fn docker_identity_error() -> EnsureError {
    EnsureError::Unexpected {
        status: 0,
        step: "docker identity",
    }
}
