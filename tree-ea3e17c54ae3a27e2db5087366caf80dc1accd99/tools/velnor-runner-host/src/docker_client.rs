//! Unix-socket Docker client. The configured path is the only endpoint.

use std::future::Future;

use bollard::errors::Error as DockerError;
use std::time::Duration;

use crate::HostError;
use crate::scale_set::EnsureError;

/// Deadline for one request to the selected Docker engine.
pub(crate) const DOCKER_OPERATION_TIMEOUT: Duration = Duration::from_secs(10);

/// Bound one Docker request while preserving its result for caller classification.
///
/// # Errors
///
/// Returns [`HostError::Docker`] when the request exceeds the operation timeout.
pub async fn docker_deadline<F: Future>(future: F) -> Result<F::Output, HostError> {
    docker_deadline_after(future, DOCKER_OPERATION_TIMEOUT).await
}

/// Deadline seam for tests and callers with a narrower operation budget.
pub(crate) async fn docker_deadline_after<F: Future>(
    future: F,
    timeout: Duration,
) -> Result<F::Output, HostError> {
    tokio::time::timeout(timeout, future)
        .await
        .map_err(|_| HostError::Docker)
}

/// Open the selected socket. This is the only constructor the host calls.
///
/// # Errors
///
/// Returns [`HostError::Docker`] when `socket` is not an absolute Unix path,
/// uses `tcp://`, or cannot be opened.
pub fn connect_unix(socket: &str) -> Result<bollard::Docker, HostError> {
    let path = unix_socket_path(socket)?;
    bollard::Docker::connect_with_unix(&path, 120, bollard::API_DEFAULT_VERSION)
        .map_err(|_| HostError::Docker)
}

/// Absolute path after an optional `unix://` prefix. `tcp://` never reaches Bollard.
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

#[cfg(test)]
mod tests;

/// Classify a container-inspect outcome fail-closed (moved from launch).
///
/// # Errors
///
/// Returns [`EnsureError::Unexpected`] with step `"docker inspect"` when the
/// running state is missing or Docker reports a non-404 failure.
pub fn classify_inspect(
    response: Result<bollard::models::ContainerInspectResponse, DockerError>,
) -> Result<bool, EnsureError> {
    match response {
        Ok(info) => match info.state.and_then(|state| state.running) {
            Some(running) => Ok(running),
            None => Err(inspect_error(200)),
        },
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(false),
        Err(DockerError::DockerResponseServerError { status_code, .. }) => {
            Err(inspect_error(status_code))
        }
        Err(_) => Err(inspect_error(0)),
    }
}

/// Fail-closed inspect error carrying the Docker status code.
#[must_use]
pub const fn inspect_error(status: u16) -> EnsureError {
    EnsureError::Unexpected {
        status,
        step: "docker inspect",
    }
}
