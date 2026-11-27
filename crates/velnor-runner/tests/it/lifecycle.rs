// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use anyhow::{Context, Result};
use velnor_runner::exec::JobResult;
use velnor_runner::lifecycle::LifecycleOutcome;

use super::exec_support::Scratch;
use super::lifecycle_support::{CANCEL_BUDGET, Settings, fixture_job, run, run_in};
use super::support::mock_github::{
    Behavior, JOB_ID, JOB_LOG_BLOB, JOB_TOKEN, MockGithub, OAUTH_TOKEN, PLAN_ID,
    RESULTS_STEPS_UPDATE, SESSION_ID, STEP_LOG_BLOB, STEP_LOG_URL, SUMMARY_BLOB, step_id,
};

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
    assert_eq!(steps[0]["external_id"], step_id(1));
    assert_ne!(steps[0]["external_id"], JOB_ID);
    assert_eq!(steps[0]["conclusion"], "succeeded");
    assert_eq!(steps[0]["status"], "completed");
    let updates = mock.requests_to("POST", RESULTS_STEPS_UPDATE)?;
    let states: Vec<(i64, i64)> = updates
        .iter()
        .map(|update| {
            let body = update.json()?;
            assert_eq!(body["steps"][0]["external_id"], step_id(1));
            Ok((
                body["change_order"].as_i64().context("change_order")?,
                body["steps"][0]["status"].as_i64().context("status")?,
            ))
        })
        .collect::<Result<_>>()?;
    assert_eq!(states, [(1, 3), (2, 6)]);
    let step_log_url = mock.server.only_request("POST", STEP_LOG_URL)?.json()?;
    assert_eq!(step_log_url["step_backend_id"], step_id(1));
    assert_eq!(step_log_url["workflow_job_run_backend_id"], JOB_ID);
    assert_eq!(step_log_url["workflow_run_backend_id"], PLAN_ID);
    let step_log = mock.requests_to("PUT", STEP_LOG_BLOB)?;
    assert_eq!(step_log.len(), 1);
    assert!(
        step_log[0]
            .text()
            .lines()
            .any(|line| line.ends_with("hello"))
    );
    assert!(mock.uploaded(JOB_LOG_BLOB)?.contains("hello"));
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
async fn lifecycle_renews_job_before_the_first_interval_passes() -> Result<()> {
    let mock = MockGithub::start(fixture_job("hello", None)?, Behavior::default()).await?;

    run(&mock, Duration::from_secs(60)).await?;

    let requests = mock.server.requests()?;
    let complete_at = requests
        .iter()
        .position(|request| request.path == "/run/completejob")
        .context("complete request")?;
    let renewals = requests[..complete_at]
        .iter()
        .filter(|request| request.path == "/run/renewjob")
        .count();
    assert_eq!(
        renewals, 1,
        "one renew happens at once, before the interval"
    );
    Ok(())
}

#[tokio::test]
async fn lifecycle_keeps_the_tool_cache_after_a_job() -> Result<()> {
    let script = "echo kept > \"$RUNNER_TOOL_CACHE/marker\"";
    let mock = MockGithub::start(fixture_job("hello", Some(script))?, Behavior::default()).await?;
    let scratch = Scratch::new()?;

    let outcome = run_in(&mock, &scratch, Settings::new().within(CANCEL_BUDGET)).await??;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    assert_eq!(
        std::fs::read_to_string(scratch.work_dir().join("_tool").join("marker"))?,
        "kept\n"
    );
    assert!(!scratch.work_dir().join("workspace").exists());
    Ok(())
}

async fn tool_cache_job(
    cache: impl FnOnce(&Scratch) -> std::path::PathBuf,
) -> Result<(Scratch, std::path::PathBuf)> {
    let script = "echo kept > \"$RUNNER_TOOL_CACHE/marker\"";
    let mock = MockGithub::start(fixture_job("hello", Some(script))?, Behavior::default()).await?;
    let scratch = Scratch::new()?;
    let cache = cache(&scratch);

    let outcome = run_in(
        &mock,
        &scratch,
        Settings::new()
            .caching_tools_in(cache.clone())
            .within(CANCEL_BUDGET),
    )
    .await??;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    Ok((scratch, cache))
}

#[tokio::test]
async fn lifecycle_clears_a_tool_cache_inside_a_cleared_directory_with_it() -> Result<()> {
    for inside in ["workspace/tools", "temp/tools", "logs/tools"] {
        let (_scratch, cache) = tool_cache_job(|scratch| scratch.work_dir().join(inside)).await?;

        assert!(!cache.exists(), "{inside}");
    }
    Ok(())
}

#[tokio::test]
async fn lifecycle_keeps_the_default_tool_cache_written_through_a_cleared_directory() -> Result<()>
{
    let (scratch, _) =
        tool_cache_job(|scratch| scratch.work_dir().join("workspace/../_tool")).await?;

    assert_eq!(
        std::fs::read_to_string(scratch.work_dir().join("_tool").join("marker"))?,
        "kept\n"
    );
    Ok(())
}
