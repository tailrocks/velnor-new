// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use anyhow::Result;
use velnor_runner::exec::JobResult;
use velnor_runner::lifecycle::{LifecycleError, LifecycleOutcome};

use super::exec_support::Scratch;
use super::lifecycle_support::shutdown_after;
use super::lifecycle_support::{CANCEL_BUDGET, Settings, fixture_job, run, run_in, run_with};
use super::support::mock_github::{Behavior, MockGithub, SessionCreate, SessionFault};

#[tokio::test]
async fn lifecycle_keeps_retrying_a_session_conflict_until_the_old_session_expires() -> Result<()> {
    let behavior = Behavior {
        session: SessionCreate::ConflictsFirst(6),
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;

    let outcome = run(&mock, Duration::from_millis(200)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    assert_eq!(mock.requests_to("POST", "/broker/session")?.len(), 7);
    Ok(())
}

async fn session_is_created_after(fault: SessionFault, failures: usize) -> Result<()> {
    let behavior = Behavior {
        session: SessionCreate::failing(fault, failures),
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;

    let outcome = run(&mock, Duration::from_millis(200)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    assert_eq!(
        mock.requests_to("POST", "/broker/session")?.len(),
        failures + 1
    );
    Ok(())
}

#[tokio::test]
async fn lifecycle_keeps_retrying_a_session_that_is_unavailable_until_it_is_created() -> Result<()>
{
    session_is_created_after(SessionFault::Unavailable, 4).await
}

#[tokio::test]
async fn lifecycle_keeps_retrying_a_session_answer_it_cannot_parse_until_it_is_created()
-> Result<()> {
    session_is_created_after(SessionFault::Unparsable, 2).await
}

#[tokio::test]
async fn lifecycle_stops_waiting_for_a_session_when_shutdown_arrives() -> Result<()> {
    for create in [
        SessionCreate::ConflictsAlways,
        SessionCreate::failing(SessionFault::Unavailable, usize::MAX),
    ] {
        let behavior = Behavior {
            session: create,
            ..Behavior::default()
        };
        let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;
        let mut settings = Settings::new()
            .within(CANCEL_BUDGET)
            .retrying_sessions_every(Duration::from_millis(200));
        settings.shutdown = shutdown_after(Duration::from_millis(500));

        let outcome = run_with(&mock, settings).await?;

        assert_eq!(outcome, LifecycleOutcome::NoJob, "{create:?}");
        assert!(mock.requests_to("DELETE", "/broker/session")?.is_empty());
    }
    Ok(())
}

async fn session_creation_ends_with_a_session_error_after(
    create: SessionCreate,
    attempts: usize,
) -> Result<()> {
    let behavior = Behavior {
        session: create,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;
    let scratch = Scratch::new()?;

    let outcome = run_in(
        &mock,
        &scratch,
        Settings::new().within(Duration::from_secs(5)),
    )
    .await?;

    assert!(
        matches!(outcome, Err(LifecycleError::Session(_))),
        "{outcome:?}"
    );
    assert_eq!(mock.requests_to("POST", "/broker/session")?.len(), attempts);
    assert!(mock.requests_to("DELETE", "/broker/session")?.is_empty());
    assert!(mock.requests_to("POST", "/run/acquirejob")?.is_empty());
    Ok(())
}

#[tokio::test]
async fn lifecycle_gives_up_on_a_session_conflict_after_eight_attempts() -> Result<()> {
    session_creation_ends_with_a_session_error_after(SessionCreate::ConflictsAlways, 8).await
}

#[tokio::test]
async fn lifecycle_gives_up_at_once_on_a_session_the_broker_forbids() -> Result<()> {
    let forbidden = SessionCreate::failing(SessionFault::Forbidden, usize::MAX);
    session_creation_ends_with_a_session_error_after(forbidden, 1).await
}
