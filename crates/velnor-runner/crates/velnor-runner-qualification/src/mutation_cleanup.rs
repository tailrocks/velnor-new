// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::query_parameters::{RemoveContainerOptionsBuilder, RemoveVolumeOptionsBuilder};

use crate::error::QualificationError as HostError;
use crate::host_config::validate_ownership;

use super::mutation_lifecycle::{
    MutationKind, MutationResult, MutationValue, MutationWait, PendingMutation, wait,
};
use super::resources::{OwnedResources, TrackedContainer};
use super::{deadline, missing, volume_owned};

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
    containers_remain: bool,
}

impl OwnedResources {
    pub(super) async fn cleanup(&mut self, docker: &Docker) -> Result<(), HostError> {
        if !self.deleting.is_empty() {
            return Err(HostError::Cleanup);
        }
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
                containers.containers_remain,
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
            if let Some(journal) = &self.journal {
                journal.clear()?;
            }
            Ok(())
        }
    }

    async fn reconcile_pending(
        &mut self,
        docker: &Docker,
        mutation: &mut PendingMutation,
    ) -> Recovery {
        let end = Instant::now() + RECOVERY_TIMEOUT;
        let owned_observed = false;
        if self.journal.is_some() {
            loop {
                match wait(mutation, RECOVERY_POLL).await {
                    MutationWait::Settled(MutationResult::Created(value)) => {
                        return if self.record_created(mutation, value).is_ok() {
                            Recovery::Resolved
                        } else {
                            Recovery::Unresolved { owned_observed }
                        };
                    }
                    MutationWait::Settled(MutationResult::Rejected(_)) => {
                        return Recovery::Resolved;
                    }
                    MutationWait::Settled(MutationResult::Uncertain(_)) => {
                        return Recovery::Unresolved { owned_observed };
                    }
                    MutationWait::Pending if Instant::now() < end => {}
                    MutationWait::Pending => return Recovery::Unresolved { owned_observed },
                }
            }
        }
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
                    if mutation.outcome.is_some() {
                        return if self.record_created(mutation, value).is_ok() {
                            Recovery::Resolved
                        } else {
                            Recovery::Unresolved { owned_observed }
                        };
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
                self.bind_container_id(name, &id)?;
                self.settle(name)
            }
            (MutationKind::Volume(name), MutationValue::Volume) => self.settle(name),
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
        &mut self,
        docker: &Docker,
        uncertain: &[String],
        observed: &[String],
    ) -> ContainerCleanup {
        let mut cleanup = ContainerCleanup {
            failed: false,
            containers_remain: false,
        };
        for container in self.containers.clone().iter().rev() {
            if !self.deleting.is_empty() {
                cleanup.failed = true;
                cleanup.containers_remain = true;
                break;
            }
            let was_uncertain = uncertain.iter().any(|name| name == &container.name);
            let unresolved = was_uncertain && !observed.iter().any(|name| name == &container.name);
            let failed = unresolved || self.cleanup_container(docker, container).await.is_err();
            cleanup.failed |= failed;
            cleanup.containers_remain |= failed;
        }
        cleanup
    }

    async fn cleanup_container(
        &mut self,
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
        let id = id.to_owned();
        let identity = format!("container:{id}");
        self.begin_delete(&identity)?;
        let options = RemoveContainerOptionsBuilder::new()
            .force(true)
            .v(false)
            .build();
        match deadline(
            docker.remove_container(&id, Some(options)),
            Duration::from_secs(3),
        )
        .await?
        {
            Ok(()) => {}
            Err(error) if missing(&error) => {}
            Err(DockerError::DockerResponseServerError {
                status_code: 400..=499,
                ..
            }) => {
                self.finish_delete(&identity)?;
                return Err(HostError::Cleanup);
            }
            Err(_) => return Err(HostError::Cleanup),
        }
        self.finish_delete(&identity)?;
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
        &mut self,
        docker: &Docker,
        unresolved_containers: bool,
        uncertain_volumes: &[String],
        observed_volumes: &[String],
    ) -> bool {
        let mut failed = false;
        for name in self.volumes.clone().iter().rev() {
            if !self.deleting.is_empty() {
                return true;
            }
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

    async fn cleanup_volume(&mut self, docker: &Docker, name: &str) -> Result<(), HostError> {
        let volume = match deadline(docker.inspect_volume(name), Duration::from_secs(3)).await? {
            Ok(volume) => volume,
            Err(error) if missing(&error) => return Ok(()),
            Err(_) => return Err(HostError::Docker),
        };
        let role = self.volume_role(name).ok_or(HostError::Ownership)?;
        if !volume_owned(&volume, name, &self.run_id, role) {
            return Err(HostError::Ownership);
        }
        let identity = format!("volume:{name}");
        self.begin_delete(&identity)?;
        let options = RemoveVolumeOptionsBuilder::new().force(false).build();
        match deadline(
            docker.remove_volume(name, Some(options)),
            Duration::from_secs(3),
        )
        .await?
        {
            Ok(()) => {}
            Err(error) if missing(&error) => {}
            Err(DockerError::DockerResponseServerError {
                status_code: 400..=499,
                ..
            }) => {
                self.finish_delete(&identity)?;
                return Err(HostError::Cleanup);
            }
            Err(_) => return Err(HostError::Cleanup),
        }
        self.finish_delete(&identity)?;
        match deadline(docker.inspect_volume(name), Duration::from_secs(3)).await? {
            Err(error) if missing(&error) => Ok(()),
            _ => Err(HostError::Cleanup),
        }
    }
}
