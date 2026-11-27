// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use anyhow::{Context, Result};
use velnor_runner::exec::JobResult;
use velnor_runner::lifecycle::LifecycleOutcome;
use velnor_runner::protocol::SessionRecovery;

use super::lifecycle_support::{CANCEL_BUDGET, Settings, fixture_job, run_with};
use super::support::mock_github::{
    Behavior, Cancel, MockGithub, RECREATED_SESSION_ID, SESSION_ID, SessionCreate, SessionFault,
};

async fn lost_poll_cancels_the_job(behavior: Behavior) -> Result<MockGithub> {
    let mock = MockGithub::start(fixture_job("cancel", None)?, behavior).await?;

    let outcome = run_with(
        &mock,
        Settings::new()
            .renewing_every(Duration::from_secs(60))
            .retrying_polls_every(Duration::from_millis(1))
            .within(CANCEL_BUDGET),
    )
    .await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Cancelled));
    assert_eq!(mock.complete_request()?["conclusion"], "canceled");
    Ok(mock)
}

fn busy_polls(mock: &MockGithub) -> Result<usize> {
    Ok(mock
        .requests_to("GET", "/broker/message")?
        .iter()
        .filter(|poll| poll.query("status") == Some("Busy"))
        .count())
}

#[tokio::test]
async fn lifecycle_cancels_the_job_when_the_busy_poll_finds_the_runner_gone() -> Result<()> {
    let mock = lost_poll_cancels_the_job(Behavior {
        cancel: Cancel::BusyPollRunnerGone,
        ..Behavior::default()
    })
    .await?;
    assert_eq!(busy_polls(&mock)?, 1);
    Ok(())
}

async fn failing_busy_poll_cancels_the_job_after(status: u16, polls: usize) -> Result<()> {
    let mock = lost_poll_cancels_the_job(Behavior {
        cancel: Cancel::BusyPollFails(status),
        ..Behavior::default()
    })
    .await?;
    assert_eq!(busy_polls(&mock)?, polls);
    Ok(())
}

#[tokio::test]
async fn lifecycle_cancels_the_job_when_the_busy_poll_stays_unauthorized_after_the_refresh()
-> Result<()> {
    failing_busy_poll_cancels_the_job_after(401, 2).await
}

#[tokio::test]
async fn lifecycle_retries_a_busy_poll_that_answers_403_until_the_tenth_error() -> Result<()> {
    failing_busy_poll_cancels_the_job_after(403, 10).await
}

#[tokio::test]
async fn lifecycle_retries_a_busy_poll_that_answers_404_until_the_tenth_error() -> Result<()> {
    failing_busy_poll_cancels_the_job_after(404, 10).await
}

#[tokio::test]
async fn lifecycle_cancels_the_job_when_the_expired_session_is_refused_a_new_one() -> Result<()> {
    let mock = lost_poll_cancels_the_job(Behavior {
        cancel: Cancel::BusyPollSessionExpiresOnce,
        session: SessionCreate::failing_when_recreated(SessionFault::Forbidden, usize::MAX),
        ..Behavior::default()
    })
    .await?;
    assert_eq!(mock.requests_to("POST", "/broker/session")?.len(), 2);
    assert_eq!(busy_polls(&mock)?, 1);
    Ok(())
}

#[tokio::test]
async fn lifecycle_cancels_the_job_when_the_expired_session_must_not_be_recreated() -> Result<()> {
    let mock = lost_poll_cancels_the_job(Behavior {
        cancel: Cancel::BusyPollSessionExpiresOnce,
        recovery: SessionRecovery::Skip,
        ..Behavior::default()
    })
    .await?;
    assert_eq!(mock.requests_to("POST", "/broker/session")?.len(), 1);
    assert_eq!(busy_polls(&mock)?, 1);
    Ok(())
}

#[tokio::test]
async fn lifecycle_keeps_recreating_an_expired_session_through_transient_errors() -> Result<()> {
    let behavior = Behavior {
        cancel: Cancel::BusyPollSessionExpiresOnce,
        session: SessionCreate::failing_when_recreated(SessionFault::Unavailable, 4),
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", Some("sleep 2"))?, behavior).await?;

    let outcome = run_with(
        &mock,
        Settings::new().renewing_every(Duration::from_secs(60)),
    )
    .await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    assert_eq!(mock.complete_request()?["conclusion"], "succeeded");
    assert_eq!(mock.requests_to("POST", "/broker/session")?.len(), 6);
    Ok(())
}

#[tokio::test]
async fn lifecycle_recreates_a_session_that_expires_under_the_busy_poll_and_finishes_the_job()
-> Result<()> {
    let behavior = Behavior {
        cancel: Cancel::BusyPollSessionExpiresOnce,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", Some("sleep 2"))?, behavior).await?;

    let outcome = run_with(
        &mock,
        Settings::new().renewing_every(Duration::from_secs(60)),
    )
    .await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    assert_eq!(mock.complete_request()?["conclusion"], "succeeded");
    assert_eq!(mock.requests_to("POST", "/broker/session")?.len(), 2);
    let busy: Vec<_> = mock
        .requests_to("GET", "/broker/message")?
        .into_iter()
        .filter(|poll| poll.query("status") == Some("Busy"))
        .map(|poll| poll.query("sessionId").map(str::to_owned))
        .collect();
    assert_eq!(busy.first(), Some(&Some(SESSION_ID.to_owned())), "{busy:?}");
    assert!(
        busy.last() == Some(&Some(RECREATED_SESSION_ID.to_owned())),
        "{busy:?}"
    );
    let closed = mock.requests_to("DELETE", "/broker/session")?;
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].query("sessionId"), Some(RECREATED_SESSION_ID));
    Ok(())
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
