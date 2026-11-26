// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use anyhow::{Context, Result};
use velnor_runner::exec::JobResult;
use velnor_runner::lifecycle::LifecycleOutcome;

use super::lifecycle_support::{CANCEL_BUDGET, Settings, fixture_job, run_with};
use super::support::mock_github::{Behavior, Cancel, MockGithub};

async fn lost_poll_cancels_the_job(status: u16) -> Result<()> {
    let behavior = Behavior {
        cancel: Cancel::BusyPollFails(status),
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
    assert_eq!(mock.complete_request()?["conclusion"], "canceled");
    Ok(())
}

#[tokio::test]
async fn lifecycle_cancels_the_job_when_the_broker_session_expires_under_the_busy_poll()
-> Result<()> {
    lost_poll_cancels_the_job(404).await
}

#[tokio::test]
async fn lifecycle_cancels_the_job_when_the_busy_poll_stays_forbidden_after_a_token_refresh()
-> Result<()> {
    lost_poll_cancels_the_job(403).await
}

#[tokio::test]
async fn lifecycle_honors_a_cancellation_that_arrives_while_the_logs_are_still_uploading()
-> Result<()> {
    let behavior = Behavior {
        cancel: Cancel::After(Duration::from_secs(2)),
        cancel_timeout: Duration::from_secs(16),
        blob_delay: Duration::from_secs(30),
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;

    let outcome = run_with(
        &mock,
        Settings::new()
            .killing_after_cancel_floor(Duration::from_secs(16))
            .renewing_every(Duration::from_secs(60))
            .within(CANCEL_BUDGET),
    )
    .await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Cancelled));
    let complete = mock.complete_request()?;
    assert_eq!(complete["conclusion"], "canceled");
    let notes = complete["annotations"].as_array().context("annotations")?;
    assert!(
        notes.iter().any(|note| note["message"]
            .as_str()
            .is_some_and(|text| text.contains("not uploaded in time"))),
        "{notes:?}"
    );
    Ok(())
}
