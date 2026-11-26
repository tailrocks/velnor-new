// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use velnor_runner::exec::{
    AdapterRegistry, JobCancel, JobResult, JobRun, StdoutSink, StepResult, run_job,
};

use crate::exec_support::{Scratch, fixture_message};

async fn run_and_cancel_after_300ms(fixture: &str) -> Result<JobRun> {
    let scratch = Scratch::new()?;
    let message = fixture_message(fixture)?;
    let cancel = JobCancel::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        trigger.cancel();
    });
    let started = Instant::now();
    let run = tokio::time::timeout(
        Duration::from_secs(10),
        run_job(
            &message,
            &scratch.options(),
            &AdapterRegistry::empty(),
            &mut StdoutSink,
            cancel,
        ),
    )
    .await
    .context("a cancelled job stops its running step instead of waiting for it to end")?;
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "a cancelled job took {:?} to finish",
        started.elapsed()
    );
    Ok(run)
}

#[tokio::test]
async fn cancel_kills_step_and_runs_always_steps() -> Result<()> {
    let run = run_and_cancel_after_300ms("cancel").await?;
    assert_eq!(run.result, JobResult::Cancelled);
    assert_eq!(run.steps.len(), 2);
    assert_eq!(run.steps[0].outcome, StepResult::Cancelled);
    assert_eq!(run.steps[1].outcome, StepResult::Succeeded);
    Ok(())
}

#[tokio::test]
async fn cancel_during_the_last_step_cancels_the_job() -> Result<()> {
    let run = run_and_cancel_after_300ms("cancel-single").await?;
    assert_eq!(run.result, JobResult::Cancelled);
    assert_eq!(run.steps[0].outcome, StepResult::Cancelled);
    Ok(())
}
