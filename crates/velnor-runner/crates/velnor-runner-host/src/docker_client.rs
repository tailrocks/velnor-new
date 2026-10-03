//! Unix-socket Docker client. The configured path is the only endpoint.

use crate::error::HostError;

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
mod tests {
    use super::unix_socket_path;
    use crate::error::HostError;

    #[test]
    fn absolute_unix_path_only() {
        assert_eq!(
            unix_socket_path("unix:///var/run/docker.sock"),
            Ok("/var/run/docker.sock".to_owned())
        );
        assert_eq!(
            unix_socket_path("/var/run/docker.sock"),
            Ok("/var/run/docker.sock".to_owned())
        );
        assert_eq!(
            unix_socket_path("tcp://127.0.0.1:2375"),
            Err(HostError::Docker)
        );
        assert_eq!(
            unix_socket_path("TCP://127.0.0.1:2375"),
            Err(HostError::Docker)
        );
        assert_eq!(
            unix_socket_path("unix://var/run/docker.sock"),
            Err(HostError::Docker)
        );
        assert_eq!(unix_socket_path("relative.sock"), Err(HostError::Docker));
        assert_eq!(unix_socket_path("unix://"), Err(HostError::Docker));
        assert_eq!(unix_socket_path(""), Err(HostError::Docker));
        assert_eq!(
            unix_socket_path("ssh://example/tmp/docker.sock"),
            Err(HostError::Docker)
        );
    }
}
