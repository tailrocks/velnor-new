//! Named volumes belong to one durable worker identity.

use std::collections::HashMap;
use std::future::Future;

use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::models::{Volume, VolumeCreateRequest};
use bollard::query_parameters::RemoveVolumeOptions;

use super::names::WorkerIdentityNames;
pub(crate) use super::names::WorkerVolumeRole;
use crate::docker_client::docker_deadline;
use crate::docker_spec::Mount as PlannedMount;
use crate::error::HostError;
use velnor_runner_core::runner_work_path;

#[derive(Debug, Eq, PartialEq)]
struct WorkerVolume {
    worker: String,
    name: String,
    role: WorkerVolumeRole,
}

/// Private proof that one exact volume passed checks for its worker and role.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct VerifiedWorkerVolume {
    volume: WorkerVolume,
}

impl VerifiedWorkerVolume {
    /// Send one delete request for this previously verified volume.
    pub(crate) async fn remove_request(&self, docker: &Docker) -> Result<(), HostError> {
        docker_deadline(docker.remove_volume(&self.volume.name, None::<RemoveVolumeOptions>))
            .await?
            .map_err(|_| HostError::Docker)
    }

    /// Confirm the verified volume's name is absent with one inspect request.
    pub(crate) async fn confirm_removed(
        &self,
        docker: &Docker,
    ) -> Result<WorkerVolumeRemoval, HostError> {
        if inspect_volume(docker, &self.volume.name).await?.is_some() {
            Ok(WorkerVolumeRemoval::StillPresent)
        } else {
            Ok(WorkerVolumeRemoval::Removed)
        }
    }
}

/// Result of verifying one exact worker volume.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum WorkerVolumeVerification {
    /// The named volume was confirmed absent by Docker's 404 response.
    Absent,
    /// The named volume exists but does not match the expected worker or role.
    OwnershipMismatch,
    /// Private token for the exact worker volume that passed checks.
    Verified(VerifiedWorkerVolume),
}

/// Result after deleting one previously verified worker volume.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkerVolumeRemoval {
    /// The exact volume was removed and confirmed absent afterwards.
    Removed,
    /// Docker accepted removal, but a follow-up inspect still found the volume.
    StillPresent,
}

pub(crate) async fn create_named_volumes<F, Fut>(
    docker: &Docker,
    worker: &str,
    mounts: &[PlannedMount],
    mut before_create: F,
) -> Result<(), HostError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<(), HostError>>,
{
    let volumes = volumes(worker)?;
    let expected_mounts = mounts_for_dind(worker)?;
    if mounts != expected_mounts
        || volumes
            .iter()
            .any(|volume| !has_mount_contract(volume.role))
    {
        return Err(HostError::ForbiddenMount);
    }
    for volume in &volumes {
        if inspect_volume(docker, &volume.name).await?.is_some() {
            return Err(HostError::Ownership);
        }
    }
    for volume in volumes {
        before_create().await?;
        let created = docker_deadline(docker.create_volume(VolumeCreateRequest {
            name: Some(volume.name.clone()),
            labels: Some(labels(&volume.worker, volume.role.label())),
            ..Default::default()
        }))
        .await?
        .map_err(|_| HostError::Docker)?;
        if !owns(&volume, &created) {
            return Err(HostError::Docker);
        }
    }
    Ok(())
}

pub(crate) async fn remove_worker_volumes(
    docker: &Docker,
    worker: &str,
) -> Result<bool, HostError> {
    for role in WorkerVolumeRole::ALL {
        match verify_worker_volume(docker, worker, role).await? {
            WorkerVolumeVerification::Absent => {}
            WorkerVolumeVerification::OwnershipMismatch => return Ok(false),
            WorkerVolumeVerification::Verified(verified) => {
                if remove_verified_worker_volume(docker, &verified).await?
                    == WorkerVolumeRemoval::StillPresent
                {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

/// Verify exactly the fixed volume for `worker` and `role`.
///
/// Only Docker's confirmed 404 means absent. A verified token is returned only
/// when the name and both ownership labels match the expected identity.
pub(crate) async fn verify_worker_volume(
    docker: &Docker,
    worker: &str,
    role: WorkerVolumeRole,
) -> Result<WorkerVolumeVerification, HostError> {
    if !valid_worker(worker) {
        return Err(HostError::ForbiddenMount);
    }
    let volume = worker_volume(worker, role)?;
    let Some(observed) = inspect_volume(docker, &volume.name).await? else {
        return Ok(WorkerVolumeVerification::Absent);
    };
    if !owns(&volume, &observed) {
        return Ok(WorkerVolumeVerification::OwnershipMismatch);
    }
    Ok(WorkerVolumeVerification::Verified(VerifiedWorkerVolume {
        volume,
    }))
}

/// Remove exactly one volume returned by [`verify_worker_volume`].
///
/// This performs no inspect before deletion so callers can recheck durable
/// cleanup authority after verification and immediately before the delete.
pub(crate) async fn remove_verified_worker_volume(
    docker: &Docker,
    verified: &VerifiedWorkerVolume,
) -> Result<WorkerVolumeRemoval, HostError> {
    verified.remove_request(docker).await?;
    verified.confirm_removed(docker).await
}

async fn inspect_volume(docker: &Docker, name: &str) -> Result<Option<Volume>, HostError> {
    match docker_deadline(docker.inspect_volume(name)).await? {
        Ok(volume) => Ok(Some(volume)),
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(None),
        Err(_) => Err(HostError::Docker),
    }
}

fn volumes(worker: &str) -> Result<Vec<WorkerVolume>, HostError> {
    if !valid_worker(worker) {
        return Err(HostError::ForbiddenMount);
    }
    WorkerVolumeRole::ALL
        .into_iter()
        .map(|role| worker_volume(worker, role))
        .collect()
}

fn valid_worker(worker: &str) -> bool {
    !worker.is_empty()
        && worker
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

fn worker_volume(worker: &str, role: WorkerVolumeRole) -> Result<WorkerVolume, HostError> {
    let names = WorkerIdentityNames::from_private_volume(worker)?;
    Ok(WorkerVolume {
        worker: worker.to_owned(),
        name: names.volume_name(role),
        role,
    })
}

fn labels(worker: &str, role: &str) -> HashMap<String, String> {
    HashMap::from([
        ("velnor.role".to_owned(), role.to_owned()),
        ("velnor.worker".to_owned(), worker.to_owned()),
    ])
}

#[derive(Clone, Copy)]
enum ContainerKind {
    Runner,
    Dind,
}

pub(crate) fn mounts_for_runner(worker: &str) -> Result<Vec<PlannedMount>, HostError> {
    mounts_for(worker, ContainerKind::Runner, false)
}

pub(crate) fn mounts_for_runner_plan(worker: &str) -> Result<Vec<PlannedMount>, HostError> {
    mounts_for(worker, ContainerKind::Runner, true)
}

pub(crate) fn mounts_for_dind(worker: &str) -> Result<Vec<PlannedMount>, HostError> {
    mounts_for(worker, ContainerKind::Dind, false)
}

fn mounts_for(
    worker: &str,
    container: ContainerKind,
    runner_plan: bool,
) -> Result<Vec<PlannedMount>, HostError> {
    if !valid_worker(worker) {
        return Err(HostError::ForbiddenMount);
    }
    let mut mounts = Vec::new();
    for role in WorkerVolumeRole::ALL {
        if runner_plan && role == WorkerVolumeRole::Home {
            continue;
        }
        if let Some(target) = target(role, container) {
            let volume = worker_volume(worker, role)?;
            mounts.push(PlannedMount {
                source: format!("volume:{}", volume.name),
                target,
            });
        }
    }
    Ok(mounts)
}

fn target(role: WorkerVolumeRole, container: ContainerKind) -> Option<String> {
    match (role, container) {
        (WorkerVolumeRole::Socket, _) => Some("/run".to_owned()),
        (WorkerVolumeRole::Home, _) => Some("/home/runner".to_owned()),
        (WorkerVolumeRole::Work, _) => Some(runner_work_path()),
        (WorkerVolumeRole::DindData, ContainerKind::Dind) => Some("/var/lib/docker".to_owned()),
        (WorkerVolumeRole::DindData | WorkerVolumeRole::RunnerMetadata, ContainerKind::Runner) => {
            None
        }
        (WorkerVolumeRole::RunnerMetadata, ContainerKind::Dind) => None,
    }
}

fn has_mount_contract(role: WorkerVolumeRole) -> bool {
    // The ownership inventory includes volumes with no guest mount. A typed publisher
    // must supply metadata through approved subpath mounts.
    match role {
        WorkerVolumeRole::Socket | WorkerVolumeRole::Home | WorkerVolumeRole::Work => {
            target(role, ContainerKind::Runner).is_some()
                && target(role, ContainerKind::Dind).is_some()
        }
        WorkerVolumeRole::DindData => {
            target(role, ContainerKind::Runner).is_none()
                && target(role, ContainerKind::Dind).is_some()
        }
        WorkerVolumeRole::RunnerMetadata => {
            target(role, ContainerKind::Runner).is_none()
                && target(role, ContainerKind::Dind).is_none()
        }
    }
}

/// Only an exact name with exactly the worker and role labels is owned.
///
/// An unlabeled name, a foreign label, a partial label, or an extra label
/// keeps the volume. Removal fails closed toward a leak, never a delete.
fn owns(expected: &WorkerVolume, observed: &Volume) -> bool {
    observed.labels.len() == 2 && labeled(expected, observed)
}

fn labeled(expected: &WorkerVolume, observed: &Volume) -> bool {
    observed.name == expected.name
        && observed.labels.get("velnor.worker").map(String::as_str)
            == Some(expected.worker.as_str())
        && observed.labels.get("velnor.role").map(String::as_str) == Some(expected.role.label())
}

#[cfg(test)]
#[path = "volumes/tests.rs"]
mod tests;
