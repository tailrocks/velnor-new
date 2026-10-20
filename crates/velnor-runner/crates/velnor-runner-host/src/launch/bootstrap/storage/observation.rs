use std::{future::Future, pin::Pin, time::Instant};

use bollard::{
    Docker,
    container::AttachContainerResults,
    exec::{CreateExecOptions, StartExecOptions, StartExecResults},
};
use futures_util::{Stream, StreamExt};
use serde::{Serialize, de::DeserializeOwned};
use tokio::io::AsyncWriteExt;
use velnor_resource_probe::worker_binding::{
    AcknowledgeRequest, SelfStorageObservation, SelfStorageRequest, WORKER_ACK_LIMIT,
    WORKER_PROTOCOL_VERSION, WORKER_RECORD_LIMIT, WORKER_REQUEST_LIMIT, WorkerRole,
    validate_acknowledgment,
};
use zeroize::Zeroize;

mod aggregate;
mod provider_entry;
use aggregate::observe_storage_resources as observe_storage_resources_core;
pub(in crate::launch) use provider_entry::observe_storage_resources_with_provider;

use crate::{
    error::HostError,
    journal::{ConnectionJournal, Journal},
    launch::{
        SelectedEngineContext, VerifiedWorkerImages,
        bootstrap::{
            PreparedAcquiredBootstrap, StartedBootstrapPair, VerifiedStorageResourceBinding,
        },
        resource_probe::ReleaseReadSecret,
    },
    worker::storage_initialization::VerifiedStorageInitializationReady,
};

pub(crate) async fn observe_storage_resources(
    docker: &Docker,
    journal: &ConnectionJournal,
    host_journal: &Journal,
    release_read_secret: &ReleaseReadSecret,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    prepared: &PreparedAcquiredBootstrap,
    started: &StartedBootstrapPair,
    initialization: &VerifiedStorageInitializationReady,
) -> Result<VerifiedStorageResourceBinding, HostError> {
    let provider = crate::launch::resource_probe::release_image_provider(release_read_secret)?;
    observe_storage_resources_with_provider(
        docker,
        journal,
        host_journal,
        &provider,
        engine,
        images,
        prepared,
        started,
        initialization,
    )
    .await
}

const HELPER_PATH: &str = "/velnor/resource-probe";
const MODE_ARGUMENT: &str = "self_storage";

/// One bounded storage report from the exact running worker container.
pub(crate) struct VerifiedWorkerStorageObservation {
    observation: SelfStorageObservation,
    exec_id: String,
    init_host_pid: u32,
    helper_host_pid: u32,
    attached: AttachContainerResults,
}

impl VerifiedWorkerStorageObservation {
    pub(crate) fn observation(&self) -> &SelfStorageObservation {
        &self.observation
    }

    pub(crate) fn exec_id(&self) -> &str {
        &self.exec_id
    }

    pub(crate) const fn init_host_pid(&self) -> u32 {
        self.init_host_pid
    }

    pub(crate) const fn helper_host_pid(&self) -> u32 {
        self.helper_host_pid
    }
}

/// Measure the fixed storage paths inside one started worker container.
pub(crate) async fn observe_worker_storage(
    docker: &Docker,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    container_id: &str,
    role: WorkerRole,
    deadline: Instant,
) -> Result<VerifiedWorkerStorageObservation, HostError> {
    within(deadline, images.verify_for_engine(docker, engine)).await??;
    let init_host_pid = verify_container(docker, container_id, role, images, deadline).await?;

    let operation_id = uuid::Uuid::new_v4().simple().to_string();
    let request = SelfStorageRequest {
        schema_version: 1,
        operation_id: operation_id.clone(),
        container_id: container_id.to_owned(),
        role,
    };
    request.validate().map_err(|_| HostError::Identity)?;
    let mut request_bytes = encode_line(&request, WORKER_REQUEST_LIMIT)?;
    let exec = within(
        deadline,
        docker.create_exec(
            container_id,
            CreateExecOptions::<String> {
                attach_stdin: Some(true),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                tty: Some(false),
                cmd: Some(vec![
                    HELPER_PATH.to_owned(),
                    "--mode".to_owned(),
                    MODE_ARGUMENT.to_owned(),
                ]),
                ..Default::default()
            },
        ),
    )
    .await?
    .map_err(|_| HostError::Docker)?;
    if exec.id.is_empty() {
        return Err(HostError::Identity);
    }
    let StartExecResults::Attached { output, input } = within(
        deadline,
        docker.start_exec(
            &exec.id,
            Some(StartExecOptions {
                detach: false,
                tty: false,
                output_capacity: Some(WORKER_RECORD_LIMIT + WORKER_ACK_LIMIT),
            }),
        ),
    )
    .await?
    .map_err(|_| HostError::Docker)?
    else {
        return Err(HostError::Docker);
    };
    let mut attached = AttachContainerResults { output, input };

    within(deadline, attached.input.write_all(&request_bytes))
        .await?
        .map_err(|_| HostError::Docker)?;
    request_bytes.zeroize();
    within(deadline, attached.input.flush())
        .await?
        .map_err(|_| HostError::Docker)?;

    let record_line = read_stdout_line(&mut attached.output, deadline).await?;
    let observation: SelfStorageObservation = decode_line(&record_line, WORKER_RECORD_LIMIT)?;
    validate_observation(&observation, &operation_id, container_id, role, images)?;
    let running = within(deadline, docker.inspect_exec(&exec.id))
        .await?
        .map_err(|_| HostError::Docker)?;
    let helper_host_pid = running.pid.and_then(|pid| u32::try_from(pid).ok());
    if running.id.as_deref() != Some(exec.id.as_str())
        || running.container_id.as_deref() != Some(container_id)
        || running.running != Some(true)
        || helper_host_pid.is_none_or(|pid| pid == 0)
    {
        return Err(HostError::Ownership);
    }

    Ok(VerifiedWorkerStorageObservation {
        observation,
        exec_id: exec.id,
        init_host_pid,
        helper_host_pid: helper_host_pid.ok_or(HostError::Identity)?,
        attached,
    })
}

/// Acknowledge the observation and require the helper to exit successfully.
pub(crate) async fn finish_worker_storage_observation(
    docker: &Docker,
    mut verified: VerifiedWorkerStorageObservation,
    deadline: Instant,
) -> Result<(), HostError> {
    let acknowledgement = AcknowledgeRequest {
        operation_id: verified.observation.operation_id.clone(),
    };
    let acknowledgement_bytes = encode_line(&acknowledgement, WORKER_ACK_LIMIT)?;
    validate_acknowledgment(&acknowledgement_bytes, &verified.observation.operation_id)
        .map_err(|_| HostError::Identity)?;
    within(
        deadline,
        verified.attached.input.write_all(&acknowledgement_bytes),
    )
    .await?
    .map_err(|_| HostError::Docker)?;
    within(deadline, verified.attached.input.shutdown())
        .await?
        .map_err(|_| HostError::Docker)?;
    drain_empty_output(&mut verified.attached.output, deadline).await?;
    let exit = within(deadline, docker.inspect_exec(&verified.exec_id))
        .await?
        .map_err(|_| HostError::Docker)?;
    if exit.id.as_deref() != Some(verified.exec_id.as_str())
        || exit.container_id.as_deref() != Some(verified.observation.container_id.as_str())
        || exit.running != Some(false)
        || exit.exit_code != Some(0)
    {
        return Err(HostError::Docker);
    }
    Ok(())
}

async fn verify_container(
    docker: &Docker,
    container_id: &str,
    role: WorkerRole,
    images: &VerifiedWorkerImages,
    deadline: Instant,
) -> Result<u32, HostError> {
    if !valid_container_id(container_id) {
        return Err(HostError::Identity);
    }
    let inspected = within(deadline, docker.inspect_container(container_id, None))
        .await?
        .map_err(|_| HostError::Docker)?;
    let image_id = match role {
        WorkerRole::Runner => images.runner().runtime_id(),
        WorkerRole::Dind => images.dind().runtime_id(),
    };
    let init_host_pid = inspected
        .state
        .as_ref()
        .and_then(|state| state.pid)
        .and_then(|pid| u32::try_from(pid).ok());
    if inspected.id.as_deref() != Some(container_id)
        || inspected.image.as_deref() != Some(image_id)
        || inspected.state.and_then(|state| state.running) != Some(true)
        || init_host_pid.is_none_or(|pid| pid == 0)
    {
        return Err(HostError::Ownership);
    }
    init_host_pid.ok_or(HostError::Identity)
}

fn validate_observation(
    observation: &SelfStorageObservation,
    operation_id: &str,
    container_id: &str,
    role: WorkerRole,
    images: &VerifiedWorkerImages,
) -> Result<(), HostError> {
    if observation.wire_mode != "self_storage"
        || observation.schema_version != 1
        || observation.operation_id != operation_id
        || observation.container_id != container_id
        || observation.role != role
        || observation.helper_protocol_version != WORKER_PROTOCOL_VERSION
        || observation.helper_source_revision != images.source_sha()
        || observation.guest_boot_id.is_empty()
        || observation.process_pid == 0
        || observation.process_start_ticks == 0
        || observation.init_start_ticks == 0
        || observation.mount_namespace_id == 0
        || observation.cgroup_namespace_id == 0
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

async fn read_stdout_line<S>(
    output: &mut Pin<Box<S>>,
    deadline: Instant,
) -> Result<Vec<u8>, HostError>
where
    S: Stream<Item = Result<bollard::container::LogOutput, bollard::errors::Error>> + Send + ?Sized,
{
    let mut bytes = Vec::with_capacity(4096);
    while let Some(frame) = within(deadline, output.next()).await? {
        let frame = frame.map_err(|_| HostError::Docker)?;
        let chunk = match frame {
            bollard::container::LogOutput::StdOut { message } => message,
            bollard::container::LogOutput::StdErr { .. }
            | bollard::container::LogOutput::Console { .. }
            | bollard::container::LogOutput::StdIn { .. } => return Err(HostError::Frame),
        };
        if let Some(newline) = chunk.iter().position(|byte| *byte == b'\n') {
            if newline + 1 != chunk.len()
                || bytes.len().saturating_add(chunk.len()) > WORKER_RECORD_LIMIT + 1
            {
                return Err(HostError::Frame);
            }
            bytes.extend_from_slice(&chunk);
            return Ok(bytes);
        }
        if bytes.len().saturating_add(chunk.len()) > WORKER_RECORD_LIMIT {
            return Err(HostError::Frame);
        }
        bytes.extend_from_slice(&chunk);
    }
    Err(HostError::Frame)
}

async fn drain_empty_output<S>(output: &mut Pin<Box<S>>, deadline: Instant) -> Result<(), HostError>
where
    S: Stream<Item = Result<bollard::container::LogOutput, bollard::errors::Error>> + Send + ?Sized,
{
    while let Some(frame) = within(deadline, output.next()).await? {
        let frame = frame.map_err(|_| HostError::Docker)?;
        if !frame.as_ref().is_empty() {
            return Err(HostError::Frame);
        }
    }
    Ok(())
}

fn encode_line<T: Serialize>(value: &T, limit: usize) -> Result<Vec<u8>, HostError> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| HostError::Frame)?;
    if bytes.is_empty() || bytes.len() > limit {
        bytes.zeroize();
        return Err(HostError::Frame);
    }
    bytes.push(b'\n');
    Ok(bytes)
}

fn decode_line<T>(bytes: &[u8], limit: usize) -> Result<T, HostError>
where
    T: DeserializeOwned + Serialize,
{
    if bytes.len() > limit + 1
        || bytes.len() < 2
        || bytes.last() != Some(&b'\n')
        || bytes[..bytes.len() - 1].contains(&b'\n')
    {
        return Err(HostError::Frame);
    }
    let body = &bytes[..bytes.len() - 1];
    let value: T = serde_json::from_slice(body).map_err(|_| HostError::Frame)?;
    if serde_json::to_vec(&value).map_err(|_| HostError::Frame)? != body {
        return Err(HostError::Frame);
    }
    Ok(value)
}

fn valid_container_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

async fn within<T>(deadline: Instant, future: impl Future<Output = T>) -> Result<T, HostError> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or(HostError::DockerTimeout)?;
    tokio::time::timeout(remaining, future)
        .await
        .map_err(|_| HostError::DockerTimeout)
}

#[cfg(test)]
#[path = "observation/tests.rs"]
mod tests;
