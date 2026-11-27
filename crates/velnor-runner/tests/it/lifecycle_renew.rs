// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use velnor_runner::exec::JobResult;
use velnor_runner::lifecycle::{LifecycleError, LifecycleOutcome};

use super::exec_support::Scratch;
use super::lifecycle_cancel::cancelled_with_cleanup;
use super::lifecycle_support::{CANCEL_BUDGET, Settings, fixture_job, run_in, run_until, run_with};
use super::support::mock_github::{Behavior, Cancel, MockGithub, Renew, STEP_LOG_BLOB};

fn assert_forced_failure(mock: &MockGithub, status: &str) -> Result<()> {
    let complete = mock.complete_request()?;
    assert_eq!(complete["conclusion"], "failed");
    assert!(
        complete
            .get("stepResults")
            .is_none_or(|steps| steps.as_array().is_some_and(Vec::is_empty))
    );
    let annotations = complete["annotations"].as_array().context("annotations")?;
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0]["level"], "failure");
    assert!(
        annotations[0]["message"].as_str().is_some_and(|message| {
            message.starts_with("The runner could not run the job: renew the job lock:")
                && message.contains(status)
        }),
        "{annotations:?}"
    );
    assert!(mock.requests_to("PUT", STEP_LOG_BLOB)?.is_empty());
    assert_eq!(mock.requests_to("DELETE", "/broker/session")?.len(), 1);
    Ok(())
}

async fn first_renew_ends_with(renew: Renew, status: &str) -> Result<MockGithub> {
    let behavior = Behavior {
        renew,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;
    let scratch = Scratch::new()?;

    let outcome = run_in(
        &mock,
        &scratch,
        Settings::new()
            .giving_up_after(Duration::from_secs(1))
            .within(CANCEL_BUDGET),
    )
    .await?;

    assert!(
        matches!(outcome, Err(LifecycleError::Renew(_))),
        "{outcome:?}"
    );
    assert_forced_failure(&mock, status)?;
    Ok(mock)
}

#[tokio::test]
async fn lifecycle_fails_the_job_when_the_first_renew_finds_it_gone() -> Result<()> {
    let mock = first_renew_ends_with(Renew::JobGone, "status=404").await?;

    assert_eq!(mock.requests_to("POST", "/run/renewjob")?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn lifecycle_fails_the_job_when_the_first_renew_never_succeeds() -> Result<()> {
    let mock = first_renew_ends_with(Renew::ServerError, "status=500").await?;

    assert!(
        mock.requests_to("POST", "/run/renewjob")?.len() > 1,
        "the first renew was retried before it gave up"
    );
    Ok(())
}

#[tokio::test]
async fn lifecycle_keeps_renewing_the_job_lock_until_the_completion_is_sent() -> Result<()> {
    let behavior = Behavior {
        complete_delay: Duration::from_millis(1500),
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;

    let outcome = run_with(
        &mock,
        Settings::new()
            .renewing_every(Duration::from_millis(200))
            .within(CANCEL_BUDGET),
    )
    .await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    let completing = mock.server.only_request("POST", "/run/completejob")?.at;
    let renewed_meanwhile = mock
        .requests_to("POST", "/run/renewjob")?
        .iter()
        .filter(|renew| renew.at > completing)
        .count();
    assert!(
        renewed_meanwhile >= 2,
        "{renewed_meanwhile} renewals while the completion was in flight"
    );
    Ok(())
}

#[tokio::test]
async fn lifecycle_cancels_the_job_when_a_later_renew_finds_it_gone() -> Result<()> {
    let behavior = Behavior {
        renew: Renew::RenewedThenJobGone,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("cancel", None)?, behavior).await?;

    let outcome = run_with(&mock, Settings::new().within(CANCEL_BUDGET)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Cancelled));
    cancelled_with_cleanup(&mock.complete_request()?)
}

#[tokio::test]
async fn lifecycle_cancels_the_job_when_a_shutdown_arrives_before_the_first_renew_answers()
-> Result<()> {
    let behavior = Behavior {
        renew: Renew::RenewedAfter(Duration::from_secs(5)),
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;
    let settings = Settings::new().within(CANCEL_BUDGET);

    let scratch = Scratch::new()?;
    let outcome = run_until(&mock, &scratch, settings, "the first renew request", || {
        Ok(!mock.requests_to("POST", "/run/renewjob")?.is_empty())
    })
    .await??;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Cancelled));
    let complete = mock.complete_request()?;
    assert_eq!(complete["conclusion"], "canceled");
    let annotations = complete["annotations"].as_array().context("annotations")?;
    assert!(
        annotations.iter().any(|note| note["message"]
            .as_str()
            .is_some_and(|text| text.contains("before it started"))),
        "{annotations:?}"
    );
    assert!(
        complete
            .get("stepResults")
            .is_none_or(|steps| steps.as_array().is_some_and(Vec::is_empty))
    );
    assert!(mock.requests_to("PUT", STEP_LOG_BLOB)?.is_empty());
    Ok(())
}

#[tokio::test]
async fn lifecycle_cancel_message_that_arrives_before_the_first_renew_answers_keeps_the_job_from_starting()
-> Result<()> {
    let behavior = Behavior {
        renew: Renew::RenewedAfter(Duration::from_millis(1500)),
        cancel: Cancel::After(Duration::from_millis(300)),
        ..Behavior::default()
    };
    let scratch = Scratch::new()?;
    let marker = scratch.work_dir().with_file_name("started");
    let script = format!("touch {}", marker.display());
    let mock = MockGithub::start(
        super::lifecycle_support::job_replacing("hello", Some(("echo hello", &script)))?,
        behavior,
    )
    .await?;

    let outcome = run_in(&mock, &scratch, Settings::new().within(CANCEL_BUDGET)).await??;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Cancelled));
    assert!(!marker.exists(), "a canceled job ran its step");
    let complete = mock.complete_request()?;
    assert_eq!(complete["conclusion"], "canceled");
    assert!(
        complete["annotations"]
            .as_array()
            .context("annotations")?
            .iter()
            .any(|note| note["message"]
                .as_str()
                .is_some_and(|text| text.contains("before it started"))),
        "{complete}"
    );
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
        super::lifecycle_support::job_replacing("hello", Some(("echo hello", &script)))?,
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
