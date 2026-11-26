// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};
use futures_util::FutureExt;
use serde_json::{Value, json};
use velnor_runner::exec::{
    AdapterRegistry, AnnotationLevel, ExecError, JobCancel, JobResult, StepAdapter, StepContext,
    StepFuture, StepResult,
};

use crate::exec_builder::{execute, expression, job, literal, mapping, run, step, with};
use crate::exec_support::Scratch;

fn repository(mut uses: Value, name: &str, kind: &str) -> Value {
    uses["Reference"] =
        json!({"Type": "repository", "Name": name, "Ref": "v1", "RepositoryType": kind});
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

struct Reporter {
    reference: &'static str,
    repository: Option<&'static str>,
    git_ref: Option<&'static str>,
}

impl StepAdapter for Reporter {
    fn handles(&self, reference: &str) -> bool {
        reference == self.reference
    }

    fn run<'a>(&'a self, ctx: &'a mut StepContext<'_>) -> StepFuture<'a> {
        async move {
            let seen = |name: &str| {
                ctx.env
                    .get(name)
                    .map(String::as_str)
                    .filter(|value| !value.is_empty())
            };
            let matches = seen("GITHUB_ACTION_REPOSITORY") == self.repository
                && seen("GITHUB_ACTION_REF") == self.git_ref;
            Ok(if matches {
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
    registry.register(Box::new(Reporter {
        reference: "actions/ctx@v1",
        repository: Some("actions/ctx"),
        git_ref: Some("v1"),
    }));
    registry.register(Box::new(Reporter {
        reference: "actions/own@v1",
        repository: None,
        git_ref: None,
    }));
    let (finished, lines) = execute(
        &job(vec![
            repository(step("a", literal("unused")), "actions/ctx", "GitHub"),
            repository(step("own", literal("unused")), "actions/own", "self"),
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
    assert_eq!(finished.steps[1].outcome, StepResult::Succeeded);
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
            repository(step("a", literal("unused")), "actions/foo", "GitHub"),
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
    let script = "echo \"::error file=$GITHUB_WORKSPACE/src/main.rs,line=5,endLine=3,col=2::reversed\"\necho '::error endLine=7,col=3::promoted'\necho '::error file=/elsewhere/a.rs::outside'\necho '::error line=1,col=5,endColumn=3::columns'";
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
    let columns = find("columns")?;
    assert_eq!(
        (columns.line, columns.column, columns.end_column),
        (Some(1), None, None)
    );
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
        !lines.iter().any(|line| line.contains("lone-cr-secret")),
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
async fn github_path_entries_end_at_a_lone_carriage_return() -> Result<()> {
    let (finished, lines) = execute(
        &job(vec![
            run("a", "printf '/opt/one\\r/opt/two\\n' >> \"$GITHUB_PATH\""),
            run("b", "echo \"path=$PATH\""),
        ]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(finished.result, JobResult::Succeeded);
    let path = lines
        .iter()
        .find_map(|line| line.strip_prefix("path="))
        .context("the PATH line")?;
    let entries: Vec<&str> = path.split(':').take(2).collect();
    assert_eq!(entries, ["/opt/two", "/opt/one"], "{path}");
    Ok(())
}
