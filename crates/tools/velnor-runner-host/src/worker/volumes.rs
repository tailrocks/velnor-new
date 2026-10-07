//! Named volumes belong to one durable worker identity.

use std::collections::HashMap;

use ::bollard::Docker;
use ::bollard::errors::Error as DockerError;
use ::bollard::models::{Volume, VolumeCreateRequest};
use ::bollard::query_parameters::RemoveVolumeOptions;

use crate::HostError;
use crate::docker_client::docker_deadline;
use crate::docker_spec::Mount as PlannedMount;
use velnor_runner_core::runner_work_path;

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
    let volumes = volumes_for_mounts(worker, mounts)?;
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
    // Check all known roles so recovery can remove either the legacy three-volume
    // layout or the Linux profile's additional shared externals volume.
    for volume in volume_catalog(worker)? {
        let Some(observed) = inspect_volume(docker, &volume.name).await? else {
            continue;
        };
        if !owns(worker, &volume, &observed) {
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

fn volume_catalog(worker: &str) -> Result<[WorkerVolume; 4], HostError> {
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
            name: format!("{worker}-externals"),
            role: "externals",
        },
        WorkerVolume {
            name: format!("{worker}-docker"),
            role: "dind-data",
        },
    ])
}

fn volumes_for_mounts(
    worker: &str,
    mounts: &[PlannedMount],
) -> Result<Vec<WorkerVolume>, HostError> {
    let all = volume_catalog(worker)?;
    let legacy = [&all[0], &all[1], &all[3]];
    let official = [&all[0], &all[1], &all[2], &all[3]];
    let expected_mounts = |volumes: &[&WorkerVolume],
                           target_for_role: fn(&str) -> Option<String>|
     -> Result<Vec<PlannedMount>, HostError> {
        volumes
            .iter()
            .map(|volume| {
                Ok(PlannedMount {
                    source: format!("volume:{}", volume.name),
                    target: target_for_role(volume.role).ok_or(HostError::ForbiddenMount)?,
                })
            })
            .collect()
    };
    if mounts == expected_mounts(&legacy, target)? {
        Ok(legacy.into_iter().cloned().collect())
    } else if mounts == expected_mounts(&official, official_target)? {
        Ok(official.into_iter().cloned().collect())
    } else {
        Err(HostError::ForbiddenMount)
    }
}

fn labels(worker: &str, role: &str) -> HashMap<String, String> {
    HashMap::from([
        ("velnor.role".to_owned(), role.to_owned()),
        ("velnor.worker".to_owned(), worker.to_owned()),
    ])
}

fn target(role: &str) -> Option<String> {
    match role {
        "socket" => Some("/run".to_owned()),
        "work" => Some(runner_work_path()),
        "externals" => Some("/home/runner/externals".to_owned()),
        "dind-data" => Some("/var/lib/docker".to_owned()),
        _ => None,
    }
}

fn official_target(role: &str) -> Option<String> {
    match role {
        // DinD gets the shared private socket volume at /var/run, where its
        // socket is /var/run/docker.sock as required by Runner.Worker actions.
        "socket" => Some("/var/run".to_owned()),
        _ => target(role),
    }
}

fn owns(worker: &str, expected: &WorkerVolume, observed: &Volume) -> bool {
    observed.name == expected.name
        && observed.labels.get("velnor.worker").map(String::as_str) == Some(worker)
        && observed.labels.get("velnor.role").map(String::as_str) == Some(expected.role)
}

#[cfg(test)]
mod tests;
