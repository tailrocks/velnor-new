// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/protocol.rs

use anyhow::{Context, Result};
use reqwest::Method;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt;

use super::super::error::{
    BrokerErrorCategory, ServiceErrorKind, github_api_error_categorized, github_api_error_with,
    service_error_kind,
};
use super::super::http::github_json_request;
use super::super::timeline::{TaskResult, TimelineRecordState};
use super::{Failure, REQUEST_TIMEOUT, RunServiceClient, is_run_service_job_not_found};

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_name: Option<String>,
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub status: TimelineRecordState,
    pub conclusion: TaskResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_log_url: Option<String>,
    pub completed_log_lines: i64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<RunServiceAnnotation>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub is_background: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_control_type: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub background_control_step_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunServiceAnnotation {
    pub level: RunServiceAnnotationLevel,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_details: Option<String>,
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

impl RunServiceClient {
    pub async fn complete_job(
        &self,
        run_service_url: &str,
        completion: &RunServiceCompleteJob,
    ) -> Result<()> {
        let url = Self::job_url(run_service_url, "completejob")?;
        let body = serde_json::to_string(completion).context("serialize complete job request")?;
        self.with_retries(|| self.complete_once(url.as_str(), &body))
            .await
    }

    pub async fn complete_job_with_acknowledgement(
        &self,
        run_service_url: &str,
        completion: &RunServiceCompleteJob,
    ) -> Result<CompletionAcknowledgement> {
        match self.complete_job(run_service_url, completion).await {
            Ok(()) => Ok(CompletionAcknowledgement::Accepted),
            Err(error) if service_error_kind(&error) == Some(ServiceErrorKind::JobNotFound) => {
                Ok(CompletionAcknowledgement::RemoteObservedTerminal)
            }
            Err(error) => Err(error),
        }
    }

    async fn complete_once(&self, url: &str, body: &str) -> Result<(), Failure> {
        let (status, text) = github_json_request(
            Method::POST,
            url,
            self.bearer_token.expose(),
            Some(body.to_owned()),
            REQUEST_TIMEOUT,
        )
        .await
        .map_err(|error| Failure::Retry(error.context("complete run-service job request")))?;
        if (200..300).contains(&status) {
            return Ok(());
        }
        if is_run_service_job_not_found(&text) {
            return Err(Failure::Fatal(github_api_error_with(
                ACTION,
                status,
                &text,
                Some(BrokerErrorCategory::Terminal),
                Some(ServiceErrorKind::JobNotFound),
            )));
        }
        let (category, failure): (_, fn(anyhow::Error) -> Failure) = if status == 401 {
            (BrokerErrorCategory::Terminal, Failure::Fatal)
        } else {
            (BrokerErrorCategory::Transient, Failure::Retry)
        };
        Err(failure(github_api_error_categorized(
            ACTION, status, &text, category,
        )))
    }
}
