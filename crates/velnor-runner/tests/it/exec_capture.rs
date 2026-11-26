// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use futures_util::FutureExt;
use velnor_runner::exec::{
    AdapterRegistry, JobCancel, JobResult, JobRun, LogSink, ProcessEnv, StepAdapter, StepContext,
    StepFuture, StepResult, run_job,
};

use crate::exec_support::{Scratch, fixture_message};

#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<String>>>);

impl LogSink for Lines {
    fn step_started(&mut self, _number: u32, _name: &str) {}

    fn line(&mut self, _number: u32, text: &str) {
        if let Ok(mut lines) = self.0.lock() {
            lines.push(text.to_owned());
        }
    }

    fn step_finished(&mut self, _number: u32, _result: StepResult) {}

    fn job_finished(&mut self, _run: &JobRun) {}
}

struct Probe(&'static str);

impl StepAdapter for Probe {
    fn handles(&self, reference: &str) -> bool {
        reference == "actions/foo@v1"
    }

    fn run<'a>(&'a self, ctx: &'a mut StepContext<'_>) -> StepFuture<'a> {
        async move {
            let args = ["-c".to_owned(), self.0.to_owned()];
            let output = ctx
                .capture(("sh", &args, ctx.workspace, ProcessEnv::Overlay(&[])))
                .await?;
            ctx.log(&format!("captured={output}"));
            Ok(StepResult::Succeeded)
        }
        .boxed()
    }
}

async fn run_probe(
    script: &'static str,
    cancel_after: Option<Duration>,
) -> Result<(JobRun, Vec<String>, Duration)> {
    let scratch = Scratch::new()?;
    let mut adapters = AdapterRegistry::empty();
    adapters.register(Box::new(Probe(script)));
    let cancel = JobCancel::new();
    if let Some(delay) = cancel_after {
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            trigger.cancel();
        });
    }
    let mut sink = Lines::default();
    let lines = sink.0.clone();
    let started = Instant::now();
    let run = tokio::time::timeout(
        Duration::from_secs(20),
        run_job(
            &fixture_message("adapter-uses")?,
            &scratch.options(),
            &adapters,
            &mut sink,
            cancel,
        ),
    )
    .await?;
    let seen = lines.lock().map_or_default(|lines| lines.clone());
    Ok((run, seen, started.elapsed()))
}

#[tokio::test]
async fn capture_returns_stdout_without_logging_it() -> Result<()> {
    let (run, lines, _) = run_probe("printf 'one\\r\\ntwo'; echo noise >&2", None).await?;
    assert_eq!(run.steps[0].outcome, StepResult::Succeeded);
    assert!(
        lines.iter().any(|line| line == "captured=one\r\ntwo"),
        "{lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line.contains("noise")),
        "{lines:?}"
    );
    Ok(())
}

#[tokio::test]
async fn capture_failure_reports_stderr_and_fails_the_step() -> Result<()> {
    let (run, lines, _) = run_probe("echo broken >&2; exit 3", None).await?;
    assert_eq!(run.steps[0].outcome, StepResult::Failed);
    assert!(
        lines.iter().any(|line| line.ends_with("failed: broken")),
        "{lines:?}"
    );
    Ok(())
}

#[tokio::test]
async fn cancel_kills_a_captured_process_and_cancels_the_step() -> Result<()> {
    let (run, lines, elapsed) = run_probe("sleep 30", Some(Duration::from_millis(300))).await?;
    assert_eq!(run.result, JobResult::Cancelled);
    assert_eq!(run.steps[0].outcome, StepResult::Cancelled);
    assert!(elapsed < Duration::from_secs(15), "{elapsed:?}");
    assert!(
        lines.iter().any(|line| line == "the step was cancelled"),
        "{lines:?}"
    );
    Ok(())
}
