// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/protocol.rs

use std::future::Future;

use anyhow::{Context, Result};
use reqwest::{Method, StatusCode};
use serde::Serialize;
use serde_json::Value;

use super::super::error::{BrokerErrorCategory, github_api_error_categorized};
use super::super::http::{GithubHttpResponse, github_json_http_response};
use super::{REQUEST_TIMEOUT, RetryDelays, RunServiceClient, run_service_error_code};
use crate::job_message::AgentJobRequestMessage;
use crate::secret::Secret;

const MAX_ATTEMPTS: u32 = 5;
const ACTION: &str = "acquire run-service job";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcquireJobSkipReason {
    NotFound,
    AlreadyAcquired,
    Unprocessable,
}

impl AcquireJobSkipReason {
    fn from_body(body: &str) -> Option<Self> {
        match run_service_error_code(body)? {
            404 => Some(Self::NotFound),
            409 => Some(Self::AlreadyAcquired),
            422 => Some(Self::Unprocessable),
            _ => None,
        }
    }

    const fn category(self) -> BrokerErrorCategory {
        match self {
            Self::NotFound => BrokerErrorCategory::Terminal,
            Self::AlreadyAcquired | Self::Unprocessable => BrokerErrorCategory::Conflict,
        }
    }
}

#[derive(Debug)]
pub struct AcquiredJobPayload {
    pub raw: Secret<Value>,
    pub message: Option<AgentJobRequestMessage>,
}

#[derive(Debug)]
pub enum AcquireJobOutcome {
    Acquired(Box<AcquiredJobPayload>),
    Skipped {
        status: StatusCode,
        body: String,
        reason: AcquireJobSkipReason,
        category: BrokerErrorCategory,
    },
}

#[derive(Debug, thiserror::Error)]
#[error("transient run-service acquire failure after retries: {0:#}")]
pub struct AcquireRetriesExhausted(#[source] anyhow::Error);

#[must_use]
pub fn is_transient_acquire_error(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.downcast_ref::<AcquireRetriesExhausted>().is_some())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AcquireJobRequest<'a> {
    job_message_id: &'a str,
    #[serde(rename = "runnerOS")]
    runner_os: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    billing_owner_id: Option<&'a str>,
}

pub async fn acquire_job<F, Fut>(
    delays: RetryDelays,
    run_service_url: &str,
    job_message_id: &str,
    runner_os: &str,
    billing_owner_id: Option<&str>,
    fresh_token: F,
) -> Result<AcquireJobOutcome>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<String>>,
{
    let url = RunServiceClient::job_url(run_service_url, "acquirejob")?;
    let body = serde_json::to_string(&AcquireJobRequest {
        job_message_id,
        runner_os,
        billing_owner_id,
    })
    .context("serialize acquire job request")?;
    let mut attempt = 1;
    loop {
        let attempted = async {
            let token = fresh_token().await.context("sign in before the acquire")?;
            acquire_once(url.as_str(), &body, &token).await
        };
        let error = match attempted.await {
            Ok(outcome) => return Ok(outcome),
            Err(error) => error,
        };
        if attempt >= MAX_ATTEMPTS {
            return Err(AcquireRetriesExhausted(error).into());
        }
        tokio::time::sleep(delays.acquire_delay()).await;
        attempt += 1;
    }
}

async fn acquire_once(url: &str, body: &str, token: &str) -> Result<AcquireJobOutcome> {
    let response = github_json_http_response(
        Method::POST,
        url,
        token,
        Some(body.to_owned()),
        REQUEST_TIMEOUT,
    )
    .await
    .context("acquire run-service job request")?;
    let status = response.status;
    if (200..300).contains(&status) {
        return decode_acquired(&response).map_err(|error| {
            github_api_error_categorized(
                ACTION,
                status,
                &format!("parse AgentJobRequestMessage response: {error}"),
                BrokerErrorCategory::Transient,
            )
        });
    }
    if let Some(reason) = AcquireJobSkipReason::from_body(response.body.expose()) {
        return Ok(AcquireJobOutcome::Skipped {
            status: StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            category: reason.category(),
            reason,
            body: response.body.into_inner(),
        });
    }
    Err(github_api_error_categorized(
        ACTION,
        status,
        response.body.expose(),
        BrokerErrorCategory::Transient,
    ))
}

fn is_json_body(response: &GithubHttpResponse) -> bool {
    let media_type = response
        .headers
        .expose()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim);
    response.status != 204
        && media_type.is_some_and(|value| value.eq_ignore_ascii_case("application/json"))
}

fn decode_acquired(response: &GithubHttpResponse) -> Result<AcquireJobOutcome, serde_json::Error> {
    if !is_json_body(response) || response.body.expose().trim().is_empty() {
        return Ok(acquired(Secret::new(Value::Null), None));
    }
    let raw: Value = serde_json::from_str(response.body.expose())?;
    let message = raw
        .is_object()
        .then(|| serde_json::from_str::<AgentJobRequestMessage>(response.body.expose()).ok())
        .flatten();
    Ok(acquired(Secret::new(raw), message))
}

fn acquired(raw: Secret<Value>, message: Option<AgentJobRequestMessage>) -> AcquireJobOutcome {
    AcquireJobOutcome::Acquired(Box::new(AcquiredJobPayload { raw, message }))
}
