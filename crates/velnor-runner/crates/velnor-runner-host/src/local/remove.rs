// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::query_parameters::{
    ListContainersOptions, ListVolumesOptions, RemoveContainerOptions, RemoveVolumeOptions,
};

use crate::docker_client::docker_deadline;
use crate::error::HostError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Removed {
    pub containers: usize,
    pub volumes: usize,
}

pub(super) async fn remove_labelled(
    docker: &Docker,
    labels: &[String],
) -> Result<Removed, HostError> {
    if labels.is_empty() || labels.iter().any(|label| label.is_empty()) {
        return Err(HostError::Ownership);
    }
    let filters = HashMap::from([("label".to_owned(), labels.to_vec())]);
    let containers = docker_deadline(docker.list_containers(Some(ListContainersOptions {
        all: true,
        filters: Some(filters.clone()),
        ..Default::default()
    })))
    .await?
    .map_err(|_| HostError::Docker)?;
    let mut removed = Removed {
        containers: 0,
        volumes: 0,
    };
    for id in containers.into_iter().filter_map(|container| container.id) {
        remove_container(docker, &id).await?;
        removed.containers += 1;
    }
    let volumes = docker_deadline(docker.list_volumes(Some(ListVolumesOptions {
        filters: Some(filters),
    })))
    .await?
    .map_err(|_| HostError::Docker)?;
    for volume in volumes.volumes.unwrap_or_default() {
        match docker_deadline(docker.remove_volume(&volume.name, None::<RemoveVolumeOptions>))
            .await?
        {
            Ok(()) => removed.volumes += 1,
            Err(DockerError::DockerResponseServerError {
                status_code: 404, ..
            }) => {}
            Err(_) => return Err(HostError::Docker),
        }
    }
    Ok(removed)
}

pub(super) async fn remove_container(docker: &Docker, id: &str) -> Result<(), HostError> {
    let options = RemoveContainerOptions {
        force: true,
        ..Default::default()
    };
    match docker_deadline(docker.remove_container(id, Some(options))).await? {
        Ok(())
        | Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(()),
        Err(_) => Err(HostError::Docker),
    }
}

pub(super) async fn labelled_agents(
    docker: &Docker,
    label: &str,
) -> Result<Vec<(String, String)>, HostError> {
    let filters = HashMap::from([("label".to_owned(), vec![label.to_owned()])]);
    let containers = docker_deadline(docker.list_containers(Some(ListContainersOptions {
        all: true,
        filters: Some(filters),
        ..Default::default()
    })))
    .await?
    .map_err(|_| HostError::Docker)?;
    let mut agents: Vec<(String, String)> = containers
        .into_iter()
        .filter_map(|container| {
            let labels = container.labels?;
            Some((
                labels.get(super::pair::SIDE_LABEL)?.clone(),
                labels.get(super::pair::AGENT_LABEL)?.clone(),
            ))
        })
        .collect();
    agents.sort();
    agents.dedup();
    Ok(agents)
}

#[cfg(test)]
#[path = "remove_tests.rs"]
mod tests;
