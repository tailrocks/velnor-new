// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use futures_util::FutureExt;
use rustix::process::{Pid, Signal, kill_process};
use serde_json::{Value, json};
use velnor_runner::exec::{
    AdapterRegistry, JobCancel, JobResult, JobRun, LogSink, StepAdapter, StepContext, StepFuture,
    StepResult, run_job,
};
use velnor_runner::job_message::AgentJobRequestMessage;

use crate::exec_builder::{
    BACKGROUND_SLEEP, BACKGROUND_STATE, assert_gone, binary, bounded, expression, finished, job,
    literal, mapping, pid_from, run, run_message, step, wait_for_file, with, write_job,
};
use crate::exec_support::{Scratch, fixture_path};

#[test]
fn step_env_is_cleared_to_the_allow_list() -> Result<()> {
    let scratch = Scratch::new()?;
    let message = job(vec![run(
        "probe",
        "echo \"probe=${VELNOR_LEAK_PROBE-unset} gh=${GH_TOKEN-unset} gt=${GITHUB_TOKEN-unset} aws=${AWS_SECRET_ACCESS_KEY-unset} ssh=${SSH_AUTH_SOCK-unset}\"\necho \"term-is-xterm=$([ \"${TERM-}\" = xterm ] && echo yes || echo no)\"\necho \"home=${HOME-unset} user=${USER-unset} lang=${LANG-unset} lcall=${LC_ALL-unset} tmpdir=${TMPDIR-unset} path=${PATH:+set}\"",
    )]);
    let path = write_job(&scratch, &message)?;
    let mut command = bounded(&scratch, &path);
    command
        .env("VELNOR_LEAK_PROBE", "leaked")
        .env("GH_TOKEN", "gh-secret")
        .env("GITHUB_TOKEN", "github-secret")
        .env("AWS_SECRET_ACCESS_KEY", "aws-secret")
        .env("SSH_AUTH_SOCK", "/tmp/agent.sock")
        .env("TERM", "xterm")
        .env("HOME", "/probe/home")
        .env("USER", "probe-user")
        .env("LANG", "C.UTF-8")
        .env("LC_ALL", "C")
        .env("TMPDIR", "/tmp/probe");
    let ran = finished(command)?;
    assert!(
        ran.has_line("probe=unset gh=unset gt=unset aws=unset ssh=unset"),
        "{}",
        ran.stdout
    );
    assert!(ran.has_line("term-is-xterm=no"), "{}", ran.stdout);
    assert!(
        ran.has_line(
            "home=/probe/home user=probe-user lang=C.UTF-8 lcall=C tmpdir=/tmp/probe path=set"
        ),
        "{}",
        ran.stdout
    );
    assert_eq!(ran.code, Some(0));
    Ok(())
}

#[test]
fn env_names_are_case_sensitive() -> Result<()> {
    let scratch = Scratch::new()?;
    let mut message = job(vec![
        with(
            run(
                "a",
                "echo \"process=$foo-$FOO\"\necho 'Foo=written' >> \"$GITHUB_ENV\"",
            ),
            "Environment",
            mapping(&[("FOO", literal("upper"))]),
        ),
        step(
            "b",
            expression("format('echo \"context={0}-{1}-{2}\"', env.foo, env.FOO, env.Foo)"),
        ),
    ]);
    message["EnvironmentVariables"] = json!([mapping(&[("foo", literal("lower"))])]);
    let ran = run_message(&scratch, &message)?;
    assert!(ran.has_line("process=lower-upper"), "{}", ran.stdout);
    assert!(ran.has_line("context=lower--written"), "{}", ran.stdout);
    Ok(())
}

#[test]
fn terminating_the_runner_kills_the_step_group() -> Result<()> {
    let scratch = Scratch::new()?;
    let path = write_job(&scratch, &job(vec![run("hold", BACKGROUND_SLEEP)]))?;
    let child = bounded(&scratch, &path)
        .stdout(Stdio::piped())
        .spawn()
        .context("start the runner")?;
    let pid_file = scratch.options().temp.join("bg.pid");
    wait_for_file(&pid_file)?;
    let runner = Pid::from_raw(i32::try_from(child.id())?).context("the runner pid")?;
    kill_process(runner, Signal::TERM).context("send SIGTERM")?;
    let output = child.wait_with_output().context("wait for the runner")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.lines().any(|line| line == "job cancelled"),
        "{stdout}"
    );
    assert_eq!(output.status.code(), Some(1));
    assert_gone(pid_from(&pid_file)?)
}

#[test]
fn timeout_kills_the_whole_step_group() -> Result<()> {
    let scratch = Scratch::new()?;
    let message = job(vec![
        with(
            run("hold", BACKGROUND_SLEEP),
            "TimeoutInMinutes",
            json!(0.02),
        ),
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

#[tokio::test]
async fn cancel_kills_the_whole_step_group() -> Result<()> {
    let scratch = Scratch::new()?;
    let message = AgentJobRequestMessage::parse_json(&serde_json::to_string(&job(vec![
        run("hold", BACKGROUND_SLEEP),
        with(
            run("check", BACKGROUND_STATE),
            "Condition",
            json!("always()"),
        ),
    ]))?)?;
    let cancel = JobCancel::new();
    let trigger = cancel.clone();
    let pid_file = scratch.options().temp.join("bg.pid");
    let watched = pid_file.clone();
    tokio::spawn(async move {
        while !watched.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        trigger.cancel();
    });
    let mut recorder = Recorder::default();
    let run = tokio::time::timeout(
        Duration::from_secs(20),
        run_job(
            &message,
            &scratch.options(),
            &AdapterRegistry::empty(),
            &mut recorder,
            cancel,
        ),
    )
    .await
    .context("run_job returns after the cancel")?;
    assert_eq!(run.result, JobResult::Cancelled);
    assert!(
        recorder.lines.iter().any(|line| line == "bg-dead"),
        "{:?}",
        recorder.lines
    );
    Ok(())
}

#[test]
fn background_processes_outlive_the_step_but_not_the_job() -> Result<()> {
    let scratch = Scratch::new()?;
    let message = job(vec![
        run(
            "start",
            "sleep 4243 >/dev/null 2>&1 &\necho $! > \"$RUNNER_TEMP/bg.pid\"",
        ),
        run(
            "check",
            "kill -0 \"$(cat \"$RUNNER_TEMP/bg.pid\")\" && echo still-running",
        ),
    ]);
    let ran = run_message(&scratch, &message)?;
    assert!(ran.has_line("still-running"), "{}", ran.stdout);
    assert_eq!(ran.code, Some(0));
    assert_gone(pid_from(&scratch.options().temp.join("bg.pid"))?)
}

#[test]
fn default_directories_are_removed_and_temp_is_not_placed_beside_a_workspace() -> Result<()> {
    let message = job(vec![run(
        "s",
        "echo \"ws=$GITHUB_WORKSPACE\"\necho \"tmp=$RUNNER_TEMP\"",
    )]);
    let scratch = Scratch::new()?;
    let path = write_job(&scratch, &message)?;
    let ran = finished(binary(&path))?;
    for key in ["ws=", "tmp="] {
        let printed = ran
            .stdout
            .lines()
            .find_map(|line| line.strip_prefix(key))
            .context("the printed directory")?;
        assert!(!Path::new(printed).exists(), "{printed} was left behind");
    }

    let workspace = scratch.workspace();
    let mut command = binary(&path);
    command.arg("--workspace").arg(&workspace);
    let ran = finished(command)?;
    let printed = ran
        .stdout
        .lines()
        .find_map(|line| line.strip_prefix("tmp="))
        .context("the printed temp")?;
    assert_ne!(Path::new(printed), workspace.with_file_name("temp"));
    assert!(!workspace.with_file_name("temp").exists());
    assert!(!Path::new(printed).exists(), "{printed} was left behind");
    assert!(workspace.exists());
    Ok(())
}

#[tokio::test]
async fn adapter_steps_honour_timeout_minutes() -> Result<()> {
    let body = std::fs::read_to_string(fixture_path("adapter-uses"))?;
    let mut message: Value = serde_json::from_str(&body)?;
    message["Steps"][0]["TimeoutInMinutes"] = json!(0.02);
    message["Steps"]
        .as_array_mut()
        .context("steps")?
        .truncate(1);
    let message = AgentJobRequestMessage::parse_json(&serde_json::to_string(&message)?)?;
    let scratch = Scratch::new()?;
    let mut adapters = AdapterRegistry::empty();
    adapters.register(Box::new(Sleeper));
    let mut recorder = Recorder::default();
    let started = Instant::now();
    let run = run_job(
        &message,
        &scratch.options(),
        &adapters,
        &mut recorder,
        JobCancel::new(),
    )
    .await;
    assert!(started.elapsed() < Duration::from_secs(20));
    assert_eq!(run.result, JobResult::Failed);
    assert_eq!(run.steps[0].outcome, StepResult::Failed);
    assert!(
        recorder
            .lines
            .iter()
            .any(|line| line == "step 1 timed out after 0.02 minutes"),
        "{:?}",
        recorder.lines
    );
    Ok(())
}
#[derive(Default)]
struct Recorder {
    lines: Vec<String>,
}

impl LogSink for Recorder {
    fn step_started(&mut self, _number: u32, _name: &str) {}

    fn line(&mut self, _number: u32, text: &str) {
        self.lines.push(text.to_owned());
    }

    fn step_finished(&mut self, _number: u32, _result: StepResult) {}

    fn job_finished(&mut self, _run: &JobRun) {}
}

struct Sleeper;

impl StepAdapter for Sleeper {
    fn handles(&self, reference: &str) -> bool {
        reference == "actions/foo@v1"
    }

    fn run<'a>(&'a self, _ctx: &'a mut StepContext<'_>) -> StepFuture<'a> {
        async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok(StepResult::Succeeded)
        }
        .boxed()
    }
}
