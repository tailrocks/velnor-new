//! Bound Docker requests for the publication helper.

use std::{future::Future, time::Instant};

use bollard::{
    Docker,
    container::LogOutput,
    errors::Error as DockerError,
    models::ContainerInspectResponse,
    query_parameters::{LogsOptionsBuilder, WaitContainerOptionsBuilder},
};
use futures_util::StreamExt;

use crate::error::HostError;

const OUTPUT_LIMIT: usize = 64 * 1024;

pub(in crate::launch::resource_probe::publication) async fn inspect(
    docker: &Docker,
    id: &str,
    deadline: Instant,
) -> Result<ContainerInspectResponse, HostError> {
    bounded(deadline, docker.inspect_container(id, None))
        .await?
        .map_err(|_| HostError::Docker)
}

pub(in crate::launch::resource_probe::publication) async fn inspect_optional(
    docker: &Docker,
    id: &str,
    deadline: Instant,
) -> Result<Option<ContainerInspectResponse>, HostError> {
    match bounded(deadline, docker.inspect_container(id, None)).await? {
        Ok(response) => Ok(Some(response)),
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(None),
        Err(_) => Err(HostError::Docker),
    }
}

pub(super) async fn wait_success(
    docker: &Docker,
    id: &str,
    deadline: Instant,
) -> Result<(), HostError> {
    let options = WaitContainerOptionsBuilder::new()
        .condition("not-running")
        .build();
    let result = async {
        let mut stream = docker.wait_container(id, Some(options));
        let result = stream
            .next()
            .await
            .ok_or(HostError::Docker)?
            .map_err(|_| HostError::Docker)?;
        (result.status_code == 0 && result.error.is_none())
            .then_some(())
            .ok_or(HostError::Docker)
    };
    bounded(deadline, result).await?
}

pub(super) async fn read_stdout(
    docker: &Docker,
    id: &str,
    deadline: Instant,
) -> Result<Vec<u8>, HostError> {
    let options = LogsOptionsBuilder::new()
        .follow(false)
        .stdout(true)
        .stderr(false)
        .timestamps(false)
        .tail("all")
        .build();
    bounded(deadline, async {
        let mut stream = docker.logs(id, Some(options));
        let mut output = Vec::new();
        while let Some(frame) = stream.next().await {
            let LogOutput::StdOut { message } = frame.map_err(|_| HostError::Docker)? else {
                return Err(HostError::Frame);
            };
            if output.len().saturating_add(message.len()) > OUTPUT_LIMIT {
                return Err(HostError::Frame);
            }
            output.extend_from_slice(&message);
        }
        Ok(output)
    })
    .await?
}

pub(super) async fn bounded<T>(
    deadline: Instant,
    future: impl Future<Output = T>,
) -> Result<T, HostError> {
    tokio::time::timeout_at(deadline.into(), future)
        .await
        .map_err(|_| HostError::DockerTimeout)
}
