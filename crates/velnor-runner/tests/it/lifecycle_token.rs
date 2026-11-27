// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use anyhow::Result;
use velnor_runner::exec::JobResult;
use velnor_runner::lifecycle::{LifecycleError, LifecycleOutcome};
use velnor_runner::protocol::TokenBackoff;

use super::exec_support::Scratch;
use super::lifecycle_support::{CANCEL_BUDGET, Settings, fixture_job, run_in, run_until, run_with};
use super::support::mock_github::{
    Behavior, MockGithub, OAUTH_TOKEN, OAuthTokens, Online, STALE_OAUTH_TOKEN,
};

#[tokio::test]
async fn lifecycle_acquires_the_job_when_a_token_exchange_fails_once() -> Result<()> {
    let behavior = Behavior {
        oauth: OAuthTokens::UnavailableAfterSignIn(1),
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;

    let outcome = run_with(&mock, Settings::new().within(CANCEL_BUDGET)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    assert_eq!(
        mock.requests_to("POST", "/oauth/token")?.len(),
        3,
        "the sign-in, the exchange that failed and the one that was retried"
    );
    mock.server.only_request("POST", "/run/acquirejob")?;
    assert_eq!(mock.complete_request()?["conclusion"], "succeeded");
    Ok(())
}

#[tokio::test]
async fn lifecycle_stops_acquiring_after_five_failed_token_exchanges() -> Result<()> {
    let behavior = Behavior {
        oauth: OAuthTokens::UnavailableAfterSignIn(usize::MAX),
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;
    let scratch = Scratch::new()?;

    let outcome = run_in(&mock, &scratch, Settings::new().within(CANCEL_BUDGET)).await?;

    assert!(
        matches!(outcome, Err(LifecycleError::Acquire(_))),
        "{outcome:?}"
    );
    assert_eq!(
        mock.requests_to("POST", "/oauth/token")?.len(),
        1 + 5 * 6,
        "the sign-in, then five acquire attempts that try the token endpoint six times each"
    );
    assert!(mock.requests_to("POST", "/run/acquirejob")?.is_empty());
    mock.server.only_request("DELETE", "/broker/session")?;
    Ok(())
}

const POLL_RETRY_DELAY: Duration = Duration::from_millis(400);

#[tokio::test]
async fn lifecycle_keeps_waiting_for_a_job_when_a_forced_token_refresh_loses_the_connection()
-> Result<()> {
    let behavior = Behavior {
        online: Online::ForceTokenRefreshThenJob,
        oauth: OAuthTokens::HangsUpAfterSignIn,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;

    let outcome = run_with(
        &mock,
        Settings::new()
            .retrying_polls_every(POLL_RETRY_DELAY)
            .within(CANCEL_BUDGET),
    )
    .await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    assert_eq!(
        mock.requests_to("POST", "/oauth/token")?.len(),
        4,
        "the sign-in, the refresh that lost the connection, the exchange before the retried poll and the exchange for the acquire"
    );
    let polls: Vec<_> = mock
        .requests_to("GET", "/broker/message")?
        .into_iter()
        .filter(|poll| poll.query("status") == Some("Online"))
        .collect();
    assert_eq!(polls.len(), 2);
    assert!(
        polls[1].at.duration_since(polls[0].at) >= POLL_RETRY_DELAY,
        "the poll waited for the retry delay after the failed refresh"
    );
    assert_eq!(mock.complete_request()?["conclusion"], "succeeded");
    Ok(())
}

#[tokio::test]
async fn lifecycle_deletes_the_session_with_a_fresh_token_when_the_last_one_expired() -> Result<()>
{
    let behavior = Behavior {
        oauth: OAuthTokens::StaleFirst,
        online: Online::Idle,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;
    let settings = Settings::new().within(CANCEL_BUDGET);
    let scratch = Scratch::new()?;

    let outcome = run_until(&mock, &scratch, settings, "the first poll", || {
        Ok(mock.polls("Online")? >= 1)
    })
    .await??;

    assert_eq!(outcome, LifecycleOutcome::NoJob);
    let deletes = mock.requests_to("DELETE", "/broker/session")?;
    let tokens: Vec<_> = deletes
        .iter()
        .map(|request| request.header("authorization"))
        .collect();
    assert_eq!(
        tokens,
        [
            Some(format!("Bearer {STALE_OAUTH_TOKEN}").as_str()),
            Some(format!("Bearer {OAUTH_TOKEN}").as_str())
        ]
    );
    Ok(())
}

#[tokio::test]
async fn lifecycle_stops_waiting_for_the_session_delete_after_its_timeout() -> Result<()> {
    let behavior = Behavior {
        oauth: OAuthTokens::StaleThenFails(503),
        online: Online::Idle,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;
    let backoff = TokenBackoff {
        min: Duration::from_secs(2),
        max: Duration::from_secs(2),
        coefficient: Duration::ZERO,
        spread: Duration::ZERO,
    };
    let settings = Settings::new()
        .backing_off_tokens(backoff)
        .closing_the_session_within(Duration::from_millis(300))
        .within(CANCEL_BUDGET);
    let scratch = Scratch::new()?;
    let started = std::time::Instant::now();

    let outcome = run_until(&mock, &scratch, settings, "the first poll", || {
        Ok(mock.polls("Online")? >= 1)
    })
    .await??;

    assert_eq!(outcome, LifecycleOutcome::NoJob);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the five retries of the token client, 10 s, were not waited for: {:?}",
        started.elapsed()
    );
    mock.server.only_request("DELETE", "/broker/session")?;
    Ok(())
}
