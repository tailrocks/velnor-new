// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;

use anyhow::Result;
use serde_json::json;
use velnor_runner::exec::{
    AdapterRegistry, AnnotationLevel, JobCancel, JobResult, JobRun, RunOptions, StepResult, run_job,
};

use crate::exec_builder::{Recorder, execute, expression, job, mapping, parse, run, step, with};
use crate::exec_support::Scratch;

const BROKEN: &str = "fromJSON('{bad')";

fn errors(finished: &JobRun) -> Vec<&str> {
    finished
        .annotations
        .iter()
        .filter(|annotation| annotation.level == AnnotationLevel::Error)
        .map(|annotation| annotation.message.as_str())
        .collect()
}

#[tokio::test]
async fn a_condition_that_fails_on_the_first_step_fails_it() -> Result<()> {
    let (finished, lines) = execute(
        &job(vec![with(
            run("a", "echo never"),
            "Condition",
            json!(BROKEN),
        )]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(finished.result, JobResult::Failed);
    assert_eq!(finished.steps[0].outcome, StepResult::Failed);
    assert!(
        errors(&finished)
            .iter()
            .any(|text| text.contains("Error parsing fromJson")),
        "{:?}",
        finished.annotations
    );
    assert!(!lines.iter().any(|line| line == "never"), "{lines:?}");
    Ok(())
}

#[tokio::test]
async fn a_condition_that_fails_after_a_failure_fails_the_step() -> Result<()> {
    let broken = with(
        run("b", "echo never"),
        "Condition",
        json!(format!("always() && {BROKEN}")),
    );
    let (finished, lines) = execute(
        &job(vec![run("a", "exit 1"), broken]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(finished.steps[0].outcome, StepResult::Failed);
    assert_eq!(finished.steps[1].outcome, StepResult::Failed);
    assert!(
        errors(&finished)
            .iter()
            .any(|text| text.contains("Error parsing fromJson")),
        "{:?}",
        finished.annotations
    );
    assert!(!lines.iter().any(|line| line == "never"), "{lines:?}");
    Ok(())
}

#[tokio::test]
async fn an_input_that_does_not_evaluate_fails_the_step() -> Result<()> {
    let (finished, _) = execute(
        &job(vec![
            step("a", expression(BROKEN)),
            with(run("b", "echo after"), "Condition", json!("always()")),
        ]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(finished.steps[0].outcome, StepResult::Failed);
    assert_eq!(finished.steps[1].outcome, StepResult::Succeeded);
    assert!(
        errors(&finished)
            .iter()
            .any(|text| text.contains("Error parsing fromJson")),
        "{:?}",
        finished.annotations
    );
    Ok(())
}

#[tokio::test]
async fn an_environment_that_fails_fails_a_step_whose_condition_is_false() -> Result<()> {
    let guarded = with(
        with(run("a", "echo never"), "Condition", json!("false")),
        "Environment",
        mapping(&[("X", expression(BROKEN))]),
    );
    let (finished, lines) = execute(
        &job(vec![guarded]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(finished.result, JobResult::Failed);
    assert_eq!(finished.steps[0].outcome, StepResult::Failed);
    assert!(
        errors(&finished)
            .iter()
            .any(|text| text.contains("Error parsing fromJson")),
        "{:?}",
        finished.annotations
    );
    assert!(!lines.iter().any(|line| line == "never"), "{lines:?}");
    Ok(())
}

async fn rejected_for(options: RunOptions) -> Result<(JobRun, Recorder)> {
    let mut recorder = Recorder::default();
    let finished = run_job(
        &parse(&job(vec![run("a", "echo never")]))?,
        &options,
        &AdapterRegistry::empty(),
        &mut recorder,
        JobCancel::new(),
    )
    .await;
    Ok((finished, recorder))
}

#[tokio::test]
async fn a_workspace_that_is_not_utf8_fails_the_job_before_any_step() -> Result<()> {
    let scratch = Scratch::new()?;
    let mut options = scratch.options();
    options.workspace = scratch
        .workspace()
        .join(OsStr::from_bytes(b"not-\xff-utf8"));
    let (finished, recorder) = rejected_for(options).await?;
    assert_eq!(finished.result, JobResult::Failed);
    assert!(finished.steps.is_empty());
    assert!(
        recorder
            .lines
            .iter()
            .any(|line| line.starts_with("the workspace path is not valid UTF-8")),
        "{:?}",
        recorder.lines
    );
    Ok(())
}

#[tokio::test]
async fn a_temp_that_is_not_utf8_fails_the_job_before_any_step() -> Result<()> {
    let scratch = Scratch::new()?;
    let mut options = scratch.options();
    options.temp = options.temp.join(OsStr::from_bytes(b"not-\xff-utf8"));
    let (finished, recorder) = rejected_for(options).await?;
    assert_eq!(finished.result, JobResult::Failed);
    assert!(finished.steps.is_empty());
    assert!(
        recorder
            .lines
            .iter()
            .any(|line| line.starts_with("the temp path is not valid UTF-8")),
        "{:?}",
        recorder.lines
    );
    Ok(())
}
