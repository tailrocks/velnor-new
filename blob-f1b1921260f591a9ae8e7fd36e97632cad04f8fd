//! Named volumes belong to one durable worker identity.

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

/// Opaque proof that one exact volume was verified for its worker and role.
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

/// One of the fixed volumes attached to a worker pair.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkerVolumeRole {
    /// Runner socket directory mounted at `/run`.
    Socket,
    /// Actions runner workspace mounted at `_work`.
    Work,
    /// Private Docker-in-Docker data mounted at `/var/lib/docker`.
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

/// Result of verifying one exact worker volume.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum WorkerVolumeVerification {
    /// The named volume was confirmed absent by Docker's 404 response.
    Absent,
    /// The named volume exists but does not match the expected worker or role.
    OwnershipMismatch,
    /// Opaque token for the exact verified worker volume.
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
mod tests {
    use std::collections::HashMap;

    use bollard::models::Volume;

    use super::{WorkerVolume, WorkerVolumeRole, owns};

    fn sample(name: &str, labels: &[(&str, &str)]) -> Volume {
        Volume {
            name: name.to_owned(),
            driver: "local".to_owned(),
            mountpoint: "/var/lib/docker/volumes/test".to_owned(),
            created_at: None,
            status: None,
            labels: labels
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
            scope: None,
            cluster_volume: None,
            options: HashMap::new(),
            usage_data: None,
        }
    }

    fn expected() -> WorkerVolume {
        WorkerVolume {
            worker: "w135".to_owned(),
            name: "w135-work".to_owned(),
            role: WorkerVolumeRole::Work,
        }
    }

    #[test]
    fn unlabeled_exact_name_is_kept() {
        let volume = sample("w135-work", &[]);
        assert!(!owns(&expected(), &volume));
    }

    #[test]
    fn exact_labels_are_owned() {
        let volume = sample(
            "w135-work",
            &[("velnor.worker", "w135"), ("velnor.role", "work")],
        );
        assert!(owns(&expected(), &volume));
    }

    #[test]
    fn foreign_worker_label_is_kept() {
        let volume = sample(
            "w135-work",
            &[("velnor.worker", "other"), ("velnor.role", "work")],
        );
        assert!(!owns(&expected(), &volume));
    }

    #[test]
    fn partial_label_is_kept() {
        let volume = sample("w135-work", &[("velnor.worker", "w135")]);
        assert!(!owns(&expected(), &volume));
    }

    #[test]
    fn extra_label_is_kept() {
        let volume = sample(
            "w135-work",
            &[
                ("velnor.worker", "w135"),
                ("velnor.role", "work"),
                ("other", "1"),
            ],
        );
        assert!(!owns(&expected(), &volume));
    }

    #[test]
    fn wrong_name_is_kept() {
        let volume = sample("other", &[]);
        assert!(!owns(&expected(), &volume));
    }
}
