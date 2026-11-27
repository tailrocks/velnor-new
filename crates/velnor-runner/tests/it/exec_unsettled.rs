// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::FutureExt;
use serde_json::{Value, json};
use velnor_runner::exec::{
    AdapterRegistry, Inputs, JobCancel, JobResult, StepAdapter, StepContext, StepFuture, StepResult,
};

use crate::exec_builder::{
    bounded, execute, expression, finished, job, mapping, run, run_message, step, with,
};
use crate::exec_support::Scratch;

const BOM: &str = "\\357\\273\\277";

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
async fn two_byte_order_marks_on_github_output_leave_one_in_the_name() -> Result<()> {
    let write = format!("printf '{BOM}{BOM}k=v\\n' >> \"$GITHUB_OUTPUT\"");
    let read = step(
        "b",
        expression(
            "format('echo \"plain={0}|marked={1}\"', steps.a.outputs.k, steps.a.outputs['\u{feff}k'])",
        ),
    );
    let (_, lines) = run_steps(&job(vec![run("a", &write), read])).await?;
    assert!(
        lines.iter().any(|line| line == "plain=|marked=v"),
        "{lines:?}"
    );
    Ok(())
}

#[tokio::test]
async fn a_byte_order_mark_on_a_later_line_of_a_stream_is_kept() -> Result<()> {
    let script = format!("printf 'x\\n{BOM}::add-mask::v\\nv\\n'");
    let (_, lines) = run_steps(&job(vec![run("a", &script)])).await?;
    assert!(lines.iter().any(|line| line == "v"), "{lines:?}");
    Ok(())
}

#[tokio::test]
async fn annotation_numbers_are_trimmed_of_dot_net_integer_whitespace_only() -> Result<()> {
    let script = "printf '::error line=\\302\\2405::nbsp\\n'\nprintf '::error line=\\t5\\v::ascii\\n'\nprintf '::error line=\\302\\2405,endLine=3::mixed\\n'";
    let (finished, _) = run_steps(&job(vec![run("a", script)])).await?;
    let line_of = |message: &str| {
        finished
            .annotations
            .iter()
            .find(|annotation| annotation.message == message)
            .map(|annotation| annotation.line)
    };
    assert_eq!(line_of("nbsp"), Some(None));
    assert_eq!(line_of("ascii"), Some(Some(5)));
    let mixed = finished
        .annotations
        .iter()
        .find(|annotation| annotation.message == "mixed")
        .map(|annotation| (annotation.line, annotation.end_line));
    assert_eq!(mixed, Some((Some(3), Some(3))));
    Ok(())
}

#[tokio::test]
async fn a_step_environment_that_fails_to_evaluate_fails_the_step_despite_continue_on_error()
-> Result<()> {
    let broken = with(
        with(
            run("a", "echo never"),
            "Environment",
            mapping(&[("K", expression("fromJSON('{bad')"))]),
        ),
        "ContinueOnError",
        json!(true),
    );
    let (finished, lines) = run_steps(&job(vec![broken])).await?;
    assert_eq!(finished.result, JobResult::Failed, "{lines:?}");
    assert_eq!(finished.steps[0].conclusion, StepResult::Failed);
    assert!(!lines.iter().any(|line| line == "never"), "{lines:?}");
    Ok(())
}

struct Writes;

impl StepAdapter for Writes {
    fn handles(&self, reference: &str) -> bool {
        reference == "actions/writes@v1"
    }

    fn run<'a>(&'a self, ctx: &'a mut StepContext<'_>) -> StepFuture<'a> {
        async move {
            ctx.set_output("k", "kept");
            Err(velnor_runner::exec::ExecError::Unsupported(
                "late".to_owned(),
            ))
        }
        .boxed()
    }
}

#[tokio::test]
async fn adapter_outputs_survive_an_adapter_error() -> Result<()> {
    let mut uses = step("w", json!({"type": 0, "lit": "unused"}));
    uses["Reference"] = json!({"Type": "repository", "Name": "actions/writes", "Ref": "v1", "RepositoryType": "GitHub"});
    let follow = with(
        run("b", "echo \"got=${{ steps.w.outputs.k }}\""),
        "Condition",
        json!("always()"),
    );
    let mut registry = AdapterRegistry::empty();
    registry.register(Box::new(Writes));
    let (finished, lines) = execute(
        &job(vec![uses, follow]),
        &Scratch::new()?,
        &registry,
        JobCancel::new(),
    )
    .await?;
    assert_eq!(finished.steps[0].outcome, StepResult::Failed, "{lines:?}");
    assert!(lines.iter().any(|line| line == "got=kept"), "{lines:?}");
    Ok(())
}

#[tokio::test]
async fn the_step_timeout_counts_the_time_spent_before_the_process_starts() -> Result<()> {
    let scratch = Scratch::new()?;
    std::fs::create_dir_all(scratch.workspace()).context("create the workspace")?;
    let big = std::fs::File::create(scratch.workspace().join("big.bin"))
        .context("create the large file")?;
    big.set_len(2 << 30).context("size the large file")?;
    let slow = with(
        run("a", "echo \"hash=${{ hashFiles('big.bin') }}\""),
        "TimeoutInMinutes",
        json!(0.005),
    );
    let (finished, lines) = tokio::time::timeout(
        Duration::from_secs(120),
        execute(
            &job(vec![slow]),
            &scratch,
            &AdapterRegistry::empty(),
            JobCancel::new(),
        ),
    )
    .await
    .context("the job did not finish")??;
    assert_eq!(finished.steps[0].outcome, StepResult::Failed, "{lines:?}");
    assert!(
        lines
            .iter()
            .any(|line| line == "step 1 timed out after 0.005 minutes"),
        "{lines:?}"
    );
    Ok(())
}

struct Instant;

impl StepAdapter for Instant {
    fn handles(&self, reference: &str) -> bool {
        reference == "actions/instant@v1"
    }

    fn run<'a>(&'a self, _ctx: &'a mut StepContext<'_>) -> StepFuture<'a> {
        async move { Ok(StepResult::Succeeded) }.boxed()
    }
}

#[tokio::test]
async fn an_adapter_that_finishes_after_the_step_timeout_expired_fails_the_step() -> Result<()> {
    let scratch = Scratch::new()?;
    std::fs::create_dir_all(scratch.workspace()).context("create the workspace")?;
    let big = std::fs::File::create(scratch.workspace().join("big.bin"))
        .context("create the large file")?;
    big.set_len(2 << 30).context("size the large file")?;
    let mut uses = with(
        step("w", json!({"type": 0, "lit": "unused"})),
        "TimeoutInMinutes",
        json!(0.005),
    );
    uses["Reference"] = json!({"Type": "repository", "Name": "actions/instant", "Ref": "v1", "RepositoryType": "GitHub"});
    uses["Inputs"] = mapping(&[("k", expression("hashFiles('big.bin')"))]);
    let mut registry = AdapterRegistry::empty();
    registry.register(Box::new(Instant));
    let (finished, lines) = tokio::time::timeout(
        Duration::from_secs(120),
        execute(&job(vec![uses]), &scratch, &registry, JobCancel::new()),
    )
    .await
    .context("the job did not finish")??;
    assert_eq!(finished.steps[0].outcome, StepResult::Failed, "{lines:?}");
    assert!(
        lines
            .iter()
            .any(|line| line == "step 1 timed out after 0.005 minutes"),
        "{lines:?}"
    );
    Ok(())
}

#[tokio::test]
async fn a_whitespace_only_step_summary_is_kept() -> Result<()> {
    let (finished, _) = run_steps(&job(vec![run(
        "a",
        "printf '  \\n' >> \"$GITHUB_STEP_SUMMARY\"",
    )]))
    .await?;
    assert!(finished.steps[0].summary_path.is_some());
    Ok(())
}

async fn summary_of(script: &str) -> Result<(velnor_runner::exec::JobRun, Vec<String>)> {
    run_steps(&job(vec![run("a", script)])).await
}

#[tokio::test]
async fn a_summary_holding_only_a_byte_order_mark_is_kept() -> Result<()> {
    let (finished, _) = summary_of(&format!("printf '{BOM}' >> \"$GITHUB_STEP_SUMMARY\"")).await?;
    assert!(finished.steps[0].summary_path.is_some());
    Ok(())
}

#[tokio::test]
async fn a_summary_of_invalid_utf8_under_the_limit_is_kept_whatever_it_decodes_to() -> Result<()> {
    let (finished, _) = summary_of(
        "head -c 400000 /dev/zero | LC_ALL=C tr '\\0' '\\377' >> \"$GITHUB_STEP_SUMMARY\"",
    )
    .await?;
    assert!(finished.steps[0].summary_path.is_some());
    assert!(
        finished.annotations.is_empty(),
        "{:?}",
        finished.annotations
    );
    Ok(())
}

#[tokio::test]
async fn a_summary_of_exactly_the_limit_is_kept() -> Result<()> {
    let (finished, _) =
        summary_of("head -c 1048576 /dev/zero | tr '\\0' a >> \"$GITHUB_STEP_SUMMARY\"").await?;
    assert!(finished.steps[0].summary_path.is_some());
    assert!(
        finished.annotations.is_empty(),
        "{:?}",
        finished.annotations
    );
    Ok(())
}

#[test]
fn root_contexts_are_found_whatever_the_case_of_their_names() -> Result<()> {
    let mut message = job(vec![step(
        "s",
        expression("format('echo \"job={0} input={1}\"', github.job, inputs.x)"),
    )]);
    message["ContextData"] = json!({
        "Github": {"t": 2, "d": [{"k": "job", "v": {"t": 0, "s": "ctx-job"}}]},
        "Inputs": {"t": 2, "d": [{"k": "x", "v": {"t": 0, "s": "1"}}]}
    });
    let ran = run_message(&Scratch::new()?, &message)?;
    assert!(ran.has_line("job=ctx-job input=1"), "{}", ran.stdout);
    Ok(())
}

#[test]
fn an_endpoint_parameter_of_only_whitespace_is_masked() -> Result<()> {
    let mut message = job(vec![run("a", "echo \"[   ]\"")]);
    message["Resources"] = json!({"Endpoints": [{
        "Name": "SystemVssConnection",
        "Authorization": {"Scheme": "OAuth", "Parameters": {"AccessToken": "tok-ws-1", "Extra": "   "}}
    }]});
    let ran = run_message(&Scratch::new()?, &message)?;
    assert!(ran.has_line("[***]"), "{}", ran.stdout);
    Ok(())
}

#[test]
fn a_secret_variable_of_only_whitespace_is_trimmed_to_nothing_and_not_masked() -> Result<()> {
    let mut message = job(vec![run("a", "echo \"[   ]\"")]);
    message["Variables"] = json!({"W": {"Value": "   ", "IsSecret": true}});
    let ran = run_message(&Scratch::new()?, &message)?;
    assert!(ran.has_line("[   ]"), "{}", ran.stdout);
    Ok(())
}

#[test]
fn adapter_inputs_are_keyed_by_ordinal_ignore_case() {
    let inputs: Inputs = [("\u{3c2}".to_owned(), "final".to_owned())]
        .into_iter()
        .collect();
    assert_eq!(inputs.get("\u{3a3}"), Some(&"final".to_owned()));
    let both: Inputs = [
        ("\u{3a3}".to_owned(), "one".to_owned()),
        ("\u{3c2}".to_owned(), "two".to_owned()),
    ]
    .into_iter()
    .collect();
    assert_eq!(
        (both.len(), both.get("\u{3c3}")),
        (1, Some(&"two".to_owned()))
    );
}

#[test]
fn a_payload_that_is_not_a_job_request_is_rejected() -> Result<()> {
    let scratch = Scratch::new()?;
    for (index, body) in [
        "{}",
        "{\"MessageType\": \"PipelineAgentJobResponse\", \"Steps\": []}",
    ]
    .into_iter()
    .enumerate()
    {
        let path = scratch
            .options()
            .temp
            .with_extension(format!("{index}.json"));
        std::fs::write(&path, body)?;
        let ran = finished(bounded(&scratch, &path))?;
        assert_eq!(ran.code, Some(2), "{body}: {}", ran.stderr);
        assert!(ran.stderr.contains("MessageType"), "{}", ran.stderr);
        assert!(ran.stdout.is_empty(), "{}", ran.stdout);
    }
    Ok(())
}

#[test]
fn the_message_type_is_matched_without_regard_to_case() -> Result<()> {
    let scratch = Scratch::new()?;
    let mut message = job(vec![run("a", "echo cased")]);
    message["MessageType"] = json!("pipelineagentjobrequest");
    let ran = run_message(&scratch, &message)?;
    assert_eq!(ran.code, Some(0), "{}", ran.stderr);
    assert!(ran.has_line("cased"), "{}", ran.stdout);
    Ok(())
}

#[tokio::test]
async fn columns_are_dropped_when_the_two_line_values_are_spelled_differently() -> Result<()> {
    let script = "echo '::error line=5,endLine=5,col=2::same'\necho '::error line=05,endLine=5,col=2::spelled'";
    let (finished, _) = run_steps(&job(vec![run("a", script)])).await?;
    let column_of = |message: &str| {
        finished
            .annotations
            .iter()
            .find(|annotation| annotation.message == message)
            .map(|annotation| annotation.column)
    };
    assert_eq!(column_of("same"), Some(Some(2)));
    assert_eq!(column_of("spelled"), Some(None));
    Ok(())
}

#[tokio::test]
async fn a_job_output_name_that_holds_a_secret_is_masked_in_the_log_and_kept_in_the_result()
-> Result<()> {
    let mut message = job(vec![run("a", "echo hi")]);
    message["Variables"] = json!({"S": {"Value": "result", "IsSecret": true}});
    message["JobOutputs"] = mapping(&[("result", crate::exec_builder::literal("ok"))]);
    let (finished, _) = run_steps(&message).await?;
    assert_eq!(
        finished.outputs.get("result").map(String::as_str),
        Some("ok")
    );
    let ran = run_message(&Scratch::new()?, &message)?;
    assert!(ran.has_line("output ***=ok"), "{}", ran.stdout);
    assert!(!ran.stdout.contains("result"), "{}", ran.stdout);
    Ok(())
}
