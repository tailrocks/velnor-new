use std::{pin::Pin, time::Instant};

use bollard::{
    Docker,
    exec::{CreateExecOptions, StartExecOptions, StartExecResults},
};
use tokio::io::AsyncWriteExt;
use velnor_resource_probe::{
    execution_channel_wire::{ExecutionChannelContext, ExecutionChannelObservation},
    worker_binding::{AcknowledgeRequest, WORKER_RECORD_LIMIT, WORKER_REQUEST_LIMIT},
};
use zeroize::Zeroize;

use crate::{
    error::HostError,
    launch::bootstrap::{BootstrapStartReceipt, VerifiedStorageResourceBinding},
};

use super::{
    acknowledgment::validated_then_ack, attachments::validate_storage_attachments, decode_line,
    encode_line, protocol, read_stdout_line, validate_observation, within,
};

const HELPER_PATH: &str = "/velnor/resource-probe";
const ACK_LIMIT: usize = 128;

pub(super) struct ChannelExchange<'a> {
    pub(super) container_id: &'a str,
    pub(super) request: &'a ExecutionChannelContext,
    pub(super) storage: &'a VerifiedStorageResourceBinding,
    pub(super) started: &'a BootstrapStartReceipt,
    pub(super) deadline: Instant,
}

pub(super) async fn exchange_channel_record(
    docker: &Docker,
    exchange: ChannelExchange<'_>,
) -> Result<ExecutionChannelObservation, HostError> {
    let ChannelExchange {
        container_id,
        request,
        storage,
        started,
        deadline,
    } = exchange;
    let mut request_bytes = encode_line(request, WORKER_REQUEST_LIMIT)?;
    let exec = create_exec(docker, container_id, deadline).await?;
    let StartExecResults::Attached {
        mut output,
        mut input,
    } = attach_exec(docker, &exec.id, deadline).await?
    else {
        return Err(HostError::Docker);
    };
    send_request(&mut input, &mut request_bytes, deadline).await?;
    let record_bytes = read_stdout_line(&mut output, deadline).await?;
    let observation: ExecutionChannelObservation = decode_line(&record_bytes, WORKER_RECORD_LIMIT)?;
    validated_then_ack(
        || {
            validate_observation(&observation, &request.operation_id, storage.guest_boot_id())?;
            validate_storage_attachments(&observation, storage, started)
        },
        || async {
            acknowledge_exec(
                docker,
                container_id,
                &exec.id,
                &request.operation_id,
                &mut input,
                &mut output,
                deadline,
            )
            .await
        },
    )
    .await?;
    Ok(observation)
}

async fn create_exec(
    docker: &Docker,
    container_id: &str,
    deadline: Instant,
) -> Result<bollard::exec::CreateExecResults, HostError> {
    within(
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
                    "execution_channel".to_owned(),
                ]),
                ..Default::default()
            },
        ),
    )
    .await?
    .map_err(|_| HostError::Docker)
    .and_then(|exec| {
        if exec.id.is_empty() {
            Err(HostError::Identity)
        } else {
            Ok(exec)
        }
    })
}

async fn attach_exec(
    docker: &Docker,
    exec_id: &str,
    deadline: Instant,
) -> Result<StartExecResults, HostError> {
    within(
        deadline,
        docker.start_exec(
            exec_id,
            Some(StartExecOptions {
                detach: false,
                tty: false,
                output_capacity: Some(WORKER_RECORD_LIMIT + ACK_LIMIT),
            }),
        ),
    )
    .await?
    .map_err(|_| HostError::Docker)
}

async fn send_request(
    input: &mut (impl tokio::io::AsyncWrite + Unpin),
    request_bytes: &mut Vec<u8>,
    deadline: Instant,
) -> Result<(), HostError> {
    within(deadline, input.write_all(request_bytes))
        .await?
        .map_err(|_| HostError::Docker)?;
    request_bytes.zeroize();
    within(deadline, input.flush())
        .await?
        .map_err(|_| HostError::Docker)
}

async fn acknowledge_exec<S>(
    docker: &Docker,
    container_id: &str,
    exec_id: &str,
    operation_id: &str,
    input: &mut (impl tokio::io::AsyncWrite + Unpin),
    output: &mut Pin<Box<S>>,
    deadline: Instant,
) -> Result<(), HostError>
where
    S: futures_util::Stream<Item = Result<bollard::container::LogOutput, bollard::errors::Error>>
        + Send
        + ?Sized,
{
    let ack = encode_line(
        &AcknowledgeRequest {
            operation_id: operation_id.to_owned(),
        },
        ACK_LIMIT,
    )?;
    within(deadline, input.write_all(&ack))
        .await?
        .map_err(|_| HostError::Docker)?;
    within(deadline, input.shutdown())
        .await?
        .map_err(|_| HostError::Docker)?;
    protocol::drain_empty_output(output, deadline).await?;
    let exit = within(deadline, docker.inspect_exec(exec_id))
        .await?
        .map_err(|_| HostError::Docker)?;
    if exit.id.as_deref() != Some(exec_id)
        || exit.container_id.as_deref() != Some(container_id)
        || exit.running != Some(false)
        || exit.exit_code != Some(0)
    {
        return Err(HostError::Docker);
    }
    Ok(())
}
