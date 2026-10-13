//! Helper provisioning: staged-helper gate plus provenance typing.
use velnor_actions_workflow_steps::{RenderError, checkout_step, merge_step, plan_step};

use super::impl_renderer_fixtures::*;

#[test]
fn strict_rejects_unstaged_internal() -> Result<(), RenderError> {
    let bare_plan = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, plan_step()],
    );
    assert!(
        strict(&fixture_ir(vec![bare_plan]), &fixture_ctx())
            .is_err_and(|err| format!("{err:?}").contains("internal_without_acquire")),
        "unstaged plan must fail closed"
    );
    let bare_final = job("required", "Required", Vec::new(), vec![merge_step()]);
    let mut final_job = bare_final.1;
    final_job.condition = Some("always()".to_owned());
    assert!(
        strict(
            &fixture_ir(vec![("required".to_owned(), final_job)]),
            &fixture_ctx()
        )
        .is_err_and(|err| format!("{err:?}").contains("internal_without_acquire")),
        "unstaged merge must fail closed"
    );
    let staged = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            acquire_fixture()?,
            plan_step(),
        ],
    );
    assert!(strict(&fixture_ir(vec![staged]), &fixture_ctx()).is_ok());
    Ok(())
}
