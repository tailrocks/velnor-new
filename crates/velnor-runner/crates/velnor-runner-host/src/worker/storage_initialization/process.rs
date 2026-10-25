use std::time::Duration;

use bollard::{
    Docker,
    container::{AttachContainerResults, LogOutput},
};
use futures_util::StreamExt;
use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    time::Instant,
};

use crate::{
    error::HostError,
    journal::{ConnectionJournal, StorageInitializationClaim},
};

use super::{
    ObservedStorageInitialization,
    adapter::same_start_claim,
    proof::VerifiedStorageInitializerStarted,
    request_result::{
        StorageInitializationAcknowledgment, StorageInitializationRequest, parse_ready,
    },
    verification::{inspect_owned_container, verify_engine},
};

const PROCESS_LIMIT: Duration = Duration::from_secs(30);
const STDOUT_LIMIT: usize = 2_048;

/// Send one request and retain the live helper after its single READY record.
pub(super) async fn run(
    docker: &Docker,
    journal: &ConnectionJournal,
    claim: &StorageInitializationClaim,
    started: &VerifiedStorageInitializerStarted,
    mut attached: AttachContainerResults,
    deadline: Instant,
) -> Result<(ObservedStorageInitialization, AttachContainerResults), HostError> {
    if !same_start_claim(claim, started) || Instant::now() >= deadline {
        return Err(HostError::Identity);
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining > PROCESS_LIMIT {
        return Err(HostError::Identity);
    }
    let result = run_inner(docker, journal, claim, started, &mut attached, deadline).await;
    match result {
        Ok(observed) => Ok((observed, attached)),
        Err(error) => {
            journal
                .mark_storage_initialization_result_uncertain(claim)
                .await?;
            Err(error)
        }
    }
}

/// Release a READY helper only after the caller has durably recorded pair mounts.
pub(super) async fn finish(
    docker: &Docker,
    journal: &ConnectionJournal,
    claim: &StorageInitializationClaim,
    started: &VerifiedStorageInitializerStarted,
    attached: &mut AttachContainerResults,
    deadline: Instant,
) -> Result<(), HostError> {
    if !same_start_claim(claim, started) || Instant::now() >= deadline {
        return Err(HostError::Identity);
    }
    verify_engine(docker, journal, claim).await?;
    let inspected = inspect_owned_container(docker, claim, started.container_id()).await?;
    if !inspected.state.as_ref().is_some_and(|state| {
        state.status == Some(bollard::models::ContainerStateStatusEnum::RUNNING)
    }) {
        return Err(HostError::Ownership);
    }
    let acknowledgment = StorageInitializationAcknowledgment::new(claim.operation_key())?;
    attached
        .input
        .write_all(&acknowledgment.stdin_payload()?)
        .await
        .map_err(|_| HostError::Docker)?;
    attached
        .input
        .shutdown()
        .await
        .map_err(|_| HostError::Docker)?;
    let drain = async {
        while let Some(item) = attached.output.next().await {
            let item = item.map_err(|_| HostError::Docker)?;
            match item {
                LogOutput::StdOut { message } if message.is_empty() => {}
                LogOutput::StdErr { message } if message.is_empty() => {}
                LogOutput::StdIn { .. } => {}
                LogOutput::StdOut { .. } | LogOutput::StdErr { .. } | LogOutput::Console { .. } => {
                    return Err(HostError::Identity);
                }
            }
        }
        Ok::<(), HostError>(())
    };
    let wait = async {
        let mut stream = docker.wait_container(started.container_id(), None).boxed();
        stream
            .next()
            .await
            .ok_or(HostError::Docker)?
            .map_err(|_| HostError::Docker)
    };
    let (drained, status) = tokio::time::timeout_at(deadline, async { tokio::join!(drain, wait) })
        .await
        .map_err(|_| HostError::DockerTimeout)?;
    drained?;
    if status?.status_code != 0 {
        return Err(HostError::Identity);
    }
    Ok(())
}

async fn run_inner(
    docker: &Docker,
    journal: &ConnectionJournal,
    claim: &StorageInitializationClaim,
    started: &VerifiedStorageInitializerStarted,
    attached: &mut AttachContainerResults,
    deadline: Instant,
) -> Result<ObservedStorageInitialization, HostError> {
    verify_engine(docker, journal, claim).await?;
    inspect_owned_container(docker, claim, started.container_id()).await?;
    let request = StorageInitializationRequest::new(claim.operation_key())?;
    let payload = request.stdin_payload()?;
    send_request(&mut attached.input, &payload).await?;
    let mut bytes = Vec::new();
    loop {
        let item = tokio::time::timeout_at(deadline, attached.output.next())
            .await
            .map_err(|_| HostError::DockerTimeout)?
            .ok_or(HostError::Docker)?
            .map_err(|_| HostError::Docker)?;
        let complete = append_ready_output(&mut bytes, item)?;
        if complete {
            break;
        }
    }
    let observed = parse_ready(claim.operation_key(), &bytes)?;
    let inspected = inspect_owned_container(docker, claim, started.container_id()).await?;
    if !inspected.state.as_ref().is_some_and(|state| {
        state.status == Some(bollard::models::ContainerStateStatusEnum::RUNNING)
    }) {
        return Err(HostError::Ownership);
    }
    Ok(observed)
}

async fn send_request<W: AsyncWrite + Unpin>(
    input: &mut W,
    payload: &[u8],
) -> Result<(), HostError> {
    input
        .write_all(payload)
        .await
        .map_err(|_| HostError::Docker)
}

fn append_ready_output(bytes: &mut Vec<u8>, item: LogOutput) -> Result<bool, HostError> {
    let chunk = match item {
        LogOutput::StdOut { message } => message,
        LogOutput::StdErr { message } if message.is_empty() => return Ok(false),
        LogOutput::StdErr { .. } | LogOutput::Console { .. } => return Err(HostError::Identity),
        LogOutput::StdIn { .. } => return Ok(false),
    };
    if bytes
        .len()
        .checked_add(chunk.len())
        .is_none_or(|length| length > STDOUT_LIMIT)
    {
        return Err(HostError::Frame);
    }
    if let Some(newline) = chunk.iter().position(|byte| *byte == b'\n') {
        if newline + 1 != chunk.len() || bytes.contains(&b'\n') {
            return Err(HostError::Identity);
        }
        bytes.extend_from_slice(&chunk);
        Ok(true)
    } else {
        bytes.extend_from_slice(&chunk);
        Ok(false)
    }
}

#[cfg(test)]
fn append_output(bytes: &mut Vec<u8>, item: LogOutput) -> Result<(), HostError> {
    let chunk = match item {
        LogOutput::StdOut { message } => message,
        LogOutput::StdErr { message } if message.is_empty() => return Ok(()),
        LogOutput::StdErr { .. } | LogOutput::Console { .. } => return Err(HostError::Identity),
        LogOutput::StdIn { .. } => return Ok(()),
    };
    if bytes
        .len()
        .checked_add(chunk.len())
        .is_none_or(|length| length > STDOUT_LIMIT)
    {
        return Err(HostError::Frame);
    }
    bytes.extend_from_slice(&chunk);
    Ok(())
}

#[cfg(test)]
mod tests {
    use bollard::container::LogOutput;
    use bytes::Bytes;

    use super::append_output;
    use crate::error::HostError;

    #[test]
    fn output_chunks_are_bounded_and_keep_stdout_order() -> Result<(), HostError> {
        let mut output = Vec::new();
        append_output(
            &mut output,
            LogOutput::StdOut {
                message: Bytes::from_static(b"{\"schema\":"),
            },
        )?;
        append_output(
            &mut output,
            LogOutput::StdOut {
                message: Bytes::from_static(b"1}\n"),
            },
        )?;
        assert_eq!(output, b"{\"schema\":1}\n");
        assert_eq!(
            append_output(
                &mut vec![0; 2_048],
                LogOutput::StdOut {
                    message: Bytes::from_static(b"x")
                }
            ),
            Err(HostError::Frame)
        );
        Ok(())
    }

    #[test]
    fn output_rejects_stderr_and_console_frames() {
        assert_eq!(
            append_output(
                &mut Vec::new(),
                LogOutput::StdErr {
                    message: Bytes::from_static(b"failure")
                }
            ),
            Err(HostError::Identity)
        );
        assert_eq!(
            append_output(
                &mut Vec::new(),
                LogOutput::Console {
                    message: Bytes::from_static(b"merged")
                }
            ),
            Err(HostError::Identity)
        );
    }

    #[tokio::test]
    async fn request_is_written_once_without_closing_stdin() -> Result<(), HostError> {
        let (mut input, mut output) = tokio::io::duplex(128);
        let payload = b"{\"operation_id\":\"17-1\"}\n";
        let sent = super::send_request(&mut input, payload);
        let received = async {
            use tokio::io::AsyncReadExt;
            let mut bytes = [0; 64];
            let count = output
                .read(&mut bytes)
                .await
                .map_err(|_| HostError::Docker)?;
            Ok::<_, HostError>(bytes[..count].to_vec())
        };
        let (sent, received) = tokio::join!(sent, received);
        assert_eq!(sent, Ok(()));
        assert_eq!(received?, payload);
        Ok(())
    }
}
