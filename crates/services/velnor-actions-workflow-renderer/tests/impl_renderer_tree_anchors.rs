//! Anchored command scalar compatibility in rendered workflows.
use velnor_actions_contract_config::{GeneratorValidation, WorkflowPolicy};
use velnor_actions_workflow_renderer::render_workflow_ir;
use velnor_actions_workflow_steps::{
    FORBIDDEN_TOKENS, INTERNAL_OP_ENV, REQUEST_FILE_ENV, RenderError,
};

use crate::impl_renderer_fixtures::validator_commands;
use crate::impl_renderer_tree::{fixture_ctx, fixture_ir};

#[test]
fn rendered_yaml_contains_no_private_subcommands() -> Result<(), RenderError> {
    let ir = fixture_ir()?;
    let mut ctx = fixture_ctx();
    ctx.validator_commands = validator_commands();
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Bootstrap);
    let text = render_workflow_ir(
        &ir,
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &ctx,
    )?;
    for token in FORBIDDEN_TOKENS {
        assert!(!text.contains(token), "leaked token: {token}");
    }
    assert!(text.contains(&format!("{INTERNAL_OP_ENV}: plan-v1")));
    assert!(text.contains(&format!(
        "{REQUEST_FILE_ENV}: ${{{{ runner.temp }}}}/velnor/r1-a1/plan-v1-request.json"
    )));
    assert!(text.lines().any(|line| {
        line.trim_start().starts_with("run:")
            && line.contains("\\\"$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0\\\"")
    }));
    assert!(!text.contains("run: $RUNNER_TEMP/velnor/bin/velnor-actions plan"));
    Ok(())
}
