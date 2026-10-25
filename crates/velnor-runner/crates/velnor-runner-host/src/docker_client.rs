use std::future::Future;
use std::time::Duration;

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
