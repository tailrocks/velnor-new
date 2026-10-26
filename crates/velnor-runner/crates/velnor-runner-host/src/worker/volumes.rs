// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::models::{Volume, VolumeCreateRequest};
use bollard::query_parameters::RemoveVolumeOptions;

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

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct VerifiedWorkerVolume {
    volume: WorkerVolume,
}

impl VerifiedWorkerVolume {
    pub(crate) async fn remove_request(&self, docker: &Docker) -> Result<(), HostError> {
        docker_deadline(docker.remove_volume(&self.volume.name, None::<RemoveVolumeOptions>))
            .await?
            .map_err(|_| HostError::Docker)
    }

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkerVolumeRole {
    Socket,

    Work,

    DindData,
}

impl WorkerVolumeRole {
    const ALL: [Self; 3] = [Self::Socket, Self::Work, Self::DindData];

    fn label(self) -> &'static str {
        match self {
            Self::Socket => "socket",
            Self::Work => "work",
            Self::DindData => "dind-data",
        }
    }

    fn suffix(self) -> &'static str {
        match self {
            Self::Socket => "",
            Self::Work => "-work",
            Self::DindData => "-docker",
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum WorkerVolumeVerification {
    Absent,

    OwnershipMismatch,

    Verified(VerifiedWorkerVolume),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkerVolumeRemoval {
    Removed,

    StillPresent,
}

pub(crate) async fn create_named_volumes(
    docker: &Docker,
    worker: &str,
    mounts: &[PlannedMount],
) -> Result<(), HostError> {
    let volumes = volumes(worker)?;
    if mounts.len() != volumes.len()
        || mounts.iter().zip(&volumes).any(|(mount, volume)| {
            mount.source != format!("volume:{}", volume.name) || target(volume.role) != mount.target
        })
    {
        return Err(HostError::ForbiddenMount);
    }
    for volume in volumes {
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

pub(crate) async fn verify_worker_volume(
    docker: &Docker,
    worker: &str,
    role: WorkerVolumeRole,
) -> Result<WorkerVolumeVerification, HostError> {
    if !valid_worker(worker) {
        return Err(HostError::ForbiddenMount);
    }
    let volume = worker_volume(worker, role);
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

fn volumes(worker: &str) -> Result<[WorkerVolume; 3], HostError> {
    if !valid_worker(worker) {
        return Err(HostError::ForbiddenMount);
    }
    Ok([
        worker_volume(worker, WorkerVolumeRole::Socket),
        worker_volume(worker, WorkerVolumeRole::Work),
        worker_volume(worker, WorkerVolumeRole::DindData),
    ])
}

fn valid_worker(worker: &str) -> bool {
    !worker.is_empty()
        && worker
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

fn worker_volume(worker: &str, role: WorkerVolumeRole) -> WorkerVolume {
    WorkerVolume {
        worker: worker.to_owned(),
        name: format!("{worker}{}", role.suffix()),
        role,
    }
}

fn labels(worker: &str, role: &str) -> HashMap<String, String> {
    HashMap::from([
        ("velnor.role".to_owned(), role.to_owned()),
        ("velnor.worker".to_owned(), worker.to_owned()),
    ])
}

fn target(role: WorkerVolumeRole) -> String {
    match role {
        WorkerVolumeRole::Socket => "/run".to_owned(),
        WorkerVolumeRole::Work => runner_work_path(),
        WorkerVolumeRole::DindData => "/var/lib/docker".to_owned(),
    }
}

fn owns(expected: &WorkerVolume, observed: &Volume) -> bool {
    observed.labels.len() == 2 && labeled(expected, observed)
}

fn labeled(expected: &WorkerVolume, observed: &Volume) -> bool {
    observed.name == expected.name
        && observed.labels.get("velnor.worker").map(String::as_str)
            == Some(expected.worker.as_str())
        && observed.labels.get("velnor.role").map(String::as_str) == Some(expected.role.label())
}
