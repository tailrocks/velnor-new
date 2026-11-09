// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use crate::journal::{Journal, State};
use crate::probes::DaemonIdentity;
use serde::{Deserialize, Serialize};

use bollard::Docker;
use bollard::models::VolumeCreateRequest;

use crate::error::QualificationError as HostError;

use super::mutation_lifecycle::{
    self, MutationKind, MutationResult, MutationValue, PendingMutation,
};
use super::{QUALIFIER_LABEL, deadline, valid_container_id, validate_host_config, volume_owned};

const DOCKER_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) struct OwnedResources {
    pub(super) run_id: String,
    pub(super) containers: Vec<TrackedContainer>,
    pub(super) volumes: Vec<String>,
    pub(super) pending_mutations: Vec<PendingMutation>,
    healthchecks: HashMap<String, Option<bollard::models::HealthConfig>>,
    next_container: u8,
    pub(super) journal: Option<Journal>,
    pub(super) daemon: Option<DaemonIdentity>,
    pub(super) settled: Vec<String>,
    pub(super) deleting: Vec<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TrackedContainer {
    pub(super) name: String,
    pub(super) role: String,
    pub(super) id: Option<String>,
}

impl OwnedResources {
    pub(super) fn new(
        images: &[(&str, Option<bollard::models::HealthConfig>)],
    ) -> Result<Self, HostError> {
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
            healthchecks: images
                .iter()
                .map(|(id, health)| ((*id).to_owned(), health.clone()))
                .collect(),
            next_container: 0,
            journal: None,
            daemon: None,
            settled: Vec::new(),
            deleting: Vec::new(),
        })
    }

    pub(super) fn journaled(
        images: &[(&str, Option<bollard::models::HealthConfig>)],
        path: &Path,
        daemon: &DaemonIdentity,
    ) -> Result<Self, HostError> {
        let (journal, state) = Journal::open(path, daemon)?;
        let mut resources = Self::new(images)?;
        resources.journal = Some(journal);
        resources.daemon = Some(daemon.clone());
        if let Some(state) = state {
            if state.run_id.len() != 32
                || !state
                    .run_id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(HostError::Ownership);
            }
            resources.run_id = state.run_id;
            resources.containers = state.containers;
            resources.volumes = state.volumes;
            resources.settled = state.settled;
            resources.deleting = state.deleting;
            for container in &resources.containers {
                if !resources.settled.contains(&container.name) {
                    resources.pending_mutations.push(PendingMutation::recovered(
                        MutationKind::Container(container.name.clone()),
                    ));
                }
            }
            for volume in &resources.volumes {
                if !resources.settled.contains(volume) {
                    resources.pending_mutations.push(PendingMutation::recovered(
                        MutationKind::Volume(volume.clone()),
                    ));
                }
            }
        } else {
            resources.persist()?;
        }
        Ok(resources)
    }

    pub(super) fn reset(&mut self) -> Result<(), HostError> {
        let fresh = Self::new(&[])?;
        self.run_id = fresh.run_id;
        self.containers.clear();
        self.volumes.clear();
        self.pending_mutations.clear();
        self.settled.clear();
        self.deleting.clear();
        self.next_container = 0;
        self.persist()
    }

    pub(super) fn persist(&self) -> Result<(), HostError> {
        if let (Some(journal), Some(daemon)) = (&self.journal, &self.daemon) {
            journal.save(&State {
                daemon_id: daemon.id.clone(),
                daemon_root: daemon.root.clone(),
                daemon_version: daemon.version.clone(),
                run_id: self.run_id.clone(),
                containers: self.containers.clone(),
                volumes: self.volumes.clone(),
                settled: self.settled.clone(),
                deleting: self.deleting.clone(),
            })?;
        }
        Ok(())
    }

    pub(super) fn settle(&mut self, name: &str) -> Result<(), HostError> {
        if !self.settled.iter().any(|item| item == name) {
            self.settled.push(name.to_owned());
        }
        self.persist()
    }

    pub(super) fn set_healthchecks(
        &mut self,
        images: &[(&str, Option<bollard::models::HealthConfig>)],
    ) {
        self.healthchecks = images
            .iter()
            .map(|(id, health)| ((*id).to_owned(), health.clone()))
            .collect();
    }

    pub(super) fn begin_delete(&mut self, identity: &str) -> Result<(), HostError> {
        if self.journal.is_some() {
            if !self.deleting.is_empty() {
                return Err(HostError::Cleanup);
            }
            self.deleting.push(identity.to_owned());
            self.persist()?;
        }
        Ok(())
    }

    pub(super) fn finish_delete(&mut self, identity: &str) -> Result<(), HostError> {
        if self.journal.is_some() {
            if self.deleting.as_slice() != [identity] {
                return Err(HostError::Ownership);
            }
            self.deleting.clear();
            self.persist()?;
        }
        Ok(())
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
        self.persist()?;
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

    pub(super) fn register_container(&mut self, role: &str) -> Result<String, HostError> {
        let name = self.next_name(role)?;
        self.containers.push(TrackedContainer {
            name: name.clone(),
            role: role.to_owned(),
            id: None,
        });
        self.persist()?;
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
        self.persist()
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
        let config = inspect.config.as_ref().ok_or(HostError::Ownership)?;
        let expected_health = self
            .healthchecks
            .get(image_id)
            .ok_or(HostError::Ownership)?;
        if !crate::same_healthcheck(config.healthcheck.as_ref(), expected_health.as_ref()) {
            return Err(HostError::Ownership);
        }
        let labels = config.labels.as_ref().ok_or(HostError::Ownership)?;
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
