// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::time::Duration;

use bollard::models::SystemInfo;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response, body::Body, client::conn::http1};
use hyper_util::rt::TokioIo;

use crate::error::HostError;

pub(crate) const DOCKER_OPERATION_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) async fn docker_deadline<F: Future>(future: F) -> Result<F::Output, HostError> {
    docker_deadline_after(future, DOCKER_OPERATION_TIMEOUT).await
}

pub(crate) async fn docker_deadline_after<F: Future>(
    future: F,
    timeout: Duration,
) -> Result<F::Output, HostError> {
    tokio::time::timeout(timeout, future)
        .await
        .map_err(|_| HostError::Docker)
}

pub(crate) const MAX_DOCKER_BODY_BYTES: usize = 4 * 1024 * 1024;

pub(crate) async fn collect_bounded<B>(
    response: Response<B>,
    limit: usize,
) -> Result<Vec<u8>, HostError>
where
    B: Body<Data = Bytes> + Unpin,
{
    if response
        .body()
        .size_hint()
        .upper()
        .is_some_and(|size| size > limit as u64)
    {
        return Err(HostError::Frame);
    }
    let mut body = response.into_body();
    let mut bytes = Vec::new();
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|_| HostError::Docker)?;
        if let Some(data) = frame.data_ref() {
            if bytes
                .len()
                .checked_add(data.len())
                .is_none_or(|size| size > limit)
            {
                return Err(HostError::Frame);
            }
            bytes.extend_from_slice(data);
        }
    }
    Ok(bytes)
}

pub(crate) async fn bounded_info(socket: &str) -> Result<SystemInfo, HostError> {
    let path = unix_socket_path(socket)?;
    let stream = tokio::net::UnixStream::connect(path)
        .await
        .map_err(|_| HostError::Docker)?;
    let (mut sender, connection) = http1::handshake(TokioIo::new(stream))
        .await
        .map_err(|_| HostError::Docker)?;
    let task = tokio::spawn(async move {
        drop(connection.await);
    });
    let request = Request::builder()
        .uri(format!(
            "/v{}.{}/info",
            bollard::API_DEFAULT_VERSION.major_version,
            bollard::API_DEFAULT_VERSION.minor_version
        ))
        .header("host", "localhost")
        .body(Full::<Bytes>::default())
        .map_err(|_| HostError::Docker);
    let result = match request {
        Ok(request) => match sender.send_request(request).await {
            Ok(response) if response.status().is_success() => {
                collect_bounded(response, MAX_DOCKER_BODY_BYTES).await
            }
            Ok(_) | Err(_) => Err(HostError::Docker),
        },
        Err(error) => Err(error),
    };
    task.abort();
    serde_json::from_slice(&result?).map_err(|_| HostError::Docker)
}

pub fn connect_unix(socket: &str) -> Result<bollard::Docker, HostError> {
    let path = unix_socket_path(socket)?;
    bollard::Docker::connect_with_unix(&path, 120, bollard::API_DEFAULT_VERSION)
        .map_err(|_| HostError::Docker)
}

fn unix_socket_path(socket: &str) -> Result<String, HostError> {
    if scheme_is(socket, "tcp") {
        return Err(HostError::Docker);
    }
    let path = socket.strip_prefix("unix://").unwrap_or(socket);
    if path.starts_with('/') && path.len() > 1 && !path.chars().any(char::is_control) {
        Ok(path.to_owned())
    } else {
        Err(HostError::Docker)
    }
}

fn scheme_is(socket: &str, scheme: &str) -> bool {
    match socket.split_once("://") {
        Some((found, _)) => found.eq_ignore_ascii_case(scheme),
        None => false,
    }
}
