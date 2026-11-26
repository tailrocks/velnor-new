// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::{Value, json};
use velnor_runner::exec::{AdapterRegistry, JobCancel, JobResult, LogSink, StepResult, run_job};

use crate::exec_builder::{
    binary, bounded, execute, expression, finished, job, literal, mapping, parse, run, run_message,
    step, with, write_job,
};
use crate::exec_support::Scratch;

fn with_shell(mut step: Value, shell: &Value) -> Value {
    if let Some(entries) = step["Inputs"]["map"].as_array_mut() {
        entries.push(json!({"key": literal("shell"), "value": shell}));
    }
    step
}

fn has_prefix_line(ran: &crate::exec_support::Ran, prefix: &str) -> bool {
    ran.stdout.lines().any(|line| line.starts_with(prefix))
}

struct CancelOnStart(JobCancel);

impl LogSink for CancelOnStart {
    fn step_started(&mut self, _number: u32, _name: &str) {
        self.0.cancel();
    }

    fn line(&mut self, _number: u32, _text: &str) {}

    fn step_finished(&mut self, _number: u32, _result: StepResult) {}

    fn job_finished(&mut self, _run: &velnor_runner::exec::JobRun) {}
}

#[tokio::test]
async fn a_cancel_during_step_startup_still_cancels_the_step() -> Result<()> {
    let scratch = Scratch::new()?;
    let cancel = JobCancel::new();
    let mut sink = CancelOnStart(cancel.clone());
    let started = Instant::now();
    let run = tokio::time::timeout(
        Duration::from_secs(60),
        run_job(
            &parse(&job(vec![run("a", "sleep 20")]))?,
            &scratch.options(),
            &AdapterRegistry::empty(),
            &mut sink,
            cancel,
        ),
    )
    .await
    .context("run_job returns")?;
    assert!(started.elapsed() < Duration::from_secs(15));
    assert_eq!(run.result, JobResult::Cancelled);
    assert_eq!(run.steps[0].outcome, StepResult::Cancelled);
    Ok(())
}

#[test]
fn quoted_custom_shell_arguments_stay_one_argument() -> Result<()> {
    let scratch = Scratch::new()?;
    let message = job(vec![with_shell(
        run("a", "echo from-script"),
        &literal("bash -c \"source {0}\""),
    )]);
    let ran = run_message(&scratch, &message)?;
    assert!(ran.has_line("from-script"), "{}", ran.stdout);
    assert_eq!(ran.code, Some(0));
    Ok(())
}

#[test]
fn github_action_and_the_event_file_are_fresh_for_each_step() -> Result<()> {
    let scratch = Scratch::new()?;
    let mut message = job(vec![
        run(
            "first",
            "echo \"action=$GITHUB_ACTION\"\necho mutated > \"$GITHUB_EVENT_PATH\"",
        ),
        step(
            "second",
            expression(
                "format('echo \"action={0} env=$GITHUB_ACTION\"; cat \"$GITHUB_EVENT_PATH\"', github.action)",
            ),
        ),
    ]);
    message["ContextData"] = json!({"github": {"t": 2, "d": [{"k": "event", "v": {"t": 2, "d": [{"k": "x", "v": "1"}]}}]}});
    let ran = run_message(&scratch, &message)?;
    assert!(ran.has_line("action=first"), "{}", ran.stdout);
    assert!(ran.has_line("action=second env=second"), "{}", ran.stdout);
    assert!(ran.has_line("{\"x\":\"1\"}"), "{}", ran.stdout);
    Ok(())
}

#[tokio::test]
async fn continue_on_error_sees_the_step_environment() -> Result<()> {
    let failing = with(
        with(
            run("a", "exit 1"),
            "Environment",
            mapping(&[("ALLOW", literal("1"))]),
        ),
        "ContinueOnError",
        expression("env.ALLOW == '1'"),
    );
    let (run, _) = execute(
        &job(vec![failing]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(run.result, JobResult::Succeeded);
    assert_eq!(run.steps[0].outcome, StepResult::Failed);
    assert_eq!(run.steps[0].conclusion, StepResult::Succeeded);
    Ok(())
}

#[tokio::test]
async fn a_continue_on_error_expression_that_fails_is_reported_when_the_step_fails() -> Result<()> {
    let broken = |script: &str| {
        with(
            run("a", script),
            "ContinueOnError",
            expression("vars.ALLOW"),
        )
    };
    let (failed, _) = execute(
        &job(vec![broken("exit 1")]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(failed.result, JobResult::Failed);
    let messages: Vec<&str> = failed
        .annotations
        .iter()
        .map(|annotation| annotation.message.as_str())
        .collect();
    assert!(
        messages.contains(&"unsupported expression context vars"),
        "{messages:?}"
    );
    let (passed, _) = execute(
        &job(vec![broken("true")]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(passed.result, JobResult::Succeeded);
    assert!(passed.annotations.is_empty(), "{:?}", passed.annotations);
    Ok(())
}

#[test]
fn background_steps_are_rejected_before_any_step_runs() -> Result<()> {
    let scratch = Scratch::new()?;
    let message = job(vec![
        run("a", "echo side-effect"),
        with(run("b", "echo background"), "Background", json!(true)),
    ]);
    let ran = run_message(&scratch, &message)?;
    assert!(
        ran.has_line("unsupported: background step"),
        "{}",
        ran.stdout
    );
    assert!(!has_prefix_line(&ran, "step "), "{}", ran.stdout);
    assert!(!ran.stdout.contains("side-effect"));
    assert_eq!(ran.code, Some(1));
    Ok(())
}

#[test]
fn a_shell_chosen_by_the_environment_is_checked_before_any_step_runs() -> Result<()> {
    let scratch = Scratch::new()?;
    let chosen = with(
        with_shell(run("b", "echo hi"), &expression("env.SH")),
        "Environment",
        mapping(&[("SH", literal("pwsh"))]),
    );
    let ran = run_message(&scratch, &job(vec![run("a", "echo side-effect"), chosen]))?;
    assert!(ran.has_line("unsupported: shell pwsh"), "{}", ran.stdout);
    assert!(!has_prefix_line(&ran, "step "), "{}", ran.stdout);
    assert!(!ran.stdout.contains("side-effect"));

    let unresolved = with_shell(run("c", "echo hi"), &expression("vars.X"));
    let ran = run_message(&scratch, &job(vec![unresolved]))?;
    assert!(
        ran.has_line("unsupported: shell unsupported expression context vars"),
        "{}",
        ran.stdout
    );
    Ok(())
}

#[test]
fn oidc_request_variables_reach_script_steps_and_stay_masked() -> Result<()> {
    let scratch = Scratch::new()?;
    let mut message = job(vec![run(
        "a",
        "echo \"url=$ACTIONS_ID_TOKEN_REQUEST_URL\"\necho \"token=$ACTIONS_ID_TOKEN_REQUEST_TOKEN\"",
    )]);
    message["Resources"] = json!({"Endpoints": [{
        "Name": "SystemVssConnection",
        "Url": "https://pipelines.example/",
        "Data": {"GenerateIdTokenUrl": "https://oidc.example/token"},
        "Authorization": {"Scheme": "OAuth", "Parameters": {"AccessToken": "tok-oidc-9"}}
    }]});
    let ran = run_message(&scratch, &message)?;
    assert!(
        ran.has_line("url=https://oidc.example/token"),
        "{}",
        ran.stdout
    );
    assert!(ran.has_line("token=***"), "{}", ran.stdout);
    Ok(())
}

#[test]
fn an_unbounded_step_summary_is_dropped_without_reading_it_whole() -> Result<()> {
    let scratch = Scratch::new()?;
    let message = job(vec![run("a", "ln -sf /dev/zero \"$GITHUB_STEP_SUMMARY\"")]);
    let path = write_job(&scratch, &message)?;
    let mut child = bounded(&scratch, &path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("start velnor-runner exec")?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while child.try_wait()?.is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let finished_in_time = child.try_wait()?.is_some();
    if !finished_in_time {
        child.kill()?;
    }
    let output = child.wait_with_output()?;
    assert!(finished_in_time, "the runner kept reading the summary");
    assert_eq!(output.status.code(), Some(0));
    Ok(())
}

#[test]
fn step_files_and_temp_directories_belong_to_the_owner() -> Result<()> {
    let scratch = Scratch::new()?;
    let message = job(vec![run(
        "a",
        "ls -l \"$0\" \"$GITHUB_OUTPUT\" \"$GITHUB_ENV\" \"$GITHUB_PATH\" \"$GITHUB_STEP_SUMMARY\" \"$RUNNER_TEMP/_github_workflow/event.json\" | cut -c1-10\nls -ld \"$RUNNER_TEMP\" \"$RUNNER_TEMP/_runner_file_commands\" \"$RUNNER_TEMP/_github_workflow\" | cut -c1-10\nls -ld \"$(dirname \"$RUNNER_TEMP\")\" | sed 's/^/root:/' | cut -c1-15",
    )]);
    let path = write_job(&scratch, &message)?;
    let inner = bounded(&scratch, &path);
    let mut wrapped = Command::new("sh");
    wrapped
        .arg("-c")
        .arg("umask 022; exec \"$0\" \"$@\"")
        .arg(inner.get_program())
        .args(inner.get_args());
    let ran = finished(wrapped)?;
    let files = ran
        .stdout
        .lines()
        .filter(|line| *line == "-rw-------")
        .count();
    let dirs = ran
        .stdout
        .lines()
        .filter(|line| *line == "drwx------")
        .count();
    assert_eq!((files, dirs), (6, 3), "{}", ran.stdout);

    let mut default_dirs = Command::new("sh");
    default_dirs
        .arg("-c")
        .arg("umask 022; exec \"$0\" \"$@\"")
        .arg(binary(&path).get_program())
        .args(binary(&path).get_args());
    let ran = finished(default_dirs)?;
    assert!(
        ran.stdout.lines().any(|line| line == "root:drwx------"),
        "{}",
        ran.stdout
    );
    Ok(())
}
