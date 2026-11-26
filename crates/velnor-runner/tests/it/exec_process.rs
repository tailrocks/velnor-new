// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::json;
use velnor_runner::exec::{
    AdapterRegistry, JobCancel, JobResult, JobRun, LogSink, StepResult, run_job,
};

use crate::exec_builder::{BACKGROUND_STATE, job, parse, run, run_message, with};
use crate::exec_support::Scratch;

#[derive(Default)]
struct SlowSink {
    lines: Vec<String>,
}

impl LogSink for SlowSink {
    fn step_started(&mut self, _number: u32, _name: &str) {}

    fn line(&mut self, _number: u32, text: &str) {
        std::thread::sleep(Duration::from_millis(10));
        self.lines.push(text.to_owned());
    }

    fn step_finished(&mut self, _number: u32, _result: StepResult) {}

    fn job_finished(&mut self, _run: &JobRun) {}
}

#[tokio::test]
async fn output_queued_when_the_process_exits_is_delivered_to_a_slow_sink() -> Result<()> {
    let scratch = Scratch::new()?;
    let mut sink = SlowSink::default();
    let run = tokio::time::timeout(
        Duration::from_secs(60),
        run_job(
            &parse(&job(vec![run(
                "a",
                "for i in $(seq 1 400); do echo \"line$i\"; done",
            )]))?,
            &scratch.options(),
            &AdapterRegistry::empty(),
            &mut sink,
            JobCancel::new(),
        ),
    )
    .await
    .context("run_job returns")?;
    assert_eq!(run.result, JobResult::Succeeded);
    assert_eq!(sink.lines.len(), 400, "{:?}", sink.lines.last());
    assert_eq!(sink.lines.last().map(String::as_str), Some("line400"));
    Ok(())
}

#[test]
fn a_descendant_that_ignores_sigterm_dies_with_a_timed_out_step() -> Result<()> {
    let scratch = Scratch::new()?;
    let hold = "(trap '' TERM; exec sleep 4242 >/dev/null 2>&1) &\necho $! > \"$RUNNER_TEMP/bg.pid\"\nwait";
    let message = job(vec![
        with(run("hold", hold), "TimeoutInMinutes", json!(0.02)),
        with(
            run("check", BACKGROUND_STATE),
            "Condition",
            json!("always()"),
        ),
    ]);
    let ran = run_message(&scratch, &message)?;
    assert!(
        ran.has_line("step 1 timed out after 0.02 minutes"),
        "{}",
        ran.stdout
    );
    assert!(ran.has_line("bg-dead"), "{}", ran.stdout);
    Ok(())
}

#[test]
fn github_path_entries_keep_their_whitespace() -> Result<()> {
    let scratch = Scratch::new()?;
    let message = job(vec![
        run(
            "add",
            "mkdir -p \"$PWD/bin \"\nprintf '#!/bin/sh\\necho from-spaced-bin\\n' > \"$PWD/bin /tool-x\"\nchmod +x \"$PWD/bin /tool-x\"\necho \"$PWD/bin \" >> \"$GITHUB_PATH\"",
        ),
        run("use", "tool-x"),
    ]);
    let ran = run_message(&scratch, &message)?;
    assert!(ran.has_line("from-spaced-bin"), "{}", ran.stdout);
    Ok(())
}
