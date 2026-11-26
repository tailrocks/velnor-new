// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

use anyhow::{Context, Result};
use futures_util::FutureExt;
use serde_json::{Value, json};
use velnor_runner::exec::{
    AdapterRegistry, AnnotationLevel, ExecError, JobCancel, JobResult, RunOptions, StepAdapter,
    StepContext, StepFuture, StepResult, run_job,
};

use crate::exec_builder::{
    Recorder, execute, expression, job, literal, mapping, parse, run, step, with,
};
use crate::exec_support::Scratch;

fn repository(mut uses: Value, name: &str) -> Value {
    uses["Reference"] =
        json!({"Type": "repository", "Name": name, "Ref": "v1", "RepositoryType": "GitHub"});
    uses
}

struct Failing;

impl StepAdapter for Failing {
    fn handles(&self, reference: &str) -> bool {
        reference == "actions/foo@v1"
    }

    fn run<'a>(&'a self, ctx: &'a mut StepContext<'_>) -> StepFuture<'a> {
        async move {
            if let Some(file) = ctx.env.get("GITHUB_ENV") {
                let _ = std::fs::write(file, "FOO=bar\n");
            }
            Err(ExecError::Invalid("adapter boom".to_owned()))
        }
        .boxed()
    }
}

struct Reporter;

impl StepAdapter for Reporter {
    fn handles(&self, reference: &str) -> bool {
        reference == "actions/ctx@v1"
    }

    fn run<'a>(&'a self, ctx: &'a mut StepContext<'_>) -> StepFuture<'a> {
        async move {
            let seen = ctx.env.get("GITHUB_ACTION_REPOSITORY").map(String::as_str)
                == Some("actions/ctx")
                && ctx.env.get("GITHUB_ACTION_REF").map(String::as_str) == Some("v1");
            Ok(if seen {
                StepResult::Succeeded
            } else {
                StepResult::Failed
            })
        }
        .boxed()
    }
}

#[tokio::test]
async fn repository_action_variables_follow_the_step() -> Result<()> {
    let mut registry = AdapterRegistry::empty();
    registry.register(Box::new(Reporter));
    let (finished, lines) = execute(
        &job(vec![
            repository(step("a", literal("unused")), "actions/ctx"),
            run(
                "b",
                "echo \"repo=[$GITHUB_ACTION_REPOSITORY] ref=[$GITHUB_ACTION_REF]\"",
            ),
        ]),
        &Scratch::new()?,
        &registry,
        JobCancel::new(),
    )
    .await?;
    assert_eq!(finished.steps[0].outcome, StepResult::Succeeded);
    assert!(
        lines.iter().any(|line| line == "repo=[] ref=[]"),
        "{lines:?}"
    );
    Ok(())
}

#[tokio::test]
async fn a_failed_adapter_still_has_its_file_commands_applied() -> Result<()> {
    let mut registry = AdapterRegistry::empty();
    registry.register(Box::new(Failing));
    let after = with(
        run("b", "echo \"foo=$FOO repo=[$GITHUB_ACTION_REPOSITORY]\""),
        "Condition",
        json!("always()"),
    );
    let (finished, lines) = execute(
        &job(vec![
            repository(step("a", literal("unused")), "actions/foo"),
            after,
        ]),
        &Scratch::new()?,
        &registry,
        JobCancel::new(),
    )
    .await?;
    assert!(
        lines.iter().any(|line| line == "foo=bar repo=[]"),
        "{lines:?}"
    );
    assert!(
        finished.annotations.iter().any(|annotation| {
            annotation.level == AnnotationLevel::Error && annotation.message == "adapter boom"
        }),
        "{:?}",
        finished.annotations
    );
    Ok(())
}

#[tokio::test]
async fn annotation_files_are_relative_and_ranges_are_validated() -> Result<()> {
    let script = "echo \"::error file=$GITHUB_WORKSPACE/src/main.rs,line=5,endLine=3,col=2::reversed\"\necho '::error endLine=7,col=3::promoted'\necho '::error file=/elsewhere/a.rs::outside'";
    let (finished, _) = execute(
        &job(vec![run("a", script)]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    let find = |message: &str| {
        finished
            .annotations
            .iter()
            .find(|annotation| annotation.message == message)
            .with_context(|| message.to_owned())
    };
    let reversed = find("reversed")?;
    assert_eq!(reversed.file.as_deref(), Some("src/main.rs"));
    assert_eq!(
        (reversed.line, reversed.end_line, reversed.column),
        (None, None, None)
    );
    let promoted = find("promoted")?;
    assert_eq!(
        (promoted.line, promoted.end_line, promoted.column),
        (Some(7), Some(7), Some(3))
    );
    assert_eq!(find("outside")?.file.as_deref(), Some("/elsewhere/a.rs"));
    Ok(())
}

#[tokio::test]
async fn an_unregistered_modern_prefix_does_not_hide_a_legacy_command() -> Result<()> {
    let script =
        "echo '::unknown::prefix ##[add-mask]legacy-hidden-value'\necho legacy-hidden-value";
    let (_, lines) = execute(
        &job(vec![run("a", script)]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert!(lines.iter().any(|line| line == "***"), "{lines:?}");
    assert!(
        !lines
            .iter()
            .skip(1)
            .any(|line| line.contains("legacy-hidden-value")),
        "{lines:?}"
    );
    Ok(())
}

#[tokio::test]
async fn a_quoted_secret_with_wide_characters_registers_its_unquoted_form() -> Result<()> {
    let mut message = job(vec![run("a", "echo 😀😀😀😀")]);
    message["Variables"] = json!({"EMOJI": {"Value": "\"😀😀😀😀\"", "IsSecret": true}});
    let (_, lines) = execute(
        &message,
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert!(lines.iter().any(|line| line == "***"), "{lines:?}");
    Ok(())
}

#[tokio::test]
async fn job_environment_tokens_see_the_ones_before_them() -> Result<()> {
    let mut message = job(vec![run("a", "echo \"bar=$BAR\"")]);
    message["EnvironmentVariables"] = json!([
        mapping(&[("FOO", literal("a"))]),
        mapping(&[("BAR", expression("format('{0}-x', env.FOO)"))])
    ]);
    let (_, lines) = execute(
        &message,
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert!(lines.iter().any(|line| line == "bar=a-x"), "{lines:?}");
    Ok(())
}

#[tokio::test]
async fn a_lone_carriage_return_ends_an_output_line() -> Result<()> {
    let script = "printf 'ordinary\\r::add-mask::lone-cr-secret\\nlone-cr-secret\\n'";
    let (_, lines) = execute(
        &job(vec![run("a", script)]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert!(lines.iter().any(|line| line == "ordinary"), "{lines:?}");
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("lone-cr-secret") && !line.starts_with("::")),
        "{lines:?}"
    );
    Ok(())
}

#[tokio::test]
async fn a_disabled_step_is_not_checked_by_preflight() -> Result<()> {
    let mut disabled = run("a", "echo never");
    if let Some(entries) = disabled["Inputs"]["map"].as_array_mut() {
        entries.push(json!({"key": literal("shell"), "value": literal("pwsh {0}")}));
    }
    let disabled = with(disabled, "Enabled", json!(false));
    let (finished, _) = execute(
        &job(vec![disabled, run("b", "echo ran")]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(finished.result, JobResult::Succeeded);
    assert_eq!(finished.steps[0].outcome, StepResult::Skipped);
    assert_eq!(finished.steps[1].outcome, StepResult::Succeeded);
    Ok(())
}

#[tokio::test]
async fn job_outputs_differing_by_case_keep_the_last_value() -> Result<()> {
    let mut message = job(vec![run("a", "echo hi")]);
    message["JobOutputs"] = mapping(&[("Foo", literal("a")), ("foo", literal("b"))]);
    let (finished, _) = execute(
        &message,
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    let outputs: Vec<(&str, &str)> = finished
        .outputs
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    assert_eq!(outputs, vec![("Foo", "b")]);
    Ok(())
}

#[tokio::test]
async fn paths_that_are_not_utf8_fail_the_job_before_any_step() -> Result<()> {
    let scratch = Scratch::new()?;
    let mut options: RunOptions = scratch.options();
    options.workspace = PathBuf::from(OsStr::from_bytes(b"/tmp/not-\xff-utf8"));
    let mut recorder = Recorder::default();
    let finished = run_job(
        &parse(&job(vec![run("a", "echo never")]))?,
        &options,
        &AdapterRegistry::empty(),
        &mut recorder,
        JobCancel::new(),
    )
    .await;
    assert_eq!(finished.result, JobResult::Failed);
    assert!(finished.steps.is_empty());
    assert!(
        recorder
            .lines
            .iter()
            .any(|line| line.contains("not valid UTF-8")),
        "{:?}",
        recorder.lines
    );
    Ok(())
}

#[tokio::test]
async fn evaluation_errors_of_a_step_are_annotated() -> Result<()> {
    let broken_condition = with(
        run("b", "echo never"),
        "Condition",
        json!("fromJSON(env.Y)"),
    );
    let (finished, _) = execute(
        &job(vec![
            step("a", expression("fromJSON(env.X)")),
            broken_condition,
        ]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    let errors: Vec<&str> = finished
        .annotations
        .iter()
        .filter(|annotation| annotation.level == AnnotationLevel::Error)
        .map(|annotation| annotation.message.as_str())
        .collect();
    assert!(
        errors
            .iter()
            .filter(|text| text.contains("unsupported expression function fromJSON"))
            .count()
            >= 2,
        "{errors:?}"
    );
    Ok(())
}

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
