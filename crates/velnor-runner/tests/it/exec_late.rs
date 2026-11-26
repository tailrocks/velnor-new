// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};
use serde_json::json;
use velnor_runner::exec::{AdapterRegistry, AnnotationLevel, JobCancel, JobResult, StepResult};

use crate::exec_builder::{execute, expression, job, literal, mapping, run, with};
use crate::exec_support::Scratch;

#[tokio::test]
async fn a_timeout_that_is_not_a_number_is_annotated_and_the_step_runs() -> Result<()> {
    let timed = with(run("a", "echo ran"), "TimeoutInMinutes", json!("abc"));
    let (finished, _) = execute(
        &job(vec![timed]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(finished.steps[0].outcome, StepResult::Succeeded);
    assert!(
        finished.annotations.iter().any(|annotation| {
            annotation.level == AnnotationLevel::Error
                && annotation.message
                    == "An error occurred when attempting to determine the step timeout."
        }),
        "{:?}",
        finished.annotations
    );
    Ok(())
}

#[tokio::test]
async fn stop_commands_resume_with_unicode_case_folding() -> Result<()> {
    let script = "echo '::stop-commands::ÄBCDEFG'\necho '::error::paused'\necho '::äbcdefg::'\necho '::error::resumed'";
    let (finished, _) = execute(
        &job(vec![run("a", script)]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    let messages: Vec<&str> = finished
        .annotations
        .iter()
        .map(|annotation| annotation.message.as_str())
        .collect();
    assert_eq!(messages, vec!["resumed"]);
    Ok(())
}

#[tokio::test]
async fn a_job_output_that_does_not_evaluate_is_annotated() -> Result<()> {
    let mut message = job(vec![run("a", "echo hi")]);
    message["JobOutputs"] = mapping(&[("x", expression("fromJSON(env.X)"))]);
    let (finished, _) = execute(
        &message,
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(finished.result, JobResult::Failed);
    assert!(
        finished.annotations.iter().any(|annotation| {
            annotation.level == AnnotationLevel::Error
                && annotation
                    .message
                    .contains("unsupported expression function fromJSON")
        }),
        "{:?}",
        finished.annotations
    );
    Ok(())
}

#[tokio::test]
async fn a_shell_that_depends_on_a_step_output_is_checked_when_its_step_runs() -> Result<()> {
    let mut late = run("b", "echo late-ran");
    if let Some(entries) = late["Inputs"]["map"].as_array_mut() {
        entries.push(json!({"key": literal("shell"), "value": expression("format('{0} -e {{0}}', steps.a.outputs.sh)")}));
    }
    let (finished, lines) = execute(
        &job(vec![
            run("a", "echo \"sh=bash\" >> \"$GITHUB_OUTPUT\""),
            late,
        ]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(finished.result, JobResult::Succeeded, "{lines:?}");
    assert!(lines.iter().any(|line| line == "late-ran"), "{lines:?}");
    Ok(())
}

#[tokio::test]
async fn annotation_messages_are_cut_at_4096_utf16_units() -> Result<()> {
    let script = "printf '::error::'\nfor _ in $(seq 3000); do printf '😀'; done\necho";
    let (finished, _) = execute(
        &job(vec![run("a", script)]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    let message = finished
        .annotations
        .iter()
        .find(|annotation| annotation.message.starts_with('😀'))
        .context("the annotation")?;
    assert_eq!(message.message.encode_utf16().count(), 4096);
    Ok(())
}
