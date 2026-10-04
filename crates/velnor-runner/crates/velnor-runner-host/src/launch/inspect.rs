//! Fail-closed Docker observations used by admission and reconciliation.

use bollard::Docker;
use bollard::errors::Error as DockerError;

use crate::scale_set::EnsureError;

pub(super) async fn container_running(docker: &Docker, id: &str) -> Result<bool, EnsureError> {
    classify_inspect(docker.inspect_container(id, None).await)
}

fn classify_inspect(
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

const fn inspect_error(status: u16) -> EnsureError {
    EnsureError::Unexpected {
        status,
        step: "docker inspect",
    }
}

#[cfg(test)]
mod tests {
    use bollard::errors::Error as DockerError;

    use super::{classify_inspect, inspect_error};

    #[test]
    fn only_not_found_is_treated_as_absent() {
        assert_eq!(
            classify_inspect(Err(DockerError::DockerResponseServerError {
                status_code: 404,
                message: "missing runner-id".to_owned(),
            })),
            Ok(false)
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
            (r#"{"State":{"Running":false}}"#, false),
            (r#"{"State":{"Running":true}}"#, true),
        ] {
            let info = serde_json::from_str(body).map_err(|error| error.to_string())?;
            assert_eq!(classify_inspect(Ok(info)), Ok(expected));
        }
        Ok(())
    }
}
