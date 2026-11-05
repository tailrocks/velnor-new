// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::time::{Duration, Instant};

use bollard::Docker;
use bollard::models::{ContainerCreateBody, HostConfig, VolumeCreateRequest};
use bollard::query_parameters::StartContainerOptions;

use crate::error::HostError;

use super::mutation_lifecycle::{
    self, MutationKind, MutationResult, MutationValue, PendingMutation,
};
use super::{
    QUALIFIER_LABEL, create_options, deadline, mount, remaining, valid_container_id,
    validate_host_config, volume_owned,
};

const DOCKER_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) struct OwnedResources {
    pub(super) run_id: String,
    pub(super) containers: Vec<TrackedContainer>,
    pub(super) volumes: Vec<String>,
    pub(super) pending_mutations: Vec<PendingMutation>,
    next_container: u8,
}

pub(super) struct TrackedContainer {
    pub(super) name: String,
    pub(super) role: String,
    pub(super) id: Option<String>,
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
            pending_mutations: Vec::new(),
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
        let request = VolumeCreateRequest {
            name: Some(name.clone()),
            labels: Some(self.labels(role)),
            ..Default::default()
        };
        let client = docker.clone();
        let expected_name = name.clone();
        let expected_role = role.to_owned();
        let run_id = self.run_id.clone();
        let mutation = mutation_lifecycle::spawn(
            format!("volume {name}"),
            MutationKind::Volume(name.clone()),
            async move {
                let created = match client.create_volume(request).await {
                    Ok(created) => created,
                    Err(error) => return mutation_lifecycle::uncertain_docker(&error),
                };
                if volume_owned(&created, &expected_name, &run_id, &expected_role) {
                    MutationResult::Created(MutationValue::Volume)
                } else {
                    mutation_lifecycle::uncertain(HostError::Ownership)
                }
            },
        );
        if !matches!(
            self.await_mutation(mutation, DOCKER_TIMEOUT).await?,
            MutationValue::Volume
        ) {
            return Err(HostError::Identity);
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
        let dind_deadline = Instant::now() + Duration::from_secs(10);
        let (name, id) = self
            .create_dind(docker, image_id, socket_volume, data_volume, dind_deadline)
            .await?;
        self.start_and_verify_dind(
            docker,
            image_id,
            &[socket_volume, data_volume],
            &name,
            &id,
            dind_deadline,
        )
        .await?;
        Ok(id)
    }

    async fn create_dind(
        &mut self,
        docker: &Docker,
        image_id: &str,
        socket_volume: &str,
        data_volume: &str,
        dind_deadline: Instant,
    ) -> Result<(String, String), HostError> {
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
        let client = docker.clone();
        let options = create_options(&name);
        let create_timeout = remaining(dind_deadline)?;
        let mutation = mutation_lifecycle::spawn(
            format!("container {name}"),
            MutationKind::Container(name.clone()),
            async move {
                let created = match client.create_container(Some(options), config).await {
                    Ok(created) => created,
                    Err(error) => return mutation_lifecycle::uncertain_docker(&error),
                };
                if valid_container_id(&created.id).is_err() {
                    return mutation_lifecycle::uncertain(HostError::Identity);
                }
                MutationResult::Created(MutationValue::ContainerId(created.id))
            },
        );
        let id = match self.await_mutation(mutation, create_timeout).await? {
            MutationValue::ContainerId(id) => id,
            MutationValue::Volume => return Err(HostError::Identity),
        };
        self.bind_container_id(&name, &id)?;
        Ok((name, id))
    }

    async fn start_and_verify_dind(
        &self,
        docker: &Docker,
        image_id: &str,
        volumes: &[&str],
        name: &str,
        id: &str,
        dind_deadline: Instant,
    ) -> Result<(), HostError> {
        let inspect = deadline(
            docker.inspect_container(id, None),
            remaining(dind_deadline)?,
        )
        .await?
        .map_err(|_| HostError::Docker)?;
        self.validate_inspect(&inspect, name, id, "dind", image_id, volumes)?;
        deadline(
            docker.start_container(id, None::<StartContainerOptions>),
            remaining(dind_deadline)?,
        )
        .await?
        .map_err(|_| HostError::Docker)?;
        let inspect = deadline(
            docker.inspect_container(id, None),
            remaining(dind_deadline)?,
        )
        .await?
        .map_err(|_| HostError::Docker)?;
        self.validate_inspect(&inspect, name, id, "dind", image_id, volumes)?;
        if inspect.state.and_then(|state| state.running) != Some(true) {
            return Err(HostError::Docker);
        }
        Ok(())
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
}
