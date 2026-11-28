// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use anyhow::Result;
use velnor_runner::exec::JobResult;
use velnor_runner::lifecycle::{LifecycleError, LifecycleOutcome};
use velnor_runner::protocol::RunnerOs;

use super::exec_support::Scratch;
use super::lifecycle_support::{
    CANCEL_BUDGET, Settings, fixture_job, job_replacing, run, run_in, run_until, run_with,
};
use super::support::RecordedRequest;
use super::support::mock_github::{
    Acknowledge, Behavior, MockGithub, OAUTH_TOKEN, OAuthTokens, Online, SESSION_ID,
    STALE_OAUTH_TOKEN,
};

#[tokio::test]
async fn lifecycle_polls_again_with_the_refreshed_token_after_a_401_poll() -> Result<()> {
    let behavior = Behavior {
        online: Online::JobForFreshTokenOnly,
        oauth: OAuthTokens::StaleFirst,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;

    let outcome = run(&mock, Duration::from_millis(200)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    let online: Vec<RecordedRequest> = mock
        .requests_to("GET", "/broker/message")?
        .into_iter()
        .filter(|poll| poll.query("status") == Some("Online"))
        .collect();
    let tokens: Vec<Option<&str>> = online
        .iter()
        .map(|poll| poll.header("authorization"))
        .collect();
    assert_eq!(
        tokens,
        [
            Some(format!("Bearer {STALE_OAUTH_TOKEN}").as_str()),
            Some(format!("Bearer {OAUTH_TOKEN}").as_str()),
        ]
    );
    assert_eq!(mock.complete_request()?["conclusion"], "succeeded");
    Ok(())
}

#[tokio::test]
async fn lifecycle_acknowledges_the_job_request_once_when_asked() -> Result<()> {
    let mock = MockGithub::start(fixture_job("hello", None)?, Behavior::default()).await?;

    run(&mock, Duration::from_millis(200)).await?;

    let acknowledge = mock.server.only_request("POST", "/broker/acknowledge")?;
    assert_eq!(acknowledge.json()?["runnerRequestId"], "request-1");
    assert_eq!(acknowledge.query("sessionId"), Some(SESSION_ID));
    Ok(())
}

#[tokio::test]
async fn lifecycle_skips_the_acknowledgement_when_not_asked() -> Result<()> {
    let behavior = Behavior {
        should_acknowledge: false,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;

    let outcome = run(&mock, Duration::from_millis(200)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    assert!(mock.requests_to("POST", "/broker/acknowledge")?.is_empty());
    Ok(())
}

#[tokio::test]
async fn lifecycle_deletes_the_session_when_acquire_fails() -> Result<()> {
    let behavior = Behavior {
        unusable_job: true,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;
    let scratch = Scratch::new()?;

    let outcome = run_in(&mock, &scratch, Settings::new()).await?;

    assert!(
        matches!(outcome, Err(LifecycleError::Acquire(_))),
        "{outcome:?}"
    );
    let delete = mock.server.only_request("DELETE", "/broker/session")?;
    assert_eq!(delete.query("sessionId"), Some(SESSION_ID));
    assert!(mock.requests_to("POST", "/run/completejob")?.is_empty());
    Ok(())
}

async fn leaves_the_job_unacquired_when_the_work_dir_entry_is_in_the_way(
    entry: &str,
) -> Result<()> {
    let mock = MockGithub::start(fixture_job("hello", None)?, Behavior::default()).await?;
    let scratch = Scratch::new()?;
    let work_dir = scratch.work_dir();
    std::fs::create_dir_all(&work_dir)?;
    std::fs::write(work_dir.join(entry), "in the way")?;

    let outcome = run_in(&mock, &scratch, Settings::new()).await?;

    assert!(
        matches!(outcome, Err(LifecycleError::WorkDir { .. })),
        "{outcome:?}"
    );
    assert!(
        mock.requests_to("POST", "/run/acquirejob")?.is_empty(),
        "the job stays unacquired so GitHub can hand it out again"
    );
    mock.server.only_request("DELETE", "/broker/session")?;
    Ok(())
}

#[tokio::test]
async fn lifecycle_does_not_acquire_a_job_it_cannot_prepare_a_work_dir_for() -> Result<()> {
    leaves_the_job_unacquired_when_the_work_dir_entry_is_in_the_way("workspace").await
}

#[tokio::test]
async fn lifecycle_does_not_acquire_a_job_it_cannot_prepare_a_log_dir_for() -> Result<()> {
    leaves_the_job_unacquired_when_the_work_dir_entry_is_in_the_way("logs").await
}

#[tokio::test]
async fn lifecycle_ends_without_a_job_when_the_acknowledgement_says_the_job_is_gone() -> Result<()>
{
    let behavior = Behavior {
        acknowledge: Acknowledge::JobGone,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;

    let outcome = run(&mock, Duration::from_millis(200)).await?;

    assert_eq!(outcome, LifecycleOutcome::NoJob);
    assert!(mock.requests_to("POST", "/run/acquirejob")?.is_empty());
    assert!(mock.requests_to("POST", "/run/completejob")?.is_empty());
    mock.server.only_request("DELETE", "/broker/session")?;
    Ok(())
}

#[tokio::test]
async fn lifecycle_runs_the_job_when_the_acknowledgement_fails_for_another_reason() -> Result<()> {
    let behavior = Behavior {
        acknowledge: Acknowledge::Unavailable,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;

    let outcome = run(&mock, Duration::from_millis(200)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    assert_eq!(mock.complete_request()?["conclusion"], "succeeded");
    Ok(())
}

#[tokio::test]
async fn lifecycle_runs_the_job_when_the_acknowledgement_says_the_runner_is_not_found() -> Result<()>
{
    let behavior = Behavior {
        acknowledge: Acknowledge::RunnerGone,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;

    let outcome = run(&mock, Duration::from_millis(200)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    mock.server.only_request("POST", "/run/acquirejob")?;
    assert_eq!(mock.complete_request()?["conclusion"], "succeeded");
    Ok(())
}

#[tokio::test]
async fn lifecycle_signs_in_again_when_the_acknowledgement_finds_the_token_expired() -> Result<()> {
    let behavior = Behavior {
        oauth: OAuthTokens::StaleFirst,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;

    let outcome = run_with(&mock, Settings::new().within(CANCEL_BUDGET)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    assert_eq!(mock.requests_to("POST", "/broker/acknowledge")?.len(), 2);
    let acquire = mock.server.only_request("POST", "/run/acquirejob")?;
    assert_eq!(
        acquire.header("authorization"),
        Some(format!("Bearer {OAUTH_TOKEN}").as_str())
    );
    Ok(())
}

#[tokio::test]
async fn lifecycle_stops_acquiring_when_shutdown_arrives() -> Result<()> {
    let behavior = Behavior {
        acquire_delay: Duration::from_secs(30),
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;
    let settings = Settings::new().within(CANCEL_BUDGET);

    let scratch = Scratch::new()?;
    let outcome = run_until(&mock, &scratch, settings, "the acquire request", || {
        Ok(!mock.requests_to("POST", "/run/acquirejob")?.is_empty())
    })
    .await??;

    assert_eq!(outcome, LifecycleOutcome::NoJob);
    assert!(mock.requests_to("POST", "/run/completejob")?.is_empty());
    mock.server.only_request("DELETE", "/broker/session")?;
    Ok(())
}

#[tokio::test]
async fn lifecycle_acquires_with_a_fresh_token_when_the_job_request_needs_no_acknowledgement()
-> Result<()> {
    let behavior = Behavior {
        oauth: OAuthTokens::StaleFirst,
        should_acknowledge: false,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;

    let outcome = run_with(&mock, Settings::new().within(CANCEL_BUDGET)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    let acquire = mock.server.only_request("POST", "/run/acquirejob")?;
    assert_eq!(
        acquire.header("authorization"),
        Some(format!("Bearer {OAUTH_TOKEN}").as_str())
    );
    Ok(())
}

#[tokio::test]
async fn lifecycle_acquires_and_runs_the_job_as_the_injected_os() -> Result<()> {
    let job = job_replacing(
        "hello",
        Some(("echo hello", "echo \"::notice::$RUNNER_OS\"")),
    )?;
    let mock = MockGithub::start(job, Behavior::default()).await?;

    let outcome = run_with(&mock, Settings::new().on(RunnerOs::MacOs)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    let acquire = mock.server.only_request("POST", "/run/acquirejob")?;
    assert_eq!(acquire.json()?["runnerOS"], "macOS");
    for poll in mock.requests_to("GET", "/broker/message")? {
        assert_eq!(poll.query("os"), Some("macOS"));
    }
    for acknowledge in mock.requests_to("POST", "/broker/acknowledge")? {
        assert_eq!(acknowledge.query("os"), Some("macOS"));
    }
    let complete = mock.complete_request()?;
    assert_eq!(
        complete["stepResults"][0]["annotations"][0]["message"],
        "macOS"
    );
    Ok(())
}
