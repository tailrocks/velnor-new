// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;
use serde_json::{Value, json};
use velnor_runner::exec::{AdapterRegistry, AnnotationLevel, JobCancel, JobResult, StepResult};

use crate::exec_builder::{execute, expression, job, literal, mapping, run, with};
use crate::exec_support::Scratch;

const FROM_ENV: &str = "format('{0} -e {{0}}', env.SH)";
const FROM_OUTPUT: &str = "format('{0} -e {{0}}', steps.a.outputs.sh)";

fn shelled(context: &str, script: &str, shell: &str) -> Value {
    let mut step = run(context, script);
    if let Some(entries) = step["Inputs"]["map"].as_array_mut() {
        entries.push(json!({"key": literal("shell"), "value": expression(shell)}));
    }
    step
}

#[tokio::test]
async fn a_shell_from_github_env_is_checked_when_its_step_runs() -> Result<()> {
    let (finished, lines) = execute(
        &job(vec![
            run("a", "echo \"SH=bash\" >> \"$GITHUB_ENV\""),
            shelled("b", "echo late-ran", FROM_ENV),
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
async fn a_shell_from_a_step_environment_reading_outputs_is_checked_when_its_step_runs()
-> Result<()> {
    let late = with(
        shelled("b", "echo late-ran", FROM_ENV),
        "Environment",
        mapping(&[("SH", expression("steps.a.outputs.sh"))]),
    );
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
async fn an_unsupported_shell_from_a_step_output_fails_its_step_without_running_it() -> Result<()> {
    let (finished, lines) = execute(
        &job(vec![
            run("a", "echo \"sh=pwsh\" >> \"$GITHUB_OUTPUT\""),
            shelled("b", "echo late-ran", FROM_OUTPUT),
        ]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(finished.result, JobResult::Failed);
    assert_eq!(finished.steps[0].outcome, StepResult::Succeeded);
    assert_eq!(finished.steps[1].outcome, StepResult::Failed);
    assert!(
        finished.annotations.iter().any(|annotation| {
            annotation.level == AnnotationLevel::Error
                && annotation.message == "unsupported: shell pwsh -e {0}"
        }),
        "{:?}",
        finished.annotations
    );
    assert!(!lines.iter().any(|line| line == "late-ran"), "{lines:?}");
    Ok(())
}
