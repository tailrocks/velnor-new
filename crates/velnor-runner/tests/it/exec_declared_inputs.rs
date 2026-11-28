// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;
use futures_util::FutureExt;
use serde_json::{Value, json};
use velnor_runner::exec::{
    AdapterRegistry, JobCancel, JobResult, StepAdapter, StepContext, StepFuture, StepResult,
};

use crate::exec_builder::{execute, expression, job, literal, step, with};
use crate::exec_support::Scratch;

struct Probe;

impl StepAdapter for Probe {
    fn handles(&self, reference: &str) -> bool {
        reference == "actions/foo@v1"
    }

    fn run<'a>(&'a self, ctx: &'a mut StepContext<'_>) -> StepFuture<'a> {
        async move {
            let name = ctx.inputs.get("name").cloned().unwrap_or_default();
            ctx.log(&format!("probe-saw name={name}"));
            Ok(StepResult::Succeeded)
        }
        .boxed()
    }
}

fn probe_step(inputs: Value) -> Value {
    let step = with(
        step("probe", literal("unused")),
        "Reference",
        json!({"Type": "repository", "Name": "actions/foo", "Ref": "v1", "RepositoryType": "GitHub"}),
    );
    with(step, "Inputs", inputs)
}

async fn run_probe(inputs: Value) -> Result<(JobResult, Vec<String>)> {
    let mut adapters = AdapterRegistry::empty();
    adapters.register(Box::new(Probe));
    let (run, lines) = execute(
        &job(vec![probe_step(inputs)]),
        &Scratch::new()?,
        &adapters,
        JobCancel::new(),
    )
    .await?;
    Ok((run.result, lines))
}

fn saw_name(lines: &[String], name: &str) -> bool {
    lines
        .iter()
        .any(|line| line == &format!("probe-saw name={name}"))
}

#[tokio::test]
async fn an_expression_valued_with_runs_the_step_with_the_evaluated_inputs() -> Result<()> {
    let (result, lines) = run_probe(expression("fromJSON('{\"name\":\"b\"}')")).await?;
    assert_eq!(result, JobResult::Succeeded, "{lines:?}");
    assert!(saw_name(&lines, "b"), "{lines:?}");
    Ok(())
}

#[tokio::test]
async fn an_expression_input_name_runs_the_step_with_the_evaluated_name() -> Result<()> {
    let inputs = json!({"type": 2, "map": [{"key": expression("'name'"), "value": literal("b")}]});
    let (result, lines) = run_probe(inputs).await?;
    assert_eq!(result, JobResult::Succeeded, "{lines:?}");
    assert!(saw_name(&lines, "b"), "{lines:?}");
    Ok(())
}

#[tokio::test]
async fn a_malformed_inputs_token_refuses_the_job_with_its_own_error() -> Result<()> {
    let (result, lines) = run_probe(json!({"type": "x"})).await?;
    assert_eq!(result, JobResult::Failed, "{lines:?}");
    assert!(
        lines.iter().any(|line| line
            == "unsupported: actions/foo@v1 Unexpected template token: the token type must be an integer"),
        "{lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line.starts_with("probe-saw")),
        "{lines:?}"
    );
    Ok(())
}
