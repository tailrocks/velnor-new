// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use velnor_runner::exec::{JobCancel, JobResult};
use velnor_runner::lifecycle::LifecycleOutcome;

use super::exec_builder::{assert_gone, pid_from, wait_for_file};
use super::exec_support::Scratch;
use super::lifecycle_support::{
    CANCEL_BUDGET, Settings, fixture_job, job_replacing, run_in, run_with,
};
use super::support::mock_github::{
    Behavior, Cancel, MessageTypes, MockGithub, Renew, STEP_LOG_BLOB,
};

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
    let complete = mock.complete_request()?;
    assert_eq!(complete["conclusion"], "canceled");
    assert!(complete["stepResults"].as_array().is_none_or(Vec::is_empty));
    assert!(mock.requests_to("PUT", STEP_LOG_BLOB)?.is_empty());
    Ok(())
}

#[tokio::test]
async fn lifecycle_does_not_start_the_job_when_the_first_renew_never_succeeds() -> Result<()> {
    let behavior = Behavior {
        renew: Renew::ServerError,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;

    let outcome = run_with(
        &mock,
        Settings::new()
            .giving_up_after(Duration::from_secs(1))
            .within(CANCEL_BUDGET),
    )
    .await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Cancelled));
    assert!(
        mock.complete_request()?["stepResults"]
            .as_array()
            .is_none_or(Vec::is_empty)
    );
    assert!(mock.requests_to("PUT", STEP_LOG_BLOB)?.is_empty());
    Ok(())
}

#[tokio::test]
async fn lifecycle_does_not_start_the_job_before_the_first_renew_succeeds() -> Result<()> {
    let behavior = Behavior {
        renew: Renew::RenewedAfter(Duration::from_millis(1500)),
        ..Behavior::default()
    };
    let scratch = Scratch::new()?;
    let marker = scratch.work_dir().with_file_name("started");
    let script = format!("touch {}", marker.display());
    let mock = MockGithub::start(
        job_replacing("hello", Some(("echo hello", &script)))?,
        behavior,
    )
    .await?;
    let started = std::time::SystemTime::now();

    run_in(&mock, &scratch, Settings::new()).await??;

    let touched = std::fs::metadata(&marker)?.modified()?;
    assert!(
        touched.duration_since(started)? >= Duration::from_millis(1400),
        "the step started before the first renew was answered"
    );
    Ok(())
}

#[tokio::test]
async fn lifecycle_gives_up_when_renew_fails_past_the_lock_grace() -> Result<()> {
    let behavior = Behavior {
        renew: Renew::ExpiringThenFailing,
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

fn hanging_cleanup(scratch: &Scratch) -> Result<serde_json::Value> {
    let pid_file = scratch.work_dir().with_file_name("cleanup.pid");
    let script = format!(
        "sleep 4343 >/dev/null 2>&1 &\necho $! > {}\nwait",
        pid_file.display()
    );
    job_replacing("cancel", Some(("echo cleanup", &script)))
}

fn assert_stopped_at_the_deadline(
    mock: &MockGithub,
    scratch: &Scratch,
    outcome: LifecycleOutcome,
) -> Result<()> {
    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Cancelled));
    let complete = mock.complete_request()?;
    assert_eq!(complete["conclusion"], "canceled");
    let notes = complete["annotations"].as_array().context("annotations")?;
    assert!(
        notes.iter().any(|note| note["message"]
            .as_str()
            .is_some_and(|text| text.contains("cancellation timeout"))),
        "{notes:?}"
    );
    let pid_file = scratch.work_dir().with_file_name("cleanup.pid");
    wait_for_file(&pid_file)?;
    assert_gone(pid_from(&pid_file)?)
}

#[tokio::test]
async fn lifecycle_stops_a_cleanup_step_that_outlives_the_cancel_timeout_of_the_message()
-> Result<()> {
    let behavior = Behavior {
        cancel: Cancel::After(Duration::from_millis(300)),
        cancel_timeout: Duration::from_secs(20),
        ..Behavior::default()
    };
    let scratch = Scratch::new()?;
    let mock = MockGithub::start(hanging_cleanup(&scratch)?, behavior).await?;
    let started = Instant::now();

    let outcome = run_in(
        &mock,
        &scratch,
        Settings::new()
            .killing_after_cancel_floor(Duration::from_secs(16))
            .renewing_every(Duration::from_secs(60))
            .within(CANCEL_BUDGET),
    )
    .await??;

    assert!(
        started.elapsed() >= Duration::from_secs(5),
        "the 20 s of the message, less the 15 s reserve, were waited: {:?}",
        started.elapsed()
    );
    assert_stopped_at_the_deadline(&mock, &scratch, outcome)
}

#[tokio::test]
async fn lifecycle_stops_a_cleanup_step_that_outlives_the_floor_after_a_shutdown() -> Result<()> {
    let scratch = Scratch::new()?;
    let mock = MockGithub::start(hanging_cleanup(&scratch)?, Behavior::default()).await?;
    let shutdown = JobCancel::new();
    let signal = shutdown.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        signal.cancel();
    });
    let mut settings = Settings::new()
        .killing_after_cancel_floor(Duration::from_secs(16))
        .renewing_every(Duration::from_secs(60))
        .within(CANCEL_BUDGET);
    settings.shutdown = shutdown;

    let outcome = run_in(&mock, &scratch, settings).await??;

    assert_stopped_at_the_deadline(&mock, &scratch, outcome)
}
