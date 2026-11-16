// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::path::Path;

use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::query_parameters::{
    ListContainersOptions, ListVolumesOptions, RemoveContainerOptions, RemoveVolumeOptions,
};

use super::cleanup_audit::Audit;
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
    let filters = HashMap::from([("label".to_owned(), labels.to_vec())]);
    remove_matching(docker, filters, None).await
}

pub(super) async fn remove_audited(
    docker: &Docker,
    labels: &[String],
    path: &Path,
) -> Result<Removed, HostError> {
    let filters = HashMap::from([("label".to_owned(), labels.to_vec())]);
    let mut audit = Audit::open(path, filters.clone()).await?;
    remove_matching(docker, filters, Some(&mut audit)).await
}

async fn remove_matching(
    docker: &Docker,
    filters: HashMap<String, Vec<String>>,
    mut audit: Option<&mut Audit>,
) -> Result<Removed, HostError> {
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
        if let Some(audit) = audit.as_mut() {
            audit.record("intent", Some(("container", &id))).await?;
        }
        let present = remove_container_present(docker, &id).await?;
        if let Some(audit) = audit.as_mut() {
            audit
                .record(
                    if present { "removed" } else { "absent" },
                    Some(("container", &id)),
                )
                .await?;
        }
        removed.containers += usize::from(present);
    }
    let volumes = docker_deadline(docker.list_volumes(Some(ListVolumesOptions {
        filters: Some(filters),
    })))
    .await?
    .map_err(|_| HostError::Docker)?;
    for volume in volumes.volumes.unwrap_or_default() {
        if let Some(audit) = audit.as_mut() {
            audit
                .record("intent", Some(("volume", &volume.name)))
                .await?;
        }
        let present =
            match docker_deadline(docker.remove_volume(&volume.name, None::<RemoveVolumeOptions>))
                .await?
            {
                Ok(()) => true,
                Err(DockerError::DockerResponseServerError {
                    status_code: 404, ..
                }) => false,
                Err(_) => return Err(HostError::Docker),
            };
        if let Some(audit) = audit.as_mut() {
            audit
                .record(
                    if present { "removed" } else { "absent" },
                    Some(("volume", &volume.name)),
                )
                .await?;
        }
        removed.volumes += usize::from(present);
    }
    Ok(removed)
}

pub(super) async fn remove_container(docker: &Docker, id: &str) -> Result<(), HostError> {
    remove_container_present(docker, id).await.map(|_| ())
}

async fn remove_container_present(docker: &Docker, id: &str) -> Result<bool, HostError> {
    let options = RemoveContainerOptions {
        force: true,
        ..Default::default()
    };
    match docker_deadline(docker.remove_container(id, Some(options))).await? {
        Ok(()) => Ok(true),
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(false),
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
