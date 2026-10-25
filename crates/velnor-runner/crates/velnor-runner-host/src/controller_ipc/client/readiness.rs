//! Read the active controller's readiness status.

use std::path::Path;
use std::time::Duration;

use crate::error::HostError;
use crate::readiness::Readiness;

use super::super::wire::Request;

/// Request the active controller's readiness status.
///
/// # Errors
///
/// Returns an error when the request, private control socket, or response is invalid.
pub fn controller_readiness_status(
    state: &Path,
    timeout: Duration,
    config_sha256: &str,
    engine_id: &str,
    docker_root_digest: &str,
) -> Result<Readiness, HostError> {
    if timeout.is_zero() {
        return Err(HostError::Frame);
    }
    let request = Request::ControllerReadiness {
        config_sha256: config_sha256.to_owned(),
        engine_id: engine_id.to_owned(),
        docker_root_digest: docker_root_digest.to_owned(),
    };
    if !super::valid_request(&request) {
        return Err(HostError::Frame);
    }
    let response = super::call_socket_response_with_timeout(
        &state.join(super::SOCKET_NAME),
        &request,
        timeout.min(super::CALL_TIMEOUT),
    )?;
    response.readiness.ok_or(HostError::Journal)
}

#[cfg(test)]
mod tests {
    use super::controller_readiness_status;
    use crate::error::HostError;
    use std::path::Path;
    use std::time::Duration;

    #[test]
    fn invalid_readiness_request_is_rejected_before_socket_access() -> Result<(), String> {
        let result = controller_readiness_status(
            Path::new("/socket-must-not-be-opened"),
            Duration::from_millis(10),
            "invalid",
            "engine",
            &"a".repeat(64),
        );
        if !matches!(result, Err(HostError::Frame)) {
            return Err(
                "The readiness client did not reject the invalid request before socket access."
                    .to_owned(),
            );
        }
        Ok(())
    }
}
