// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::Value;
use velnor_runner::exec::JobResult;
use velnor_runner::lifecycle::LifecycleOutcome;

use super::lifecycle_support::{
    CANCEL_BUDGET, Settings, fixture_job, job_with_steps, run, run_with,
};
use super::support::mock_github::{
    Behavior, JOB_LOG_BLOB, MockGithub, RESULTS_STEPS_UPDATE, STEP_LOG_BLOB, StepStatus,
};

const MANY_STEPS: usize = 40;

const WORKFLOW_STEP_COMPLETED: i64 = 6;
const CONCLUSION_SUCCESS: i64 = 2;
const CONCLUSION_SKIPPED: i64 = 7;

fn array<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>> {
    value[key].as_array().with_context(|| format!("{key} list"))
}

#[tokio::test]
async fn lifecycle_reports_unsupported_job_as_failed_completion() -> Result<()> {
    let mock =
        MockGithub::start(fixture_job("unsupported-uses", None)?, Behavior::default()).await?;

    let outcome = run(&mock, Duration::from_millis(200)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Failed));
    let complete = mock.complete_request()?;
    assert_eq!(complete["conclusion"], "failed");
    let job_annotations = array(&complete, "annotations")?;
    assert_eq!(job_annotations.len(), 1);
    assert_eq!(job_annotations[0]["level"], "failure");
    assert!(
        job_annotations[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("unsupported: uses actions/foo@v1"))
    );
    assert!(
        mock.uploaded(JOB_LOG_BLOB)?
            .contains("unsupported: uses actions/foo@v1")
    );
    Ok(())
}

#[tokio::test]
async fn lifecycle_puts_step_annotations_in_the_step_result_and_job_ones_in_the_list() -> Result<()>
{
    let mock = MockGithub::start(fixture_job("annotations", None)?, Behavior::default()).await?;

    let outcome = run(&mock, Duration::from_millis(200)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Failed));
    let complete = mock.complete_request()?;
    assert!(
        complete
            .get("annotations")
            .is_none_or(|list| list.as_array().is_some_and(Vec::is_empty)),
        "step annotations stay out of the job list: {complete}"
    );
    let steps = array(&complete, "stepResults")?;
    let annotations = array(&steps[0], "annotations")?;
    let summary: Vec<(&str, &str)> = annotations
        .iter()
        .map(|annotation| {
            Ok((
                annotation["level"].as_str().context("level")?,
                annotation["message"].as_str().context("message")?,
            ))
        })
        .collect::<Result<_>>()?;
    assert_eq!(
        summary,
        [
            ("failure", "bad\nthing"),
            ("notice", "fyi"),
            ("warning", "careful"),
            ("failure", "Process completed with exit code 1."),
        ]
    );
    assert_eq!(annotations[0]["title"], "T:1");
    assert_eq!(annotations[0]["path"], "a.rs");
    assert_eq!(annotations[0]["startLine"], 3);
    assert_eq!(annotations[0]["stepNumber"], 1);
    Ok(())
}

#[tokio::test]
async fn lifecycle_reports_continue_on_error_and_skipped_steps_in_the_final_batch() -> Result<()> {
    let mock =
        MockGithub::start(fixture_job("continue-on-error", None)?, Behavior::default()).await?;

    let outcome = run(&mock, Duration::from_millis(200)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Failed));
    let complete = mock.complete_request()?;
    let steps = array(&complete, "stepResults")?;
    let conclusions: Vec<&Value> = steps.iter().map(|step| &step["conclusion"]).collect();
    assert_eq!(
        conclusions,
        ["succeeded", "succeeded", "failed", "succeeded", "skipped"]
    );
    for step in &steps[..4] {
        let started = step["started_at"].as_str().context("started_at")?;
        let completed = step["completed_at"].as_str().context("completed_at")?;
        assert!(started <= completed, "{started} {completed}");
    }
    assert!(steps[4]["completed_at"].is_string());
    let updates = mock.requests_to("POST", RESULTS_STEPS_UPDATE)?;
    let last = updates.last().context("a final step batch")?.json()?;
    let batch: Vec<(i64, i64, i64)> = array(&last, "steps")?
        .iter()
        .map(|step| {
            Ok((
                step["number"].as_i64().context("number")?,
                step["status"].as_i64().context("status")?,
                step["conclusion"].as_i64().context("conclusion")?,
            ))
        })
        .collect::<Result<_>>()?;
    assert_eq!(
        batch,
        [
            (1, WORKFLOW_STEP_COMPLETED, CONCLUSION_SUCCESS),
            (5, WORKFLOW_STEP_COMPLETED, CONCLUSION_SKIPPED),
        ]
    );
    assert!(last["steps"][0]["started_at"].is_string());
    assert!(last["steps"][0]["completed_at"].is_string());
    assert!(last["steps"][1]["completed_at"].is_string());
    Ok(())
}

#[tokio::test]
async fn lifecycle_completes_the_job_even_when_every_upload_fails() -> Result<()> {
    let behavior = Behavior {
        blob_status: 500,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;

    let outcome = run(&mock, Duration::from_millis(200)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    assert!(!mock.requests_to("PUT", STEP_LOG_BLOB)?.is_empty());
    assert!(!mock.requests_to("PUT", JOB_LOG_BLOB)?.is_empty());
    let complete = mock.complete_request()?;
    assert_eq!(complete["conclusion"], "succeeded");
    assert_eq!(
        array(&complete, "stepResults")?[0]["conclusion"],
        "succeeded"
    );
    Ok(())
}

#[tokio::test]
async fn lifecycle_does_not_wait_for_step_status_updates_that_never_answer() -> Result<()> {
    let behavior = Behavior {
        status: StepStatus::Stalls,
        ..Behavior::default()
    };
    let mock = MockGithub::start(job_with_steps(MANY_STEPS)?, behavior).await?;
    let started = Instant::now();

    let outcome = run_with(
        &mock,
        Settings::new()
            .reporting_within(Duration::from_millis(500))
            .within(CANCEL_BUDGET),
    )
    .await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    assert!(started.elapsed() < Duration::from_secs(10));
    let complete = mock.complete_request()?;
    assert_eq!(array(&complete, "stepResults")?.len(), MANY_STEPS);
    Ok(())
}

#[tokio::test]
async fn lifecycle_uploads_only_a_few_step_logs_at_once() -> Result<()> {
    let behavior = Behavior {
        blob_delay: Duration::from_millis(600),
        ..Behavior::default()
    };
    let mock = MockGithub::start(job_with_steps(12)?, behavior).await?;

    let outcome = run_with(&mock, Settings::new().within(CANCEL_BUDGET)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    assert_eq!(mock.requests_to("PUT", STEP_LOG_BLOB)?.len(), 12);
    let peak = mock.server.peak_concurrency(STEP_LOG_BLOB)?;
    assert!((2..=4).contains(&peak), "{peak} uploads were in flight");
    Ok(())
}

#[tokio::test]
async fn lifecycle_completes_the_job_when_the_uploads_do_not_finish_within_the_drain_budget()
-> Result<()> {
    let behavior = Behavior {
        blob_delay: Duration::from_secs(30),
        ..Behavior::default()
    };
    let mock = MockGithub::start(job_with_steps(12)?, behavior).await?;
    let started = Instant::now();

    let outcome = run_with(
        &mock,
        Settings::new()
            .draining_uploads_within(Duration::from_secs(1))
            .within(CANCEL_BUDGET),
    )
    .await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    assert!(started.elapsed() < Duration::from_secs(10));
    let complete = mock.complete_request()?;
    assert_eq!(complete["conclusion"], "succeeded");
    let annotations = array(&complete, "annotations")?;
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0]["level"], "warning");
    assert!(
        annotations[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("not uploaded in time")),
        "{annotations:?}"
    );
    Ok(())
}

#[tokio::test]
async fn lifecycle_sends_the_later_step_updates_in_order_when_the_first_one_never_answers()
-> Result<()> {
    let behavior = Behavior {
        status: StepStatus::StallsFirst,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", Some("sleep 1"))?, behavior).await?;

    let outcome = run_with(
        &mock,
        Settings::new()
            .reporting_within(Duration::from_millis(300))
            .within(CANCEL_BUDGET),
    )
    .await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    let orders: Vec<i64> = mock
        .requests_to("POST", RESULTS_STEPS_UPDATE)?
        .iter()
        .map(|update| {
            update.json()?["change_order"]
                .as_i64()
                .context("change_order")
        })
        .collect::<Result<_>>()?;
    assert!(orders.len() >= 2, "{orders:?}");
    assert!(
        orders.windows(2).all(|pair| pair[0] < pair[1]),
        "{orders:?}"
    );
    Ok(())
}

fn with_environment(mut job: Value, url: &Value) -> Value {
    job["ActionsEnvironment"] = serde_json::json!({"Name": "production", "Url": url});
    job
}

#[tokio::test]
async fn lifecycle_sends_the_evaluated_environment_url_with_the_completion() -> Result<()> {
    let url = serde_json::json!({
        "type": 3,
        "expr": "format('https://deploy.example/{0}', 'app')"
    });
    let job = with_environment(fixture_job("hello", None)?, &url);
    let mock = MockGithub::start(job, Behavior::default()).await?;

    let outcome = run(&mock, Duration::from_millis(200)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    let complete = mock.complete_request()?;
    assert_eq!(complete["environmentUrl"], "https://deploy.example/app");
    Ok(())
}

#[tokio::test]
async fn lifecycle_leaves_out_an_environment_url_that_reveals_a_secret() -> Result<()> {
    let url = serde_json::json!({
        "type": 3,
        "expr": "format('https://deploy.example/{0}', secrets.MY_SECRET)"
    });
    let mut job = with_environment(fixture_job("hello", None)?, &url);
    job["Variables"] =
        serde_json::json!({"MY_SECRET": {"Value": "s3cr3t-value", "IsSecret": true}});
    let mock = MockGithub::start(job, Behavior::default()).await?;

    let outcome = run(&mock, Duration::from_millis(200)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    let complete = mock.complete_request()?;
    assert!(complete.get("environmentUrl").is_none_or(Value::is_null));
    let notes = array(&complete, "annotations")?;
    assert!(
        notes.iter().any(|note| note["message"]
            .as_str()
            .is_some_and(|text| text.contains("Skip setting environment url"))),
        "{notes:?}"
    );
    Ok(())
}
