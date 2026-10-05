//! Bounded Bollard adapter for the guest sampler.

use std::collections::HashMap;

use bollard::Docker;
use bollard::models::SystemInfo;
use bollard::query_parameters::{LogsOptionsBuilder, RemoveContainerOptionsBuilder};
use futures_util::{Stream, StreamExt};

use crate::worker::resources::confirmed_not_found;

use super::{CreatedProbe, GuestDockerClient, GuestSampleFailure, ProbeContainer, ProbeImage};
use crate::worker::resources::guest::GuestProbeCreate;

/// Bollard client restricted to the sampler's fixed probe operations.
#[derive(Debug, Clone)]
pub(super) struct BollardGuestDocker {
    docker: Docker,
}

impl BollardGuestDocker {
    /// Bind the adapter to the selected Docker client.
    pub(super) fn new(docker: Docker) -> Self {
        Self { docker }
    }
}

impl GuestDockerClient for BollardGuestDocker {
    async fn info(&self) -> Result<SystemInfo, GuestSampleFailure> {
        self.docker
            .info()
            .await
            .map_err(|_| GuestSampleFailure::Docker)
    }

    async fn inspect_image(
        &self,
        reference: &str,
    ) -> Result<Option<ProbeImage>, GuestSampleFailure> {
        let image = match self.docker.inspect_image(reference).await {
            Ok(image) => image,
            Err(error) if confirmed_not_found(&error) => return Ok(None),
            Err(_) => return Err(GuestSampleFailure::Docker),
        };
        let repo_tags = match image.repo_tags {
            Some(tags) => tags,
            None => Vec::new(),
        };
        Ok(Some(ProbeImage {
            id: image.id,
            repo_tags,
            os: image.os,
            architecture: image.architecture,
        }))
    }

    async fn inspect_container(
        &self,
        name: &str,
    ) -> Result<Option<ProbeContainer>, GuestSampleFailure> {
        let inspected = match self.docker.inspect_container(name, None).await {
            Ok(container) => container,
            Err(error) if confirmed_not_found(&error) => return Ok(None),
            Err(_) => return Err(GuestSampleFailure::Docker),
        };
        let labels = match inspected.config.and_then(|config| config.labels) {
            Some(labels) => labels,
            None => HashMap::new(),
        };
        let state = inspected.state;
        Ok(Some(ProbeContainer {
            id: inspected.id,
            name: inspected.name,
            image: inspected.image,
            labels,
            running: state.as_ref().and_then(|state| state.running),
            exit_code: state.and_then(|state| state.exit_code),
        }))
    }

    async fn create_container(
        &self,
        request: GuestProbeCreate,
    ) -> Result<CreatedProbe, GuestSampleFailure> {
        let created = self
            .docker
            .create_container(Some(request.options), request.config)
            .await
            .map_err(|_| GuestSampleFailure::Docker)?;
        Ok(CreatedProbe {
            id: created.id,
            has_warnings: !created.warnings.is_empty(),
        })
    }

    async fn start_container(&self, id: &str) -> Result<(), GuestSampleFailure> {
        self.docker
            .start_container(id, None)
            .await
            .map_err(|_| GuestSampleFailure::Docker)
    }

    async fn read_output(&self, id: &str, limit: usize) -> Result<Vec<u8>, GuestSampleFailure> {
        let options = LogsOptionsBuilder::new()
            .follow(false)
            .stdout(true)
            .stderr(false)
            .timestamps(false)
            .tail("all")
            .build();
        collect_bounded_output(self.docker.logs(id, Some(options)), limit).await
    }

    async fn remove_container(&self, id: &str) -> Result<(), GuestSampleFailure> {
        let options = RemoveContainerOptionsBuilder::new().force(true).build();
        match self.docker.remove_container(id, Some(options)).await {
            Ok(()) => Ok(()),
            Err(error) if confirmed_not_found(&error) => Ok(()),
            Err(_) => Err(GuestSampleFailure::Docker),
        }
    }
}

async fn collect_bounded_output<S, C, E>(
    stream: S,
    limit: usize,
) -> Result<Vec<u8>, GuestSampleFailure>
where
    S: Stream<Item = Result<C, E>>,
    C: AsRef<[u8]>,
{
    let mut bytes = Vec::with_capacity(limit);
    futures_util::pin_mut!(stream);
    while let Some(item) = stream.next().await {
        let item = item.map_err(|_| GuestSampleFailure::Docker)?;
        let chunk = item.as_ref();
        bytes
            .len()
            .checked_add(chunk.len())
            .filter(|size| *size <= limit)
            .ok_or(GuestSampleFailure::OutputLimit)?;
        bytes.extend_from_slice(chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use futures_util::stream;

    use super::{GuestSampleFailure, collect_bounded_output};

    #[tokio::test]
    async fn output_limit_is_checked_before_a_chunk_is_appended() {
        let chunks = stream::iter([Ok::<_, ()>(vec![b'x'; 513])]);
        assert_eq!(
            collect_bounded_output(chunks, 512).await,
            Err(GuestSampleFailure::OutputLimit)
        );
    }
}
