use bollard::{Docker, errors::Error as DockerError, models::ContainerInspectResponse};

use crate::{
    docker_client::docker_deadline,
    error::HostError,
    journal::{ConnectionJournal, StorageInitializationClaim},
    launch::SelectedEngineContext,
    worker::{WorkerIdentityNames, WorkerVolumeRole},
};

pub(super) async fn verify_engine(
    docker: &Docker,
    journal: &ConnectionJournal,
    claim: &StorageInitializationClaim,
) -> Result<(), HostError> {
    let selected = journal
        .engine_context()
        .await?
        .ok_or(HostError::Ownership)?;
    let info = docker_deadline(docker.info())
        .await?
        .map_err(|_| HostError::Docker)?;
    let actual = SelectedEngineContext::capture(&selected, &info)?;
    if !engine_identity_matches(
        actual.engine_id(),
        actual.docker_root_digest(),
        claim.engine_id(),
        claim.docker_root_digest(),
    ) {
        return Err(HostError::Ownership);
    }
    Ok(())
}

pub(super) async fn verify_image(
    docker: &Docker,
    claim: &StorageInitializationClaim,
) -> Result<Vec<String>, HostError> {
    let image = docker_deadline(docker.inspect_image(claim.image_runtime_id()))
        .await?
        .map_err(|_| HostError::Docker)?;
    if image.id.as_deref() != Some(claim.image_runtime_id())
        || image.os.as_deref() != Some("linux")
        || image.architecture.as_deref() != Some("amd64")
    {
        return Err(HostError::Ownership);
    }
    let env = image
        .config
        .as_ref()
        .and_then(|config| config.env.as_ref())
        .ok_or(HostError::Ownership)?;
    if !image_environment_matches(env) {
        return Err(HostError::Ownership);
    }
    Ok(env.clone())
}

pub(super) async fn verify_volumes(
    docker: &Docker,
    claim: &StorageInitializationClaim,
) -> Result<(), HostError> {
    if claim.layout_version() != 2 {
        return Err(HostError::Ownership);
    }
    let metadata_name = claim.metadata_volume_name().ok_or(HostError::Ownership)?;
    let metadata_id = claim.metadata_volume_id().ok_or(HostError::Ownership)?;
    let base = claim
        .home_volume()
        .strip_suffix("-home")
        .ok_or(HostError::Ownership)?;
    let names = WorkerIdentityNames::from_private_volume(base)?;
    if names.volume_name(WorkerVolumeRole::Home) != claim.home_volume()
        || names.volume_name(WorkerVolumeRole::Work) != claim.work_volume()
        || names.volume_name(WorkerVolumeRole::RunnerMetadata) != metadata_name
    {
        return Err(HostError::Ownership);
    }
    for (name, role, expected_id) in [
        (claim.home_volume(), "runner-home", None),
        (claim.work_volume(), "runner-work", None),
        (metadata_name, "engine-metadata", Some(metadata_id)),
    ] {
        let volume = docker_deadline(docker.inspect_volume(name))
            .await?
            .map_err(|error| match error {
                DockerError::DockerResponseServerError {
                    status_code: 404, ..
                } => HostError::Ownership,
                _ => HostError::Docker,
            })?;
        let labels = volume.labels;
        if !volume_identity_matches(
            &volume.name,
            &labels,
            name,
            claim.connection_id(),
            claim.reservation_id(),
            claim.operation_id(),
            role,
        ) || expected_id.is_some_and(|expected| volume.name != expected)
        {
            return Err(HostError::Ownership);
        }
    }
    Ok(())
}

pub(super) async fn inspect_container(
    docker: &Docker,
    id: &str,
) -> Result<Option<ContainerInspectResponse>, HostError> {
    match docker_deadline(docker.inspect_container(id, None)).await? {
        Ok(container) => Ok(Some(container)),
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(None),
        Err(_) => Err(HostError::Docker),
    }
}

pub(super) async fn inspect_owned_container(
    docker: &Docker,
    claim: &StorageInitializationClaim,
    id: &str,
) -> Result<ContainerInspectResponse, HostError> {
    let inspected = inspect_container(docker, id)
        .await?
        .ok_or(HostError::Ownership)?;
    if !container_matches_claim_identity(&inspected, claim, id) {
        return Err(HostError::Ownership);
    }
    Ok(inspected)
}

pub(super) fn container_matches_claim(
    inspected: &ContainerInspectResponse,
    claim: &StorageInitializationClaim,
    expected_env: &[String],
) -> bool {
    let identity = InitializerIdentity::from_claim(claim);
    container_matches_identity(
        inspected,
        &identity,
        inspected.id.as_deref().unwrap_or_default(),
    ) && inspected.image.as_deref() == Some(identity.image_runtime_id)
        && initializer_config_matches(inspected, &identity, expected_env)
        && initializer_host_config_matches(inspected, &identity)
        && initializer_mounts_match(inspected, &identity)
}

/// Check one helper against the identity saved in the pre-JIT cleanup inventory.
pub(crate) fn pre_jit_container_matches(
    inspected: &ContainerInspectResponse,
    identity: &crate::journal::PreJitInitializerIdentity,
    expected_env: &[String],
) -> bool {
    let view = InitializerIdentity::from_pre_jit(identity);
    let Some(id) = inspected.id.as_deref() else {
        return false;
    };
    identity.helper_id().is_none_or(|saved| saved == id)
        && container_matches_identity(inspected, &view, id)
        && inspected.image.as_deref() == Some(view.image_runtime_id)
        && initializer_config_matches(inspected, &view, expected_env)
        && initializer_host_config_matches(inspected, &view)
        && initializer_mounts_match(inspected, &view)
}

mod predicates;
pub(crate) use predicates::container_matches_claim_identity;
#[cfg(test)]
use predicates::{IMAGE_PATH_ENV, capabilities_match};
use predicates::{
    InitializerIdentity, container_matches_identity, engine_identity_matches,
    image_environment_matches, initializer_config_matches, initializer_host_config_matches,
    initializer_mounts_match, volume_identity_matches,
};

#[cfg(test)]
#[path = "verification/tests.rs"]
mod tests;
