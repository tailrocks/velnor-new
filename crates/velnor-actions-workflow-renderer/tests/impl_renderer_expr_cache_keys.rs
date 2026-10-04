//! Exact GitHub expressions used by private cache paths and MBX writers.

use std::collections::BTreeMap;

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_workflow_renderer::{
    RenderError, checkout_step, plan_step, render_workflow_ir, shell_step,
};

use super::impl_renderer_fixtures::*;

#[test]
fn workspace_cache_homes_and_job_identity_pass_expression_validation() -> Result<(), RenderError> {
    let workspace = "${{ github.workspace }}";
    let cargo_home = format!("{workspace}/.velnor/cache/cargo");
    let rustup_home = format!("{workspace}/.velnor/cache/rustup");
    let step = shell_step(
        "Check private cache homes",
        vec!["true".to_owned()],
        BTreeMap::from([
            ("CARGO_HOME".to_owned(), cargo_home.clone()),
            ("MISE_CARGO_HOME".to_owned(), cargo_home),
            ("RUSTUP_HOME".to_owned(), rustup_home.clone()),
            ("MISE_RUSTUP_HOME".to_owned(), rustup_home),
            ("MBX_JOB_ID".to_owned(), "${{ github.job }}".to_owned()),
        ]),
    )?;
    let task = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, step, plan_step()],
    );
    let rendered = render_workflow_ir(
        &fixture_ir(vec![task]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(rendered.contains("${{ github.workspace }}"), "{rendered}");
    assert!(rendered.contains("${{ github.job }}"), "{rendered}");
    Ok(())
}
