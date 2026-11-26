// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/protocol.rs

use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer};
use std::time::Duration;
use url::Url;
use uuid::Uuid;

use super::http::{slash_url, validate_known_service_url};

mod acquire;
mod complete;
mod renew;

pub use acquire::{
    AcquireJobOutcome, AcquireJobSkipReason, AcquireRetriesExhausted, AcquiredJobPayload,
    is_transient_acquire_error,
};
pub use complete::{
    CompletionAcknowledgement, RunServiceAnnotation, RunServiceAnnotationLevel,
    RunServiceCompleteJob, RunServiceStepResult, RunServiceTelemetry, RunServiceVariableValue,
};
pub use renew::{RenewJobResponse, renew_failure_is_job_gone};

const KNOWN_HOSTS: &[&str] = &["run.actions.githubusercontent.com"];
const RUN_SERVICE_ERROR_SOURCE: &str = "actions-run-service";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryDelays {
    pub acquire_min: Duration,
    pub acquire_max: Duration,
    pub complete_initial: Duration,
    pub complete_max: Duration,
}

impl RetryDelays {
    pub const IMMEDIATE: Self = Self {
        acquire_min: Duration::ZERO,
        acquire_max: Duration::ZERO,
        complete_initial: Duration::ZERO,
        complete_max: Duration::ZERO,
    };

    fn acquire_delay(&self) -> Duration {
        let span = self.acquire_max.saturating_sub(self.acquire_min);
        let span_millis = span.as_millis();
        if span_millis == 0 {
            return self.acquire_min;
        }
        let jitter = Uuid::new_v4().as_u128() % span_millis;
        self.acquire_min + Duration::from_millis(u64::try_from(jitter).unwrap_or(0))
    }

    fn complete_delay(&self, attempt: u32) -> Duration {
        self.complete_initial
            .saturating_mul(
                1_u32
                    .checked_shl(attempt.saturating_sub(1))
                    .unwrap_or(u32::MAX),
            )
            .min(self.complete_max)
    }
}

impl Default for RetryDelays {
    fn default() -> Self {
        Self {
            acquire_min: Duration::from_secs(5),
            acquire_max: Duration::from_secs(15),
            complete_initial: Duration::from_secs(5),
            complete_max: Duration::from_secs(60),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunServiceClient {
    bearer_token: String,
    delays: RetryDelays,
}

impl RunServiceClient {
    pub fn new(bearer_token: impl Into<String>) -> Result<Self> {
        Ok(Self {
            bearer_token: bearer_token.into(),
            delays: RetryDelays::default(),
        })
    }

    #[must_use]
    pub const fn with_retry_delays(mut self, delays: RetryDelays) -> Self {
        self.delays = delays;
        self
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
