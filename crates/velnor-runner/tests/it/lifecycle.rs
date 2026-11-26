// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::Value;
use velnor_runner::exec::{AdapterRegistry, JobResult};
use velnor_runner::lifecycle::{LifecycleOutcome, RunJitOptions, run_jit};

use super::exec_support::{Scratch, fixture_path};
use super::support::mock_github::{
    Behavior, JOB_ID, JOB_LOG_BLOB, JOB_TOKEN, MockGithub, OAUTH_TOKEN, PLAN_ID,
    RESULTS_STEPS_UPDATE, Renew, SESSION_ID, STEP_LOG_BLOB, SUMMARY_BLOB,
};

const RUN_BUDGET: Duration = Duration::from_secs(60);

fn fixture_job(name: &str, script: Option<&str>) -> Result<Value> {
    let mut text = std::fs::read_to_string(fixture_path(name)).context("read the fixture")?;
    if let Some(script) = script {
        text = text.replace("echo hello", script);
    }
    serde_json::from_str(&text).context("parse the fixture")
}

async fn run(mock: &MockGithub, renew_interval: Duration) -> Result<LifecycleOutcome> {
    let scratch = Scratch::new()?;
    let options = RunJitOptions {
        jit: mock.jit().to_owned(),
        work_dir: scratch.work_dir(),
        renew_interval,
        cancel_poll_error_delay: Duration::from_millis(50),
        adapters: AdapterRegistry::empty(),
    };
    let outcome = tokio::time::timeout(RUN_BUDGET, run_jit(options))
        .await
        .context("run_jit finished in time")??;
    assert!(
        !scratch.work_dir().join("workspace").exists(),
        "the workspace is removed after the job"
    );
    Ok(outcome)
}

#[tokio::test]
async fn lifecycle_run_acquires_executes_and_completes_job() -> Result<()> {
    let mock = MockGithub::start(fixture_job("hello", None)?, Behavior::default()).await?;

    let outcome = run(&mock, Duration::from_millis(200)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    let acquire = mock.server.only_request("POST", "/run/acquirejob")?;
    assert_eq!(
        acquire.header("authorization"),
        Some(format!("Bearer {OAUTH_TOKEN}").as_str())
    );
    assert_eq!(acquire.json()?["jobMessageId"], "request-1");
    let complete = mock.server.only_request("POST", "/run/completejob")?;
    assert_eq!(
        complete.header("authorization"),
        Some(format!("Bearer {JOB_TOKEN}").as_str())
    );
    let body = complete.json()?;
    assert_eq!(body["conclusion"], "succeeded");
    assert_eq!(body["planId"], PLAN_ID);
    assert_eq!(body["jobId"], JOB_ID);
    assert_eq!(body["billingOwnerId"], "owner-1");
    let steps = body["stepResults"].as_array().context("step results")?;
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0]["name"], "Say hello");
    assert_eq!(steps[0]["number"], 1);
    assert_eq!(steps[0]["external_id"], JOB_ID);
    assert_eq!(steps[0]["conclusion"], "succeeded");
    assert_eq!(steps[0]["status"], "completed");
    let updates = mock.requests_to("POST", RESULTS_STEPS_UPDATE)?;
    let states: Vec<(i64, i64)> = updates
        .iter()
        .map(|update| {
            let body = update.json()?;
            Ok((
                body["change_order"].as_i64().context("change_order")?,
                body["steps"][0]["status"].as_i64().context("status")?,
            ))
        })
        .collect::<Result<_>>()?;
    assert_eq!(states, [(1, 3), (2, 6)]);
    let step_log = mock.requests_to("PUT", STEP_LOG_BLOB)?;
    assert_eq!(step_log.len(), 1);
    assert!(
        step_log[0]
            .text()
            .lines()
            .any(|line| line.ends_with("hello"))
    );
    assert!(
        mock.requests_to("PUT", JOB_LOG_BLOB)?[0]
            .text()
            .contains("hello")
    );
    let delete = mock.server.only_request("DELETE", "/broker/session")?;
    assert_eq!(delete.query("sessionId"), Some(SESSION_ID));
    Ok(())
}

#[tokio::test]
async fn lifecycle_uploads_masked_step_summary() -> Result<()> {
    let mock = MockGithub::start(fixture_job("step-summary", None)?, Behavior::default()).await?;

    run(&mock, Duration::from_millis(200)).await?;

    let summary = mock.requests_to("PUT", SUMMARY_BLOB)?;
    assert_eq!(summary.len(), 1);
    assert!(summary[0].text().contains("secret is ***"));
    assert!(!summary[0].text().contains("s3cr3t-value"));
    Ok(())
}

#[tokio::test]
async fn lifecycle_renews_job_while_step_runs() -> Result<()> {
    let mock =
        MockGithub::start(fixture_job("hello", Some("sleep 2"))?, Behavior::default()).await?;

    run(&mock, Duration::from_millis(200)).await?;

    let requests = mock.server.requests()?;
    let complete_at = requests
        .iter()
        .position(|request| request.path == "/run/completejob")
        .context("complete request")?;
    let renewals: Vec<_> = requests[..complete_at]
        .iter()
        .filter(|request| request.path == "/run/renewjob")
        .collect();
    assert!(renewals.len() >= 3, "saw {} renewals", renewals.len());
    for renewal in renewals {
        assert_eq!(
            renewal.header("authorization"),
            Some(format!("Bearer {JOB_TOKEN}").as_str())
        );
        assert_eq!(renewal.json()?["planId"], PLAN_ID);
    }
    Ok(())
}

#[tokio::test]
async fn lifecycle_reports_unsupported_job_as_failed_completion() -> Result<()> {
    let mock =
        MockGithub::start(fixture_job("unsupported-uses", None)?, Behavior::default()).await?;

    let outcome = run(&mock, Duration::from_millis(200)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Failed));
    assert_eq!(mock.complete_request()?["conclusion"], "failed");
    let job_log = mock.requests_to("PUT", JOB_LOG_BLOB)?;
    assert_eq!(job_log.len(), 1);
    assert!(
        job_log[0]
            .text()
            .contains("unsupported: uses actions/foo@v1")
    );
    Ok(())
}

#[tokio::test]
async fn lifecycle_cancel_message_cancels_running_step() -> Result<()> {
    let behavior = Behavior {
        cancel_after: Some(Duration::from_millis(300)),
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("cancel", None)?, behavior).await?;
    let started = Instant::now();

    let outcome = run(&mock, Duration::from_secs(60)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Cancelled));
    assert_eq!(mock.complete_request()?["conclusion"], "canceled");
    assert!(started.elapsed() < Duration::from_secs(15));
    Ok(())
}

#[tokio::test]
async fn lifecycle_renew_404_cancels_job() -> Result<()> {
    let behavior = Behavior {
        renew: Renew::JobGone,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("cancel", None)?, behavior).await?;
    let started = Instant::now();

    let outcome = run(&mock, Duration::from_millis(200)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Cancelled));
    assert_eq!(mock.complete_request()?["conclusion"], "canceled");
    assert!(started.elapsed() < Duration::from_secs(15));
    Ok(())
}
