//! Tree-policy rejection cases: unpinned refs, alint pin, unmarked inputs.

use std::collections::BTreeMap;

use velnor_actions_contract_config::{GeneratorValidation, WorkflowPolicy};
use velnor_actions_contract_workflow::{Step, StepKind};
use velnor_actions_workflow_renderer::{render_tree, render_workflow_ir};
use velnor_actions_workflow_steps::{ALINT_USES, RenderError};

use crate::impl_renderer_tree_policy::{
    actionlint_bytes, fixture_ctx, fixture_ir, task_job, validator_commands,
};

const VERSION: &str = "0.1.0";

#[test]
fn renderer_rejects_unpinned_actions_inside_ir() -> Result<(), RenderError> {
    let ctx = fixture_ctx();
    for (name, uses) in [
        ("Fetch", "actions/checkout@main"),
        ("Run Alint", "asamarts/alint@v0.16.1"),
    ] {
        let mut ir = fixture_ir()?;
        ir.jobs.insert(
            "velnor-task".to_owned(),
            task_job(Step {
                name: name.to_owned(),
                id: None,
                role: None,
                condition: None,
                kind: StepKind::Action {
                    uses: uses.to_owned(),
                    with: BTreeMap::new(),
                    env: BTreeMap::new(),
                },
            }),
        );
        let err = render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx)
            .expect_err("unpinned ref must be rejected");
        assert!(
            format!("{err:?}").contains("unpinned_ref"),
            "wrong rejection for {uses}: {err:?}"
        );
    }
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
        "asamarts/alint@9f9d34ba0eae3888299b9e570f43338b0e7f2cdb"
    );
    assert!(text.contains("  alint:"), "alint job missing:\n{text}");
    assert!(
        text.contains("uses: asamarts/alint@9f9d34ba0eae3888299b9e570f43338b0e7f2cdb"),
        "full-SHA pin missing:\n{text}"
    );
    assert!(
        !text.contains("asamarts/alint@v"),
        "tag ref emitted:\n{text}"
    );
    Ok(())
}

#[test]
fn tree_rejects_unmarked_inputs() -> Result<(), RenderError> {
    let workflow = render_workflow_ir(
        &fixture_ir()?,
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(render_tree(&workflow, "config-variables: []\n", VERSION).is_err());
    assert!(render_tree("name: x\n", &actionlint_bytes()?, VERSION).is_err());
    assert!(render_tree(&workflow, &actionlint_bytes()?, "9.9.9").is_err());
    Ok(())
}
