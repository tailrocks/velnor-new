// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use anyhow::Result;
use velnor_runner::exec::JobResult;
use velnor_runner::lifecycle::{LifecycleError, LifecycleOutcome};
use velnor_runner::protocol::TokenBackoff;

use super::exec_support::Scratch;
use super::lifecycle_support::{Settings, fixture_job, run_in, run_with};
use super::support::mock_github::{Behavior, MockGithub, OAuthTokens, SessionCreate};

const EXCHANGES_WITHOUT_FAILURE: usize = 2;
const ATTEMPTS: usize = 6;
const SLACK: Duration = Duration::from_millis(400);
const BUDGET: Duration = Duration::from_secs(3);
const GAP_BUDGET: Duration = Duration::from_secs(10);
const PAST: &str = "Wed, 21 Oct 2015 07:28:00 GMT";
const FUTURE: &str = "Wed, 21 Oct 2099 07:28:00 GMT";

fn unavailable(status: u16, times: usize, retry_after: Option<&'static str>) -> OAuthTokens {
    OAuthTokens::Fails {
        status,
        after: 0,
        times,
        retry_after,
    }
}

fn backoff(min_ms: u64, max_ms: u64, coefficient_ms: u64) -> TokenBackoff {
    TokenBackoff {
        min: Duration::from_millis(min_ms),
        max: Duration::from_millis(max_ms),
        coefficient: Duration::from_millis(coefficient_ms),
        spread: Duration::ZERO,
    }
}

async fn token_request_gaps(
    failures: usize,
    retry_after: Option<&'static str>,
    backoff: TokenBackoff,
) -> Result<Vec<Duration>> {
    let behavior = Behavior {
        oauth: unavailable(429, failures, retry_after),
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;

    let outcome = run_with(
        &mock,
        Settings::new()
            .backing_off_tokens(backoff)
            .within(GAP_BUDGET),
    )
    .await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    let requests = mock.requests_to("POST", "/oauth/token")?;
    Ok(requests
        .windows(2)
        .take(failures)
        .map(|pair| pair[1].at.duration_since(pair[0].at))
        .collect())
}

fn assert_waited(gap: Duration, expected_ms: u64) {
    let expected = Duration::from_millis(expected_ms);
    assert!(
        (expected..=expected + SLACK).contains(&gap),
        "waited {gap:?}, expected {expected:?} and at most {SLACK:?} more"
    );
}

fn assert_waited_between(gap: Duration, least_ms: u64, most_ms: u64) {
    let (least, most) = (
        Duration::from_millis(least_ms),
        Duration::from_millis(most_ms),
    );
    assert!(
        (least..=most + SLACK).contains(&gap),
        "waited {gap:?}, expected {least:?} to {most:?} and at most {SLACK:?} more"
    );
}

#[tokio::test]
async fn lifecycle_retries_every_status_the_token_client_of_the_runner_retries() -> Result<()> {
    for status in [429, 500, 502, 503, 504] {
        let behavior = Behavior {
            oauth: unavailable(status, 1, None),
            ..Behavior::default()
        };
        let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;

        let outcome = run_with(&mock, Settings::new().within(BUDGET)).await?;

        assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
        assert_eq!(
            mock.requests_to("POST", "/oauth/token")?.len(),
            EXCHANGES_WITHOUT_FAILURE + 1,
            "status {status}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn lifecycle_gives_up_at_once_on_a_token_status_the_client_does_not_retry() -> Result<()> {
    for status in [401, 403, 404, 501] {
        let behavior = Behavior {
            oauth: unavailable(status, usize::MAX, None),
            ..Behavior::default()
        };
        let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;
        let scratch = Scratch::new()?;

        let outcome = run_in(&mock, &scratch, Settings::new().within(BUDGET)).await?;

        assert!(
            matches!(outcome, Err(LifecycleError::SignIn(_))),
            "status {status}: {outcome:?}"
        );
        assert_eq!(
            mock.requests_to("POST", "/oauth/token")?.len(),
            1,
            "status {status}"
        );
        assert!(mock.requests_to("POST", "/broker/session")?.is_empty());
    }
    Ok(())
}

#[tokio::test]
async fn lifecycle_gives_up_on_a_sign_in_that_still_fails_after_five_retries() -> Result<()> {
    let behavior = Behavior {
        oauth: unavailable(503, usize::MAX, None),
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;
    let scratch = Scratch::new()?;

    let outcome = run_in(&mock, &scratch, Settings::new().within(BUDGET)).await?;

    assert!(
        matches!(outcome, Err(LifecycleError::SignIn(_))),
        "{outcome:?}"
    );
    assert_eq!(mock.requests_to("POST", "/oauth/token")?.len(), ATTEMPTS);
    assert!(mock.requests_to("POST", "/broker/session")?.is_empty());
    Ok(())
}

#[tokio::test]
async fn lifecycle_gives_up_on_a_sign_in_that_fails_for_good_while_the_session_is_created()
-> Result<()> {
    let behavior = Behavior {
        session: SessionCreate::Answers(&[401]),
        oauth: OAuthTokens::Fails {
            status: 503,
            after: 1,
            times: usize::MAX,
            retry_after: None,
        },
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;
    let scratch = Scratch::new()?;

    let outcome = run_in(&mock, &scratch, Settings::new().within(BUDGET)).await?;

    assert!(
        matches!(outcome, Err(LifecycleError::SignIn(_))),
        "{outcome:?}"
    );
    assert_eq!(
        mock.requests_to("POST", "/oauth/token")?.len(),
        1 + ATTEMPTS
    );
    assert_eq!(mock.requests_to("POST", "/broker/session")?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn lifecycle_waits_longer_after_every_failed_token_request() -> Result<()> {
    let gaps = token_request_gaps(2, None, backoff(300, 10_000, 400)).await?;

    assert_waited_between(gaps[0], 300 + 320, 300 + 480);
    assert_waited_between(gaps[1], 300 + 960, 300 + 1_440);
    Ok(())
}

#[tokio::test]
async fn lifecycle_never_waits_longer_than_the_maximum_backoff_for_a_token() -> Result<()> {
    let gaps = token_request_gaps(2, None, backoff(0, 100, 10_000)).await?;

    assert_waited(gaps[0], 100);
    assert_waited(gaps[1], 100);
    Ok(())
}

#[tokio::test]
async fn lifecycle_waits_for_the_retry_after_of_the_token_endpoint() -> Result<()> {
    let gaps = token_request_gaps(1, Some("0"), backoff(0, 5_000, 10_000)).await?;

    assert_waited(gaps[0], 0);
    Ok(())
}

#[tokio::test]
async fn lifecycle_waits_at_least_the_minimum_backoff_when_retry_after_is_shorter() -> Result<()> {
    let gaps = token_request_gaps(1, Some("0"), backoff(300, 5_000, 10_000)).await?;

    assert_waited(gaps[0], 300);
    Ok(())
}

#[tokio::test]
async fn lifecycle_waits_at_most_the_maximum_backoff_when_retry_after_is_longer() -> Result<()> {
    let gaps = token_request_gaps(1, Some("3600"), backoff(0, 300, 0)).await?;

    assert_waited(gaps[0], 300);
    Ok(())
}

#[tokio::test]
async fn lifecycle_reads_a_retry_after_given_as_a_date() -> Result<()> {
    let gaps = token_request_gaps(1, Some(FUTURE), backoff(0, 300, 0)).await?;

    assert_waited(gaps[0], 300);
    Ok(())
}

#[tokio::test]
async fn lifecycle_ignores_a_retry_after_date_that_has_passed() -> Result<()> {
    let gaps = token_request_gaps(1, Some(PAST), backoff(0, 300, 10_000)).await?;

    assert_waited(gaps[0], 300);
    Ok(())
}
