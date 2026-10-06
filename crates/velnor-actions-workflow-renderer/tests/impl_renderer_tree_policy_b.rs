//! Workflow/tree invariant cases, part 2 (final gate, bare-command rejection).
use std::collections::BTreeMap;
use velnor_actions_contract::{Job, JobTimeout, Permissions, Step, StepKind, WorkflowPolicy};
use velnor_actions_workflow_renderer::{
    RenderError, checkout_step, merge_step, render_workflow_ir, shell_step,
};

use crate::impl_renderer_tree_policy::{LABEL, checkout_pin, fixture_ctx, fixture_ir, task_job};

#[test]
fn final_gate_keeps_exact_name_and_condition() -> Result<(), RenderError> {
    let ctx = fixture_ctx();
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "required".to_owned(),
        Job {
            display_name: "Wrong Name".to_owned(),
            runs_on: LABEL.to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::CRATE,
            needs: vec!["plan".to_owned()],
            condition: Some("always()".to_owned()),
            permissions: None,
            environment: None,
            steps: vec![checkout_step(&checkout_pin())?, merge_step()],
        },
    );
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_err());
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "required".to_owned(),
        Job {
            display_name: "Required".to_owned(),
            runs_on: LABEL.to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::REQUIRED,
            needs: vec!["plan".to_owned()],
            condition: None,
            permissions: None,
            environment: None,
            steps: vec![checkout_step(&checkout_pin())?, merge_step()],
        },
    );
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_err());
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "required".to_owned(),
        Job {
            display_name: "Required".to_owned(),
            runs_on: LABEL.to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::REQUIRED,
            needs: vec!["plan".to_owned()],
            condition: Some("always()".to_owned()),
            permissions: Some(Permissions {
                contents: velnor_actions_contract::workflow::permissions::PermissionLevel::Read,
                actions: velnor_actions_contract::workflow::permissions::PermissionLevel::Read,
                pull_requests:
                    velnor_actions_contract::workflow::permissions::PermissionLevel::None,
                id_token: velnor_actions_contract::workflow::permissions::PermissionLevel::None,
            }),
            environment: None,
            steps: vec![checkout_step(&checkout_pin())?, merge_step()],
        },
    );
    let text = render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx)?;
    assert!(text.contains("name: Required"));
    assert!(text.contains("if: always()"));
    Ok(())
}

#[test]
fn renderer_rejects_bare_commands_inside_ir() -> Result<(), RenderError> {
    let ctx = fixture_ctx();
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "velnor-task".to_owned(),
        task_job(Step {
            name: "Install".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Shell {
                run: vec!["cargo".to_owned(), "install".to_owned(), "x".to_owned()],
                env: BTreeMap::new(),
            },
        }),
    );
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_err());
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "velnor-task".to_owned(),
        task_job(Step {
            name: "Fetch".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Action {
                uses: "actions/checkout@main".to_owned(),
                with: BTreeMap::new(),
                env: BTreeMap::new(),
            },
        }),
    );
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_err());
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "velnor-task".to_owned(),
        task_job(Step {
            name: "Run Alint".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Action {
                uses: "asamarts/alint@v0.16.1".to_owned(),
                with: BTreeMap::new(),
                env: BTreeMap::new(),
            },
        }),
    );
    let err = render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx)
        .expect_err("alint tag ref must be rejected");
    assert!(
        format!("{err:?}").contains("unpinned_ref"),
        "wrong rejection: {err:?}"
    );
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "velnor-task".to_owned(),
        task_job(shell_step(
            "Focused",
            vec!["true".to_owned()],
            BTreeMap::new(),
        )?),
    );
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_ok());
    Ok(())
}
