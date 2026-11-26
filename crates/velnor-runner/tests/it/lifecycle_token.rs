// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use anyhow::Result;
use velnor_runner::exec::JobResult;
use velnor_runner::lifecycle::{LifecycleError, LifecycleOutcome};

use super::exec_support::Scratch;
use super::lifecycle_support::{CANCEL_BUDGET, Settings, fixture_job, run_in, run_with};
use super::support::mock_github::{Behavior, MockGithub, OAuthTokens, Online};

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

    let outcome = run_in(
        &mock,
        &scratch,
        Settings::new().within(Duration::from_secs(5)),
    )
    .await?;

    assert!(
        matches!(outcome, Err(LifecycleError::Acquire(_))),
        "{outcome:?}"
    );
    assert_eq!(
        mock.requests_to("POST", "/oauth/token")?.len(),
        6,
        "the sign-in and five attempts"
    );
    assert!(mock.requests_to("POST", "/run/acquirejob")?.is_empty());
    mock.server.only_request("DELETE", "/broker/session")?;
    Ok(())
}

#[tokio::test]
async fn lifecycle_keeps_waiting_for_a_job_when_a_forced_token_refresh_fails_once() -> Result<()> {
    let behavior = Behavior {
        online: Online::ForceTokenRefreshThenJob,
        oauth: OAuthTokens::UnavailableAfterSignIn(1),
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;

    let outcome = run_with(&mock, Settings::new().within(CANCEL_BUDGET)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    assert_eq!(mock.polls("Online")?, 2);
    assert_eq!(mock.complete_request()?["conclusion"], "succeeded");
    Ok(())
}
