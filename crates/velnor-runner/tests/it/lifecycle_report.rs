// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;
use velnor_runner::exec::JobResult;
use velnor_runner::lifecycle::LifecycleOutcome;

use super::lifecycle_support::{fixture_job, run};
use super::support::mock_github::{
    Behavior, JOB_LOG_BLOB, MockGithub, RESULTS_STEPS_UPDATE, STEP_LOG_BLOB,
};

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
    let job_log = mock.requests_to("PUT", JOB_LOG_BLOB)?;
    assert!(
        job_log
            .first()
            .context("the job log was uploaded")?
            .text()
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
