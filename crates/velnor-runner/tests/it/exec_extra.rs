// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use anyhow::{Context, Result};
use futures_util::FutureExt;
use serde_json::json;
use velnor_runner::exec::{
    AdapterRegistry, AnnotationLevel, JobCancel, JobResult, StdoutSink, StepAdapter, StepContext,
    StepFuture, StepResult, run_job,
};
use velnor_runner::job_message::AgentJobRequestMessage;

use crate::exec_builder::{job, run};
use crate::exec_support::{Scratch, fixture_message, fixture_path, run_fixture, run_path};
use crate::lifecycle_support::job_replacing;

const HOST_OS: &str = if cfg!(target_os = "macos") {
    "macOS"
} else {
    "Linux"
};

#[test]
fn shell_forms_expand_like_github() -> Result<()> {
    let ran = run_fixture("shell-forms", &Scratch::new()?)?;
    for expected in [
        "bare",
        "continued-after-false",
        "default-shell-continues",
        "step 4 failed",
        "job failed",
    ] {
        assert!(ran.has_line(expected), "{expected}\n{}", ran.stdout);
    }
    assert!(!ran.has_line("not-reached"));
    assert_eq!(ran.code, Some(1));
    Ok(())
}

#[test]
fn github_context_exports_env_and_event_file() -> Result<()> {
    let scratch = Scratch::new()?;
    let ran = run_fixture("github-context", &scratch)?;
    assert!(
        ran.has_line(
            &format!(
                "repo=tailrocks/velnor-new event=pull_request protected=false runner=self-hosted job=build os={HOST_OS} run=42"
            )
        ),
        "{}",
        ran.stdout
    );
    let event = ran
        .lines()
        .into_iter()
        .find(|line| line.starts_with('{'))
        .context("the printed event.json")?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(event)?,
        serde_json::json!({"pull_request": {"number": 7}})
    );
    let workspace = scratch.workspace();
    let expected = format!(
        "token=*** ws={} env=self-hosted",
        workspace.to_string_lossy()
    );
    assert!(ran.has_line(&expected), "{}", ran.stdout);
    assert_eq!(ran.code, Some(0));
    Ok(())
}

#[test]
fn missing_job_file_is_a_usage_error() -> Result<()> {
    let ran = run_path(Path::new("/nonexistent.json"), &Scratch::new()?)?;
    assert_eq!(ran.stdout, "");
    assert!(ran.stderr.contains("/nonexistent.json"), "{}", ran.stderr);
    assert_eq!(ran.code, Some(2));
    Ok(())
}

#[tokio::test]
async fn annotations_carry_properties_and_exit_code() -> Result<()> {
    let scratch = Scratch::new()?;
    let run = run_job(
        &fixture_message("annotations")?,
        &scratch.options(),
        &AdapterRegistry::empty(),
        &mut StdoutSink,
        JobCancel::new(),
    )
    .await;
    assert_eq!(run.result, JobResult::Failed);
    let error = run
        .annotations
        .iter()
        .find(|annotation| annotation.title.as_deref() == Some("T:1"))
        .context("the error annotation with its title")?;
    assert_eq!(error.level, AnnotationLevel::Error);
    assert_eq!(error.message, "bad\nthing");
    assert_eq!(error.file.as_deref(), Some("a.rs"));
    assert_eq!(error.line, Some(3));
    let levels: Vec<(AnnotationLevel, &str)> = run
        .annotations
        .iter()
        .map(|annotation| (annotation.level, annotation.message.as_str()))
        .collect();
    assert!(levels.contains(&(AnnotationLevel::Notice, "fyi")));
    assert!(levels.contains(&(AnnotationLevel::Warning, "careful")));
    assert!(levels.contains(&(
        AnnotationLevel::Error,
        "Process completed with exit code 1."
    )));
    Ok(())
}

#[test]
fn working_directory_input_and_default_resolve_under_workspace() -> Result<()> {
    let ran = run_fixture("working-directory", &Scratch::new()?)?;
    let lines = ran.lines();
    assert!(
        lines.iter().any(|line| line.ends_with("/workspace/sub")),
        "{}",
        ran.stdout
    );
    assert!(
        lines
            .iter()
            .any(|line| line.ends_with("/workspace/sub/dir")),
        "{}",
        ran.stdout
    );
    assert_eq!(ran.code, Some(0));
    Ok(())
}

#[test]
fn env_precedence_job_step_and_github_env() -> Result<()> {
    let ran = run_fixture("env-precedence", &Scratch::new()?)?;
    assert!(ran.has_line("job step job"), "{}", ran.stdout);
    assert!(ran.has_line("written-written"), "{}", ran.stdout);
    assert_eq!(ran.code, Some(0));
    Ok(())
}

#[tokio::test]
async fn step_summary_is_saved_masked() -> Result<()> {
    let scratch = Scratch::new()?;
    let run = run_job(
        &fixture_message("step-summary")?,
        &scratch.options(),
        &AdapterRegistry::empty(),
        &mut StdoutSink,
        JobCancel::new(),
    )
    .await;
    assert_eq!(run.result, JobResult::Succeeded);
    let path = run.steps[0]
        .summary_path
        .as_ref()
        .context("the first step has a summary")?;
    assert_eq!(std::fs::read_to_string(path)?, "secret is ***\n");
    assert!(run.steps[1].summary_path.is_none());
    Ok(())
}

struct Greeter;

impl StepAdapter for Greeter {
    fn handles(&self, reference: &str) -> bool {
        reference == "actions/foo@v1"
    }

    fn run<'a>(&'a self, ctx: &'a mut StepContext<'_>) -> StepFuture<'a> {
        async move {
            let who = ctx.inputs.get("who").cloned().unwrap_or_default();
            ctx.log(&format!("greeting {who}"));
            ctx.set_output("greeting", format!("hello-{who}"));
            Ok(StepResult::Succeeded)
        }
        .boxed()
    }
}

#[tokio::test]
async fn registered_adapter_runs_uses_step_and_feeds_outputs() -> Result<()> {
    let scratch = Scratch::new()?;
    let mut adapters = AdapterRegistry::empty();
    adapters.register(Box::new(Greeter));
    let run = run_job(
        &fixture_message("adapter-uses")?,
        &scratch.options(),
        &adapters,
        &mut StdoutSink,
        JobCancel::new(),
    )
    .await;
    assert_eq!(run.result, JobResult::Succeeded);
    assert_eq!(run.steps[0].name, "Greet");
    assert_eq!(run.steps[0].context_id.as_deref(), Some("foo"));
    assert_eq!(run.steps[1].outcome, StepResult::Succeeded);
    Ok(())
}

#[tokio::test]
async fn refusal_annotations_are_masked() -> Result<()> {
    let scratch = Scratch::new()?;
    let body = std::fs::read_to_string(fixture_path("unsupported-uses"))?.replace(
        "\"Steps\": [",
        "\"Variables\": {\"HIDDEN\": {\"Value\": \"actions/foo@v1\", \"IsSecret\": true}},\n  \"Steps\": [",
    );
    let run = run_job(
        &AgentJobRequestMessage::parse_json(&body)?,
        &scratch.options(),
        &AdapterRegistry::empty(),
        &mut StdoutSink,
        JobCancel::new(),
    )
    .await;
    assert_eq!(run.result, JobResult::Failed);
    let messages: Vec<&str> = run
        .annotations
        .iter()
        .map(|annotation| annotation.message.as_str())
        .collect();
    assert_eq!(messages, ["unsupported: uses ***"]);
    Ok(())
}

#[tokio::test]
async fn workflow_command_annotations_mask_message_title_and_file() -> Result<()> {
    let scratch = Scratch::new()?;
    let body = std::fs::read_to_string(fixture_path("annotations"))?
        .replace(
            "\"Steps\": [",
            "\"Variables\": {\"HIDDEN\": {\"Value\": \"sekret-token\", \"IsSecret\": true}},\n  \"Steps\": [",
        )
        .replace("T%3A1,file=a.rs", "t-sekret-token,file=sekret-token.rs")
        .replace("bad%0Athing", "bad sekret-token");
    let run = run_job(
        &AgentJobRequestMessage::parse_json(&body)?,
        &scratch.options(),
        &AdapterRegistry::empty(),
        &mut StdoutSink,
        JobCancel::new(),
    )
    .await;
    let error = run
        .annotations
        .iter()
        .find(|annotation| annotation.level == AnnotationLevel::Error && annotation.line == Some(3))
        .context("the error annotation")?;
    assert_eq!(error.message, "bad ***");
    assert_eq!(error.title.as_deref(), Some("t-***"));
    assert_eq!(error.file.as_deref(), Some("***.rs"));
    Ok(())
}

#[tokio::test]
async fn annotation_title_and_file_are_masked_like_the_message() -> Result<()> {
    const SECRET: &str = "s3cr3t-Value-12345";
    let scratch = Scratch::new()?;
    let mut message = job(vec![run(
        "s",
        &format!("echo '::error title={SECRET},file={SECRET}::{SECRET}'"),
    )]);
    message["Variables"] = json!({"MY_SECRET": {"Value": SECRET, "IsSecret": true}});
    let message: AgentJobRequestMessage = serde_json::from_value(message)?;

    let ran = run_job(
        &message,
        &scratch.options(),
        &AdapterRegistry::empty(),
        &mut StdoutSink,
        JobCancel::new(),
    )
    .await;

    let annotation = ran.annotations.first().context("the error annotation")?;
    assert_eq!(annotation.message, "***");
    assert_eq!(annotation.title.as_deref(), Some("***"));
    assert_eq!(annotation.file.as_deref(), Some("***"));
    Ok(())
}

#[tokio::test]
async fn preflight_annotations_are_masked_like_the_log() -> Result<()> {
    const SECRET: &str = "s3cr3t-Value-12345";
    let scratch = Scratch::new()?;
    let mut job = job_replacing(
        "unsupported-shell",
        Some(("pwsh {0}", "pwsh-s3cr3t-Value-12345 {0}")),
    )?;
    job["Variables"] = json!({"MY_SECRET": {"Value": SECRET, "IsSecret": true}});
    let message: AgentJobRequestMessage = serde_json::from_value(job)?;

    let ran = run_job(
        &message,
        &scratch.options(),
        &AdapterRegistry::empty(),
        &mut StdoutSink,
        JobCancel::new(),
    )
    .await;

    let annotation = ran
        .annotations
        .first()
        .context("the preflight annotation")?;
    assert_eq!(annotation.message, "unsupported: shell pwsh-*** {0}");
    Ok(())
}
