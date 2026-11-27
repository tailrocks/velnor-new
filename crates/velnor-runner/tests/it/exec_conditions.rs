// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;
use serde_json::json;
use velnor_runner::exec::{AdapterRegistry, JobCancel, JobResult, StepResult};

use crate::exec_builder::{execute, expression, job, literal, mapping, run, with};
use crate::exec_support::Scratch;

fn keep_going() -> serde_json::Value {
    json!({"type": 5, "bool": true})
}

#[tokio::test]
async fn a_condition_that_does_not_evaluate_fails_the_step_whatever_continue_on_error_says()
-> Result<()> {
    let broken = with(
        with(
            run("a", "echo never"),
            "Condition",
            json!("fromJSON(env.X)"),
        ),
        "ContinueOnError",
        keep_going(),
    );
    let (finished, lines) = execute(
        &job(vec![broken]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(finished.result, JobResult::Failed, "{lines:?}");
    assert_eq!(finished.steps[0].outcome, StepResult::Failed);
    assert_eq!(finished.steps[0].conclusion, StepResult::Failed);
    assert!(!lines.iter().any(|line| line == "never"), "{lines:?}");
    Ok(())
}

#[tokio::test]
async fn a_string_valued_continue_on_error_expression_is_not_a_boolean() -> Result<()> {
    let failing = with(
        with(
            run("a", "exit 1"),
            "Environment",
            mapping(&[("ALLOW", literal("true"))]),
        ),
        "ContinueOnError",
        expression("env.ALLOW"),
    );
    let (finished, _) = execute(
        &job(vec![failing]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(finished.result, JobResult::Failed);
    assert_eq!(finished.steps[0].conclusion, StepResult::Failed);
    assert!(
        finished.annotations.iter().any(|annotation| annotation
            .message
            .starts_with("The step failed and an error occurred when attempting to determine whether to continue on error.")),
        "{:?}",
        finished.annotations
    );
    Ok(())
}

#[tokio::test]
async fn a_boolean_continue_on_error_expression_keeps_the_job_green() -> Result<()> {
    let failing = with(run("a", "exit 1"), "ContinueOnError", expression("1 == 1"));
    let (finished, _) = execute(
        &job(vec![failing]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(finished.result, JobResult::Succeeded);
    assert_eq!(finished.steps[0].conclusion, StepResult::Succeeded);
    Ok(())
}
