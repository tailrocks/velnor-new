// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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
