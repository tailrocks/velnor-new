//! Lint policy identity and pin cases.

use super::*;
use velnor_actions_contract::GeneratorValidation;
use velnor_actions_workflow_renderer::ALINT_USES;

#[test]
fn bad_lint_display_rejected_on_both_policies() -> Result<(), RenderError> {
    let mut ir = fixture_ir()?;
    if let Some(lint) = ir.jobs.get_mut(LINT_ID) {
        lint.display_name = "Wrong".to_owned();
    }
    let err = render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &fixture_ctx())
        .err()
        .ok_or_else(|| RenderError::InvalidWorkflow("consumer accepted".to_owned()))?;
    assert!(err.to_string().contains("bad_lint_name"), "got {err}");
    let support = velnor_support();
    let err = render_workflow_ir(
        &ir,
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &fixture_ctx(),
    )
    .err()
    .ok_or_else(|| RenderError::InvalidWorkflow("velnor accepted".to_owned()))?;
    assert!(err.to_string().contains("bad_lint_name"), "got {err}");
    Ok(())
}

#[test]
fn velnor_policy_emits_full_sha_alint_pin() -> Result<(), RenderError> {
    let mut ctx = fixture_ctx();
    ctx.validator_commands = validator_commands();
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Bootstrap);
    let text = render_workflow_ir(
        &fixture_ir()?,
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &ctx,
    )?;
    assert_eq!(
        ALINT_USES,
        "asamarts/alint@d93c0283b19dd78afcd8a4b303f1556a7759ba81"
    );
    assert!(text.contains("  alint:"), "alint job missing:\n{text}");
    assert!(
        text.contains("uses: asamarts/alint@d93c0283b19dd78afcd8a4b303f1556a7759ba81"),
        "full-SHA pin missing:\n{text}"
    );
    assert!(
        !text.contains("asamarts/alint@v"),
        "tag ref emitted:\n{text}"
    );
    Ok(())
}
