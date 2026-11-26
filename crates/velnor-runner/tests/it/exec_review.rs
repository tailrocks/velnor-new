// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use futures_util::FutureExt;
use serde_json::{Value, json};
use velnor_runner::exec::{
    AdapterRegistry, Annotation, JobCancel, JobResult, StepAdapter, StepContext, StepFuture,
    StepResult, run_job,
};

use crate::exec_builder::{
    Recorder, assert_gone, execute, expression, job, mapping, parse, pid_from, run, run_message,
    step, wait_for_file,
};
use crate::exec_support::Scratch;

const SECRET: &str = "annot-secret-91";

fn with_secret(mut message: Value) -> Value {
    message["Variables"] = json!({"S": {"Value": SECRET, "IsSecret": true}});
    message
}

fn leaks(annotations: &[Annotation], value: &str) -> bool {
    annotations.iter().any(|annotation| {
        [
            Some(&annotation.message),
            annotation.title.as_ref(),
            annotation.file.as_ref(),
        ]
        .into_iter()
        .flatten()
        .any(|text| text.contains(value))
    })
}

#[tokio::test]
async fn annotations_are_masked_in_every_field() -> Result<()> {
    let message = with_secret(job(vec![
        run(
            "a",
            &format!(
                "echo '::error title={SECRET},file={SECRET}.rs::saw {SECRET}'\necho '::notice title=late-title-77::hi'"
            ),
        ),
        run("b", "echo '::add-mask::late-title-77'"),
    ]));
    let (run, _) = execute(
        &message,
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(run.result, JobResult::Succeeded);
    assert!(run.annotations.len() >= 2, "{:?}", run.annotations);
    assert!(!leaks(&run.annotations, SECRET), "{:?}", run.annotations);
    assert!(
        !leaks(&run.annotations, "late-title-77"),
        "{:?}",
        run.annotations
    );
    Ok(())
}

#[tokio::test]
async fn rejection_lines_are_masked() -> Result<()> {
    let mut uses = step("u", json!(null));
    uses["Reference"] = json!({
        "Type": "repository",
        "Name": format!("{SECRET}/action"),
        "Ref": "v1",
        "RepositoryType": "GitHub"
    });
    let message = with_secret(job(vec![uses]));
    let (run, _) = execute(
        &message,
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(run.result, JobResult::Failed);
    assert!(
        run.annotations
            .iter()
            .any(|annotation| annotation.message.starts_with("unsupported: uses ***")),
        "{:?}",
        run.annotations
    );
    assert!(!leaks(&run.annotations, SECRET), "{:?}", run.annotations);
    Ok(())
}

#[tokio::test]
async fn a_regex_hint_that_does_not_compile_fails_the_job() -> Result<()> {
    let mut message = job(vec![run("a", "echo ran")]);
    message["Mask"] = json!([{"Type": "regex", "Value": "(unclosed"}]);
    let (run, lines) = execute(
        &message,
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(run.result, JobResult::Failed);
    assert!(run.steps.is_empty(), "{:?}", run.steps);
    let expected = "mask hint 1 is not a valid regular expression";
    assert!(lines.iter().any(|line| line == expected), "{lines:?}");
    assert!(
        !leaks(&run.annotations, "unclosed"),
        "{:?}",
        run.annotations
    );
    Ok(())
}

#[test]
fn step_outputs_are_case_insensitive_and_the_last_write_wins() -> Result<()> {
    let scratch = Scratch::new()?;
    let message = job(vec![
        run(
            "a",
            "echo 'Foo=first' >> \"$GITHUB_OUTPUT\"\necho 'foo=second' >> \"$GITHUB_OUTPUT\"",
        ),
        step(
            "b",
            expression("format('echo \"got={0}-{1}\"', steps.a.outputs.FOO, steps.a.outputs.Foo)"),
        ),
    ]);
    let ran = run_message(&scratch, &message)?;
    assert!(ran.has_line("got=second-second"), "{}", ran.stdout);
    Ok(())
}

#[test]
fn a_long_line_is_truncated_with_a_marker_and_the_step_goes_on() -> Result<()> {
    let scratch = Scratch::new()?;
    let message = job(vec![run(
        "long",
        "head -c 5000000 /dev/zero | tr '\\0' a\necho\necho after-long\nhead -c 3000000 /dev/zero | tr '\\0' b",
    )]);
    let started = Instant::now();
    let ran = run_message(&scratch, &message)?;
    assert!(started.elapsed() < Duration::from_secs(30));
    let marker = " [line truncated at 1048576 bytes]";
    let longest = ran.lines().into_iter().map(str::len).max().unwrap_or(0);
    assert!(longest <= (1 << 20) + marker.len(), "{longest}");
    for kept in ["a", "b"] {
        let line = ran
            .lines()
            .into_iter()
            .find(|line| line.starts_with(kept))
            .context("the truncated line")?;
        assert!(line.ends_with(marker), "{kept}");
    }
    assert!(ran.has_line("after-long"), "{}", ran.stdout);
    assert_eq!(ran.code, Some(0));
    Ok(())
}

enum Mode {
    IgnoreCancel,
    Environment,
    Background,
    OversizedCapture,
}

struct Probe(Mode);

impl StepAdapter for Probe {
    fn handles(&self, reference: &str) -> bool {
        reference == "actions/foo@v1"
    }

    fn run<'a>(&'a self, ctx: &'a mut StepContext<'_>) -> StepFuture<'a> {
        async move {
            match self.0 {
                Mode::IgnoreCancel => {
                    tokio::time::sleep(Duration::from_secs(120)).await;
                }
                Mode::Environment => {
                    ctx.env = BTreeMap::from([("A".to_owned(), "1".to_owned())]);
                    let extra = [("B".to_owned(), "2".to_owned())];
                    let printed = ctx.capture("/usr/bin/env", &[], &extra).await?;
                    let mut names: Vec<&str> = printed.lines().collect();
                    names.sort_unstable();
                    ctx.log(&format!("env={}", names.join(",")));
                }
                Mode::Background => {
                    let script = "sleep 4246 >/dev/null 2>&1 &\necho $! > \"$RUNNER_TEMP/bg2.pid\"";
                    let args = ["-c".to_owned(), script.to_owned()];
                    ctx.run_process("sh", &args, &[]).await?;
                }
                Mode::OversizedCapture => {
                    let script = "head -c 9437184 /dev/zero | tr '\\0' a";
                    let args = ["-c".to_owned(), script.to_owned()];
                    let printed = ctx.capture("sh", &args, &[]).await?;
                    ctx.log(&format!("captured {} bytes", printed.len()));
                }
            }
            Ok(StepResult::Succeeded)
        }
        .boxed()
    }
}

fn uses_job() -> Value {
    let mut uses = step("foo", json!(null));
    uses["Reference"] = json!({
        "Type": "repository",
        "Name": "actions/foo",
        "Ref": "v1",
        "RepositoryType": "GitHub"
    });
    uses["Inputs"] = mapping(&[]);
    job(vec![uses])
}

fn registry(mode: Mode) -> AdapterRegistry {
    let mut adapters = AdapterRegistry::empty();
    adapters.register(Box::new(Probe(mode)));
    adapters
}

#[tokio::test]
async fn an_adapter_that_ignores_cancel_is_dropped_and_the_step_is_cancelled() -> Result<()> {
    let cancel = JobCancel::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        trigger.cancel();
    });
    let started = Instant::now();
    let (run, _) = execute(
        &uses_job(),
        &Scratch::new()?,
        &registry(Mode::IgnoreCancel),
        cancel,
    )
    .await?;
    assert!(started.elapsed() < Duration::from_secs(20));
    assert_eq!(run.result, JobResult::Cancelled);
    assert_eq!(run.steps[0].outcome, StepResult::Cancelled);
    Ok(())
}

#[tokio::test]
async fn adapter_processes_start_from_the_step_environment_only() -> Result<()> {
    let (run, lines) = execute(
        &uses_job(),
        &Scratch::new()?,
        &registry(Mode::Environment),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(run.steps[0].outcome, StepResult::Succeeded);
    assert!(lines.iter().any(|line| line == "env=A=1,B=2"), "{lines:?}");
    Ok(())
}

#[tokio::test]
async fn adapter_processes_are_killed_with_the_job() -> Result<()> {
    let scratch = Scratch::new()?;
    let (run, _) = execute(
        &uses_job(),
        &scratch,
        &registry(Mode::Background),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(run.result, JobResult::Succeeded);
    assert_gone(pid_from(&scratch.options().temp.join("bg2.pid"))?)
}

#[tokio::test]
async fn capture_refuses_more_output_than_its_limit() -> Result<()> {
    let (run, lines) = execute(
        &uses_job(),
        &Scratch::new()?,
        &registry(Mode::OversizedCapture),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(run.steps[0].outcome, StepResult::Failed);
    assert!(
        lines
            .iter()
            .any(|line| line.ends_with("printed more than 8388608 bytes")),
        "{:?}",
        lines.iter().map(String::len).collect::<Vec<_>>()
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_the_job_kills_the_processes_it_started() -> Result<()> {
    let scratch = Scratch::new()?;
    let message = parse(&job(vec![run(
        "hold",
        "sleep 4247 >/dev/null 2>&1 &\necho $! > \"$RUNNER_TEMP/bg3.pid\"\nwait",
    )]))?;
    let options = scratch.options();
    let task = tokio::spawn(async move {
        let mut recorder = Recorder::default();
        run_job(
            &message,
            &options,
            &AdapterRegistry::empty(),
            &mut recorder,
            JobCancel::new(),
        )
        .await
    });
    let pid_file = scratch.options().temp.join("bg3.pid");
    tokio::task::spawn_blocking({
        let pid_file = pid_file.clone();
        move || wait_for_file(&pid_file)
    })
    .await??;
    task.abort();
    let _ = task.await;
    assert_gone(pid_from(&pid_file)?)
}
