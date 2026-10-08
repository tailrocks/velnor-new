//! Bollard implementation of the cleanup effect boundary.

use bollard::Docker;
use bollard::query_parameters::{RemoveContainerOptionsBuilder, StopContainerOptionsBuilder};
use zeroize::Zeroizing;

use crate::HostError;
use crate::docker_client::docker_deadline;
use crate::worker::{
    WorkerNetworkPlan, remove_worker_network, remove_worker_volumes, worker_id_for_name,
};

mod commands;
mod helpers;
use helpers::{outer_id, outer_name, parse_ids, role_label, valid_docker_id};

use super::{
    ChildResourceInventory, ChildResourceKind, ContainerObservation, GenerationObservation,
    OuterContainerRole, RunnerStopEvidence, RunnerStopPolicy, WorkerCleanupEngine,
    WorkerGenerationIdentity,
};

/// Docker-backed cleanup operations for a single official runner/DinD pair.
#[derive(Debug)]
pub struct DockerCleanupEngine<'a> {
    docker: &'a Docker,
}

impl<'a> DockerCleanupEngine<'a> {
    /// Use the already-authorized host Docker client; it is never passed to jobs.
    #[must_use]
    pub const fn new(docker: &'a Docker) -> Self {
        Self { docker }
    }
}

impl WorkerCleanupEngine for DockerCleanupEngine<'_> {
    async fn inspect_generation(
        &self,
        identity: &WorkerGenerationIdentity,
    ) -> Result<GenerationObservation, HostError> {
        let runner = self
            .inspect_owned(identity, OuterContainerRole::Runner)
            .await?;
        let dind = self
            .inspect_owned(identity, OuterContainerRole::Dind)
            .await?;
        Ok(GenerationObservation { runner, dind })
    }

    async fn stop_runner(
        &self,
        identity: &WorkerGenerationIdentity,
        policy: &RunnerStopPolicy,
    ) -> Result<RunnerStopEvidence, HostError> {
        let before = self
            .inspect_owned(identity, OuterContainerRole::Runner)
            .await?;
        if !before.present {
            return Ok(RunnerStopEvidence {
                stopped: true,
                forced: false,
            });
        }
        if !before.running {
            return Ok(RunnerStopEvidence {
                stopped: true,
                forced: false,
            });
        }
        let RunnerStopPolicy::StopAtDeadline { grace_seconds, .. } = policy else {
            return Err(HostError::Docker);
        };
        let timeout = i32::try_from(*grace_seconds).map_err(|_| HostError::Identity)?;
        let options = StopContainerOptionsBuilder::new().t(timeout).build();
        let _stop_response = docker_deadline(
            self.docker
                .stop_container(identity.runner_container_id(), Some(options)),
        )
        .await?;
        let after_grace = self
            .inspect_owned(identity, OuterContainerRole::Runner)
            .await?;
        if !after_grace.running {
            return Ok(RunnerStopEvidence {
                stopped: true,
                forced: false,
            });
        }
        let _kill_response = docker_deadline(
            self.docker
                .kill_container(identity.runner_container_id(), None),
        )
        .await?;
        let after_kill = self
            .inspect_owned(identity, OuterContainerRole::Runner)
            .await?;
        Ok(RunnerStopEvidence {
            stopped: after_kill.present && !after_kill.running,
            forced: true,
        })
    }

    async fn stop_dind(
        &self,
        identity: &WorkerGenerationIdentity,
    ) -> Result<super::DindStopEvidence, HostError> {
        let before = self
            .inspect_owned(identity, OuterContainerRole::Dind)
            .await?;
        if !before.present || !before.running {
            return Ok(super::DindStopEvidence { stopped: true });
        }
        let options = StopContainerOptionsBuilder::new().t(10).build();
        let _stop_response = docker_deadline(
            self.docker
                .stop_container(identity.dind_container_id(), Some(options)),
        )
        .await;
        let after = self
            .inspect_owned(identity, OuterContainerRole::Dind)
            .await?;
        if !after.present || !after.running {
            Ok(super::DindStopEvidence { stopped: true })
        } else {
            Err(HostError::Docker)
        }
    }

    async fn runner_diagnostics(
        &self,
        identity: &WorkerGenerationIdentity,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, HostError> {
        self.download_runner_diagnostics(identity).await
    }

    async fn list_dind_children(
        &self,
        identity: &WorkerGenerationIdentity,
    ) -> Result<ChildResourceInventory, HostError> {
        let dind = self
            .inspect_owned(identity, OuterContainerRole::Dind)
            .await?;
        if !dind.present || !dind.running {
            return Err(HostError::Docker);
        }
        let containers = self
            .run_dind_command(
                identity,
                vec!["container", "ls", "--all", "--quiet", "--no-trunc"],
            )
            .await?;
        let networks = self
            .run_dind_command(
                identity,
                vec!["network", "ls", "--quiet", "--filter", "type=custom"],
            )
            .await?;
        Ok(ChildResourceInventory {
            containers: parse_ids(&containers)?,
            networks: parse_ids(&networks)?,
        })
    }

    async fn remove_dind_child(
        &self,
        identity: &WorkerGenerationIdentity,
        id: &str,
        kind: ChildResourceKind,
    ) -> Result<(), HostError> {
        if !valid_docker_id(id) {
            return Err(HostError::Identity);
        }
        let command = match kind {
            ChildResourceKind::Container => vec!["container", "rm", "--force", id],
            ChildResourceKind::Network => vec!["network", "rm", id],
        };
        let removal = self.run_dind_command(identity, command).await;
        let remaining = self.list_dind_children(identity).await?;
        let still_present = match kind {
            ChildResourceKind::Container => remaining.containers.iter().any(|item| item == id),
            ChildResourceKind::Network => remaining.networks.iter().any(|item| item == id),
        };
        if still_present {
            return Err(removal.err().unwrap_or(HostError::Docker));
        }
        Ok(())
    }

    async fn remove_outer_container(
        &self,
        identity: &WorkerGenerationIdentity,
        role: OuterContainerRole,
    ) -> Result<(), HostError> {
        let observation = self.inspect_owned(identity, role).await?;
        if !observation.present {
            return Ok(());
        }
        if observation.running {
            return Err(HostError::Docker);
        }
        let id = outer_id(identity, role);
        let options = RemoveContainerOptionsBuilder::new().force(true).build();
        let response = docker_deadline(self.docker.remove_container(id, Some(options))).await?;
        if response.is_err() && self.inspect_id(id).await? {
            return Err(HostError::Docker);
        }
        if self.inspect_id(id).await? {
            return Err(HostError::Docker);
        }
        let name = outer_name(identity, role);
        if worker_id_for_name(
            self.docker,
            &name,
            identity.worker_volume(),
            role_label(role),
        )
        .await?
        .is_some()
        {
            return Err(HostError::Docker);
        }
        Ok(())
    }

    async fn remove_outer_network(
        &self,
        identity: &WorkerGenerationIdentity,
    ) -> Result<(), HostError> {
        let (Some(name), Some(network_id)) =
            (identity.outer_network_name(), identity.outer_network_id())
        else {
            return Err(HostError::Identity);
        };
        let plan = WorkerNetworkPlan::for_worker(identity.worker_volume())?;
        if plan.name() != name {
            return Err(HostError::Identity);
        }
        remove_worker_network(self.docker, &plan, network_id).await
    }

    async fn remove_named_volumes(
        &self,
        identity: &WorkerGenerationIdentity,
    ) -> Result<Vec<String>, HostError> {
        if !remove_worker_volumes(self.docker, identity.worker_volume()).await? {
            return Err(HostError::Docker);
        }
        crate::worker::volumes::worker_volume_names(identity.worker_volume())
    }
}

impl DockerCleanupEngine<'_> {
    async fn inspect_owned(
        &self,
        identity: &WorkerGenerationIdentity,
        role: OuterContainerRole,
    ) -> Result<ContainerObservation, HostError> {
        let name = outer_name(identity, role);
        let recorded = outer_id(identity, role);
        match worker_id_for_name(
            self.docker,
            &name,
            identity.worker_volume(),
            role_label(role),
        )
        .await?
        {
            Some(found) if found == recorded => {
                let response =
                    docker_deadline(self.docker.inspect_container(recorded, None)).await?;
                let body = response.map_err(|_| HostError::Docker)?;
                let exact_id = body.id.as_deref() == Some(recorded);
                let state = body.state.ok_or(HostError::Docker)?;
                if !exact_id {
                    return Err(HostError::Docker);
                }
                Ok(ContainerObservation {
                    present: true,
                    running: state.running.ok_or(HostError::Docker)?,
                    started: Some(super::docker_started_at_observation(
                        state.started_at.as_deref(),
                    )),
                })
            }
            Some(_) => Err(HostError::Docker),
            None => {
                if self.inspect_id(recorded).await? {
                    return Err(HostError::Docker);
                }
                Ok(ContainerObservation {
                    present: false,
                    running: false,
                    started: None,
                })
            }
        }
    }

    async fn inspect_id(&self, id: &str) -> Result<bool, HostError> {
        match docker_deadline(self.docker.inspect_container(id, None)).await? {
            Ok(_) => Ok(true),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(false),
            Err(_) => Err(HostError::Docker),
        }
    }
}
