//! Fail-closed Docker observations used by admission and reconciliation.

use bollard::Docker;
use bollard::errors::Error as DockerError;

use crate::docker_client::docker_deadline;
use crate::scale_set::EnsureError;

/// Presence state returned by an exact container inspect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContainerState {
    /// Docker returned 404.
    Missing,
    /// The container exists and is running.
    Running,
    /// The container exists but is stopped.
    Stopped,
}

pub(super) async fn container_running(docker: &Docker, id: &str) -> Result<bool, EnsureError> {
    Ok(matches!(
        container_state(docker, id).await?,
        ContainerState::Running
    ))
}

pub(super) async fn container_state(
    docker: &Docker,
    id: &str,
) -> Result<ContainerState, EnsureError> {
    let response = docker_deadline(docker.inspect_container(id, None))
        .await
        .map_err(|_| inspect_error(0))?;
    classify_inspect(response)
}

fn classify_inspect(
    response: Result<bollard::models::ContainerInspectResponse, DockerError>,
) -> Result<ContainerState, EnsureError> {
    match response {
        Ok(info) => match info.state.and_then(|state| state.running) {
            Some(true) => Ok(ContainerState::Running),
            Some(false) => Ok(ContainerState::Stopped),
            None => Err(inspect_error(200)),
        },
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(ContainerState::Missing),
        Err(DockerError::DockerResponseServerError { status_code, .. }) => {
            Err(inspect_error(status_code))
        }
        Err(_) => Err(inspect_error(0)),
    }
}

const fn inspect_error(status: u16) -> EnsureError {
    EnsureError::Unexpected {
        status,
        step: "docker inspect",
    }
}

#[cfg(test)]
mod tests {
    use bollard::errors::Error as DockerError;

    use super::{ContainerState, classify_inspect, inspect_error};

    #[test]
    fn only_not_found_is_treated_as_absent() {
        assert_eq!(
            classify_inspect(Err(DockerError::DockerResponseServerError {
                status_code: 404,
                message: "missing runner-id".to_owned(),
            })),
            Ok(ContainerState::Missing)
        );
        assert_eq!(
            classify_inspect(Err(DockerError::DockerResponseServerError {
                status_code: 503,
                message: "temporary failure".to_owned(),
            })),
            Err(inspect_error(503))
        );
    }

    #[test]
    fn running_state_must_be_present() -> Result<(), String> {
        for body in ["{}", r#"{"State":{}}"#] {
            let info = serde_json::from_str(body).map_err(|error| error.to_string())?;
            assert_eq!(classify_inspect(Ok(info)), Err(inspect_error(200)));
        }
        Ok(())
    }

    #[test]
    fn explicit_running_value_is_preserved() -> Result<(), String> {
        for (body, expected) in [
            (r#"{"State":{"Running":false}}"#, ContainerState::Stopped),
            (r#"{"State":{"Running":true}}"#, ContainerState::Running),
        ] {
            let info = serde_json::from_str(body).map_err(|error| error.to_string())?;
            assert_eq!(classify_inspect(Ok(info)), Ok(expected));
        }
        Ok(())
    }
}
