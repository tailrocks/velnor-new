// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/runner.rs

use std::time::Duration;

use anyhow::{Context, anyhow};

use super::LifecycleError;
use super::session::Session;
use crate::context_value::ordinal_ignore_case_eq;
use crate::job_message::AgentJobRequestMessage;
use crate::protocol::{
    Acknowledgement, AcquireJobOutcome, FeedStreamClient, RetryDelays, RunServiceClient,
    RunnerJobRequestRef, RunnerOs, TwirpResultsClient, acquire_job,
};

const ACKNOWLEDGE_BUDGET: Duration = Duration::from_secs(5);
const RESULTS_ENDPOINT_VARIABLE: &str = "system.github.results_endpoint";
const ACCESS_TOKEN_PARAMETER: &str = "AccessToken";
const FEED_STREAM_URL: &str = "FeedStreamUrl";

#[derive(Debug)]
pub(super) struct JobConnection {
    pub(super) run_service: RunServiceClient,
    pub(super) run_service_url: String,
    pub(super) results: Option<TwirpResultsClient>,
    pub(super) feed: Option<FeedStreamClient>,
}

async fn acknowledge(session: &Session, runner_request_id: &str) -> Acknowledgement {
    match tokio::time::timeout(ACKNOWLEDGE_BUDGET, session.acknowledge(runner_request_id)).await {
        Ok(Ok(acknowledgement)) => acknowledgement,
        Ok(Err(error)) => {
            eprintln!("warning: acknowledge job request: {error:#}");
            Acknowledgement::Accepted
        }
        Err(_) => {
            eprintln!("warning: acknowledge job request timed out");
            Acknowledgement::Accepted
        }
    }
}

pub(super) async fn acquire(
    session: &Session,
    request: &RunnerJobRequestRef,
    os: RunnerOs,
    delays: RetryDelays,
) -> Result<Option<AgentJobRequestMessage>, LifecycleError> {
    if request.should_acknowledge
        && acknowledge(session, &request.runner_request_id).await == Acknowledgement::JobNotFound
    {
        return Ok(None);
    }
    let run_service_url = request
        .run_service_url
        .as_deref()
        .filter(|url| !url.is_empty())
        .ok_or_else(|| LifecycleError::Acquire(anyhow!("job request has no run_service_url")))?;
    let outcome = acquire_job(
        delays,
        run_service_url,
        &request.runner_request_id,
        os.as_str(),
        request.billing_owner_id.as_deref(),
        || session.fresh_access_token(),
    )
    .await
    .map_err(LifecycleError::Acquire)?;
    match outcome {
        AcquireJobOutcome::Acquired(payload) => payload.message.map(Some).ok_or_else(|| {
            LifecycleError::Acquire(anyhow!("acquired job is not a valid job message"))
        }),
        AcquireJobOutcome::Skipped { status, reason, .. } => {
            Err(LifecycleError::Skipped { status, reason })
        }
    }
}

fn variable<'a>(message: &'a AgentJobRequestMessage, name: &str) -> Option<&'a str> {
    message
        .variables
        .iter()
        .find(|(known, _)| ordinal_ignore_case_eq(known, name))
        .and_then(|(_, variable)| variable.value.as_ref())
        .map(|value| value.expose().as_str())
        .filter(|value| !value.is_empty())
}

pub(super) fn connect(
    message: &AgentJobRequestMessage,
    block_budget: Duration,
    delays: RetryDelays,
) -> Result<JobConnection, LifecycleError> {
    let endpoint = message
        .system_connection()
        .map_err(LifecycleError::Acquire)?;
    let run_service_url = endpoint
        .url
        .clone()
        .filter(|url| !url.is_empty())
        .ok_or_else(|| {
            LifecycleError::Acquire(anyhow!("SystemVssConnection endpoint has no URL"))
        })?;
    let token = endpoint
        .authorization
        .as_ref()
        .and_then(|authorization| authorization.parameter_string(ACCESS_TOKEN_PARAMETER))
        .filter(|token| !token.is_empty())
        .context("SystemVssConnection endpoint has no AccessToken")
        .map_err(LifecycleError::Acquire)?;
    let results = variable(message, RESULTS_ENDPOINT_VARIABLE).and_then(|url| {
        TwirpResultsClient::new(url, token)
            .map(|client| client.with_block_budget(block_budget))
            .inspect_err(|error| eprintln!("warning: results service disabled: {error:#}"))
            .ok()
    });
    let feed = endpoint
        .data_string(FEED_STREAM_URL)
        .filter(|url| !url.is_empty())
        .and_then(|url| {
            FeedStreamClient::new(url, token)
                .inspect_err(|error| eprintln!("warning: live feed disabled: {error:#}"))
                .ok()
        });
    Ok(JobConnection {
        run_service: RunServiceClient::new(token)
            .map_err(LifecycleError::Acquire)?
            .with_retry_delays(delays),
        run_service_url,
        results,
        feed,
    })
}
