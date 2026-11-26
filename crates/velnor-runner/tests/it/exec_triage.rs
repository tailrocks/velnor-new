// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};
use serde_json::{Value, json};
use velnor_runner::exec::{AdapterRegistry, AnnotationLevel, JobCancel, JobResult, StepResult};

use crate::exec_builder::{execute, expression, job, mapping, run, with};
use crate::exec_support::Scratch;

const BOM: &str = "\\357\\273\\277";

fn defaults_shell(shell: Value) -> Value {
    json!([mapping(&[("run", mapping(&[("shell", shell)]))])])
}

async fn run_steps(message: &Value) -> Result<(velnor_runner::exec::JobRun, Vec<String>)> {
    execute(
        message,
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await
}

#[tokio::test]
async fn an_evaluated_default_shell_that_is_unsupported_fails_the_job_without_running_a_step()
-> Result<()> {
    let message = with(
        job(vec![run("a", "echo ran")]),
        "Defaults",
        defaults_shell(expression("format('pw{0}', 'sh')")),
    );
    let (finished, lines) = run_steps(&message).await?;
    assert_eq!(finished.result, JobResult::Failed, "{lines:?}");
    assert!(finished.steps.is_empty(), "{:?}", finished.steps);
    assert!(!lines.iter().any(|line| line == "ran"), "{lines:?}");
    Ok(())
}

#[tokio::test]
async fn an_unevaluable_defaults_entry_fails_the_job_with_no_steps_and_an_annotation() -> Result<()>
{
    let message = with(
        job(vec![run("a", "echo ran")]),
        "Defaults",
        defaults_shell(expression("fromJSON('{bad')")),
    );
    let (finished, lines) = run_steps(&message).await?;
    assert_eq!(finished.result, JobResult::Failed, "{lines:?}");
    assert!(finished.steps.is_empty(), "{:?}", finished.steps);
    assert!(
        finished
            .annotations
            .iter()
            .any(|annotation| annotation.level == AnnotationLevel::Error),
        "{:?}",
        finished.annotations
    );
    Ok(())
}

#[tokio::test]
async fn a_stop_token_longer_than_six_utf16_units_is_masked() -> Result<()> {
    let script = "echo '::stop-commands::😀😀😀😀'\necho '::error::paused'\necho '::😀😀😀😀::'";
    let (_, lines) = run_steps(&job(vec![run("a", script)])).await?;
    assert!(
        !lines.iter().any(|line| line.contains("😀😀😀😀")),
        "{lines:?}"
    );
    Ok(())
}

#[tokio::test]
async fn a_summary_that_cannot_be_saved_fails_the_step_and_keeps_its_outputs() -> Result<()> {
    let script = "echo 'k=v' >> \"$GITHUB_OUTPUT\"\necho hi >> \"$GITHUB_STEP_SUMMARY\"\ntouch \"$RUNNER_TEMP/step_summary\"";
    let follow = with(
        run("b", "echo \"got=${{ steps.a.outputs.k }}\""),
        "Condition",
        json!("always()"),
    );
    let (finished, lines) = run_steps(&job(vec![run("a", script), follow])).await?;
    assert_eq!(finished.steps[0].outcome, StepResult::Failed, "{lines:?}");
    assert!(lines.iter().any(|line| line == "got=v"), "{lines:?}");
    Ok(())
}

#[tokio::test]
async fn a_byte_order_mark_does_not_hide_a_command_on_either_stream() -> Result<()> {
    let out = format!("printf '{BOM}::add-mask::stdout-bom-value\\nstdout-bom-value\\n'");
    let err = format!(
        "printf '{BOM}::add-mask::stderr-bom-value\\n' >&2\nsleep 1\necho stderr-bom-value"
    );
    let (_, lines) = run_steps(&job(vec![run("a", &out), run("b", &err)])).await?;
    assert!(
        !lines.iter().any(|line| line.contains("-bom-value\n")
            || line == "stdout-bom-value"
            || line == "stderr-bom-value"),
        "{lines:?}"
    );
    Ok(())
}

#[tokio::test]
async fn a_byte_order_mark_does_not_corrupt_the_first_github_path_entry() -> Result<()> {
    let first = format!(
        "mkdir -p \"$PWD/bin\"\nprintf '#!/bin/sh\\necho tool-ran\\n' > \"$PWD/bin/bomtool\"\nchmod +x \"$PWD/bin/bomtool\"\nprintf '{BOM}%s\\n' \"$PWD/bin\" >> \"$GITHUB_PATH\""
    );
    let (finished, lines) = run_steps(&job(vec![run("a", &first), run("b", "bomtool")])).await?;
    assert_eq!(finished.result, JobResult::Succeeded, "{lines:?}");
    assert!(lines.iter().any(|line| line == "tool-ran"), "{lines:?}");
    Ok(())
}

#[tokio::test]
async fn annotation_numbers_with_surrounding_whitespace_are_kept() -> Result<()> {
    let (finished, _) = run_steps(&job(vec![run("a", "echo '::error line= 5 ::failed'")])).await?;
    let annotation = finished
        .annotations
        .iter()
        .find(|annotation| annotation.message == "failed")
        .context("the annotation")?;
    assert_eq!(annotation.line, Some(5));
    Ok(())
}

#[tokio::test]
async fn secret_debug_flags_are_not_registered_as_masks() -> Result<()> {
    let mut message = job(vec![run("a", "echo flag=true")]);
    message["Variables"] = json!({
        "ACTIONS_STEP_DEBUG": {"Value": "true", "IsSecret": true},
        "actions_runner_debug": {"Value": "true", "IsSecret": true}
    });
    let (_, lines) = run_steps(&message).await?;
    assert!(lines.iter().any(|line| line == "flag=true"), "{lines:?}");
    Ok(())
}
