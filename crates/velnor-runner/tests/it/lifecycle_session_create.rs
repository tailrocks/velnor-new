// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use anyhow::Result;
use velnor_runner::exec::JobResult;
use velnor_runner::lifecycle::{LifecycleError, LifecycleOutcome};

use super::exec_support::Scratch;
use super::lifecycle_support::shutdown_after;
use super::lifecycle_support::{CANCEL_BUDGET, Settings, fixture_job, run, run_in, run_with};
use super::support::mock_github::{
    Behavior, MockGithub, OAUTH_TOKEN, OAuthTokens, STALE_OAUTH_TOKEN, SessionCreate, SessionFault,
};

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

const EXCHANGES_WITHOUT_REFRESH: usize = 2;

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

async fn session_is_created_through(
    answers: &'static [u16],
    oauth: OAuthTokens,
) -> Result<MockGithub> {
    let behavior = Behavior {
        session: SessionCreate::Answers(answers),
        oauth,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;

    let outcome = run(&mock, Duration::from_millis(200)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    Ok(mock)
}

#[tokio::test]
async fn lifecycle_signs_in_again_when_the_token_expires_while_the_session_is_unavailable()
-> Result<()> {
    let mock = session_is_created_through(&[503, 503, 503, 401], OAuthTokens::StaleFirst).await?;

    let creations = mock.requests_to("POST", "/broker/session")?;
    let tokens: Vec<_> = creations
        .iter()
        .map(|request| request.header("authorization"))
        .collect();
    let (stale, fresh) = (bearer(STALE_OAUTH_TOKEN), bearer(OAUTH_TOKEN));
    assert_eq!(
        tokens,
        [
            Some(stale.as_str()),
            Some(stale.as_str()),
            Some(stale.as_str()),
            Some(stale.as_str()),
            Some(fresh.as_str())
        ]
    );
    assert_eq!(
        mock.requests_to("POST", "/oauth/token")?.len(),
        EXCHANGES_WITHOUT_REFRESH + 1
    );
    Ok(())
}

#[tokio::test]
async fn lifecycle_signs_in_again_for_every_run_of_unauthorized_answers_to_the_session()
-> Result<()> {
    let mock = session_is_created_through(&[401, 503, 401], OAuthTokens::Fresh).await?;

    assert_eq!(mock.requests_to("POST", "/broker/session")?.len(), 4);
    assert_eq!(
        mock.requests_to("POST", "/oauth/token")?.len(),
        EXCHANGES_WITHOUT_REFRESH + 2
    );
    Ok(())
}

#[tokio::test]
async fn lifecycle_retries_a_failed_sign_in_while_the_session_is_unauthorized() -> Result<()> {
    let mock = session_is_created_through(&[401], OAuthTokens::UnavailableAfterSignIn(1)).await?;

    assert_eq!(mock.requests_to("POST", "/broker/session")?.len(), 2);
    assert_eq!(
        mock.requests_to("POST", "/oauth/token")?.len(),
        EXCHANGES_WITHOUT_REFRESH + 2
    );
    let paths: Vec<String> = mock
        .server
        .requests()?
        .into_iter()
        .map(|request| request.path)
        .filter(|path| path == "/oauth/token" || path == "/broker/session")
        .collect();
    assert_eq!(
        paths[..5],
        [
            "/oauth/token",
            "/broker/session",
            "/oauth/token",
            "/oauth/token",
            "/broker/session"
        ]
    );
    Ok(())
}

#[tokio::test]
async fn lifecycle_retries_a_sign_in_that_fails_before_the_first_session_is_created() -> Result<()>
{
    let mock = session_is_created_through(&[], OAuthTokens::UnavailableFirst(3)).await?;

    assert_eq!(mock.requests_to("POST", "/broker/session")?.len(), 1);
    assert_eq!(
        mock.requests_to("POST", "/oauth/token")?.len(),
        EXCHANGES_WITHOUT_REFRESH + 3
    );
    Ok(())
}

async fn sign_in_ends_the_run_after(oauth: OAuthTokens, exchanges: usize) -> Result<()> {
    let behavior = Behavior {
        oauth,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;
    let scratch = Scratch::new()?;
    let settings = Settings::new()
        .within(Duration::from_secs(5))
        .retrying_sessions_every(Duration::from_millis(1));

    let outcome = run_in(&mock, &scratch, settings).await?;

    assert!(
        matches!(outcome, Err(LifecycleError::SignIn(_))),
        "{outcome:?}"
    );
    assert_eq!(mock.requests_to("POST", "/oauth/token")?.len(), exchanges);
    assert!(mock.requests_to("POST", "/broker/session")?.is_empty());
    assert!(mock.requests_to("DELETE", "/broker/session")?.is_empty());
    Ok(())
}

#[tokio::test]
async fn lifecycle_gives_up_at_once_when_the_runner_registration_is_deleted() -> Result<()> {
    let deleted = OAuthTokens::Rejected("invalid_client", "The client was deleted");
    sign_in_ends_the_run_after(deleted, 1).await
}

#[tokio::test]
async fn lifecycle_gives_up_at_once_when_a_deleted_registration_also_reports_clock_skew()
-> Result<()> {
    let both = OAuthTokens::Rejected(
        "invalid_client",
        "The assertion is not valid yet. Current server time is 2026-11-27T10:00:00Z",
    );
    sign_in_ends_the_run_after(both, 1).await
}

#[tokio::test]
async fn lifecycle_gives_up_on_a_clock_skewed_sign_in_after_sixty_attempts() -> Result<()> {
    let skewed = OAuthTokens::Rejected(
        "invalid_request",
        "The assertion is not valid yet. Current server time is 2026-11-27T10:00:00Z",
    );
    sign_in_ends_the_run_after(skewed, 60).await
}

#[tokio::test]
async fn lifecycle_retries_a_session_creation_the_broker_answers_with_404_and_400() -> Result<()> {
    let mock = session_is_created_through(&[404, 400], OAuthTokens::Fresh).await?;

    assert_eq!(mock.requests_to("POST", "/broker/session")?.len(), 3);
    Ok(())
}

#[tokio::test]
async fn lifecycle_gives_up_on_a_session_that_stays_unauthorized_after_signing_in_again()
-> Result<()> {
    let behavior = Behavior {
        session: SessionCreate::Answers(&[401, 401]),
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
    assert_eq!(mock.requests_to("POST", "/broker/session")?.len(), 2);
    assert_eq!(mock.requests_to("POST", "/oauth/token")?.len(), 2);
    assert!(mock.requests_to("POST", "/run/acquirejob")?.is_empty());
    Ok(())
}
