//! Release-tree size cap: the direct release renderer enforces the workflow byte cap.

use velnor_actions_contract_workflow::ScheduleTrigger;
use velnor_actions_workflow_release::release_tree::render_release_workflow;
use velnor_actions_workflow_steps::RenderError;

#[test]
fn direct_release_renderer_rejects_an_oversized_workflow() -> Result<(), RenderError> {
    let mut spec = super::impl_renderer_release_tree::spec()?;
    spec.triggers.schedule = Some(ScheduleTrigger {
        cron: vec!["0 6 * * 1".to_owned(); 30_000],
    });
    let error = render_release_workflow(&spec, &super::impl_renderer_release_tree::ctx())
        .expect_err("direct release render must enforce the cap");
    assert!(
        error
            .to_string()
            .contains("workflow_too_large:.github/workflows/release.yml")
    );
    Ok(())
}
