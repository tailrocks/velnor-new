//! Strict sweep: every emitted step carries a name.
use velnor_actions_contract_config::{GeneratorValidation, WorkflowPolicy};
use velnor_actions_workflow_render_strict::render_workflow_ir_strict;
use velnor_actions_workflow_steps::{RenderError, checkout_step, merge_step, plan_step};

use super::impl_renderer_fixtures::*;

#[test]
fn every_emitted_step_has_name() -> Result<(), RenderError> {
    let mut ctx = fixture_ctx();
    ctx.validator_commands = validator_commands();
    ctx.candidate = Some(velnor_actions_workflow_jobs::CandidateSpec {
        build: mise_argv("mbx@1.0.0", "mbx", &["build"]),
        qualify: vec!["sh".to_owned(), "-c".to_owned(), "true".to_owned()],
    });
    let plan = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            acquire_fixture()?,
            plan_step(),
        ],
    );
    let task = job(
        "velnor-task",
        "Task",
        vec!["plan".to_owned()],
        vec![
            checkout_step(&checkout_pin())?,
            scrubbed_shell_step(
                "Run task",
                vec!["sh".to_owned(), "-c".to_owned(), "echo hi".to_owned()],
            )?,
        ],
    );
    let lint = job(
        "actionlint",
        "Actionlint",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            scrubbed_shell_step(
                "Run actionlint",
                mise_argv("actionlint@1.7.12", "actionlint", &["-color"]),
            )?,
        ],
    );
    let mut final_job = job(
        "required",
        "Required",
        vec!["plan".to_owned()],
        vec![acquire_fixture()?, merge_step()],
    )
    .1;
    final_job.condition = Some("always()".to_owned());
    let ir = fixture_ir(vec![plan, task, lint, ("required".to_owned(), final_job)]);
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Candidate);
    let text = render_workflow_ir_strict(
        &ir,
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &ctx,
        &mise(),
    )?;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("- ") {
            assert!(
                !rest.contains(':') || rest.starts_with("name:"),
                "nameless step item: {line}"
            );
        }
    }
    assert!(text.matches("- name:").count() >= 20, "sweep:\n{text}");
    Ok(())
}
