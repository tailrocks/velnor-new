use crate::{
    docker_client::{DOCKER_OPERATION_TIMEOUT, docker_deadline_after},
    journal::{EngineContext, LocalWorkerHealthCandidate},
    launch::SelectedEngineContext,
};
use bollard::{
    Docker,
    errors::Error as DockerError,
    models::{ContainerInspectResponse, ContainerStateStatusEnum as Status},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RunnerObservation {
    Running,
    Exited { exit_code: i32 },
    Missing,
    Held(&'static str),
}

pub(super) async fn inspect_runner(
    docker: &Docker,
    engine: &EngineContext,
    selected: &SelectedEngineContext,
    candidate: &LocalWorkerHealthCandidate,
    io_deadline: tokio::time::Instant,
) -> RunnerObservation {
    if candidate.engine_id() != selected.engine_id()
        || candidate.docker_root_digest() != selected.docker_root_digest()
    {
        return RunnerObservation::Held("selected engine binding mismatch");
    }
    let deadline = io_deadline.min(tokio::time::Instant::now() + DOCKER_OPERATION_TIMEOUT);
    let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
    let Ok(Ok(info)) = docker_deadline_after(docker.info(), remaining).await else {
        return RunnerObservation::Held("docker daemon identity unavailable");
    };
    let Ok(current) = SelectedEngineContext::capture(engine, &info) else {
        return RunnerObservation::Held("docker daemon identity changed");
    };
    if current != *selected {
        return RunnerObservation::Held("docker daemon identity changed");
    }
    let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
    if remaining.is_zero() {
        return RunnerObservation::Held("docker inspect deadline expired");
    }
    let Ok(response) = docker_deadline_after(
        docker.inspect_container(candidate.runner_container_id(), None),
        remaining,
    )
    .await
    else {
        return RunnerObservation::Held("docker inspect deadline expired");
    };
    classify_response(response, candidate.runner_container_id())
}

fn classify_response(
    response: Result<ContainerInspectResponse, DockerError>,
    expected_id: &str,
) -> RunnerObservation {
    match response {
        Ok(response) => classify_inspection(response, expected_id),
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => RunnerObservation::Missing,
        Err(_) => RunnerObservation::Held("docker inspect failed"),
    }
}

pub(super) fn classify_inspection(
    response: ContainerInspectResponse,
    expected_id: &str,
) -> RunnerObservation {
    if response.id.as_deref() != Some(expected_id) {
        return RunnerObservation::Held("docker inspect identity mismatch");
    }
    let Some(state) = response.state else {
        return RunnerObservation::Held("docker inspect state missing");
    };
    match state.status {
        Some(Status::RUNNING) if state.running == Some(true) => RunnerObservation::Running,
        Some(Status::EXITED) if state.running == Some(false) => {
            match state.exit_code.and_then(|code| i32::try_from(code).ok()) {
                Some(exit_code) => RunnerObservation::Exited { exit_code },
                None => RunnerObservation::Held("docker exit code missing or out of range"),
            }
        }
        _ => RunnerObservation::Held("docker runner state is not exact running or exited"),
    }
}
