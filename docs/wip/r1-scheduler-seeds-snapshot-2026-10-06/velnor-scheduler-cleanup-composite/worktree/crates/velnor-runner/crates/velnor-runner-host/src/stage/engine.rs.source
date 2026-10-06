//! Docker implementation of the bounded worker-pair engine contract.

use bollard::Docker;
use bollard::query_parameters::RemoveContainerOptionsBuilder;
use tokio::time::timeout;

use crate::action_archive_seed::ActionArchiveLease;
use crate::error::HostError;
use crate::journal::LaunchIdentity;
use crate::worker::{
    CreateProjection, ResourceBudget, confirmed_not_found, create_only, create_owned_volumes,
    deliver_jit, list_launch, probe_dind, remove_owned_volume, start_id, verify_owned_volume,
    verify_container as verify_worker_container, verify_engine as verify_worker_engine,
};

use super::{
    CONTAINER_REMOVE_CALL, ContainerRecord, DindProbe, INSPECT_CALL, PairEngine, WorkerVolume,
};

impl PairEngine for Docker {
    async fn prepare_volumes(&self, identity: &LaunchIdentity) -> Result<(), HostError> {
        create_owned_volumes(self, identity).await
    }

    async fn create(&self, spec: &CreateProjection) -> Result<String, HostError> {
        create_only(self, spec).await
    }

    async fn start(&self, id: &str) -> Result<(), HostError> {
        start_id(self, id).await
    }

    async fn probe_dind(&self, id: &str) -> Result<DindProbe, HostError> {
        probe_dind(self, id).await
    }

    async fn list_launch(
        &self,
        identity: &LaunchIdentity,
    ) -> Result<Vec<ContainerRecord>, HostError> {
        list_launch(self, identity).await
    }

    async fn verify_container(
        &self,
        identity: &LaunchIdentity,
        role: &str,
        id: &str,
        dind_id: Option<&str>,
        archive_lease: Option<&ActionArchiveLease>,
        require_running: bool,
    ) -> Result<ContainerRecord, HostError> {
        verify_worker_container(
            self,
            identity,
            role,
            id,
            dind_id,
            archive_lease,
            None,
            require_running,
        )
        .await
    }

    async fn verify_container_with_budget(
        &self,
        identity: &LaunchIdentity,
        role: &str,
        id: &str,
        dind_id: Option<&str>,
        archive_lease: Option<&ActionArchiveLease>,
        resource_budget: ResourceBudget,
        require_running: bool,
    ) -> Result<ContainerRecord, HostError> {
        verify_worker_container(
            self,
            identity,
            role,
            id,
            dind_id,
            archive_lease,
            Some(resource_budget),
            require_running,
        )
        .await
    }

    async fn write_jit(&self, id: &str, jit: &[u8]) -> Result<(), HostError> {
        deliver_jit(self, id, jit).await
    }

    async fn remove(&self, id: &str) -> Result<(), HostError> {
        let options = RemoveContainerOptionsBuilder::new().force(true).build();
        match timeout(
            CONTAINER_REMOVE_CALL,
            self.remove_container(id, Some(options)),
        )
        .await
        {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) if confirmed_not_found(&error) => Ok(()),
            Err(_) | Ok(Err(_)) => {
                if confirm_container_absent(self, id).await? {
                    Ok(())
                } else {
                    Err(HostError::Cleanup)
                }
            }
        }
    }

    async fn inspect_container(
        &self,
        id_or_name: &str,
    ) -> Result<Option<ContainerRecord>, HostError> {
        let found = match timeout(INSPECT_CALL, self.inspect_container(id_or_name, None)).await {
            Err(_) => return Err(HostError::DockerTimeout),
            Ok(Err(error)) if confirmed_not_found(&error) => return Ok(None),
            Ok(Err(_)) => return Err(HostError::Docker),
            Ok(Ok(found)) => found,
        };
        Ok(Some(ContainerRecord {
            id: found
                .id
                .filter(|id| valid_container_id(id))
                .ok_or(HostError::Ownership)?,
            labels: found
                .config
                .and_then(|config| config.labels)
                .ok_or(HostError::Ownership)?,
            running: found.state.and_then(|state| state.running),
        }))
    }

    async fn remove_volume(
        &self,
        identity: &LaunchIdentity,
        volume: WorkerVolume,
    ) -> Result<(), HostError> {
        remove_owned_volume(self, identity, volume).await
    }

    async fn verify_volume(
        &self,
        identity: &LaunchIdentity,
        volume: WorkerVolume,
    ) -> Result<(), HostError> {
        verify_owned_volume(self, identity, volume).await
    }

    async fn verify_engine(&self, identity: &LaunchIdentity) -> Result<(), HostError> {
        verify_worker_engine(self, identity).await
    }
}

fn valid_container_id(id: &str) -> bool {
    (12..=64).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

async fn confirm_container_absent(docker: &Docker, id: &str) -> Result<bool, HostError> {
    match timeout(INSPECT_CALL, docker.inspect_container(id, None)).await {
        Err(_) => Err(HostError::DockerTimeout),
        Ok(Err(error)) if confirmed_not_found(&error) => Ok(true),
        Ok(Err(_)) => Err(HostError::Docker),
        Ok(Ok(_)) => Ok(false),
    }
}
