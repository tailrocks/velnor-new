//! Fail-closed Docker observations used by admission and reconciliation.

use bollard::Docker;
use bollard::errors::Error as DockerError;

use crate::docker_client::docker_deadline;
use crate::scale_set::EnsureError;

pub(crate) async fn container_running(docker: &Docker, id: &str) -> Result<bool, EnsureError> {
    let response = docker_deadline(docker.inspect_container(id, None))
        .await
        .map_err(|_| inspect_error(0))?;
    classify_inspect(response)
}

pub(crate) fn classify_inspect(
    response: Result<bollard::models::ContainerInspectResponse, DockerError>,
) -> Result<bool, EnsureError> {
    match response {
        Ok(info) => match info.state {
            Some(state) => still_live(&state),
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

/// A container that has not exited still owns its slot.
///
/// `created` is recorded before start, so `Running: false` is not cleanup.
fn still_live(state: &bollard::models::ContainerState) -> Result<bool, EnsureError> {
    use bollard::models::ContainerStateStatusEnum as Status;
    match state.status {
        Some(
            Status::CREATED
            | Status::RUNNING
            | Status::PAUSED
            | Status::RESTARTING
            | Status::REMOVING
            | Status::STOPPING,
        ) => Ok(true),
        Some(Status::EXITED | Status::DEAD) => Ok(false),
        Some(Status::EMPTY) | None => Err(inspect_error(200)),
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
    fn status_must_prove_a_terminal_or_live_lifecycle() -> Result<(), String> {
        for body in [
            "{}",
            r#"{"State":{}}"#,
            r#"{"State":{"Running":false}}"#,
            r#"{"State":{"Running":true}}"#,
            r#"{"State":{"Status":""}}"#,
        ] {
            let info = serde_json::from_str(body).map_err(|error| error.to_string())?;
            assert_eq!(classify_inspect(Ok(info)), Err(inspect_error(200)));
        }
        Ok(())
    }

    #[test]
    fn nonterminal_states_keep_the_slot_until_exit() -> Result<(), String> {
        for (body, expected) in [
            (r#"{"State":{"Status":"created","Running":false}}"#, true),
            (r#"{"State":{"Status":"paused","Running":false}}"#, true),
            (r#"{"State":{"Status":"restarting","Running":false}}"#, true),
            (r#"{"State":{"Status":"removing","Running":false}}"#, true),
            (r#"{"State":{"Status":"stopping","Running":false}}"#, true),
            (r#"{"State":{"Status":"exited","Running":false}}"#, false),
            (r#"{"State":{"Status":"dead","Running":false}}"#, false),
        ] {
            let info = serde_json::from_str(body).map_err(|error| error.to_string())?;
            assert_eq!(classify_inspect(Ok(info)), Ok(expected));
        }
        Ok(())
    }
}
