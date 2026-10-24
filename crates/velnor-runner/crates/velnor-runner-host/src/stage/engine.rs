use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::query_parameters::RemoveContainerOptionsBuilder;

use crate::docker_client::docker_deadline;
use crate::error::HostError;

pub(crate) trait PairEngine {
    async fn remove(&self, id: &str) -> Result<(), HostError>;
    async fn id_for_name(&self, name: &str) -> Result<Option<String>, HostError>;
}

impl PairEngine for Docker {
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
}
