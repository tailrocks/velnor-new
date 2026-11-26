// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use velnor_runner::exec::JobResult;
use velnor_runner::lifecycle::{LifecycleError, LifecycleOutcome};

use super::exec_support::Scratch;
use super::lifecycle_support::{Settings, fixture_job, run, run_in};
use super::support::mock_github::{Behavior, MockGithub, Online, SESSION_ID};

struct Restore(PathBuf);

impl Drop for Restore {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
    }
}

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
    std::fs::set_permissions(&work_dir, std::fs::Permissions::from_mode(0o555))?;
    let _restore = Restore(work_dir.clone());
    if std::fs::write(work_dir.join("probe"), "").is_ok() {
        return Ok(());
    }

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
