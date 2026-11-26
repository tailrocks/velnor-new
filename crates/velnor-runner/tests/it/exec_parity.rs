// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Command;

use anyhow::{Context, Result};
use futures_util::FutureExt;
use serde_json::{Value, json};
use velnor_runner::exec::{
    AdapterRegistry, AnnotationLevel, JobCancel, JobResult, StepAdapter, StepContext, StepFuture,
    StepResult,
};

use crate::exec_builder::{
    binary, execute, finished, job, literal, mapping, run, run_message, step, with, write_job,
};
use crate::exec_support::Scratch;

fn secrets(message: &mut Value, pairs: &[(&str, &str)]) {
    let variables: serde_json::Map<String, Value> = pairs
        .iter()
        .map(|(name, value)| {
            (
                (*name).to_owned(),
                json!({"Value": value, "IsSecret": true}),
            )
        })
        .collect();
    message["Variables"] = Value::Object(variables);
}

#[tokio::test]
async fn workflow_command_property_names_are_case_insensitive() -> Result<()> {
    let message = job(vec![run(
        "a",
        "echo '::error Title=T,File=src/main.rs,Line=3,EndLine=3,Col=5,EndColumn=6::boom'",
    )]);
    let (finished, _) = execute(
        &message,
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    let annotation = finished
        .annotations
        .iter()
        .find(|annotation| annotation.message == "boom")
        .context("the error annotation")?;
    assert_eq!(annotation.title.as_deref(), Some("T"));
    assert_eq!(annotation.file.as_deref(), Some("src/main.rs"));
    assert_eq!(
        (
            annotation.line,
            annotation.end_line,
            annotation.column,
            annotation.end_column
        ),
        (Some(3), Some(3), Some(5), Some(6))
    );
    Ok(())
}

#[test]
fn overlapping_secrets_are_masked_as_one_range() -> Result<()> {
    let mut message = job(vec![run("a", "echo ABCDEFGHI\necho aaaa-aaa-aa")]);
    secrets(
        &mut message,
        &[("ONE", "ABCDE"), ("TWO", "EFGHI"), ("THREE", "aa")],
    );
    let ran = run_message(&Scratch::new()?, &message)?;
    assert!(ran.has_line("***"), "{}", ran.stdout);
    assert!(ran.has_line("***-***-***"), "{}", ran.stdout);
    Ok(())
}

#[tokio::test]
async fn a_secret_across_the_line_limit_is_masked_before_the_cut() -> Result<()> {
    let hidden = "Zq9xK2mW7pQ4";
    let script =
        format!("head -c 1048573 /dev/zero | tr '\\0' a\nprintf '%s' '{hidden}'\necho bbbbbbbbbb");
    let mut message = job(vec![run("a", &script)]);
    secrets(&mut message, &[("KEY", hidden)]);
    let (finished, lines) = execute(
        &message,
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(finished.result, JobResult::Succeeded);
    let long = lines
        .iter()
        .find(|line| line.starts_with("aaaa"))
        .context("the long line")?;
    assert!(!long.contains("Zq9"), "a secret prefix reached the log");
    assert!(
        long.ends_with("*** [line truncated at 1048576 bytes]"),
        "{}",
        &long[long.len() - 60..]
    );
    Ok(())
}

struct Reader;

impl StepAdapter for Reader {
    fn handles(&self, reference: &str) -> bool {
        reference == "actions/foo@v1"
    }

    fn run<'a>(&'a self, ctx: &'a mut StepContext<'_>) -> StepFuture<'a> {
        async move {
            let found = ctx.inputs.get("token").map(String::as_str) == Some("abc")
                && ctx.inputs.get("enablecrossosarchive").map(String::as_str) == Some("yes");
            Ok(if found {
                StepResult::Succeeded
            } else {
                StepResult::Failed
            })
        }
        .boxed()
    }
}

#[tokio::test]
async fn adapter_inputs_are_found_whatever_their_case() -> Result<()> {
    let mut uses = step("foo", literal("unused"));
    uses["Reference"] = json!({"Type": "repository", "Name": "actions/foo", "Ref": "v1", "RepositoryType": "GitHub"});
    uses["Inputs"] = mapping(&[
        ("Token", literal("abc")),
        ("enableCrossOsArchive", literal("yes")),
    ]);
    let mut registry = AdapterRegistry::empty();
    registry.register(Box::new(Reader));
    let (finished, _) = execute(
        &job(vec![
            uses,
            run("show", "echo seen=${{ steps.foo.outputs.seen }}"),
        ]),
        &Scratch::new()?,
        &registry,
        JobCancel::new(),
    )
    .await?;
    assert_eq!(finished.result, JobResult::Succeeded);
    assert_eq!(finished.steps[0].outcome, StepResult::Succeeded);
    Ok(())
}

#[test]
fn the_default_workspace_root_is_private() -> Result<()> {
    let scratch = Scratch::new()?;
    let message = job(vec![run(
        "a",
        "ls -ld \"$(dirname \"$GITHUB_WORKSPACE\")\" \"$GITHUB_WORKSPACE\" | cut -c1-10",
    )]);
    let path = write_job(&scratch, &message)?;
    let inner = binary(&path);
    let mut wrapped = Command::new("sh");
    wrapped
        .arg("-c")
        .arg("umask 022; exec \"$0\" \"$@\"")
        .arg(inner.get_program())
        .args(inner.get_args())
        .arg("--temp")
        .arg(scratch.options().temp);
    let ran = finished(wrapped)?;
    let private = ran
        .stdout
        .lines()
        .filter(|line| *line == "drwx------")
        .count();
    assert_eq!(private, 2, "{}", ran.stdout);
    Ok(())
}

#[tokio::test]
async fn a_step_condition_sees_the_step_environment() -> Result<()> {
    let guarded = with(
        with(
            run("a", "echo ran"),
            "Environment",
            mapping(&[("RUN_CHECK", literal("true"))]),
        ),
        "Condition",
        json!("env.RUN_CHECK == 'true'"),
    );
    let (finished, _) = execute(
        &job(vec![guarded]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(finished.steps[0].outcome, StepResult::Succeeded);
    Ok(())
}

#[test]
fn runner_debug_follows_the_step_debug_variable() -> Result<()> {
    let probe = "echo \"debug=$RUNNER_DEBUG\"";
    let plain = run_message(&Scratch::new()?, &job(vec![run("a", probe)]))?;
    assert!(plain.has_line("debug="), "{}", plain.stdout);
    let mut message = job(vec![
        run("a", probe),
        with(
            run("b", "echo cond"),
            "Condition",
            json!("runner.debug == '1'"),
        ),
    ]);
    message["Variables"] = json!({"ACTIONS_STEP_DEBUG": {"Value": "true"}});
    let debug = run_message(&Scratch::new()?, &message)?;
    assert!(debug.has_line("debug=1"), "{}", debug.stdout);
    assert!(debug.has_line("cond"), "{}", debug.stdout);
    Ok(())
}

#[test]
fn a_custom_shell_without_a_placeholder_is_rejected_before_any_step() -> Result<()> {
    let mut shell_step = run("a", "echo should-not-run");
    if let Some(entries) = shell_step["Inputs"]["map"].as_array_mut() {
        entries.push(json!({"key": literal("shell"), "value": literal("bash -e")}));
    }
    let ran = run_message(&Scratch::new()?, &job(vec![shell_step]))?;
    assert!(ran.has_line("unsupported: shell bash -e"), "{}", ran.stdout);
    assert!(!ran.stdout.contains("should-not-run"), "{}", ran.stdout);
    assert_eq!(ran.code, Some(1));
    Ok(())
}

#[tokio::test]
async fn a_step_timeout_is_an_error_annotation() -> Result<()> {
    let timed = with(run("a", "sleep 30"), "TimeoutInMinutes", json!(0.02));
    let (finished, _) = execute(
        &job(vec![timed]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert!(
        finished.annotations.iter().any(|annotation| {
            annotation.level == AnnotationLevel::Error
                && annotation.message == "step 1 timed out after 0.02 minutes"
        }),
        "{:?}",
        finished.annotations
    );
    Ok(())
}
