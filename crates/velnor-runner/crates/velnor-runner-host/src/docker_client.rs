//! Unix-socket Docker client. The configured path is the only endpoint.

use crate::error::HostError;

/// Open the selected socket. This is the only constructor the host calls.
///
/// # Errors
///
/// Returns [`HostError::Docker`] when the socket cannot be opened.
pub fn connect_unix(socket: &str) -> Result<bollard::Docker, HostError> {
    bollard::Docker::connect_with_unix(socket, 120, bollard::API_DEFAULT_VERSION)
        .map_err(|_| HostError::Docker)
}
