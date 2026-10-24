use std::{future::Future, time::Instant};

use crate::{
    error::HostError,
    journal::ConnectionJournal,
    launch::{SelectedEngineContext, VerifiedWorkerImages},
    worker::execution_authorization::{
        ExecutionRequestRead, VerifiedExecutionRequest, read_execution_request,
    },
};

use super::RunnerReadyBootstrap;

const REQUEST_RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

#[derive(Debug, PartialEq, Eq)]
pub(super) enum StartedRequestReadError {
    DeadlineElapsed,
    Read(HostError),
}

#[derive(Debug, PartialEq, Eq)]
enum RequestWaitError {
    DeadlineElapsed,
    Read(HostError),
}

pub(super) async fn read_started_request(
    scoped: &ConnectionJournal,
    runner: &RunnerReadyBootstrap,
    docker: &bollard::Docker,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    deadline: Instant,
) -> Result<VerifiedExecutionRequest, StartedRequestReadError> {
    let channel = runner.channel();
    wait_for_request(deadline, || async {
        let intent = scoped.intent(runner.intent_id()).await?;
        if intent.runner_completed || intent.cleanup_proven {
            return Err(HostError::RunnerActive);
        }
        match read_execution_request(docker, engine, images, channel, deadline).await? {
            ExecutionRequestRead::NotYetPresent => Ok(None),
            ExecutionRequestRead::Present(request) => Ok(Some(request)),
        }
    })
    .await
    .map_err(|error| match error {
        RequestWaitError::DeadlineElapsed => StartedRequestReadError::DeadlineElapsed,
        RequestWaitError::Read(error) => StartedRequestReadError::Read(error),
    })
}

async fn wait_for_request<T, F, Fut>(deadline: Instant, mut read: F) -> Result<T, RequestWaitError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Option<T>, HostError>>,
{
    loop {
        if Instant::now() >= deadline {
            return Err(RequestWaitError::DeadlineElapsed);
        }
        if let Some(request) = read().await.map_err(RequestWaitError::Read)? {
            return Ok(request);
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or(RequestWaitError::DeadlineElapsed)?;
        let delay = remaining.min(REQUEST_RETRY_INTERVAL);
        tokio::time::sleep(delay).await;
        if delay == remaining || Instant::now() >= deadline {
            return Err(RequestWaitError::DeadlineElapsed);
        }
    }
}

#[cfg(test)]
#[path = "request_wait_tests.rs"]
mod tests;
