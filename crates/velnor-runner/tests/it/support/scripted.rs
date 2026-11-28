// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Error, Result};
use serde_json::Value;
use velnor_runner::protocol::{
    GitHubApiError, RetryDelays, RunServiceClient, RunServiceCompleteJob, TaskResult,
};

use super::{JOB_ID, PLAN_ID, RecordedRequest, Reply, TOKEN, TestServer};

pub(crate) fn assert_bearer(request: &RecordedRequest) {
    assert_eq!(
        request.header("authorization"),
        Some(format!("Bearer {TOKEN}").as_str())
    );
}

pub(crate) fn assert_github_status(error: &Error, status: u16, action: &str) -> Result<()> {
    let api_error = error
        .downcast_ref::<GitHubApiError>()
        .with_context(|| format!("expected GitHubApiError, got {error:#}"))?;
    assert_eq!(api_error.status, status);
    assert_eq!(api_error.action, action);
    Ok(())
}

#[derive(Clone)]
pub(crate) enum Scripted {
    Text(u16, &'static str),
    Json(u16, Value),
    Hangup,
}

pub(crate) fn scripted(
    steps: Vec<Scripted>,
) -> impl Fn(&RecordedRequest) -> Reply + Send + Sync + 'static {
    let next = AtomicUsize::new(0);
    move |_| {
        let index = next.fetch_add(1, Ordering::SeqCst);
        match steps.get(index).or_else(|| steps.last()) {
            Some(Scripted::Text(status, body)) => Reply::text(*status, body),
            Some(Scripted::Json(status, body)) => Reply::json(*status, body),
            Some(Scripted::Hangup) => Reply::hangup(),
            None => Reply::status(500),
        }
    }
}

pub(crate) fn run_service_error(code: u16) -> Value {
    serde_json::json!({
        "source": "actions-run-service",
        "statusCode": code,
        "errorMessage": "run-service refused"
    })
}

pub(crate) fn complete_job() -> RunServiceCompleteJob {
    RunServiceCompleteJob {
        plan_id: PLAN_ID.to_owned(),
        job_id: JOB_ID.to_owned(),
        conclusion: TaskResult::Succeeded,
        outputs: BTreeMap::new(),
        step_results: Vec::new(),
        annotations: Vec::new(),
        telemetry: Vec::new(),
        environment_url: None,
        billing_owner_id: Some("42".to_owned()),
        infrastructure_failure_category: None,
    }
}

pub(crate) fn immediate_run_service() -> Result<RunServiceClient> {
    Ok(RunServiceClient::new(TOKEN)?.with_retry_delays(RetryDelays::IMMEDIATE))
}

pub(crate) fn run_service_url(server: &TestServer) -> String {
    format!("{}/run/jobs/123", server.uri())
}

pub(crate) fn foreign_source_error(code: u16) -> Value {
    serde_json::json!({
        "source": "some-other-service",
        "statusCode": code,
        "errorMessage": "elsewhere"
    })
}
