use std::{pin::Pin, time::Instant};

use bollard::container::LogOutput;
use futures_util::{Stream, StreamExt};
use serde::{Serialize, de::DeserializeOwned};
use tokio::time::Instant as TokioInstant;
use velnor_resource_probe::worker_binding::WORKER_RECORD_LIMIT;
use zeroize::Zeroize;

use crate::error::HostError;

pub(in crate::worker::execution_authorization::channel) async fn within<T>(
    deadline: Instant,
    future: impl Future<Output = T>,
) -> Result<T, HostError> {
    tokio::time::timeout_at(TokioInstant::from_std(deadline), future)
        .await
        .map_err(|_| HostError::DockerTimeout)
}

use std::future::Future;

pub(in crate::worker::execution_authorization::channel) async fn read_stdout_line<S>(
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

pub(in crate::worker::execution_authorization::channel) async fn drain_empty_output<S>(
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

pub(in crate::worker::execution_authorization::channel) fn encode_line<T: Serialize>(
    value: &T,
    limit: usize,
) -> Result<Vec<u8>, HostError> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| HostError::Frame)?;
    if bytes.is_empty() || bytes.len() >= limit {
        bytes.zeroize();
        return Err(HostError::Frame);
    }
    bytes.push(b'\n');
    Ok(bytes)
}

pub(in crate::worker::execution_authorization::channel) fn decode_line<T>(
    bytes: &[u8],
    limit: usize,
) -> Result<T, HostError>
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
