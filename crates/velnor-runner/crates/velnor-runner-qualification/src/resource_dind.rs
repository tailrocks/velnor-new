// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use bollard::Docker;
use bollard::models::{ContainerCreateBody, HostConfig};
use bollard::query_parameters::StartContainerOptions;

use crate::error::QualificationError as HostError;
use crate::mutation_lifecycle::{self, MutationKind, MutationResult, MutationValue};
use crate::resources::OwnedResources;
use crate::{create_options, deadline, mount, remaining, valid_container_id};

impl OwnedResources {
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
}
