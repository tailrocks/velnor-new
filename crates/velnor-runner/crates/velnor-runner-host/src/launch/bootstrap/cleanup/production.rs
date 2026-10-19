//! Read-only proof that one held reservation has no Docker objects.

use bollard::{
    Docker, errors::Error as DockerError, query_parameters::InspectContainerOptionsBuilder,
};

use crate::{
    docker_client::docker_deadline,
    error::HostError,
    journal::{EngineContext, StorageCleanupIdentity, StorageRole},
    launch::SelectedEngineContext,
    worker::{WorkerContainerRole, WorkerIdentityNames, WorkerVolumeRole},
};

/// Exact objects confirmed absent from one selected Docker engine.
pub(crate) struct ObservedDockerCleanupAbsence {
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    runner_container_id: String,
    dind_container_id: String,
    volume_inventory: Vec<(StorageRole, String)>,
    absent_container_ids: [String; 2],
    absent_container_names: [String; 2],
    absent_volumes: Vec<String>,
}

impl ObservedDockerCleanupAbsence {
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

    pub(crate) fn runner_container_id(&self) -> &str {
        &self.runner_container_id
    }

    pub(crate) fn dind_container_id(&self) -> &str {
        &self.dind_container_id
    }

    pub(crate) fn volume_inventory(&self) -> &[(StorageRole, String)] {
        &self.volume_inventory
    }

    pub(crate) fn absent_container_ids(&self) -> &[String; 2] {
        &self.absent_container_ids
    }

    pub(crate) fn absent_container_names(&self) -> &[String; 2] {
        &self.absent_container_names
    }

    pub(crate) fn absent_volumes(&self) -> &[String] {
        &self.absent_volumes
    }
}

/// Observe exact Docker object absence on the recorded engine.
///
/// This function does not prove guest cgroup or mount absence.
///
/// # Errors
///
/// Returns an error when engine identity or any exact Docker object is uncertain.
pub(crate) async fn observe_docker_cleanup_absence(
    docker: &Docker,
    selected: &EngineContext,
    identity: &StorageCleanupIdentity,
) -> Result<ObservedDockerCleanupAbsence, HostError> {
    validate_identity(identity)?;
    let info = docker_deadline(docker.info())
        .await?
        .map_err(|_| HostError::Docker)?;
    let observed = SelectedEngineContext::capture(selected, &info)?;
    if !observed.matches_selection(selected)
        || observed.engine_id() != identity.engine_id()
        || observed.docker_root_digest() != identity.docker_root_digest()
    {
        return Err(HostError::Ownership);
    }

    let names = WorkerIdentityNames::from_private_volume(identity.private_volume())?;
    let container_names = [
        names.container_name(WorkerContainerRole::Runner),
        names.container_name(WorkerContainerRole::Dind),
    ];
    let container_ids = [
        identity.runner_container_id().to_owned(),
        identity.dind_container_id().to_owned(),
    ];
    for (id, name) in container_ids.iter().zip(&container_names) {
        require_container_absent(docker, id).await?;
        require_container_absent(docker, name).await?;
    }

    let mut absent_volumes = Vec::with_capacity(identity.volumes().len());
    for (_, name) in identity.volumes() {
        require_volume_absent(docker, name).await?;
        absent_volumes.push(name.clone());
    }
    Ok(ObservedDockerCleanupAbsence {
        reservation_id: identity.reservation_id(),
        operation_id: identity.operation_id(),
        engine_id: observed.engine_id().to_owned(),
        docker_root_digest: observed.docker_root_digest().to_owned(),
        runner_container_id: container_ids[0].clone(),
        dind_container_id: container_ids[1].clone(),
        volume_inventory: identity.volumes().to_vec(),
        absent_container_ids: container_ids,
        absent_container_names: container_names,
        absent_volumes,
    })
}

fn validate_identity(identity: &StorageCleanupIdentity) -> Result<(), HostError> {
    if identity.reservation_id() <= 0
        || identity.operation_id() <= 0
        || identity.runner_container_id() == identity.dind_container_id()
        || identity.engine_id().is_empty()
        || !lower_hex(identity.docker_root_digest(), 64)
        || !lower_hex(identity.instance_binding_digest(), 64)
    {
        return Err(HostError::Identity);
    }
    let names = WorkerIdentityNames::from_private_volume(identity.private_volume())?;
    let expected: &[(StorageRole, WorkerVolumeRole)] = match identity.topology_version() {
        1 => &[
            (StorageRole::SharedRun, WorkerVolumeRole::Socket),
            (StorageRole::RunnerHome, WorkerVolumeRole::Home),
            (StorageRole::RunnerWork, WorkerVolumeRole::Work),
            (StorageRole::DindData, WorkerVolumeRole::DindData),
        ],
        2 => &[
            (StorageRole::SharedRun, WorkerVolumeRole::Socket),
            (StorageRole::RunnerHome, WorkerVolumeRole::Home),
            (StorageRole::RunnerWork, WorkerVolumeRole::Work),
            (StorageRole::DindData, WorkerVolumeRole::DindData),
            (
                StorageRole::EngineMetadata,
                WorkerVolumeRole::RunnerMetadata,
            ),
        ],
        _ => return Err(HostError::Identity),
    };
    if identity.volumes().len() != expected.len()
        || expected.iter().zip(identity.volumes()).any(
            |((expected_role, expected_name), (role, name))| {
                role != expected_role || name != &names.volume_name(*expected_name)
            },
        )
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

async fn require_container_absent(docker: &Docker, reference: &str) -> Result<(), HostError> {
    let options = InspectContainerOptionsBuilder::new().build();
    match docker_deadline(docker.inspect_container(reference, Some(options))).await? {
        Ok(_) => Err(HostError::Ownership),
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(()),
        Err(_) => Err(HostError::Docker),
    }
}

async fn require_volume_absent(docker: &Docker, name: &str) -> Result<(), HostError> {
    match docker_deadline(docker.inspect_volume(name)).await? {
        Ok(_) => Err(HostError::Ownership),
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(()),
        Err(_) => Err(HostError::Docker),
    }
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
#[path = "production/tests.rs"]
mod tests;
