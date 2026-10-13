//! Bounded Docker calls used only by the controller probe projection.

use futures_util::StreamExt;

use bollard::Docker;
use bollard::container::LogOutput;
use bollard::query_parameters::{
    LogsOptionsBuilder, RemoveContainerOptionsBuilder, StartContainerOptions,
    StopContainerOptionsBuilder, WaitContainerOptionsBuilder,
};

use crate::error::HostError;

use super::lifecycle::Deadline;

pub(super) async fn create(
    docker: &Docker,
    projection: &super::projection::ProbeProjection,
    deadline: &Deadline,
) -> Result<String, HostError> {
    let response = deadline
        .docker(
            docker.create_container(Some(projection.options.clone()), projection.config.clone()),
        )
        .await?
        .map_err(|_| HostError::Docker)?;
    if valid_container_id(&response.id) {
        Ok(response.id)
    } else {
        Err(HostError::Identity)
    }
}

pub(super) async fn start(docker: &Docker, id: &str, deadline: &Deadline) -> Result<(), HostError> {
    deadline
        .docker(docker.start_container(id, None::<StartContainerOptions>))
        .await?
        .map_err(|_| HostError::Docker)
}

pub(super) async fn wait_success(
    docker: &Docker,
    id: &str,
    deadline: &Deadline,
) -> Result<(), HostError> {
    let options = WaitContainerOptionsBuilder::new()
        .condition("not-running")
        .build();
    let wait = async {
        let mut responses = docker.wait_container(id, Some(options));
        let response = responses
            .next()
            .await
            .ok_or(HostError::Docker)?
            .map_err(|_| HostError::Docker)?;
        (response.status_code == 0 && response.error.is_none())
            .then_some(())
            .ok_or(HostError::Docker)
    };
    deadline.docker(wait).await?
}

pub(super) async fn stdout(
    docker: &Docker,
    id: &str,
    deadline: &Deadline,
) -> Result<Vec<u8>, HostError> {
    let options = LogsOptionsBuilder::new()
        .follow(false)
        .stdout(true)
        .stderr(false)
        .timestamps(false)
        .tail("all")
        .build();
    let read = async {
        let mut stream = docker.logs(id, Some(options));
        let mut output = Vec::with_capacity(512);
        while let Some(item) = stream.next().await {
            let item = item.map_err(|_| HostError::Docker)?;
            let LogOutput::StdOut { message } = item else {
                return Err(HostError::Docker);
            };
            if output.len().saturating_add(message.len()) > 512 {
                return Err(HostError::Frame);
            }
            output.extend_from_slice(&message);
        }
        Ok(output)
    };
    deadline.docker(read).await?
}

pub(super) async fn stop(docker: &Docker, id: &str, deadline: &Deadline) -> Result<(), HostError> {
    let options = StopContainerOptionsBuilder::new().t(1).build();
    deadline
        .docker(docker.stop_container(id, Some(options)))
        .await?
        .map_err(|_| HostError::Docker)
}

pub(super) async fn remove(
    docker: &Docker,
    id: &str,
    deadline: &Deadline,
) -> Result<(), HostError> {
    let options = RemoveContainerOptionsBuilder::new()
        .force(false)
        .v(false)
        .build();
    deadline
        .docker(docker.remove_container(id, Some(options)))
        .await?
        .map_err(|_| HostError::Docker)
}

fn valid_container_id(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
