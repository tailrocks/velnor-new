//! Unix-socket Docker client. The configured path is the only endpoint.

use std::fmt;
use std::future::Future;

use bollard::errors::Error as DockerError;
use std::time::Duration;
use tokio::time::Instant;

use crate::HostError;
use crate::scale_set::EnsureError;

pub(crate) mod daemon_guard;

/// Deadline for one request to the selected Docker engine.
pub(crate) const DOCKER_OPERATION_TIMEOUT: Duration = Duration::from_secs(10);

/// Sanitized data returned by the Docker Engine's read-only `/version` endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerVersion {
    /// Server version string.
    pub server_version: String,
    /// API version when supplied by the server.
    pub api_version: Option<String>,
    /// Server operating system when supplied by the server.
    pub os: Option<String>,
    /// Server architecture when supplied by the server.
    pub architecture: Option<String>,
}

/// Opaque identity binding for one configured Docker Unix endpoint.
///
/// The daemon ID is copied byte-for-byte from Docker's read-only `/info` response.
/// Its format is deliberately not interpreted. Debug output redacts both fields.
#[derive(Clone, PartialEq, Eq)]
pub struct DockerDaemonBinding {
    endpoint: String,
    engine_id: String,
}

impl fmt::Debug for DockerDaemonBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DockerDaemonBinding")
            .field("endpoint", &"<redacted>")
            .field("engine_id", &"<redacted>")
            .finish()
    }
}

impl DockerDaemonBinding {
    /// The normalized absolute socket path selected for this daemon.
    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// The exact opaque ID returned by the Docker Engine's `/info` endpoint.
    #[must_use]
    pub fn engine_id(&self) -> &str {
        &self.engine_id
    }
}

/// Observe the Docker daemon identity before a Linux launch reservation or effect.
///
/// This is one bounded, read-only `/info` request. Its result is an observation,
/// not a cryptographic attestation or a cleanup proof. Later requests must use
/// [`connect_unix_bound`] or the bound inventory API to compare the same ID on
/// the connection that carries each request.
///
/// # Errors
///
/// Returns [`HostError::Docker`] when the endpoint, deadline, response, or
/// reported daemon ID is invalid or unavailable.
pub async fn observe_docker_daemon_binding_until(
    endpoint: &str,
    deadline: Instant,
) -> Result<DockerDaemonBinding, HostError> {
    let endpoint = unix_socket_path(endpoint)?;
    let engine_id = daemon_guard::observe_engine_id(&endpoint, deadline).await?;
    Ok(DockerDaemonBinding {
        endpoint,
        engine_id,
    })
}

/// Connect a Bollard client whose every HTTP request is bound to the observed daemon.
///
/// For every operation, the transport opens one socket, checks `/info`, then sends
/// the original request on that same HTTP/1 connection. It never reconnects or
/// retries. The legacy [`connect_unix`] constructor remains available for callers
/// that do not use daemon-incarnation state.
///
/// # Errors
///
/// Returns [`HostError::Docker`] when the binding or transport cannot be used.
pub fn connect_unix_bound(binding: &DockerDaemonBinding) -> Result<bollard::Docker, HostError> {
    daemon_guard::connect_bound(binding)
}

/// Probe the configured Docker endpoint with only a bounded GET `/version`.
///
/// The returned platform metadata is observational; it is not runner-profile,
/// admission, or isolation evidence.
///
/// # Errors
///
/// Returns [`HostError::Docker`] when the endpoint, runtime, request, or
/// response is unavailable or malformed.
pub fn read_version_blocking(endpoint: &str) -> Result<DockerVersion, HostError> {
    read_version_blocking_after(endpoint, DOCKER_OPERATION_TIMEOUT)
}

fn read_version_blocking_after(
    endpoint: &str,
    timeout: Duration,
) -> Result<DockerVersion, HostError> {
    if tokio::runtime::Handle::try_current().is_ok() {
        return Err(HostError::Docker);
    }
    let docker = connect_unix(endpoint)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| HostError::Docker)?;
    let response = runtime.block_on(async {
        docker_deadline_after(docker.version(), timeout)
            .await?
            .map_err(|_| HostError::Docker)
    })?;
    let server_version = response
        .version
        .filter(|value| !value.trim().is_empty())
        .ok_or(HostError::Docker)?;
    Ok(DockerVersion {
        server_version,
        api_version: response.api_version,
        os: response.os,
        architecture: response.arch,
    })
}

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
pub(crate) fn unix_socket_path(socket: &str) -> Result<String, HostError> {
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
