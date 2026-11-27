// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use futures_util::FutureExt;
use rustix::process::{Pid, Signal, kill_process};
use serde_json::{Value, json};
use velnor_runner::exec::{
    AdapterRegistry, ExecError, JobCancel, JobResult, JobRun, LogSink, StepAdapter, StepContext,
    StepFuture, StepResult, run_job,
};

use crate::exec_builder::{
    BACKGROUND_SLEEP, assert_gone, bounded, job, literal, parse, pid_from, run, step,
    wait_for_file, with, write_job,
};
use crate::exec_support::Scratch;

const EXPIRED: f64 = 0.000_000_01;
const TIMED_OUT: &str = "step 1 timed out after 0.00000001 minutes";

struct Probe {
    cancel: Option<JobCancel>,
    on_ready: Option<JobCancel>,
    lines: Vec<String>,
}

impl LogSink for Probe {
    fn step_started(&mut self, _number: u32, _name: &str) {
        if let Some(cancel) = &self.cancel {
            cancel.cancel();
        }
    }

    fn line(&mut self, _number: u32, text: &str) {
        if text == "ready"
            && let Some(cancel) = &self.on_ready
        {
            cancel.cancel();
        }
        self.lines.push(text.to_owned());
    }

    fn step_finished(&mut self, _number: u32, _result: StepResult) {}

    fn job_finished(&mut self, _run: &JobRun) {}
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cancels {
    Never,
    AtStart,
    InAdapter,
    OnReady,
}

struct Ends {
    cancel: Option<JobCancel>,
    fail: bool,
}

impl StepAdapter for Ends {
    fn handles(&self, reference: &str) -> bool {
        reference == "actions/ends@v1"
    }

    fn run<'a>(&'a self, _ctx: &'a mut StepContext<'_>) -> StepFuture<'a> {
        async move {
            if let Some(cancel) = &self.cancel {
                cancel.cancel();
            }
            if self.fail {
                Err(ExecError::Unsupported("boom".to_owned()))
            } else {
                Ok(StepResult::Succeeded)
            }
        }
        .boxed()
    }
}

fn uses(extra: &[(&str, Value)]) -> Value {
    let mut action = step("w", literal("unused"));
    action["Reference"] = json!({"Type": "repository", "Name": "actions/ends", "Ref": "v1", "RepositoryType": "GitHub"});
    extra.iter().fold(action, |action, (key, value)| {
        with(action, key, value.clone())
    })
}

async fn go(message: &Value, fail: bool, cancels: Cancels) -> Result<(JobRun, Vec<String>)> {
    let cancel = JobCancel::new();
    let mut registry = AdapterRegistry::empty();
    registry.register(Box::new(Ends {
        cancel: (cancels == Cancels::InAdapter).then(|| cancel.clone()),
        fail,
    }));
    let mut probe = Probe {
        cancel: (cancels == Cancels::AtStart).then(|| cancel.clone()),
        on_ready: (cancels == Cancels::OnReady).then(|| cancel.clone()),
        lines: Vec::new(),
    };
    let finished = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        run_job(
            &parse(message)?,
            &Scratch::new()?.options(),
            &registry,
            &mut probe,
            cancel.clone(),
        ),
    )
    .await
    .context("run_job returns")?;
    Ok((finished, probe.lines))
}

fn timeout_lines(lines: &[String]) -> usize {
    lines.iter().filter(|line| *line == TIMED_OUT).count()
}

#[tokio::test]
async fn a_cancel_beats_an_expired_deadline_for_a_script_step() -> Result<()> {
    let slow = with(run("a", "sleep 5"), "TimeoutInMinutes", json!(EXPIRED));
    let (finished, lines) = go(&job(vec![slow]), false, Cancels::AtStart).await?;
    assert_eq!(
        finished.steps[0].outcome,
        StepResult::Cancelled,
        "{lines:?}"
    );
    assert_eq!(timeout_lines(&lines), 0, "{lines:?}");
    Ok(())
}

#[tokio::test]
async fn a_cancel_beats_an_expired_deadline_for_an_adapter_step() -> Result<()> {
    let action = uses(&[("TimeoutInMinutes", json!(EXPIRED))]);
    let (finished, lines) = go(&job(vec![action]), false, Cancels::AtStart).await?;
    assert_eq!(
        finished.steps[0].outcome,
        StepResult::Cancelled,
        "{lines:?}"
    );
    assert_eq!(timeout_lines(&lines), 0, "{lines:?}");
    Ok(())
}

#[tokio::test]
async fn a_succeeding_adapter_that_ends_under_a_job_cancel_is_cancelled() -> Result<()> {
    let (finished, lines) = go(&job(vec![uses(&[])]), false, Cancels::InAdapter).await?;
    assert_eq!(
        finished.steps[0].outcome,
        StepResult::Cancelled,
        "{lines:?}"
    );
    assert_eq!(finished.result, JobResult::Cancelled);
    Ok(())
}

#[tokio::test]
async fn a_failing_adapter_under_a_job_cancel_is_cancelled() -> Result<()> {
    let (finished, lines) = go(&job(vec![uses(&[])]), true, Cancels::InAdapter).await?;
    assert_eq!(
        finished.steps[0].outcome,
        StepResult::Cancelled,
        "{lines:?}"
    );
    assert_eq!(finished.steps[0].conclusion, StepResult::Cancelled);
    assert_eq!(finished.result, JobResult::Cancelled);
    Ok(())
}

#[tokio::test]
async fn a_failing_adapter_under_a_job_cancel_is_cancelled_despite_continue_on_error() -> Result<()>
{
    let action = uses(&[("ContinueOnError", json!(true))]);
    let (finished, lines) = go(&job(vec![action]), true, Cancels::InAdapter).await?;
    assert_eq!(
        finished.steps[0].conclusion,
        StepResult::Cancelled,
        "{lines:?}"
    );
    assert_eq!(finished.result, JobResult::Cancelled);
    Ok(())
}

#[tokio::test]
async fn a_failing_adapter_after_the_deadline_prints_the_timeout_once() -> Result<()> {
    let action = uses(&[("TimeoutInMinutes", json!(EXPIRED))]);
    let (finished, lines) = go(&job(vec![action]), true, Cancels::Never).await?;
    assert_eq!(finished.steps[0].outcome, StepResult::Failed, "{lines:?}");
    assert_eq!(timeout_lines(&lines), 1, "{lines:?}");
    Ok(())
}

#[tokio::test]
async fn a_timed_out_script_prints_the_timeout_once() -> Result<()> {
    let slow = with(run("a", "sleep 5"), "TimeoutInMinutes", json!(EXPIRED));
    let (finished, lines) = go(&job(vec![slow]), false, Cancels::Never).await?;
    assert_eq!(finished.steps[0].outcome, StepResult::Failed, "{lines:?}");
    assert_eq!(timeout_lines(&lines), 1, "{lines:?}");
    Ok(())
}

#[tokio::test]
async fn an_always_step_keeps_running_when_the_job_is_cancelled_and_the_job_ends_cancelled()
-> Result<()> {
    let keeper = with(
        run("a", "sleep 1\necho survived"),
        "Condition",
        json!("always()"),
    );
    let (finished, lines) = go(&job(vec![keeper]), false, Cancels::AtStart).await?;
    assert_eq!(
        finished.steps[0].outcome,
        StepResult::Succeeded,
        "{lines:?}"
    );
    assert!(lines.iter().any(|line| line == "survived"), "{lines:?}");
    assert_eq!(finished.result, JobResult::Cancelled);
    Ok(())
}

#[tokio::test]
async fn a_cancel_during_an_always_step_that_fails_with_continue_on_error_cancels_the_job()
-> Result<()> {
    let action = uses(&[
        ("Condition", json!("always()")),
        ("ContinueOnError", json!(true)),
    ]);
    let (finished, lines) = go(&job(vec![action]), true, Cancels::InAdapter).await?;
    assert_eq!(finished.steps[0].outcome, StepResult::Failed, "{lines:?}");
    assert_eq!(finished.steps[0].conclusion, StepResult::Succeeded);
    assert_eq!(finished.result, JobResult::Cancelled);
    Ok(())
}

#[tokio::test]
async fn a_deadline_under_a_job_cancel_cancels_a_script_step_whatever_continue_on_error_says()
-> Result<()> {
    let keeper = with(
        with(
            with(run("a", "sleep 5"), "TimeoutInMinutes", json!(EXPIRED)),
            "Condition",
            json!("always()"),
        ),
        "ContinueOnError",
        json!(true),
    );
    let (finished, lines) = go(&job(vec![keeper]), false, Cancels::AtStart).await?;
    assert_eq!(
        finished.steps[0].outcome,
        StepResult::Cancelled,
        "{lines:?}"
    );
    assert_eq!(finished.steps[0].conclusion, StepResult::Cancelled);
    assert_eq!(timeout_lines(&lines), 0, "{lines:?}");
    assert_eq!(finished.result, JobResult::Cancelled);
    Ok(())
}

#[tokio::test]
async fn a_deadline_under_a_job_cancel_cancels_an_adapter_step_whatever_continue_on_error_says()
-> Result<()> {
    let action = uses(&[
        ("TimeoutInMinutes", json!(EXPIRED)),
        ("Condition", json!("always()")),
        ("ContinueOnError", json!(true)),
    ]);
    let (finished, lines) = go(&job(vec![action]), true, Cancels::AtStart).await?;
    assert_eq!(
        finished.steps[0].outcome,
        StepResult::Cancelled,
        "{lines:?}"
    );
    assert_eq!(finished.steps[0].conclusion, StepResult::Cancelled);
    assert_eq!(timeout_lines(&lines), 0, "{lines:?}");
    Ok(())
}

#[tokio::test]
async fn a_step_condition_is_evaluated_against_the_workspace_when_the_cancel_fires() -> Result<()> {
    let guarded = with(
        run("a", "touch marker\necho ready\nsleep 20"),
        "Condition",
        json!("always() && hashFiles('marker') == ''"),
    );
    let started = Instant::now();
    let (finished, lines) = go(&job(vec![guarded]), false, Cancels::OnReady).await?;
    assert_eq!(
        finished.steps[0].outcome,
        StepResult::Cancelled,
        "{lines:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(15), "{lines:?}");
    assert_eq!(finished.result, JobResult::Cancelled);
    Ok(())
}

#[test]
fn a_second_signal_ends_the_runner_while_an_always_step_runs() -> Result<()> {
    let scratch = Scratch::new()?;
    let temp = scratch.options().temp;
    let hold = run("hold", "echo up > \"$RUNNER_TEMP/first.up\"\nsleep 60");
    let cleanup = with(
        run("cleanup", BACKGROUND_SLEEP),
        "Condition",
        json!("always()"),
    );
    let path = write_job(&scratch, &job(vec![hold, cleanup]))?;
    let mut child = bounded(&scratch, &path)
        .stdout(Stdio::null())
        .spawn()
        .context("start the runner")?;
    let runner = Pid::from_raw(i32::try_from(child.id())?).context("the runner pid")?;
    wait_for_file(&temp.join("first.up"))?;
    kill_process(runner, Signal::TERM).context("send the first signal")?;
    wait_for_file(&temp.join("bg.pid"))?;
    assert!(child.try_wait()?.is_none(), "one signal ended the runner");
    kill_process(runner, Signal::TERM).context("send the second signal")?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() > deadline {
            child.kill().context("stop the stuck runner")?;
            child.wait().context("reap the stuck runner")?;
            bail!("a second signal did not end the runner");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(status.code(), Some(130));
    assert_gone(pid_from(&temp.join("bg.pid"))?)
}
