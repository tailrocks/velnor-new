// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::time::{Duration, Instant};

use bollard::Docker;
use bollard::models::{ContainerCreateBody, HostConfig, VolumeCreateRequest};
use bollard::query_parameters::{
    RemoveContainerOptionsBuilder, RemoveVolumeOptionsBuilder, StartContainerOptions,
};

use crate::error::HostError;

use super::{
    QUALIFIER_LABEL, create_options, deadline, missing, mount, remaining, valid_container_id,
    validate_host_config, volume_owned,
};

const DOCKER_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) struct OwnedResources {
    run_id: String,
    containers: Vec<TrackedContainer>,
    volumes: Vec<String>,
    next_container: u8,
}

struct TrackedContainer {
    name: String,
    role: String,
    id: Option<String>,
}

impl OwnedResources {
    pub(super) fn new() -> Result<Self, HostError> {
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| HostError::Identity)?;
        let mut run_id = String::with_capacity(32);
        for byte in bytes {
            use std::fmt::Write;
            write!(&mut run_id, "{byte:02x}").map_err(|_| HostError::Identity)?;
        }
        Ok(Self {
            run_id,
            containers: Vec::new(),
            volumes: Vec::new(),
            next_container: 0,
        })
    }

    pub(super) fn socket_volume(&self) -> String {
        format!("velnor-qualify-socket-{}", self.run_id)
    }

    pub(super) fn data_volume(&self) -> String {
        format!("velnor-qualify-data-{}", self.run_id)
    }

    pub(super) async fn create_volume(
        &mut self,
        docker: &Docker,
        role: &str,
    ) -> Result<String, HostError> {
        let name = match role {
            "socket" => self.socket_volume(),
            "data" => self.data_volume(),
            _ => return Err(HostError::Identity),
        };
        self.volumes.push(name.clone());
        let created = deadline(
            docker.create_volume(VolumeCreateRequest {
                name: Some(name.clone()),
                labels: Some(self.labels(role)),
                ..Default::default()
            }),
            DOCKER_TIMEOUT,
        )
        .await?
        .map_err(|_| HostError::Docker)?;
        if !volume_owned(&created, &name, &self.run_id, role) {
            return Err(HostError::Ownership);
        }
        let inspected = deadline(docker.inspect_volume(&name), DOCKER_TIMEOUT)
            .await?
            .map_err(|_| HostError::Docker)?;
        if !volume_owned(&inspected, &name, &self.run_id, role) {
            return Err(HostError::Ownership);
        }
        Ok(name)
    }

    pub(super) async fn start_dind(
        &mut self,
        docker: &Docker,
        image_id: &str,
        socket_volume: &str,
        data_volume: &str,
    ) -> Result<String, HostError> {
        let name = self.register_container("dind")?;
        let mounts = vec![
            mount(socket_volume, "/run"),
            mount(data_volume, "/var/lib/docker"),
        ];
        let config = ContainerCreateBody {
            image: Some(image_id.to_owned()),
            labels: Some(self.labels("dind")),
            host_config: Some(HostConfig {
                privileged: Some(true),
                network_mode: Some("none".to_owned()),
                port_bindings: None,
                mounts: Some(mounts),
                auto_remove: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };
        let dind_deadline = Instant::now() + Duration::from_secs(10);
        let create = deadline(
            docker.create_container(Some(create_options(&name)), config),
            remaining(dind_deadline)?,
        )
        .await?
        .map_err(|_| HostError::Docker)?;
        self.bind_container_id(&name, &create.id)?;
        let inspect = deadline(
            docker.inspect_container(&create.id, None),
            remaining(dind_deadline)?,
        )
        .await?
        .map_err(|_| HostError::Docker)?;
        self.validate_inspect(
            &inspect,
            &name,
            &create.id,
            "dind",
            image_id,
            &[socket_volume, data_volume],
        )?;
        deadline(
            docker.start_container(&create.id, None::<StartContainerOptions>),
            remaining(dind_deadline)?,
        )
        .await?
        .map_err(|_| HostError::Docker)?;
        let inspect = deadline(
            docker.inspect_container(&create.id, None),
            remaining(dind_deadline)?,
        )
        .await?
        .map_err(|_| HostError::Docker)?;
        self.validate_inspect(
            &inspect,
            &name,
            &create.id,
            "dind",
            image_id,
            &[socket_volume, data_volume],
        )?;
        if inspect.state.and_then(|state| state.running) != Some(true) {
            return Err(HostError::Docker);
        }
        Ok(create.id)
    }

    pub(super) fn register_container(&mut self, role: &str) -> Result<String, HostError> {
        let name = self.next_name(role)?;
        self.containers.push(TrackedContainer {
            name: name.clone(),
            role: role.to_owned(),
            id: None,
        });
        Ok(name)
    }

    pub(super) fn bind_container_id(&mut self, name: &str, id: &str) -> Result<(), HostError> {
        valid_container_id(id)?;
        let tracked = self
            .containers
            .iter_mut()
            .find(|container| container.name == name)
            .ok_or(HostError::Ownership)?;
        if tracked.id.as_deref().is_some_and(|recorded| recorded != id) {
            return Err(HostError::Ownership);
        }
        tracked.id = Some(id.to_owned());
        Ok(())
    }

    pub(super) fn validate_inspect(
        &self,
        inspect: &bollard::models::ContainerInspectResponse,
        name: &str,
        id: &str,
        role: &str,
        image_id: &str,
        volumes: &[&str],
    ) -> Result<(), HostError> {
        if inspect.id.as_deref() != Some(id)
            || inspect
                .name
                .as_deref()
                .is_none_or(|value| value.strip_prefix('/').unwrap_or(value) != name)
            || inspect.image.as_deref() != Some(image_id)
        {
            return Err(HostError::Ownership);
        }
        let labels = inspect
            .config
            .as_ref()
            .and_then(|config| config.labels.as_ref())
            .ok_or(HostError::Ownership)?;
        if labels.get(QUALIFIER_LABEL).map(String::as_str) != Some(self.run_id.as_str())
            || labels
                .get("io.velnor.runner-image-qualifier.role")
                .map(String::as_str)
                != Some(role)
        {
            return Err(HostError::Ownership);
        }
        validate_host_config(inspect, role, volumes)?;
        Ok(())
    }

    fn next_name(&mut self, role: &str) -> Result<String, HostError> {
        if role.is_empty()
            || !role
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
        {
            return Err(HostError::Identity);
        }
        self.next_container = self
            .next_container
            .checked_add(1)
            .ok_or(HostError::Identity)?;
        Ok(format!(
            "velnor-qualify-{role}-{}-{}",
            self.run_id, self.next_container
        ))
    }

    pub(super) fn labels(&self, role: &str) -> HashMap<String, String> {
        HashMap::from([
            (QUALIFIER_LABEL.to_owned(), self.run_id.clone()),
            (
                "io.velnor.runner-image-qualifier.role".to_owned(),
                role.to_owned(),
            ),
        ])
    }

    pub(super) async fn cleanup(&mut self, docker: &Docker) -> Result<(), HostError> {
        let mut failed = false;
        for container in self.containers.iter().rev() {
            if self.cleanup_container(docker, container).await.is_err() {
                failed = true;
            }
        }
        for name in self.volumes.iter().rev() {
            if self.cleanup_volume(docker, name).await.is_err() {
                failed = true;
            }
        }
        if failed {
            Err(HostError::Cleanup)
        } else {
            Ok(())
        }
    }

    async fn cleanup_container(
        &self,
        docker: &Docker,
        container: &TrackedContainer,
    ) -> Result<(), HostError> {
        let inspect = match deadline(
            docker.inspect_container(&container.name, None),
            Duration::from_secs(3),
        )
        .await?
        {
            Ok(inspect) => inspect,
            Err(error) if missing(&error) => return Ok(()),
            Err(_) => return Err(HostError::Docker),
        };
        let id = self.validate_ownership(&inspect, container)?;
        let options = RemoveContainerOptionsBuilder::new()
            .force(true)
            .v(false)
            .build();
        let removed = deadline(
            docker.remove_container(id, Some(options)),
            Duration::from_secs(3),
        )
        .await?;
        match removed {
            Ok(()) => {}
            Err(error) if missing(&error) => {}
            Err(_) => return Err(HostError::Cleanup),
        }
        match deadline(
            docker.inspect_container(&container.name, None),
            Duration::from_secs(3),
        )
        .await?
        {
            Err(error) if missing(&error) => Ok(()),
            _ => Err(HostError::Cleanup),
        }
    }

    fn validate_ownership<'a>(
        &self,
        inspect: &'a bollard::models::ContainerInspectResponse,
        tracked: &TrackedContainer,
    ) -> Result<&'a str, HostError> {
        let id = inspect.id.as_deref().ok_or(HostError::Ownership)?;
        valid_container_id(id)?;
        if tracked.id.as_deref().is_some_and(|recorded| recorded != id)
            || inspect
                .name
                .as_deref()
                .is_none_or(|name| name.strip_prefix('/').unwrap_or(name) != tracked.name)
        {
            return Err(HostError::Ownership);
        }
        let labels = inspect
            .config
            .as_ref()
            .and_then(|config| config.labels.as_ref())
            .ok_or(HostError::Ownership)?;
        if labels.get(QUALIFIER_LABEL).map(String::as_str) != Some(self.run_id.as_str())
            || labels
                .get("io.velnor.runner-image-qualifier.role")
                .map(String::as_str)
                != Some(tracked.role.as_str())
        {
            return Err(HostError::Ownership);
        }
        Ok(id)
    }

    async fn cleanup_volume(&self, docker: &Docker, name: &str) -> Result<(), HostError> {
        let volume = match deadline(docker.inspect_volume(name), Duration::from_secs(3)).await? {
            Ok(volume) => volume,
            Err(error) if missing(&error) => return Ok(()),
            Err(_) => return Err(HostError::Docker),
        };
        let role = if name == self.socket_volume() {
            "socket"
        } else if name == self.data_volume() {
            "data"
        } else {
            return Err(HostError::Ownership);
        };
        if !volume_owned(&volume, name, &self.run_id, role) {
            return Err(HostError::Ownership);
        }
        let options = RemoveVolumeOptionsBuilder::new().force(false).build();
        let removed = deadline(
            docker.remove_volume(name, Some(options)),
            Duration::from_secs(3),
        )
        .await?;
        match removed {
            Ok(()) => {}
            Err(error) if missing(&error) => {}
            Err(_) => return Err(HostError::Cleanup),
        }
        match deadline(docker.inspect_volume(name), Duration::from_secs(3)).await? {
            Err(error) if missing(&error) => Ok(()),
            _ => Err(HostError::Cleanup),
        }
    }
}
