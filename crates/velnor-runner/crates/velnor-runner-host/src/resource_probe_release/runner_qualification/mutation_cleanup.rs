// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use bollard::Docker;
use bollard::query_parameters::{RemoveContainerOptionsBuilder, RemoveVolumeOptionsBuilder};

use crate::error::HostError;

use super::mutation_lifecycle::{
    MutationKind, MutationResult, MutationValue, MutationWait, PendingMutation, wait,
};
use super::resources::{OwnedResources, TrackedContainer};
use super::{QUALIFIER_LABEL, deadline, missing, valid_container_id, volume_owned};

const RECOVERY_TIMEOUT: Duration = Duration::from_secs(3);
const RECOVERY_POLL: Duration = Duration::from_millis(100);
const RECOVERY_REQUEST: Duration = Duration::from_secs(1);

enum Recovery {
    Resolved,
    Unresolved { owned_observed: bool },
}

enum Observation {
    Missing,
    Owned(MutationValue),
}

struct ContainerCleanup {
    failed: bool,
    unresolved_uncertain: bool,
}

impl OwnedResources {
    pub(super) async fn cleanup(&mut self, docker: &Docker) -> Result<(), HostError> {
        let mut failed = false;
        let mut uncertain_containers = Vec::new();
        let mut observed_containers = Vec::new();
        let mut uncertain_volumes = Vec::new();
        let mut observed_volumes = Vec::new();
        let pending = std::mem::take(&mut self.pending_mutations);
        for mut mutation in pending {
            match wait(&mut mutation, Duration::from_secs(5)).await {
                MutationWait::Settled(MutationResult::Created(value)) => {
                    failed |= self.record_created(&mutation, value).is_err();
                }
                MutationWait::Settled(MutationResult::Rejected(_)) => {}
                MutationWait::Settled(MutationResult::Uncertain(_)) | MutationWait::Pending => {
                    match self.reconcile_pending(docker, &mut mutation).await {
                        Recovery::Resolved => {}
                        Recovery::Unresolved { owned_observed } => {
                            eprintln!(
                                "runner image qualification recovery identity: {}",
                                mutation.identity
                            );
                            match mutation.kind {
                                MutationKind::Container(name) => {
                                    uncertain_containers.push(name.clone());
                                    if owned_observed {
                                        observed_containers.push(name);
                                    }
                                }
                                MutationKind::Volume(name) => {
                                    uncertain_volumes.push(name.clone());
                                    if owned_observed {
                                        observed_volumes.push(name);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        let containers = self
            .cleanup_containers(docker, &uncertain_containers, &observed_containers)
            .await;
        failed |= containers.failed;
        failed |= self
            .cleanup_volumes(
                docker,
                containers.unresolved_uncertain,
                &uncertain_volumes,
                &observed_volumes,
            )
            .await;
        if !uncertain_containers.is_empty() {
            Err(HostError::ContainerCreateUncertain)
        } else if !uncertain_volumes.is_empty() {
            Err(HostError::VolumeCreateUncertain)
        } else if failed {
            Err(HostError::Cleanup)
        } else {
            Ok(())
        }
    }

    async fn reconcile_pending(
        &mut self,
        docker: &Docker,
        mutation: &mut PendingMutation,
    ) -> Recovery {
        let end = Instant::now() + RECOVERY_TIMEOUT;
        let mut owned_observed = false;
        loop {
            match wait(mutation, RECOVERY_POLL).await {
                MutationWait::Settled(MutationResult::Created(value)) => {
                    return if self.record_created(mutation, value).is_ok() {
                        Recovery::Resolved
                    } else {
                        Recovery::Unresolved { owned_observed }
                    };
                }
                MutationWait::Settled(MutationResult::Rejected(_)) => return Recovery::Resolved,
                MutationWait::Settled(MutationResult::Uncertain(_)) | MutationWait::Pending => {}
            }
            match self.observe_owned(docker, mutation, end).await {
                Ok(Observation::Owned(value)) => {
                    if self.record_created(mutation, value).is_err() {
                        return Recovery::Unresolved { owned_observed };
                    }
                    owned_observed = true;
                    if mutation.outcome.is_some() {
                        return Recovery::Resolved;
                    }
                }
                Ok(Observation::Missing) | Err(HostError::DockerTimeout | HostError::Docker) => {}
                Err(_) => return Recovery::Unresolved { owned_observed },
            }
            let remaining = end.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Recovery::Unresolved { owned_observed };
            }
            tokio::time::sleep(RECOVERY_POLL.min(remaining)).await;
        }
    }

    async fn observe_owned(
        &self,
        docker: &Docker,
        mutation: &PendingMutation,
        end: Instant,
    ) -> Result<Observation, HostError> {
        let timeout = RECOVERY_REQUEST.min(end.saturating_duration_since(Instant::now()));
        if timeout.is_zero() {
            return Err(HostError::DockerTimeout);
        }
        match &mutation.kind {
            MutationKind::Container(name) => {
                let inspect = match deadline(docker.inspect_container(name, None), timeout).await? {
                    Ok(inspect) => inspect,
                    Err(error) if missing(&error) => return Ok(Observation::Missing),
                    Err(_) => return Err(HostError::Docker),
                };
                let tracked = self
                    .containers
                    .iter()
                    .find(|container| container.name == *name)
                    .ok_or(HostError::Ownership)?;
                let id = validate_ownership(&self.run_id, &inspect, tracked)?.to_owned();
                Ok(Observation::Owned(MutationValue::ContainerId(id)))
            }
            MutationKind::Volume(name) => {
                let volume = match deadline(docker.inspect_volume(name), timeout).await? {
                    Ok(volume) => volume,
                    Err(error) if missing(&error) => return Ok(Observation::Missing),
                    Err(_) => return Err(HostError::Docker),
                };
                let role = self.volume_role(name).ok_or(HostError::Ownership)?;
                if !volume_owned(&volume, name, &self.run_id, role) {
                    return Err(HostError::Ownership);
                }
                Ok(Observation::Owned(MutationValue::Volume))
            }
        }
    }

    fn record_created(
        &mut self,
        mutation: &PendingMutation,
        value: MutationValue,
    ) -> Result<(), HostError> {
        match (&mutation.kind, value) {
            (MutationKind::Container(name), MutationValue::ContainerId(id)) => {
                self.bind_container_id(name, &id)
            }
            (MutationKind::Volume(_), MutationValue::Volume) => Ok(()),
            _ => Err(HostError::Identity),
        }
    }

    fn volume_role(&self, name: &str) -> Option<&'static str> {
        if name == self.socket_volume() {
            Some("socket")
        } else if name == self.data_volume() {
            Some("data")
        } else {
            None
        }
    }

    async fn cleanup_containers(
        &self,
        docker: &Docker,
        uncertain: &[String],
        observed: &[String],
    ) -> ContainerCleanup {
        let mut cleanup = ContainerCleanup {
            failed: false,
            unresolved_uncertain: false,
        };
        for container in self.containers.iter().rev() {
            let was_uncertain = uncertain.iter().any(|name| name == &container.name);
            let unresolved = was_uncertain && !observed.iter().any(|name| name == &container.name);
            let failed = unresolved || self.cleanup_container(docker, container).await.is_err();
            cleanup.failed |= failed;
            cleanup.unresolved_uncertain |= was_uncertain && failed;
        }
        cleanup
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
        let id = validate_ownership(&self.run_id, &inspect, container)?;
        let options = RemoveContainerOptionsBuilder::new()
            .force(true)
            .v(false)
            .build();
        match deadline(
            docker.remove_container(id, Some(options)),
            Duration::from_secs(3),
        )
        .await?
        {
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

    async fn cleanup_volumes(
        &self,
        docker: &Docker,
        unresolved_containers: bool,
        uncertain_volumes: &[String],
        observed_volumes: &[String],
    ) -> bool {
        let mut failed = false;
        for name in self.volumes.iter().rev() {
            if unresolved_containers
                || (uncertain_volumes.iter().any(|uncertain| uncertain == name)
                    && !observed_volumes.iter().any(|observed| observed == name))
            {
                eprintln!("runner image qualification recovery identity: volume {name}");
                failed = true;
            } else if self.cleanup_volume(docker, name).await.is_err() {
                failed = true;
            }
        }
        failed
    }

    async fn cleanup_volume(&self, docker: &Docker, name: &str) -> Result<(), HostError> {
        let volume = match deadline(docker.inspect_volume(name), Duration::from_secs(3)).await? {
            Ok(volume) => volume,
            Err(error) if missing(&error) => return Ok(()),
            Err(_) => return Err(HostError::Docker),
        };
        let role = self.volume_role(name).ok_or(HostError::Ownership)?;
        if !volume_owned(&volume, name, &self.run_id, role) {
            return Err(HostError::Ownership);
        }
        let options = RemoveVolumeOptionsBuilder::new().force(false).build();
        match deadline(
            docker.remove_volume(name, Some(options)),
            Duration::from_secs(3),
        )
        .await?
        {
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

fn validate_ownership<'a>(
    run_id: &str,
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
    if labels.get(QUALIFIER_LABEL).map(String::as_str) != Some(run_id)
        || labels
            .get("io.velnor.runner-image-qualifier.role")
            .map(String::as_str)
            != Some(tracked.role.as_str())
    {
        return Err(HostError::Ownership);
    }
    Ok(id)
}
