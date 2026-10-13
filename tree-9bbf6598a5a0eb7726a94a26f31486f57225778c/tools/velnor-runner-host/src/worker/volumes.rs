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
    // Check all known roles so recovery can remove both the legacy and Linux
    // profile layouts without scanning or deleting unrelated volumes.
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

fn volume_catalog(worker: &str) -> Result<[WorkerVolume; 6], HostError> {
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
        WorkerVolume {
            name: format!("{worker}-home"),
            role: "home-state",
        },
        WorkerVolume {
            name: format!("{worker}-tmp"),
            role: "runner-temp",
        },
    ])
}

fn volumes_for_mounts(
    worker: &str,
    mounts: &[PlannedMount],
) -> Result<Vec<WorkerVolume>, HostError> {
    let all = volume_catalog(worker)?;
    let socket = volume_for_role(&all, "socket")?;
    let work = volume_for_role(&all, "work")?;
    let externals = volume_for_role(&all, "externals")?;
    let dind_data = volume_for_role(&all, "dind-data")?;
    let home = volume_for_role(&all, "home-state")?;
    let temp = volume_for_role(&all, "runner-temp")?;
    let legacy = vec![
        volume_mount(socket, "/run", false),
        volume_mount(work, &runner_work_path(), false),
        volume_mount(dind_data, "/var/lib/docker", false),
    ];
    if mounts == legacy {
        return Ok(vec![socket.clone(), work.clone(), dind_data.clone()]);
    }

    let mut official = vec![
        volume_mount(home, "/home/runner", false),
        volume_mount(work, &runner_work_path(), false),
        volume_mount(externals, "/home/runner/externals", true),
        volume_mount(socket, "/run/docker", false),
        volume_mount(temp, "/tmp", false),
    ];
    official.extend([
        volume_mount(socket, "/var/run", false),
        volume_mount(work, &runner_work_path(), false),
        volume_mount(externals, "/home/runner/externals", true),
        volume_mount(dind_data, "/var/lib/docker", false),
    ]);
    if mounts == official {
        Ok(all.to_vec())
    } else {
        Err(HostError::ForbiddenMount)
    }
}

fn volume_for_role<'a>(
    volumes: &'a [WorkerVolume; 6],
    role: &'static str,
) -> Result<&'a WorkerVolume, HostError> {
    let mut matches = volumes.iter().filter(|volume| volume.role == role);
    let volume = matches.next().ok_or(HostError::ForbiddenMount)?;
    if matches.next().is_some() {
        return Err(HostError::ForbiddenMount);
    }
    Ok(volume)
}

fn volume_mount(volume: &WorkerVolume, target: &str, read_only: bool) -> PlannedMount {
    PlannedMount {
        source: format!("volume:{}", volume.name),
        target: target.to_owned(),
        read_only,
    }
}

fn labels(worker: &str, role: &str) -> HashMap<String, String> {
    HashMap::from([
        ("velnor.role".to_owned(), role.to_owned()),
        ("velnor.worker".to_owned(), worker.to_owned()),
    ])
}

fn owns(worker: &str, expected: &WorkerVolume, observed: &Volume) -> bool {
    observed.name == expected.name
        && observed.labels.get("velnor.worker").map(String::as_str) == Some(worker)
        && observed.labels.get("velnor.role").map(String::as_str) == Some(expected.role)
}

#[cfg(test)]
mod tests;
