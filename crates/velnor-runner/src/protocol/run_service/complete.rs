// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/protocol.rs

use anyhow::{Context, Result};
use reqwest::Method;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt;

use super::super::error::{BrokerErrorCategory, github_api_error_categorized, is_retriable_status};
use super::super::http::github_json_request;
use super::super::timeline::{TaskResult, TimelineRecordState};
use super::{REQUEST_TIMEOUT, RunServiceClient, is_run_service_job_not_found};

const MAX_ATTEMPTS: u32 = 5;
const ACTION: &str = "complete run-service job";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionAcknowledgement {
    Accepted,
    RemoteObservedTerminal,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunServiceCompleteJob {
    pub plan_id: String,
    pub job_id: String,
    pub conclusion: TaskResult,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub outputs: BTreeMap<String, RunServiceVariableValue>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub step_results: Vec<RunServiceStepResult>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<RunServiceAnnotation>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub telemetry: Vec<RunServiceTelemetry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_owner_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub infrastructure_failure_category: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunServiceTelemetry {
    pub message: String,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunServiceVariableValue {
    pub value: String,
    pub is_secret: bool,
}

impl fmt::Debug for RunServiceVariableValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = if self.is_secret {
            "<redacted>"
        } else {
            self.value.as_str()
        };
        f.debug_struct("RunServiceVariableValue")
            .field("value", &value)
            .field("is_secret", &self.is_secret)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RunServiceStepResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<i64>,
    pub name: String,
    pub status: TimelineRecordState,
    pub conclusion: TaskResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<RunServiceAnnotation>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunServiceAnnotation {
    pub level: RunServiceAnnotationLevel,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_line: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_column: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_column: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_number: Option<i64>,
    pub is_infrastructure_issue: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RunServiceAnnotationLevel {
    Notice,
    Warning,
    Failure,
}

enum Classification {
    Accepted,
    RemoteObservedTerminal,
    Retryable,
    Permanent,
}

fn classify(status: u16, body: &str) -> Classification {
    match status {
        200..=299 => Classification::Accepted,
        status if status != 0 && is_run_service_job_not_found(body) => {
            Classification::RemoteObservedTerminal
        }
        status if is_retriable_status(status) => Classification::Retryable,
        _ => Classification::Permanent,
    }
}

impl RunServiceClient {
    pub async fn complete_job(
        &self,
        run_service_url: &str,
        completion: &RunServiceCompleteJob,
    ) -> Result<()> {
        self.complete_job_with_acknowledgement(run_service_url, completion)
            .await
            .map(|_| ())
    }

    pub async fn complete_job_with_acknowledgement(
        &self,
        run_service_url: &str,
        completion: &RunServiceCompleteJob,
    ) -> Result<CompletionAcknowledgement> {
        let url = Self::job_url(run_service_url, "completejob")?;
        let body = serde_json::to_string(completion).context("serialize complete job request")?;
        let mut attempt = 1;
        loop {
            let outcome = github_json_request(
                Method::POST,
                url.as_str(),
                self.bearer_token.expose(),
                Some(body.clone()),
                REQUEST_TIMEOUT,
            )
            .await;
            let (error, terminal) = match outcome {
                Ok((status, text)) => match classify(status, &text) {
                    Classification::Accepted => return Ok(CompletionAcknowledgement::Accepted),
                    Classification::RemoteObservedTerminal => {
                        return Ok(CompletionAcknowledgement::RemoteObservedTerminal);
                    }
                    Classification::Retryable => (
                        github_api_error_categorized(
                            ACTION,
                            status,
                            &text,
                            BrokerErrorCategory::Transient,
                        ),
                        false,
                    ),
                    Classification::Permanent => (
                        github_api_error_categorized(
                            ACTION,
                            status,
                            &text,
                            BrokerErrorCategory::Terminal,
                        ),
                        true,
                    ),
                },
                Err(error) => (error.context("complete run-service job request"), false),
            };
            if terminal || attempt >= MAX_ATTEMPTS {
                return Err(error);
            }
            tokio::time::sleep(self.delays.complete_delay(attempt)).await;
            attempt += 1;
        }
    }
}
