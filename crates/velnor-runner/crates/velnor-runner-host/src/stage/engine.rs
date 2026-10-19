use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::query_parameters::RemoveContainerOptionsBuilder;

use crate::docker_client::docker_deadline;
use crate::error::HostError;
use crate::launch::{SelectedEngineContext, VerifiedWorkerImages};
use crate::worker::{
    CreateProjection, WorkerVolumeRole, WorkerVolumeVerification, create_named_volumes,
    create_only, deliver_jit, remove_worker_volumes, start_id, verify_worker_volume,
};

use super::StageAdmission;

pub(crate) trait PairEngine {
    async fn verify_worker_images(
        &self,
        expected: &SelectedEngineContext,
        images: &VerifiedWorkerImages,
    ) -> Result<(), HostError>;
    async fn prepare_volumes(
        &self,
        volume: &str,
        admission: &StageAdmission,
        expected: &SelectedEngineContext,
        images: &VerifiedWorkerImages,
    ) -> Result<(), HostError>;
    async fn create(&self, spec: &CreateProjection) -> Result<String, HostError>;
    async fn start(&self, id: &str) -> Result<(), HostError>;
    async fn write_jit(&self, id: &str, jit: &[u8]) -> Result<(), HostError>;
    async fn remove(&self, id: &str) -> Result<(), HostError>;
    async fn id_for_name(&self, name: &str) -> Result<Option<String>, HostError>;
    async fn worker_id_for_name(
        &self,
        name: &str,
        volume: &str,
        role: &str,
    ) -> Result<Option<String>, HostError>;
    async fn verify_volume(
        &self,
        worker: &str,
        role: WorkerVolumeRole,
    ) -> Result<WorkerVolumeVerification, HostError>;
    async fn remove_worker_volumes(&self, volume: &str) -> Result<bool, HostError>;
    async fn running(&self, id: &str) -> Result<bool, HostError>;
}

/// Records container ids before the next external start.
pub(crate) trait PairSink {
    async fn volume(&self, volume: &str) -> Result<(), HostError>;
    async fn dind(&self, id: &str) -> Result<(), HostError>;
    async fn runner(&self, id: &str) -> Result<(), HostError>;
}

/// Sink that does not record ids.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct Forget;

#[cfg(test)]
#[expect(
    clippy::unused_async_trait_impl,
    reason = "sink matches the async trait and does not await"
)]
impl PairSink for Forget {
    async fn volume(&self, _volume: &str) -> Result<(), HostError> {
        Ok(())
    }

    async fn dind(&self, _id: &str) -> Result<(), HostError> {
        Ok(())
    }

    async fn runner(&self, _id: &str) -> Result<(), HostError> {
        Ok(())
    }
}

impl PairEngine for Docker {
    async fn verify_worker_images(
        &self,
        expected: &SelectedEngineContext,
        images: &VerifiedWorkerImages,
    ) -> Result<(), HostError> {
        if !expected.matches_images(images) {
            return Err(HostError::Ownership);
        }
        images.verify_for_engine(self, expected).await
    }

    async fn prepare_volumes(
        &self,
        volume: &str,
        admission: &StageAdmission,
        expected: &SelectedEngineContext,
        images: &VerifiedWorkerImages,
    ) -> Result<(), HostError> {
        let mounts = crate::worker::dind_mounts(volume)?;
        let mut verify = || async { admission.verify(self, expected, images).await };
        Box::pin(docker_deadline(create_named_volumes(
            self,
            volume,
            &mounts,
            &mut verify,
        )))
        .await??;
        Ok(())
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

    async fn verify_volume(
        &self,
        worker: &str,
        role: WorkerVolumeRole,
    ) -> Result<WorkerVolumeVerification, HostError> {
        verify_worker_volume(self, worker, role).await
    }

    async fn remove_worker_volumes(&self, volume: &str) -> Result<bool, HostError> {
        remove_worker_volumes(self, volume).await
    }

    async fn running(&self, id: &str) -> Result<bool, HostError> {
        let response = Box::pin(docker_deadline(self.inspect_container(id, None))).await?;
        match crate::launch::classify_inspect(response) {
            Ok(running) => Ok(running),
            Err(_) => Err(HostError::Docker),
        }
    }
}
