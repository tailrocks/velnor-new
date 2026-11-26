// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::FutureExt;
use serde_json::{Value, json};
use velnor_runner::exec::{
    AdapterRegistry, JobCancel, JobResult, StepAdapter, StepContext, StepFuture, StepResult,
};

use crate::exec_builder::{execute, expression, job, mapping, run, step, with};
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
    let script =
        "printf '::error line=\\302\\2405::nbsp\\n'\nprintf '::error line=\\t5\\v::ascii\\n'";
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
    Ok(())
}
