// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use bollard::query_parameters::WaitContainerOptionsBuilder;
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

const RAW_LIMIT: usize = 64 * 1024;

pub async fn read_logs(
    docker: &bollard::Docker,
    id: &str,
    timeout: Duration,
) -> Result<Vec<u8>, HostError> {
    // bollard's log decoder hides a frame header whose payload never arrives, so the
    // logs endpoint is read as raw bytes and the multiplexed frames are checked here.
    let raw_docker = docker.clone().with_request_modifier(|mut request| {
        let uri = request.uri().to_string();
        if let Some(position) = uri.rfind("/export") {
            let logs = format!(
                "{}/logs?follow=false&stdout=true&stderr=true&timestamps=false&tail=all",
                &uri[..position]
            );
            if let Ok(parsed) = logs.parse() {
                *request.uri_mut() = parsed;
            }
        }
        request
    });
    let read = async {
        let mut stream = raw_docker.export_container(id);
        let mut raw = Vec::with_capacity(512);
        while let Some(item) = stream.next().await {
            let chunk = item.map_err(|_| HostError::Docker)?;
            if raw.len().saturating_add(chunk.len()) > RAW_LIMIT {
                return Err(HostError::Frame);
            }
            raw.extend_from_slice(&chunk);
        }
        parse_frames(&raw)
    };
    deadline(read, timeout).await?
}

fn parse_frames(mut raw: &[u8]) -> Result<Vec<u8>, HostError> {
    let mut output = Vec::with_capacity(512);
    while !raw.is_empty() {
        let (header, rest) = raw.split_at_checked(8).ok_or(HostError::Frame)?;
        let length = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);
        let length = usize::try_from(length).map_err(|_| HostError::Frame)?;
        if header[1..4] != [0, 0, 0] || length > rest.len() {
            return Err(HostError::Frame);
        }
        let (payload, rest) = rest.split_at(length);
        match header[0] {
            1 => append(&mut output, payload)?,
            2 if payload.is_empty() => {}
            2 => return Err(HostError::Docker),
            _ => return Err(HostError::Frame),
        }
        raw = rest;
    }
    Ok(output)
}

pub(super) fn append(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), HostError> {
    if output.len().saturating_add(bytes.len()) > OUTPUT_LIMIT {
        return Err(HostError::Frame);
    }
    output.extend_from_slice(bytes);
    Ok(())
}
