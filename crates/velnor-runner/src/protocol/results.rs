// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/protocol.rs

use anyhow::{Context, Result, bail};
use reqwest::Method;
use serde::{Serialize, Serializer};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use super::error::sanitize_response_body;
use super::http::{github_json_request, validate_known_service_url};
use super::results_wire::{
    Acknowledgement, ScopeRequest, SignedUrlResponse, SoftSizeLimitExceeded,
};
use crate::secret::Secret;

const RECEIVER: &str = "twirp/results.services.receiver.Receiver";
const STEPS_UPDATE: &str =
    "twirp/github.actions.results.api.v1.WorkflowStepUpdateService/WorkflowStepsUpdate";
pub(super) const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub const BLOCK_BUDGET: Duration = Duration::from_secs(60);
const ATTEMPTS: u64 = 3;
const RETRY_STEP: Duration = Duration::from_millis(200);
const KNOWN_HOSTS: &[&str] = &["results-receiver.actions.githubusercontent.com"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    InProgress = 3,
    Pending = 5,
    Completed = 6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepConclusion {
    Unknown = 0,
    Success = 2,
    Failure = 3,
    Cancelled = 4,
    Skipped = 7,
}

impl Serialize for StepStatus {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(*self as u8)
    }
}

impl Serialize for StepConclusion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(*self as u8)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TwirpStep {
    pub external_id: String,
    pub number: usize,
    pub name: String,
    pub status: StepStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    pub conclusion: StepConclusion,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub is_background: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_control_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_control_step_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_group_id: Option<String>,
}

#[derive(Serialize)]
struct WorkflowStepsUpdateRequest<'a> {
    steps: &'a [TwirpStep],
    change_order: i64,
    workflow_job_run_backend_id: &'a str,
    workflow_run_backend_id: &'a str,
}

#[derive(Clone, Copy)]
pub(super) struct BlobKind {
    pub(super) label: &'static str,
    pub(super) signed_url_method: &'static str,
    pub(super) metadata_method: &'static str,
    pub(super) signed_url: fn(SignedUrlResponse) -> Option<String>,
}

pub(super) const STEP_LOG: BlobKind = BlobKind {
    label: "step log",
    signed_url_method: "GetStepLogsSignedBlobURL",
    metadata_method: "CreateStepLogsMetadata",
    signed_url: |response| response.logs_url,
};
pub(super) const JOB_LOG: BlobKind = BlobKind {
    label: "job log",
    signed_url_method: "GetJobLogsSignedBlobURL",
    metadata_method: "CreateJobLogsMetadata",
    signed_url: |response| response.logs_url,
};
pub(super) const STEP_SUMMARY: BlobKind = BlobKind {
    label: "step summary",
    signed_url_method: "GetStepSummarySignedBlobURL",
    metadata_method: "CreateStepSummaryMetadata",
    signed_url: |response| response.summary_url,
};

#[derive(Debug, Clone)]
pub struct TwirpResultsClient {
    pub(super) results_service_url: String,
    token: Secret<String>,
    pub(super) block_budget: Duration,
}

impl TwirpResultsClient {
    pub fn new(results_service_url: &str, token: impl Into<String>) -> Result<Self> {
        let url =
            validate_known_service_url(results_service_url, "ResultsServiceUrl", KNOWN_HOSTS)?;
        Ok(Self {
            results_service_url: url.as_str().trim_end_matches('/').to_owned(),
            token: Secret::new(token.into()),
            block_budget: BLOCK_BUDGET,
        })
    }

    #[must_use]
    pub fn with_block_budget(mut self, budget: Duration) -> Self {
        self.block_budget = budget;
        self
    }

    #[must_use]
    pub fn from_endpoint_data(
        data: &BTreeMap<String, String>,
        token: &str,
    ) -> Option<Result<Self>> {
        let url = data
            .get("ResultsServiceUrl")
            .filter(|url| !url.is_empty())?;
        if token.is_empty() {
            return None;
        }
        Some(Self::new(url, token))
    }

    pub async fn update_steps(
        &self,
        steps: &[TwirpStep],
        workflow_run_backend_id: &str,
        workflow_job_run_backend_id: &str,
        change_order: i64,
    ) -> Result<()> {
        let body = serde_json::to_string(&WorkflowStepsUpdateRequest {
            steps,
            change_order,
            workflow_job_run_backend_id,
            workflow_run_backend_id,
        })
        .context("serialize WorkflowStepsUpdate")?;
        let url = format!("{}/{STEPS_UPDATE}", self.results_service_url);
        retrying("WorkflowStepsUpdate", || async {
            self.twirp_acknowledged(&url, &body, "WorkflowStepsUpdate")
                .await
        })
        .await
    }

    pub async fn upload_step_log(
        &self,
        plan_id: &str,
        job_id: &str,
        step_id: &str,
        path: &Path,
    ) -> Result<()> {
        let scope = ScopeRequest {
            run: plan_id,
            job: job_id,
            step: Some(step_id),
        };
        self.upload_log(STEP_LOG, &scope, path).await
    }

    pub async fn upload_job_log(&self, plan_id: &str, job_id: &str, path: &Path) -> Result<()> {
        let scope = ScopeRequest {
            run: plan_id,
            job: job_id,
            step: None,
        };
        self.upload_log(JOB_LOG, &scope, path).await
    }

    pub async fn upload_step_summary(
        &self,
        plan_id: &str,
        job_id: &str,
        step_id: &str,
        path: &Path,
    ) -> Result<()> {
        let scope = ScopeRequest {
            run: plan_id,
            job: job_id,
            step: Some(step_id),
        };
        self.upload_summary(&scope, path).await
    }

    pub(super) fn receiver_url(&self, method: &str) -> String {
        format!("{}/{RECEIVER}/{method}", self.results_service_url)
    }

    pub(super) async fn twirp_acknowledged(
        &self,
        url: &str,
        body: &str,
        method: &str,
    ) -> Result<()> {
        let text = self.twirp_post(url, body, method).await?;
        let acknowledgement: Acknowledgement =
            serde_json::from_str(&text).with_context(|| format!("parse {method}"))?;
        if !acknowledgement.ok {
            bail!("{method} returned ok=false");
        }
        Ok(())
    }

    pub(super) async fn twirp_post(&self, url: &str, body: &str, method: &str) -> Result<String> {
        let (status, text) = github_json_request(
            Method::POST,
            url,
            self.token.expose(),
            Some(body.to_owned()),
            REQUEST_TIMEOUT,
        )
        .await
        .with_context(|| format!("{method} request"))?;
        if !(200..300).contains(&status) {
            bail!(
                "{method} failed: status={status}, body={}",
                sanitize_response_body(&text)
            );
        }
        Ok(text)
    }
}

pub(super) async fn retrying<F, Fut>(label: &str, mut attempt: F) -> Result<()>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    let mut last_error = String::new();
    for number in 0..ATTEMPTS {
        match attempt().await {
            Ok(()) => return Ok(()),
            Err(error) if error.is::<SoftSizeLimitExceeded>() => {
                return Err(error.context(format!("{label} failed")));
            }
            Err(error) => last_error = format!("{error:#}"),
        }
        if number + 1 < ATTEMPTS {
            tokio::time::sleep(RETRY_STEP * u32::try_from(number + 1).unwrap_or(1)).await;
        }
    }
    bail!("{label} failed after {ATTEMPTS} attempts: {last_error}")
}
