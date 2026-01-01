//! Per-crate MSRV: rust-version tool, --locked, PR exclusion.
use std::collections::BTreeMap;
use velnor_actions_contract_workflow::StepRole;
use velnor_actions_workflow_steps::{RenderError, checkout_step, plan_step, shell_step};

use super::impl_renderer_fixtures::*;

fn argv() -> Vec<String> {
    vec![
        "mise".to_owned(),
        "exec".to_owned(),
        "rust@1.98".to_owned(),
        "--".to_owned(),
        "cargo".to_owned(),
        "check".to_owned(),
        "--locked".to_owned(),
    ]
}

#[test]
fn pr_render_rejects_msrv_steps() -> Result<(), RenderError> {
    let mut msrv = shell_step("Presentation-only label", argv(), BTreeMap::new())?;
    msrv.role = Some(StepRole::MsrvQualification);
    let leaking = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, msrv, plan_step()],
    );
    assert!(
        strict(&fixture_ir(vec![leaking]), &fixture_ctx())
            .is_err_and(|err| format!("{err:?}").contains("msrv_in_pr_workflow")),
        "MSRV step in PR workflow must fail"
    );
    let named = job(
        "velnor-qual",
        "MSRV velnor-actions-contract",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?],
    );
    strict(&fixture_ir(vec![named]), &fixture_ctx())?;
    let identified = job(
        "msrv-qual",
        "Presentation-only qualification",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?],
    );
    assert!(
        strict(&fixture_ir(vec![identified]), &fixture_ctx())
            .is_err_and(|err| format!("{err:?}").contains("msrv_in_pr_workflow")),
        "MSRV job id in PR workflow must fail independently of display name"
    );
    Ok(())
}
