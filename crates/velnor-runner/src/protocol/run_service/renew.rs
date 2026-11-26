// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/protocol.rs

use anyhow::{Context, Result};
use reqwest::Method;
use serde::{Deserialize, Serialize};

use super::super::error::github_api_error;
use super::super::http::github_json_request;
use super::{REQUEST_TIMEOUT, RunServiceClient, is_run_service_job_not_found};
use crate::protocol::error::GitHubApiError;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RenewJobRequest<'a> {
    plan_id: &'a str,
    job_id: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RenewJobResponse {
    #[serde(rename = "lockedUntil", alias = "LockedUntil")]
    pub locked_until: String,
}

#[must_use]
pub fn renew_failure_is_job_gone(error: &anyhow::Error) -> bool {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<GitHubApiError>())
        .is_some_and(|api| is_run_service_job_not_found(&api.body))
}

impl RunServiceClient {
    pub async fn renew_job(
        &self,
        run_service_url: &str,
        plan_id: &str,
        job_id: &str,
    ) -> Result<RenewJobResponse> {
        let url = Self::job_url(run_service_url, "renewjob")?;
        let body = serde_json::to_string(&RenewJobRequest { plan_id, job_id })
            .context("serialize renew job request")?;
        let (status, text) = github_json_request(
            Method::POST,
            url.as_str(),
            self.bearer_token.expose(),
            Some(body),
            REQUEST_TIMEOUT,
        )
        .await?;
        if !(200..300).contains(&status) {
            return Err(github_api_error("renew run-service job", status, &text));
        }
        serde_json::from_str(&text).context("parse renew run-service job response")
    }
}
