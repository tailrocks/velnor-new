//! Bounded Docker calls used only by the controller probe projection.

use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

use bollard::Docker;
use bollard::container::LogOutput;
use bollard::query_parameters::{
    AttachContainerOptionsBuilder, LogsOptionsBuilder, RemoveContainerOptionsBuilder,
    StartContainerOptions, StopContainerOptionsBuilder, WaitContainerOptionsBuilder,
};

use crate::error::HostError;

use super::lifecycle::Deadline;
use super::record::MAX_OUTPUT_BYTES;

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
        let mut output = Vec::with_capacity(MAX_OUTPUT_BYTES);
        while let Some(item) = stream.next().await {
            let item = item.map_err(|_| HostError::Docker)?;
            let LogOutput::StdOut { message } = item else {
                return Err(HostError::Docker);
            };
            if output.len().saturating_add(message.len()) > MAX_OUTPUT_BYTES {
                return Err(HostError::Frame);
            }
            output.extend_from_slice(&message);
        }
        Ok(output)
    };
    deadline.docker(read).await?
}

pub(super) async fn owned_cgroups(
    docker: &Docker,
    projection: &super::projection::ProbeProjection,
    id: &str,
    deadline: &Deadline,
) -> Result<OwnedCgroupAttachment, HostError> {
    let options = AttachContainerOptionsBuilder::new()
        .stdin(true)
        .stdout(true)
        .stderr(true)
        .stream(true)
        .logs(false)
        .build();
    let attached = deadline
        .docker(docker.attach_container(id, Some(options)))
        .await?
        .map_err(|_| HostError::Docker)?;
    deadline
        .docker(docker.start_container(id, None::<StartContainerOptions>))
        .await?
        .map_err(|_| HostError::Docker)?;
    let started = super::inspect::by_reference(docker, id, deadline)
        .await?
        .ok_or(HostError::ContainerStartUncertain)?;
    if !super::inspect::matches(projection, &started, id) || !super::inspect::running(&started) {
        return Err(HostError::Ownership);
    }
    let host_pid =
        super::inspect::running_host_pid(&started).ok_or(HostError::ContainerStartUncertain)?;
    Ok(OwnedCgroupAttachment { attached, host_pid })
}

pub(super) struct OwnedCgroupAttachment {
    attached: bollard::container::AttachContainerResults,
    host_pid: u32,
}

impl OwnedCgroupAttachment {
    pub(super) const fn host_pid(&self) -> u32 {
        self.host_pid
    }
}

pub(super) async fn owned_cgroups_output(
    docker: &Docker,
    projection: &super::projection::ProbeProjection,
    id: &str,
    mut attachment: OwnedCgroupAttachment,
    request_line: &[u8],
    deadline: &Deadline,
) -> Result<Vec<u8>, HostError> {
    use bollard::container::LogOutput;

    deadline
        .within(attachment.attached.input.write_all(request_line))
        .await?
        .map_err(|_| HostError::Docker)?;
    deadline
        .within(attachment.attached.input.shutdown())
        .await?
        .map_err(|_| HostError::Docker)?;

    let mut output = Vec::with_capacity(4096);
    while let Some(frame) = deadline.within(attachment.attached.output.next()).await? {
        let frame = frame.map_err(|_| HostError::Docker)?;
        let chunk = match frame {
            LogOutput::StdOut { message } => message,
            LogOutput::StdErr { .. } | LogOutput::Console { .. } | LogOutput::StdIn { .. } => {
                return Err(HostError::Frame);
            }
        };
        if output.len().saturating_add(chunk.len())
            > velnor_resource_probe::worker_binding::WORKER_RECORD_LIMIT
        {
            return Err(HostError::Frame);
        }
        output.extend_from_slice(&chunk);
    }
    wait_success(docker, id, deadline).await?;
    let exited = super::inspect::by_reference(docker, id, deadline)
        .await?
        .ok_or(HostError::Ownership)?;
    if !super::inspect::matches(projection, &exited, id)
        || !super::inspect::exited_successfully(&exited)
    {
        return Err(HostError::Docker);
    }
    Ok(output)
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
