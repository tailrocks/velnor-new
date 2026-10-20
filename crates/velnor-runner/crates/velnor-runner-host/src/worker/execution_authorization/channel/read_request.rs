//! Read one fixed source-proof request from the measured Runner inbox.

use std::{pin::Pin, time::Instant};

use bollard::{
    Docker,
    container::LogOutput,
    exec::{CreateExecOptions, StartExecOptions, StartExecResults},
};
use futures_util::Stream;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use velnor_resource_probe::{
    execution_channel_wire::{ExecutionRequestContext, ExecutionRequestOutcome},
    worker_binding::{
        AcknowledgeRequest, WORKER_ACK_LIMIT, WORKER_RECORD_LIMIT, WORKER_REQUEST_LIMIT,
    },
};

use crate::{
    error::HostError,
    launch::{SelectedEngineContext, VerifiedWorkerImages},
};

mod encoding;
use encoding::{REQUEST_LIMIT, decode_lower_hex, lower_hex};

use super::{
    VerifiedProofChannelBinding,
    measure::{decode_line, drain_empty_output, encode_line, read_stdout_line, within},
};

const HELPER_PATH: &str = "/velnor/resource-probe";
const MODE_ARGUMENT: &str = "execution_request";

/// One request read from the exact Runner channel.
pub(crate) struct VerifiedExecutionRequest {
    bytes: Vec<u8>,
}

/// Result of one bounded read from a measured Runner inbox.
pub(crate) enum ExecutionRequestRead {
    /// The request did not exist on its first no-follow open.
    NotYetPresent,
    /// The request was read and its file and channel identities were validated.
    Present(VerifiedExecutionRequest),
}

impl VerifiedExecutionRequest {
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Read a bounded request through the fixed helper inside its verified Runner.
pub(crate) async fn read_execution_request(
    docker: &Docker,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    channel: &VerifiedProofChannelBinding,
    deadline: Instant,
) -> Result<ExecutionRequestRead, HostError> {
    if !channel.valid()
        || channel.engine_id() != engine.engine_id()
        || channel.docker_root_digest() != engine.docker_root_digest()
    {
        return Err(HostError::Identity);
    }
    within(deadline, images.verify_for_engine(docker, engine)).await??;
    inspect_runner(docker, images, channel.runner_container_id(), deadline).await?;

    let operation_id = uuid::Uuid::new_v4().simple().to_string();
    let mut expected_channel = channel.observation().clone();
    expected_channel.operation_id = operation_id.clone();
    let context = ExecutionRequestContext {
        schema_version: 3,
        operation_id: operation_id.clone(),
        expected_guest_boot_id: expected_channel.guest_boot_id.clone(),
        expected_channel,
    };
    run_request_exec(docker, images, channel, context, deadline).await
}

async fn run_request_exec(
    docker: &Docker,
    images: &VerifiedWorkerImages,
    channel: &VerifiedProofChannelBinding,
    context: ExecutionRequestContext,
    deadline: Instant,
) -> Result<ExecutionRequestRead, HostError> {
    let operation_id = context.operation_id.clone();
    let mut request_bytes = encode_line(&context, WORKER_REQUEST_LIMIT)?;
    let (exec_id, attached) =
        start_request_exec(docker, channel.runner_container_id(), deadline).await?;
    let StartExecResults::Attached {
        mut output,
        mut input,
    } = attached
    else {
        return Err(HostError::Docker);
    };
    within(deadline, input.write_all(&request_bytes))
        .await?
        .map_err(|_| HostError::Docker)?;
    request_bytes.fill(0);
    within(deadline, input.flush())
        .await?
        .map_err(|_| HostError::Docker)?;

    let record_line = read_stdout_line(&mut output, deadline).await?;
    let outcome = decode_line::<ExecutionRequestOutcome>(&record_line, WORKER_RECORD_LIMIT)?;
    let result = validate_outcome(
        outcome,
        &operation_id,
        &context.expected_guest_boot_id,
        &context.expected_channel,
    )?;
    acknowledge_read(
        ReadAcknowledgement {
            docker,
            channel,
            exec_id: &exec_id,
            operation_id: &operation_id,
            images,
            deadline,
        },
        &mut input,
        &mut output,
    )
    .await?;
    Ok(result)
}

async fn start_request_exec(
    docker: &Docker,
    container_id: &str,
    deadline: Instant,
) -> Result<(String, StartExecResults), HostError> {
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
    let attached = within(
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
    .map_err(|_| HostError::Docker)?;
    Ok((exec.id, attached))
}

struct ReadAcknowledgement<'a> {
    docker: &'a Docker,
    channel: &'a VerifiedProofChannelBinding,
    exec_id: &'a str,
    operation_id: &'a str,
    images: &'a VerifiedWorkerImages,
    deadline: Instant,
}

async fn acknowledge_read<S>(
    acknowledgement_context: ReadAcknowledgement<'_>,
    input: &mut (impl tokio::io::AsyncWrite + Unpin),
    output: &mut Pin<Box<S>>,
) -> Result<(), HostError>
where
    S: Stream<Item = Result<LogOutput, bollard::errors::Error>> + Send + ?Sized,
{
    let ReadAcknowledgement {
        docker,
        channel,
        exec_id,
        operation_id,
        images,
        deadline,
    } = acknowledgement_context;
    let acknowledgement = encode_line(
        &AcknowledgeRequest {
            operation_id: operation_id.to_owned(),
        },
        WORKER_ACK_LIMIT,
    )?;
    within(deadline, input.write_all(&acknowledgement))
        .await?
        .map_err(|_| HostError::Docker)?;
    within(deadline, input.shutdown())
        .await?
        .map_err(|_| HostError::Docker)?;
    drain_empty_output(output, deadline).await?;
    let exit = within(deadline, docker.inspect_exec(exec_id))
        .await?
        .map_err(|_| HostError::Docker)?;
    if exit.id.as_deref() != Some(exec_id)
        || exit.container_id.as_deref() != Some(channel.runner_container_id())
        || exit.running != Some(false)
        || exit.exit_code != Some(0)
    {
        return Err(HostError::Ownership);
    }
    inspect_runner(docker, images, channel.runner_container_id(), deadline).await?;
    Ok(())
}

fn validate_outcome(
    outcome: ExecutionRequestOutcome,
    operation_id: &str,
    expected_guest_boot_id: &str,
    expected_channel: &velnor_resource_probe::execution_channel_wire::ExecutionChannelObservation,
) -> Result<ExecutionRequestRead, HostError> {
    match outcome {
        ExecutionRequestOutcome::NotYetPresent {
            schema_version,
            operation_id: response_operation_id,
            guest_boot_id,
            channel: observed_channel,
        } => {
            if schema_version != 3
                || response_operation_id != operation_id
                || guest_boot_id != expected_guest_boot_id
                || &observed_channel != expected_channel
            {
                return Err(HostError::Identity);
            }
            Ok(ExecutionRequestRead::NotYetPresent)
        }
        ExecutionRequestOutcome::Present { record } => {
            let bytes = decode_lower_hex(&record.bytes_hex)?;
            if record.schema_version != 3
                || record.operation_id != operation_id
                || record.guest_boot_id != expected_guest_boot_id
                || &record.channel != expected_channel
                || record.inode == 0
                || record.owner_uid != 1000
                || record.owner_gid != 1000
                || record.mode != 0o600
                || bytes.is_empty()
                || usize::try_from(record.size).ok() != Some(bytes.len())
                || bytes.len() > REQUEST_LIMIT
                || record.sha256 != lower_hex(&Sha256::digest(&bytes))
            {
                return Err(HostError::Identity);
            }
            Ok(ExecutionRequestRead::Present(VerifiedExecutionRequest {
                bytes,
            }))
        }
    }
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

#[cfg(test)]
mod tests {
    use super::{ExecutionRequestRead, decode_lower_hex, validate_outcome};
    use velnor_resource_probe::execution_channel_wire::{
        ExecutionChannelDirectory, ExecutionChannelObservation, ExecutionRequestOutcome,
    };

    fn channel() -> ExecutionChannelObservation {
        let directory = ExecutionChannelDirectory {
            target: "/opt/velnor/source-proof/control".to_owned(),
            inode: 10,
            owner_uid: 0,
            owner_gid: 0,
            mode: 0o555,
            mount_id: 11,
            statx_mount_id: 11,
            mount_root: "/source-proof/control".to_owned(),
            mount_source: "/dev/vda".to_owned(),
            mount_device_major: 254,
            mount_device_minor: 1,
            statx_device_major: 254,
            statx_device_minor: 1,
            filesystem_type_magic: 0x9123_683e,
            filesystem_type_name: "btrfs".to_owned(),
            filesystem_id: 12,
            read_only: true,
            executable_mount: false,
        };
        ExecutionChannelObservation {
            schema_version: 3,
            operation_id: "a".repeat(32),
            guest_boot_id: "boot".to_owned(),
            mount_namespace_id: 13,
            control: directory.clone(),
            inbox: ExecutionChannelDirectory {
                target: "/opt/velnor/source-proof/inbox".to_owned(),
                inode: 14,
                owner_uid: 1000,
                owner_gid: 1000,
                mode: 0o700,
                ..directory.clone()
            },
            job_facts: ExecutionChannelDirectory {
                target: "/opt/velnor/job-facts".to_owned(),
                inode: 15,
                owner_uid: 0,
                owner_gid: 0,
                mode: 0o555,
                ..directory
            },
        }
    }

    #[test]
    fn request_hex_requires_bounded_lowercase_pairs() {
        assert_eq!(
            decode_lower_hex("00a1ff").expect("valid hex"),
            [0, 0xa1, 0xff]
        );
        assert!(decode_lower_hex("0").is_err());
        assert!(decode_lower_hex("AA").is_err());
        assert!(decode_lower_hex(&"a1".repeat(16 * 1024 + 1)).is_err());
    }

    #[test]
    fn not_yet_present_requires_the_exact_operation_boot_and_channel() {
        let expected = channel();
        let absent = ExecutionRequestOutcome::NotYetPresent {
            schema_version: 3,
            operation_id: "b".repeat(32),
            guest_boot_id: "boot".to_owned(),
            channel: expected.clone(),
        };
        assert!(matches!(
            validate_outcome(absent.clone(), &"b".repeat(32), "boot", &expected),
            Ok(ExecutionRequestRead::NotYetPresent)
        ));
        assert!(validate_outcome(absent.clone(), &"c".repeat(32), "boot", &expected).is_err());
        assert!(validate_outcome(absent.clone(), &"b".repeat(32), "other", &expected).is_err());
        let mut changed_channel = expected.clone();
        changed_channel.inbox.inode += 1;
        assert!(validate_outcome(absent, &"b".repeat(32), "boot", &changed_channel).is_err());
    }
}
