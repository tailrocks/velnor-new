// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use velnor_runner::exec::JobResult;
use velnor_runner::lifecycle::LifecycleOutcome;

use super::lifecycle_support::{CANCEL_BUDGET, Settings, fixture_job, run_with};
use super::support::mock_github::{Behavior, Cancel, MessageTypes, MockGithub, Renew};

fn cancelled_with_cleanup(complete: &serde_json::Value) -> Result<()> {
    assert_eq!(complete["conclusion"], "canceled");
    let steps = complete["stepResults"].as_array().context("step results")?;
    let conclusions: Vec<_> = steps.iter().map(|step| &step["conclusion"]).collect();
    assert_eq!(conclusions, ["canceled", "succeeded"]);
    assert_eq!(steps[1]["name"], "Cleanup");
    Ok(())
}

#[tokio::test]
async fn lifecycle_cancel_message_cancels_running_step() -> Result<()> {
    let behavior = Behavior {
        cancel: Cancel::After(Duration::from_millis(300)),
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("cancel", None)?, behavior).await?;

    let outcome = run_with(
        &mock,
        Settings::new()
            .renewing_every(Duration::from_secs(60))
            .within(CANCEL_BUDGET),
    )
    .await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Cancelled));
    cancelled_with_cleanup(&mock.complete_request()?)
}

#[tokio::test]
async fn lifecycle_cancel_message_for_another_job_is_ignored() -> Result<()> {
    let behavior = Behavior {
        cancel: Cancel::AfterACancelOfAnotherJob(Duration::from_millis(700)),
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("cancel", None)?, behavior).await?;

    let outcome = run_with(
        &mock,
        Settings::new()
            .renewing_every(Duration::from_secs(60))
            .within(CANCEL_BUDGET),
    )
    .await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Cancelled));
    assert!(
        mock.polls("Busy")? >= 2,
        "the job kept running after the cancellation of another job"
    );
    cancelled_with_cleanup(&mock.complete_request()?)
}

#[tokio::test]
async fn lifecycle_message_types_are_matched_case_insensitively() -> Result<()> {
    let behavior = Behavior {
        message_types: MessageTypes::Lowercase,
        cancel: Cancel::After(Duration::from_millis(300)),
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("cancel", None)?, behavior).await?;

    let outcome = run_with(
        &mock,
        Settings::new()
            .renewing_every(Duration::from_secs(60))
            .within(CANCEL_BUDGET),
    )
    .await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Cancelled));
    cancelled_with_cleanup(&mock.complete_request()?)
}

#[tokio::test]
async fn lifecycle_renew_404_cancels_job() -> Result<()> {
    let behavior = Behavior {
        renew: Renew::JobGone,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("cancel", None)?, behavior).await?;

    let outcome = run_with(&mock, Settings::new().within(CANCEL_BUDGET)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Cancelled));
    cancelled_with_cleanup(&mock.complete_request()?)
}

#[tokio::test]
async fn lifecycle_gives_up_when_renew_fails_past_the_lock_grace() -> Result<()> {
    let behavior = Behavior {
        renew: Renew::ServerError,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("cancel", None)?, behavior).await?;
    let started = Instant::now();

    let outcome = run_with(
        &mock,
        Settings::new()
            .giving_up_after(Duration::from_secs(1))
            .within(CANCEL_BUDGET),
    )
    .await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Cancelled));
    assert!(
        mock.requests_to("POST", "/run/renewjob")?.len() >= 2,
        "the renewal was retried before it gave up"
    );
    assert!(started.elapsed() < CANCEL_BUDGET);
    cancelled_with_cleanup(&mock.complete_request()?)
}
