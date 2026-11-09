// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use bollard::container::LogOutput;
use bollard::query_parameters::{LogsOptionsBuilder, WaitContainerOptionsBuilder};
use futures_util::StreamExt;

use crate::error::QualificationError as HostError;
use crate::{OUTPUT_LIMIT, deadline};

pub(super) async fn wait_success(
    docker: &bollard::Docker,
    id: &str,
    timeout: Duration,
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
        if response.status_code == 0 && response.error.is_none() {
            Ok(())
        } else {
            Err(HostError::Docker)
        }
    };
    deadline(wait, timeout).await?
}

pub(super) async fn read_logs(
    docker: &bollard::Docker,
    id: &str,
    timeout: Duration,
) -> Result<Vec<u8>, HostError> {
    let options = LogsOptionsBuilder::new()
        .follow(false)
        .stdout(true)
        .stderr(true)
        .timestamps(false)
        .tail("all")
        .build();
    let read = async {
        let mut stream = docker.logs(id, Some(options));
        let mut output = Vec::with_capacity(512);
        while let Some(item) = stream.next().await {
            match item.map_err(|_| HostError::Docker)? {
                LogOutput::StdOut { message } => append(&mut output, &message)?,
                LogOutput::StdErr { message } if message.is_empty() => {}
                _ => return Err(HostError::Docker),
            }
        }
        Ok(output)
    };
    deadline(read, timeout).await?
}

pub(super) fn append(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), HostError> {
    if output.len().saturating_add(bytes.len()) > OUTPUT_LIMIT {
        return Err(HostError::Frame);
    }
    output.extend_from_slice(bytes);
    Ok(())
}
