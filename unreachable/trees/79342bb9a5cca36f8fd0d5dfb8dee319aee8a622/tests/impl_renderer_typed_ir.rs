//! Typed-IR rendering: job overrides plus dispatch/schedule triggers.
//!
//! Pins the generic renderer paths added for the typed contract IR:
//! per-job `environment`/`permissions` overrides and optional
//! `workflow_dispatch`/`schedule` triggers render verbatim; `none`
//! scopes stay absent.
use velnor_actions_contract::{Permissions, workflow::permissions::PermissionLevel};
use velnor_actions_workflow_renderer::{RenderError, checkout_step, plan_step};

use super::impl_renderer_fixtures::*;

#[test]
fn job_overrides_render_environment_and_permissions() -> Result<(), RenderError> {
    let (id, mut job) = job(
        "velnor-plan",
        "Velnor Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            acquire_fixture()?,
            plan_step(),
        ],
    );
    job.environment = Some("release".to_owned());
    let permissions = Permissions {
        pull_requests: PermissionLevel::Read,
        ..Permissions::default()
    };
    job.permissions = Some(permissions);
    let ir = fixture_ir(vec![(id, job)]);
    let text = strict(&ir, &fixture_ctx())?;
    assert!(text.contains("environment: release"), "{text}");
    assert!(text.contains("pull-requests: read"), "{text}");
    assert!(!text.contains("id-token"), "{text}");
    Ok(())
}

#[test]
fn dispatch_and_schedule_triggers_render() -> Result<(), RenderError> {
    let plan = job(
        "velnor-plan",
        "Velnor Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            acquire_fixture()?,
            plan_step(),
        ],
    );
    let mut ir = fixture_ir(vec![plan]);
    ir.triggers.workflow_dispatch = Some(velnor_actions_contract::workflow::ir::WorkflowDispatch {
        inputs: vec![velnor_actions_contract::workflow::ir::DispatchInput {
            name: "plan".to_owned(),
            required: true,
            default: Some("plan-r1-a1".to_owned()),
        }],
    });
    ir.triggers.schedule = Some(velnor_actions_contract::ScheduleTrigger {
        cron: vec!["0 6 * * 1".to_owned()],
    });
    let text = strict(&ir, &fixture_ctx())?;
    assert!(text.contains("workflow_dispatch:"), "{text}");
    assert!(text.contains("type: string"), "{text}");
    assert!(text.contains("default: plan-r1-a1"), "{text}");
    assert!(text.contains("schedule:"), "{text}");
    assert!(text.contains("cron: 0 6 * * 1"), "{text}");
    Ok(())
}
