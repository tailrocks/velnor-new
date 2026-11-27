// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use velnor_runner::exec::{JobCancel, JobResult};
use velnor_runner::lifecycle::LifecycleOutcome;

use super::exec_builder::{assert_gone, pid_from, wait_for_file};
use super::exec_support::Scratch;
use super::lifecycle_support::{
    CANCEL_BUDGET, RUN_BUDGET, Settings, fixture_job, job_replacing, replace_literal, run_in,
    run_until, run_with,
};
use super::support::mock_github::{Behavior, Cancel, MessageTypes, MockGithub};

const CLEANUP_WINDOW: Duration = Duration::from_secs(10);
const CLEANUP_FLOOR: Duration = Duration::from_secs(25);

pub(crate) fn cancelled_with_cleanup(complete: &serde_json::Value) -> Result<()> {
    assert_eq!(complete["conclusion"], "canceled");
    let steps = complete["stepResults"].as_array().context("step results")?;
    let conclusions: Vec<_> = steps.iter().map(|step| &step["conclusion"]).collect();
    assert_eq!(conclusions, ["canceled", "succeeded"]);
    assert_eq!(steps[1]["name"], "Cleanup");
    Ok(())
}

#[tokio::test]
async fn lifecycle_cancel_message_cancels_running_step() -> Result<()> {
    let behavior = Behavior {
        cancel: Cancel::After(Duration::from_millis(300)),
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("cancel", None)?, behavior).await?;

    let outcome = run_with(
        &mock,
        Settings::new()
            .renewing_every(Duration::from_secs(60))
            .within(CANCEL_BUDGET),
    )
    .await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Cancelled));
    cancelled_with_cleanup(&mock.complete_request()?)
}

#[tokio::test]
async fn lifecycle_hosted_runner_shutdown_cancels_the_running_job() -> Result<()> {
    let behavior = Behavior {
        cancel: Cancel::HostedShutdownAfter(Duration::from_millis(300)),
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("cancel", None)?, behavior).await?;

    let outcome = run_with(
        &mock,
        Settings::new()
            .renewing_every(Duration::from_secs(60))
            .within(CANCEL_BUDGET),
    )
    .await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Cancelled));
    cancelled_with_cleanup(&mock.complete_request()?)
}

#[tokio::test]
async fn lifecycle_cancel_message_for_another_job_is_ignored() -> Result<()> {
    let behavior = Behavior {
        cancel: Cancel::AfterACancelOfAnotherJob(Duration::from_millis(700)),
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("cancel", None)?, behavior).await?;

    let outcome = run_with(
        &mock,
        Settings::new()
            .renewing_every(Duration::from_secs(60))
            .within(CANCEL_BUDGET),
    )
    .await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Cancelled));
    assert!(
        mock.polls("Busy")? >= 2,
        "the job kept running after the cancellation of another job"
    );
    cancelled_with_cleanup(&mock.complete_request()?)
}

#[tokio::test]
async fn lifecycle_message_types_are_matched_case_insensitively() -> Result<()> {
    let behavior = Behavior {
        message_types: MessageTypes::Lowercase,
        cancel: Cancel::After(Duration::from_millis(300)),
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("cancel", None)?, behavior).await?;

    let outcome = run_with(
        &mock,
        Settings::new()
            .renewing_every(Duration::from_secs(60))
            .within(CANCEL_BUDGET),
    )
    .await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Cancelled));
    cancelled_with_cleanup(&mock.complete_request()?)
}

#[tokio::test]
async fn lifecycle_cancel_with_a_timeout_that_does_not_parse_still_cancels_with_the_default()
-> Result<()> {
    let behavior = Behavior {
        cancel: Cancel::After(Duration::from_millis(300)),
        cancel_timeout_text: Some("soon"),
        ..Behavior::default()
    };
    let mock = MockGithub::start(fixture_job("cancel", None)?, behavior).await?;

    let outcome = run_with(
        &mock,
        Settings::new()
            .renewing_every(Duration::from_secs(60))
            .within(CANCEL_BUDGET),
    )
    .await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Cancelled));
    cancelled_with_cleanup(&mock.complete_request()?)
}

#[tokio::test]
async fn lifecycle_cancel_message_timeout_below_the_floor_still_lets_the_cleanup_finish()
-> Result<()> {
    let behavior = Behavior {
        cancel: Cancel::After(Duration::from_millis(300)),
        cancel_timeout: Duration::from_secs(1),
        ..Behavior::default()
    };
    let mock = MockGithub::start(
        job_replacing("cancel", Some(("echo cleanup", "sleep 1; echo cleanup")))?,
        behavior,
    )
    .await?;

    let outcome = run_with(
        &mock,
        Settings::new()
            .killing_after_cancel_floor(Duration::from_secs(18))
            .renewing_every(Duration::from_secs(60))
            .within(CANCEL_BUDGET),
    )
    .await?;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Cancelled));
    let complete = mock.complete_request()?;
    cancelled_with_cleanup(&complete)?;
    assert!(
        complete["annotations"]
            .as_array()
            .is_none_or(|notes| notes.iter().all(|note| !note["message"]
                .as_str()
                .is_some_and(|text| text.contains("cancellation timeout")))),
        "{complete}"
    );
    Ok(())
}

fn step_started_file(scratch: &Scratch) -> std::path::PathBuf {
    scratch.work_dir().with_file_name("step.started")
}

fn hanging_cleanup(scratch: &Scratch) -> Result<serde_json::Value> {
    let pid_file = scratch.work_dir().with_file_name("cleanup.pid");
    let script = format!(
        "sleep 4343 >/dev/null 2>&1 &\necho $! > {}\nwait",
        pid_file.display()
    );
    let mut job = job_replacing("cancel", Some(("echo cleanup", &script)))?;
    let running = format!("touch {}; sleep 60", step_started_file(scratch).display());
    anyhow::ensure!(
        replace_literal(&mut job, "sleep 60", &running),
        "the fixture has a running step"
    );
    Ok(job)
}

fn assert_stopped_at_the_deadline(
    mock: &MockGithub,
    scratch: &Scratch,
    outcome: LifecycleOutcome,
) -> Result<()> {
    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Cancelled));
    let complete = mock.complete_request()?;
    assert_eq!(complete["conclusion"], "canceled");
    let notes = complete["annotations"].as_array().context("annotations")?;
    assert!(
        notes.iter().any(|note| note["message"]
            .as_str()
            .is_some_and(|text| text.contains("cancellation timeout"))),
        "{notes:?}"
    );
    let pid_file = scratch.work_dir().with_file_name("cleanup.pid");
    wait_for_file(&pid_file)?;
    assert_gone(pid_from(&pid_file)?)
}

#[tokio::test]
async fn lifecycle_stops_a_cleanup_step_that_outlives_the_cancel_timeout_of_the_message()
-> Result<()> {
    let behavior = Behavior {
        cancel: Cancel::After(Duration::from_millis(300)),
        cancel_timeout: CLEANUP_FLOOR,
        ..Behavior::default()
    };
    let scratch = Scratch::new()?;
    let mock = MockGithub::start(hanging_cleanup(&scratch)?, behavior).await?;
    let started = Instant::now();

    let outcome = run_in(
        &mock,
        &scratch,
        Settings::new()
            .killing_after_cancel_floor(Duration::from_secs(16))
            .renewing_every(Duration::from_secs(60))
            .within(RUN_BUDGET),
    )
    .await??;

    assert!(
        started.elapsed() >= CLEANUP_WINDOW,
        "the 25 s of the message, less the 15 s reserve, were waited: {:?}",
        started.elapsed()
    );
    assert_stopped_at_the_deadline(&mock, &scratch, outcome)
}

#[tokio::test]
async fn lifecycle_stops_a_cleanup_step_that_outlives_the_floor_after_a_shutdown() -> Result<()> {
    let scratch = Scratch::new()?;
    let mock = MockGithub::start(hanging_cleanup(&scratch)?, Behavior::default()).await?;
    let settings = Settings::new()
        .killing_after_cancel_floor(CLEANUP_FLOOR)
        .renewing_every(Duration::from_secs(60))
        .within(RUN_BUDGET);

    let started = step_started_file(&scratch);

    let outcome = run_until(&mock, &scratch, settings, "the step to start", || {
        Ok(started.exists())
    })
    .await??;

    assert_stopped_at_the_deadline(&mock, &scratch, outcome)
}

#[tokio::test]
async fn lifecycle_warns_about_dropped_logs_when_a_job_is_stopped_at_the_cancel_deadline()
-> Result<()> {
    let scratch = Scratch::new()?;
    let behavior = Behavior {
        blob_delay: Duration::from_secs(30),
        ..Behavior::default()
    };
    let mock = MockGithub::start(hanging_cleanup(&scratch)?, behavior).await?;
    let settings = Settings::new()
        .killing_after_cancel_floor(CLEANUP_FLOOR)
        .renewing_every(Duration::from_secs(60))
        .within(RUN_BUDGET);

    let started = step_started_file(&scratch);

    let outcome = run_until(&mock, &scratch, settings, "the step to start", || {
        Ok(started.exists())
    })
    .await??;

    assert_stopped_at_the_deadline(&mock, &scratch, outcome)?;
    let complete = mock.complete_request()?;
    let notes = complete["annotations"].as_array().context("annotations")?;
    assert!(
        notes.iter().any(|note| note["message"]
            .as_str()
            .is_some_and(|text| text.contains("not uploaded in time"))),
        "{notes:?}"
    );
    Ok(())
}

fn leave_a_large_workspace(scratch: &Scratch) -> Result<std::path::PathBuf> {
    let workspace = scratch.work_dir().join("workspace");
    for directory in 0..200 {
        let path = workspace.join(format!("directory-{directory}"));
        std::fs::create_dir_all(&path)?;
        for file in 0..100 {
            std::fs::write(path.join(format!("file-{file}")), b"x")?;
        }
    }
    Ok(workspace)
}

#[tokio::test]
async fn lifecycle_stops_clearing_a_leftover_workspace_when_shutdown_arrives() -> Result<()> {
    let scratch = Scratch::new()?;
    let workspace = leave_a_large_workspace(&scratch)?;
    let mock = MockGithub::start(fixture_job("hello", None)?, Behavior::default()).await?;
    let shutdown = JobCancel::new();
    let signal = shutdown.clone();
    let watched = workspace.clone();
    tokio::spawn(async move {
        while std::fs::read_dir(&watched).is_ok_and(|entries| entries.count() == 200) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        signal.cancel();
    });
    let mut settings = Settings::new().within(CANCEL_BUDGET);
    settings.shutdown = shutdown;

    let outcome = run_in(&mock, &scratch, settings).await??;

    assert_eq!(outcome, LifecycleOutcome::NoJob);
    assert!(
        std::fs::read_dir(&workspace).is_ok_and(|entries| entries.count() > 0),
        "the run returned before the leftover workspace was gone"
    );
    assert!(mock.requests_to("POST", "/run/acquirejob")?.is_empty());
    mock.server.only_request("DELETE", "/broker/session")?;
    Ok(())
}

const DIRECTORIES: usize = 100;

fn growing_a_large_workspace() -> Result<serde_json::Value> {
    let script = format!(
        "seq 1 {DIRECTORIES} | xargs -P 8 -I{{}} sh -c \
         'mkdir directory-{{}} && cd directory-{{}} && seq 1 50 | xargs touch'"
    );
    job_replacing("hello", Some(("echo hello", &script)))
}

#[tokio::test]
async fn lifecycle_stops_clearing_the_workspace_after_the_job_when_shutdown_arrives() -> Result<()>
{
    let scratch = Scratch::new()?;
    let workspace = scratch.work_dir().join("workspace");
    let mock = MockGithub::start(growing_a_large_workspace()?, Behavior::default()).await?;
    let shutdown = JobCancel::new();
    let signal = shutdown.clone();
    let watched = workspace.clone();
    tokio::spawn(async move {
        while std::fs::read_dir(&watched).map_or(true, |entries| entries.count() < DIRECTORIES) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        while std::fs::read_dir(&watched).is_ok_and(|entries| entries.count() == DIRECTORIES) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        signal.cancel();
    });
    let mut settings = Settings::new().within(RUN_BUDGET);
    settings.shutdown = shutdown;

    let outcome = run_in(&mock, &scratch, settings).await??;

    assert_eq!(outcome, LifecycleOutcome::Completed(JobResult::Succeeded));
    assert_eq!(mock.complete_request()?["conclusion"], "succeeded");
    assert!(
        std::fs::read_dir(&workspace).is_ok_and(|entries| entries.count() > 0),
        "the run returned before the workspace was gone"
    );
    mock.server.only_request("DELETE", "/broker/session")?;
    Ok(())
}
