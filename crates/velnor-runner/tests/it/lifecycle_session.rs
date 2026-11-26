// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use anyhow::{Context, Result};
use velnor_runner::exec::{JobCancel, JobResult};
use velnor_runner::lifecycle::{LifecycleError, LifecycleOutcome};

use super::exec_support::Scratch;
use super::lifecycle_support::{CANCEL_BUDGET, Settings, fixture_job, run, run_in, run_with};
use super::support::mock_github::{
    Acknowledge, Behavior, MockGithub, OAUTH_TOKEN, OAuthTokens, Online, SESSION_ID, SessionCreate,
};

#[tokio::test]
async fn lifecycle_refreshes_the_token_after_a_401_poll_and_still_runs_the_job() -> Result<()> {
    let behavior = Behavior {
        online: Online::JobAfterOneUnauthorized,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;

    let outcome = run(&mock, Duration::from_millis(200)).await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    assert_eq!(
        mock.requests_to("POST", "/oauth/token")?.len(),
        2,
        "one sign-in and one refresh"
    );
    assert_eq!(mock.polls("Online")?, 2, "the 401 poll was retried");
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

#[tokio::test]
async fn lifecycle_does_not_acquire_a_job_it_cannot_prepare_a_work_dir_for() -> Result<()> {
    let mock = MockGithub::start(fixture_job("hello", None)?, Behavior::default()).await?;
    let scratch = Scratch::new()?;
    let work_dir = scratch.work_dir();
    std::fs::create_dir_all(&work_dir)?;
    std::fs::write(work_dir.join("workspace"), "in the way")?;

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
async fn lifecycle_completes_the_job_as_failed_when_it_cannot_start_it() -> Result<()> {
    let mock = MockGithub::start(fixture_job("hello", None)?, Behavior::default()).await?;
    let scratch = Scratch::new()?;
    let work_dir = scratch.work_dir();
    std::fs::create_dir_all(&work_dir)?;
    std::fs::write(work_dir.join("logs"), "in the way")?;

    let outcome = run_in(&mock, &scratch, Settings::new()).await?;

    assert!(
        matches!(outcome, Err(LifecycleError::WorkDir { .. })),
        "{outcome:?}"
    );
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
        annotations[0]["message"]
            .as_str()
            .is_some_and(|message| message.starts_with("The runner could not run the job:")),
        "{annotations:?}"
    );
    assert_eq!(mock.requests_to("DELETE", "/broker/session")?.len(), 1);
    Ok(())
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

fn shutdown_after(delay: Duration) -> JobCancel {
    let shutdown = JobCancel::new();
    let signal = shutdown.clone();
    tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        signal.cancel();
    });
    shutdown
}

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

#[tokio::test]
async fn lifecycle_stops_waiting_for_a_session_when_shutdown_arrives() -> Result<()> {
    let behavior = Behavior {
        session: SessionCreate::ConflictsAlways,
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("hello", None)?, behavior).await?;
    let mut settings = Settings::new().within(CANCEL_BUDGET);
    settings.shutdown = shutdown_after(Duration::from_millis(500));

    let outcome = run_with(&mock, settings).await?;

    assert_eq!(outcome, LifecycleOutcome::NoJob);
    assert!(mock.requests_to("DELETE", "/broker/session")?.is_empty());
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
    let mut settings = Settings::new().within(CANCEL_BUDGET);
    settings.shutdown = shutdown_after(Duration::from_millis(500));

    let outcome = run_with(&mock, settings).await?;

    assert_eq!(outcome, LifecycleOutcome::NoJob);
    assert!(mock.requests_to("POST", "/run/completejob")?.is_empty());
    mock.server.only_request("DELETE", "/broker/session")?;
    Ok(())
}
