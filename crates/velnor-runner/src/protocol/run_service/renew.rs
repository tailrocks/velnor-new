// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/protocol.rs

use anyhow::{Context, Result};
use reqwest::Method;
use serde::{Deserialize, Serialize};

use super::super::error::{
    ServiceErrorKind, github_api_error, github_api_error_with, service_error_kind,
};
use super::super::http::github_json_request;
use super::{Failure, REQUEST_TIMEOUT, RunServiceClient, is_run_service_job_not_found};

const ACTION: &str = "renew run-service job";

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
    service_error_kind(error) == Some(ServiceErrorKind::JobNotFound)
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
        self.with_retries(|| self.renew_once(url.as_str(), &body))
            .await
    }

    async fn renew_once(&self, url: &str, body: &str) -> Result<RenewJobResponse, Failure> {
        let (status, text) = github_json_request(
            Method::POST,
            url,
            self.bearer_token.expose(),
            Some(body.to_owned()),
            REQUEST_TIMEOUT,
        )
        .await
        .map_err(|error| Failure::Retry(error.context("renew run-service job request")))?;
        if !(200..300).contains(&status) {
            return Err(if is_run_service_job_not_found(&text) {
                Failure::Fatal(github_api_error_with(
                    ACTION,
                    status,
                    &text,
                    None,
                    Some(ServiceErrorKind::JobNotFound),
                ))
            } else {
                Failure::Retry(github_api_error(ACTION, status, &text))
            });
        }
        serde_json::from_str(&text)
            .context("parse renew run-service job response")
            .map_err(Failure::Retry)
    }
}
