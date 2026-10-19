//! Measure the proof channel inside the owned Runner container.

use std::{future::Future, pin::Pin, time::Instant};

use bollard::{
    Docker,
    container::LogOutput,
    exec::{CreateExecOptions, StartExecOptions, StartExecResults},
};
use futures_util::{Stream, StreamExt};
use serde::{Serialize, de::DeserializeOwned};
use tokio::{io::AsyncWriteExt, time::Instant as TokioInstant};
use velnor_resource_probe::{
    execution_channel_wire::{ExecutionChannelContext, ExecutionChannelObservation},
    worker_binding::{AcknowledgeRequest, WORKER_RECORD_LIMIT, WORKER_REQUEST_LIMIT},
};
use velnor_runner_github::RunnerReference;
use zeroize::Zeroize;

use crate::{
    error::HostError,
    journal::StorageRole,
    launch::{
        SelectedEngineContext, VerifiedWorkerImages,
        bootstrap::{
            BootstrapStartReceipt, VerifiedCreatedStorageObject, VerifiedStorageResourceBinding,
        },
        steps::AcquiredProvisioning,
    },
};

use super::VerifiedProofChannelBinding;

mod attachments;
use attachments::{make_mount, validate_storage_attachments};

const HELPER_PATH: &str = "/velnor/resource-probe";
const CONTROL_PATH: &str = "/opt/velnor/source-proof/control";
const CONTROL_SOURCE: &str = "source-proof/control";
const INBOX_PATH: &str = "/opt/velnor/source-proof/inbox";
const INBOX_SOURCE: &str = "source-proof/inbox";
const JOB_FACTS_PATH: &str = "/opt/velnor/job-facts";
const JOB_FACTS_SOURCE: &str = "job-facts";
const ACK_LIMIT: usize = 128;

/// Measure the exact Runner channel after the host starts the owned container pair.
pub(crate) async fn measure_proof_channel(
    docker: &Docker,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    acquired: &AcquiredProvisioning,
    started: &BootstrapStartReceipt,
    storage: &VerifiedStorageResourceBinding,
    metadata: &VerifiedCreatedStorageObject,
    runner: &RunnerReference,
    deadline: Instant,
) -> Result<VerifiedProofChannelBinding, HostError> {
    validate_inputs(engine, acquired, started, storage, metadata, runner)?;
    within(deadline, images.verify_for_engine(docker, engine)).await??;
    inspect_runner(docker, images, started.runner_container_id(), deadline).await?;

    let operation_id = uuid::Uuid::new_v4().simple().to_string();
    let request = ExecutionChannelContext {
        schema_version: 3,
        operation_id: operation_id.clone(),
        expected_guest_boot_id: storage.guest_boot_id().to_owned(),
    };
    let mut request_bytes = encode_line(&request, WORKER_REQUEST_LIMIT)?;
    let exec = within(
        deadline,
        docker.create_exec(
            started.runner_container_id(),
            CreateExecOptions::<String> {
                attach_stdin: Some(true),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                tty: Some(false),
                cmd: Some(vec![
                    HELPER_PATH.to_owned(),
                    "--mode".to_owned(),
                    "execution_channel".to_owned(),
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
    let StartExecResults::Attached {
        mut output,
        mut input,
    } = within(
        deadline,
        docker.start_exec(
            &exec.id,
            Some(StartExecOptions {
                detach: false,
                tty: false,
                output_capacity: Some(WORKER_RECORD_LIMIT + ACK_LIMIT),
            }),
        ),
    )
    .await?
    .map_err(|_| HostError::Docker)?
    else {
        return Err(HostError::Docker);
    };

    within(deadline, input.write_all(&request_bytes))
        .await?
        .map_err(|_| HostError::Docker)?;
    request_bytes.zeroize();
    within(deadline, input.flush())
        .await?
        .map_err(|_| HostError::Docker)?;
    let record_bytes = read_stdout_line(&mut output, deadline).await?;
    let observation: ExecutionChannelObservation = decode_line(&record_bytes, WORKER_RECORD_LIMIT)?;
    validate_observation(&observation, &operation_id, storage)?;
    validate_storage_attachments(&observation, storage, started)?;

    let ack = encode_line(&AcknowledgeRequest { operation_id }, ACK_LIMIT)?;
    within(deadline, input.write_all(&ack))
        .await?
        .map_err(|_| HostError::Docker)?;
    within(deadline, input.shutdown())
        .await?
        .map_err(|_| HostError::Docker)?;
    drain_empty_output(&mut output, deadline).await?;
    let exit = within(deadline, docker.inspect_exec(&exec.id))
        .await?
        .map_err(|_| HostError::Docker)?;
    if exit.id.as_deref() != Some(exec.id.as_str())
        || exit.container_id.as_deref() != Some(started.runner_container_id())
        || exit.running != Some(false)
        || exit.exit_code != Some(0)
    {
        return Err(HostError::Docker);
    }
    inspect_runner(docker, images, started.runner_container_id(), deadline).await?;
    make_binding(acquired, started, storage, metadata, runner, &observation)
}

fn validate_inputs(
    engine: &SelectedEngineContext,
    acquired: &AcquiredProvisioning,
    started: &BootstrapStartReceipt,
    storage: &VerifiedStorageResourceBinding,
    metadata: &VerifiedCreatedStorageObject,
    runner: &RunnerReference,
) -> Result<(), HostError> {
    let assignment = acquired.request();
    let request_id = assignment.request_id().ok_or(HostError::Identity)?;
    if !started.runner_stdin_open()
        || !started.jit_withheld()
        || started.reservation_id() != acquired.reservation_id()
        || started.operation_id() != acquired.intent_id()
        || started.reservation_id() != storage.reservation_id()
        || started.operation_id() != storage.operation_id()
        || started.engine_id() != engine.engine_id()
        || started.docker_root_digest() != engine.docker_root_digest()
        || started.runner_container_id() != storage.runner_container_id()
        || started.dind_container_id() != storage.dind_container_id()
        || runner.id <= 0
        || runner.name != acquired.runner_name()
        || runner.runner_scale_set_id != assignment.scale_set_id()
        || metadata.reservation_id() != acquired.reservation_id()
        || metadata.operation_id() != acquired.intent_id()
        || metadata.engine_id() != engine.engine_id()
        || metadata.docker_root_digest() != engine.docker_root_digest()
        || metadata.role() != StorageRole::EngineMetadata
        || storage.instance_binding_digest().is_empty()
        || storage.guest_boot_id().is_empty()
        || request_id <= 0
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

async fn inspect_runner(
    docker: &Docker,
    images: &VerifiedWorkerImages,
    container_id: &str,
    deadline: Instant,
) -> Result<(), HostError> {
    let inspect = within(deadline, docker.inspect_container(container_id, None))
        .await?
        .map_err(|_| HostError::Docker)?;
    if inspect.id.as_deref() != Some(container_id)
        || inspect.image.as_deref() != Some(images.runner().runtime_id())
        || inspect.state.and_then(|state| state.running) != Some(true)
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

fn validate_observation(
    observation: &ExecutionChannelObservation,
    operation_id: &str,
    storage: &VerifiedStorageResourceBinding,
) -> Result<(), HostError> {
    if observation.schema_version != 3
        || observation.operation_id != operation_id
        || observation.guest_boot_id != storage.guest_boot_id()
        || observation.mount_namespace_id == 0
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn make_binding(
    acquired: &AcquiredProvisioning,
    started: &BootstrapStartReceipt,
    storage: &VerifiedStorageResourceBinding,
    metadata: &VerifiedCreatedStorageObject,
    runner: &RunnerReference,
    observation: &ExecutionChannelObservation,
) -> Result<VerifiedProofChannelBinding, HostError> {
    let assignment = acquired.request();
    let witness_request_id = assignment.request_id().ok_or(HostError::Identity)?;
    let binding = VerifiedProofChannelBinding {
        reservation_id: acquired.reservation_id(),
        operation_id: acquired.intent_id(),
        connection_id: assignment.connection_id().as_str().to_owned(),
        scale_set_id: assignment.scale_set_id(),
        witness_request_id,
        engine_id: started.engine_id().to_owned(),
        docker_root_digest: started.docker_root_digest().to_owned(),
        runner_id: runner.id,
        runner_name: runner.name.clone(),
        runner_container_id: started.runner_container_id().to_owned(),
        dind_container_id: started.dind_container_id().to_owned(),
        instance_binding_digest: storage.instance_binding_digest().to_owned(),
        metadata_volume_name: metadata.docker_name().to_owned(),
        metadata_volume_id: metadata.docker_id().to_owned(),
        control: make_mount(
            &observation.control,
            CONTROL_PATH,
            CONTROL_SOURCE,
            storage,
            started.runner_container_id(),
            true,
            StorageRole::EngineMetadata,
        )?,
        control_directory: observation.control.clone(),
        inbox: make_mount(
            &observation.inbox,
            INBOX_PATH,
            INBOX_SOURCE,
            storage,
            started.runner_container_id(),
            false,
            StorageRole::RunnerHome,
        )?,
        job_facts: make_mount(
            &observation.job_facts,
            JOB_FACTS_PATH,
            JOB_FACTS_SOURCE,
            storage,
            started.runner_container_id(),
            true,
            StorageRole::EngineMetadata,
        )?,
        observation: observation.clone(),
        dind_attachment_absent: true,
        channel_digest: String::new(),
    };
    let mut binding = binding;
    binding.channel_digest = binding.computed_digest()?;
    if binding.valid() {
        Ok(binding)
    } else {
        Err(HostError::Identity)
    }
}

pub(super) async fn within<T>(
    deadline: Instant,
    future: impl Future<Output = T>,
) -> Result<T, HostError> {
    tokio::time::timeout_at(TokioInstant::from_std(deadline), future)
        .await
        .map_err(|_| HostError::DockerTimeout)
}

pub(super) async fn read_stdout_line<S>(
    output: &mut Pin<Box<S>>,
    deadline: Instant,
) -> Result<Vec<u8>, HostError>
where
    S: Stream<Item = Result<LogOutput, bollard::errors::Error>> + Send + ?Sized,
{
    let mut bytes = Vec::with_capacity(4096);
    while let Some(frame) = within(deadline, output.next()).await? {
        let frame = frame.map_err(|_| HostError::Docker)?;
        let chunk = match frame {
            LogOutput::StdOut { message } => message,
            LogOutput::StdErr { .. } | LogOutput::Console { .. } | LogOutput::StdIn { .. } => {
                return Err(HostError::Frame);
            }
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

pub(super) async fn drain_empty_output<S>(
    output: &mut Pin<Box<S>>,
    deadline: Instant,
) -> Result<(), HostError>
where
    S: Stream<Item = Result<LogOutput, bollard::errors::Error>> + Send + ?Sized,
{
    while let Some(frame) = within(deadline, output.next()).await? {
        if !frame.map_err(|_| HostError::Docker)?.as_ref().is_empty() {
            return Err(HostError::Frame);
        }
    }
    Ok(())
}

pub(super) fn encode_line<T: Serialize>(value: &T, limit: usize) -> Result<Vec<u8>, HostError> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| HostError::Frame)?;
    if bytes.is_empty() || bytes.len() >= limit {
        bytes.zeroize();
        return Err(HostError::Frame);
    }
    bytes.push(b'\n');
    Ok(bytes)
}

pub(super) fn decode_line<T>(bytes: &[u8], limit: usize) -> Result<T, HostError>
where
    T: DeserializeOwned + Serialize,
{
    if bytes.len() > limit + 1 || bytes.last() != Some(&b'\n') {
        return Err(HostError::Frame);
    }
    let record: T =
        serde_json::from_slice(&bytes[..bytes.len() - 1]).map_err(|_| HostError::Frame)?;
    if serde_json::to_vec(&record).map_err(|_| HostError::Frame)? != bytes[..bytes.len() - 1] {
        return Err(HostError::Frame);
    }
    Ok(record)
}
