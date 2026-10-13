//! Named volumes belong to one durable worker identity.

use std::collections::HashMap;

use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::models::{Volume, VolumeCreateRequest};
use bollard::query_parameters::RemoveVolumeOptions;

use crate::docker_client::docker_deadline;
use crate::docker_spec::Mount as PlannedMount;
use crate::error::HostError;

#[derive(Clone)]
struct WorkerVolume {
    name: String,
    role: &'static str,
}

pub(crate) async fn create_named_volumes(
    docker: &Docker,
    worker: &str,
    mounts: &[PlannedMount],
) -> Result<(), HostError> {
    let volumes = volumes(worker)?;
    if mounts.len() != volumes.len()
        || mounts.iter().zip(&volumes).any(|(mount, volume)| {
            mount.source != format!("volume:{}", volume.name) || mount.target != target(volume.role)
        })
    {
        return Err(HostError::ForbiddenMount);
    }
    for volume in volumes {
        let created = docker_deadline(docker.create_volume(VolumeCreateRequest {
            name: Some(volume.name.clone()),
            labels: Some(labels(worker, volume.role)),
            ..Default::default()
        }))
        .await?
        .map_err(|_| HostError::Docker)?;
        if !owns(worker, &volume, &created) {
            return Err(HostError::Docker);
        }
    }
    Ok(())
}

pub(crate) async fn remove_worker_volumes(
    docker: &Docker,
    worker: &str,
) -> Result<bool, HostError> {
    for volume in volumes(worker)? {
        let Some(observed) = inspect_volume(docker, &volume.name).await? else {
            continue;
        };
        if !releasable(worker, &volume, &observed) {
            return Ok(false);
        }
        docker_deadline(docker.remove_volume(&volume.name, None::<RemoveVolumeOptions>))
            .await?
            .map_err(|_| HostError::Docker)?;
        if inspect_volume(docker, &volume.name).await?.is_some() {
            return Ok(false);
        }
    }
    Ok(true)
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
    if worker.is_empty()
        || !worker
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
    {
        return Err(HostError::ForbiddenMount);
    }
    Ok([
        WorkerVolume {
            name: worker.to_owned(),
            role: "socket",
        },
        WorkerVolume {
            name: format!("{worker}-work"),
            role: "work",
        },
        WorkerVolume {
            name: format!("{worker}-docker"),
            role: "dind-data",
        },
    ])
}

fn labels(worker: &str, role: &str) -> HashMap<String, String> {
    HashMap::from([
        ("velnor.role".to_owned(), role.to_owned()),
        ("velnor.worker".to_owned(), worker.to_owned()),
    ])
}

fn target(role: &str) -> &'static str {
    match role {
        "socket" => "/run",
        "work" => "/home/runner/work",
        "dind-data" => "/var/lib/docker",
        _ => "",
    }
}

fn owns(worker: &str, expected: &WorkerVolume, observed: &Volume) -> bool {
    labeled(worker, expected, observed)
}

/// An exact name with no labels belongs to this worker.
/// A foreign label or a partial label keeps the volume.
fn releasable(worker: &str, expected: &WorkerVolume, observed: &Volume) -> bool {
    if observed.name != expected.name {
        return false;
    }
    if observed.labels.is_empty() {
        return true;
    }
    observed.labels.len() == 2 && labeled(worker, expected, observed)
}

fn labeled(worker: &str, expected: &WorkerVolume, observed: &Volume) -> bool {
    observed.name == expected.name
        && observed.labels.get("velnor.worker").map(String::as_str) == Some(worker)
        && observed.labels.get("velnor.role").map(String::as_str) == Some(expected.role)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use bollard::models::Volume;

    use super::{WorkerVolume, releasable};

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
            name: "w135-work".to_owned(),
            role: "work",
        }
    }

    #[test]
    fn unlabeled_exact_name_is_releasable() {
        let volume = sample("w135-work", &[]);
        assert!(releasable("w135", &expected(), &volume));
    }

    #[test]
    fn exact_labels_are_releasable() {
        let volume = sample(
            "w135-work",
            &[("velnor.worker", "w135"), ("velnor.role", "work")],
        );
        assert!(releasable("w135", &expected(), &volume));
    }

    #[test]
    fn foreign_worker_label_is_kept() {
        let volume = sample(
            "w135-work",
            &[("velnor.worker", "other"), ("velnor.role", "work")],
        );
        assert!(!releasable("w135", &expected(), &volume));
    }

    #[test]
    fn partial_label_is_kept() {
        let volume = sample("w135-work", &[("velnor.worker", "w135")]);
        assert!(!releasable("w135", &expected(), &volume));
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
        assert!(!releasable("w135", &expected(), &volume));
    }

    #[test]
    fn wrong_name_is_kept() {
        let volume = sample("other", &[]);
        assert!(!releasable("w135", &expected(), &volume));
    }
}
