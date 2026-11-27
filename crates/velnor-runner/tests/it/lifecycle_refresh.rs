// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use anyhow::Result;
use velnor_runner::exec::JobResult;
use velnor_runner::lifecycle::{LifecycleError, LifecycleOutcome};

use super::exec_support::Scratch;
use super::lifecycle_support::{CANCEL_BUDGET, Settings, fixture_job, run_in, run_with};
use super::support::mock_github::{Behavior, Cancel, MockGithub, OAuthTokens, Online};

const SIGN_IN_AND_ACQUIRE: usize = 2;
const DELETED: OAuthTokens =
    OAuthTokens::RejectedAfterSignIn("invalid_client", "The client was deleted");
const REFUSED_AFTER_ACQUIRE: OAuthTokens = OAuthTokens::Fails {
    status: 404,
    after: SIGN_IN_AND_ACQUIRE,
    times: usize::MAX,
    retry_after: None,
};
const REFUSED_AFTER_SIGN_IN: OAuthTokens = OAuthTokens::Fails {
    status: 404,
    after: 1,
    times: usize::MAX,
    retry_after: None,
};

async fn idle_run_ends_with_a_poll_error(oauth: OAuthTokens) -> Result<()> {
    let behavior = Behavior {
        online: Online::ForceTokenRefreshThenJob,
        oauth,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;
    let scratch = Scratch::new()?;

    let outcome = run_in(
        &mock,
        &scratch,
        Settings::new().within(Duration::from_secs(10)),
    )
    .await?;

    assert!(
        matches!(outcome, Err(LifecycleError::Poll(_))),
        "{outcome:?}"
    );
    assert_eq!(mock.requests_to("POST", "/oauth/token")?.len(), 2);
    assert_eq!(mock.polls("Online")?, 1);
    assert!(mock.requests_to("POST", "/run/acquirejob")?.is_empty());
    Ok(())
}

#[tokio::test]
async fn lifecycle_ends_the_run_when_a_forced_token_refresh_is_refused() -> Result<()> {
    idle_run_ends_with_a_poll_error(REFUSED_AFTER_SIGN_IN).await
}

#[tokio::test]
async fn lifecycle_ends_the_run_when_the_registration_is_deleted_during_a_forced_token_refresh()
-> Result<()> {
    idle_run_ends_with_a_poll_error(DELETED).await
}

async fn busy_run(cancel: Cancel, oauth: OAuthTokens) -> Result<MockGithub> {
    let behavior = Behavior {
        cancel,
        oauth,
        ..Behavior::default()
    };
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

#[tokio::test]
async fn lifecycle_cancels_the_job_when_the_refresh_after_a_busy_poll_401_is_refused() -> Result<()>
{
    let mock = busy_run(Cancel::BusyPollFails(401), REFUSED_AFTER_ACQUIRE).await?;

    assert_eq!(mock.polls("Busy")?, 1);
    assert_eq!(
        mock.requests_to("POST", "/oauth/token")?.len(),
        SIGN_IN_AND_ACQUIRE + 1
    );
    Ok(())
}

#[tokio::test]
async fn lifecycle_refreshes_the_token_when_the_busy_poll_receives_a_force_token_refresh()
-> Result<()> {
    let cancel = Cancel::AfterAForcedTokenRefresh(Duration::from_millis(300));
    let mock = busy_run(cancel, OAuthTokens::Fresh).await?;

    assert!(mock.polls("Busy")? >= 2);
    assert_eq!(
        mock.requests_to("POST", "/oauth/token")?.len(),
        SIGN_IN_AND_ACQUIRE + 1
    );
    Ok(())
}

#[tokio::test]
async fn lifecycle_cancels_the_job_when_the_forced_refresh_of_the_busy_poll_is_refused()
-> Result<()> {
    let cancel = Cancel::AfterAForcedTokenRefresh(Duration::from_millis(300));
    let mock = busy_run(cancel, REFUSED_AFTER_ACQUIRE).await?;

    assert_eq!(mock.polls("Busy")?, 1);
    assert_eq!(
        mock.requests_to("POST", "/oauth/token")?.len(),
        SIGN_IN_AND_ACQUIRE + 1
    );
    Ok(())
}
