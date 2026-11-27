// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/protocol.rs

use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer};
use std::future::Future;
use std::time::Duration;
use url::Url;

use super::http::{slash_url, validate_known_service_url};
use super::token_backoff::{TokenBackoff, random_between};
use crate::secret::Secret;

mod acquire;
mod complete;
mod renew;

pub use acquire::{
    AcquireJobOutcome, AcquireJobSkipReason, AcquireRetriesExhausted, AcquiredJobPayload,
    acquire_job, is_transient_acquire_error,
};
pub use complete::{
    CompletionAcknowledgement, RunServiceAnnotation, RunServiceAnnotationLevel,
    RunServiceCompleteJob, RunServiceStepResult, RunServiceTelemetry, RunServiceVariableValue,
};
pub use renew::{RenewJobResponse, renew_failure_is_job_gone};

const KNOWN_HOSTS: &[&str] = &["run.actions.githubusercontent.com"];
const RUN_SERVICE_ERROR_SOURCE: &str = "actions-run-service";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_ATTEMPTS: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryDelays {
    pub min: Duration,
    pub max: Duration,
    pub token: TokenBackoff,
}

impl RetryDelays {
    pub const IMMEDIATE: Self = Self {
        min: Duration::ZERO,
        max: Duration::ZERO,
        token: TokenBackoff::IMMEDIATE,
    };

    fn delay(&self) -> Duration {
        random_between(self.min, self.max)
    }
}

impl Default for RetryDelays {
    fn default() -> Self {
        Self {
            min: Duration::from_secs(5),
            max: Duration::from_secs(15),
            token: TokenBackoff::default(),
        }
    }
}

enum Failure {
    Fatal(anyhow::Error),
    Retry(anyhow::Error),
}

async fn retry<T, F, Fut>(delays: RetryDelays, mut once: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, Failure>>,
{
    let mut attempt = 1;
    loop {
        match once().await {
            Ok(value) => return Ok(value),
            Err(Failure::Fatal(error)) => return Err(error),
            Err(Failure::Retry(error)) if attempt >= MAX_ATTEMPTS => return Err(error),
            Err(Failure::Retry(_)) => {}
        }
        tokio::time::sleep(delays.delay()).await;
        attempt += 1;
    }
}

#[derive(Debug, Clone)]
pub struct RunServiceClient {
    bearer_token: Secret<String>,
    delays: RetryDelays,
}

impl RunServiceClient {
    pub fn new(bearer_token: impl Into<String>) -> Result<Self> {
        Ok(Self {
            bearer_token: Secret::new(bearer_token.into()),
            delays: RetryDelays::default(),
        })
    }

    #[must_use]
    pub const fn with_retry_delays(mut self, delays: RetryDelays) -> Self {
        self.delays = delays;
        self
    }

    async fn with_retries<T, F, Fut>(&self, once: F) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, Failure>>,
    {
        retry(self.delays, once).await
    }

    fn job_url(run_service_url: &str, action: &str) -> Result<Url> {
        let validated = validate_known_service_url(run_service_url, "run-service", KNOWN_HOSTS)?;
        slash_url(&validated)
            .join(action)
            .with_context(|| format!("build run-service {action} URL"))
    }
}

#[derive(Debug, Deserialize)]
struct RunServiceError {
    #[serde(default)]
    source: Option<String>,
    #[serde(
        default,
        rename = "statusCode",
        deserialize_with = "deserialize_status_code"
    )]
    code: Option<i32>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum WireStatusCode {
    Number(i32),
    Text(String),
}

fn deserialize_status_code<'de, D>(deserializer: D) -> Result<Option<i32>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(
        Option::<WireStatusCode>::deserialize(deserializer)?.and_then(|value| match value {
            WireStatusCode::Number(code) => Some(code),
            WireStatusCode::Text(text) => text.trim().parse().ok(),
        }),
    )
}

fn run_service_error_code(body: &str) -> Option<i32> {
    let error: RunServiceError = serde_json::from_str(body).ok()?;
    if error.source.as_deref() == Some(RUN_SERVICE_ERROR_SOURCE) {
        error.code
    } else {
        None
    }
}

fn is_run_service_job_not_found(body: &str) -> bool {
    run_service_error_code(body) == Some(404)
}
