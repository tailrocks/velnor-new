//! Docker implementation of the worker pair engine.

use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::query_parameters::RemoveContainerOptionsBuilder;

use crate::docker_client::docker_deadline;
use crate::docker_spec::RunnerImageProfile;
use crate::error::HostError;
use crate::worker::{
    CreateProjection, create_named_volumes, create_only, deliver_jit, dind_create,
    dind_create_for_profile, remove_worker_volumes, start_id,
};

use super::PairEngine;

impl PairEngine for Docker {
    async fn prepare_volumes(&self, volume: &str) -> Result<(), HostError> {
        Box::pin(docker_deadline(create_named_volumes(
            self,
            volume,
            &dind_create(volume)?.mounts,
        )))
        .await?
    }

    async fn prepare_volumes_for_profile(
        &self,
        volume: &str,
        profile: Option<&RunnerImageProfile>,
    ) -> Result<(), HostError> {
        let mounts = match profile {
            Some(profile) => dind_create_for_profile(volume, profile)?.mounts,
            None => dind_create(volume)?.mounts,
        };
        Box::pin(docker_deadline(create_named_volumes(self, volume, &mounts))).await?
    }

    async fn create(&self, spec: &CreateProjection) -> Result<String, HostError> {
        Box::pin(docker_deadline(create_only(self, spec))).await?
    }

    async fn start(&self, id: &str) -> Result<(), HostError> {
        Box::pin(docker_deadline(start_id(self, id))).await?
    }

    async fn write_jit(&self, id: &str, jit: &[u8]) -> Result<(), HostError> {
        Box::pin(docker_deadline(deliver_jit(self, id, jit))).await?
    }

    async fn remove(&self, id: &str) -> Result<(), HostError> {
        let options = RemoveContainerOptionsBuilder::new().force(true).build();
        Box::pin(docker_deadline(self.remove_container(id, Some(options))))
            .await?
            .map_err(|_| HostError::Docker)
    }

    async fn id_for_name(&self, name: &str) -> Result<Option<String>, HostError> {
        if name.is_empty() {
            return Err(HostError::Docker);
        }
        match Box::pin(docker_deadline(self.inspect_container(name, None))).await? {
            Ok(body) => body
                .id
                .filter(|id| !id.is_empty())
                .map(Some)
                .ok_or(HostError::Docker),
            Err(DockerError::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(None),
            Err(_) => Err(HostError::Docker),
        }
    }

    async fn worker_id_for_name(
        &self,
        name: &str,
        volume: &str,
        role: &str,
    ) -> Result<Option<String>, HostError> {
        crate::worker::worker_id_for_name(self, name, volume, role).await
    }

    async fn remove_worker_volumes(&self, volume: &str) -> Result<bool, HostError> {
        remove_worker_volumes(self, volume).await
    }

    async fn running(&self, id: &str) -> Result<bool, HostError> {
        let response = Box::pin(docker_deadline(self.inspect_container(id, None))).await?;
        match crate::docker_client::classify_inspect(response) {
            Ok(running) => Ok(running),
            Err(_) => Err(HostError::Docker),
        }
    }
}
