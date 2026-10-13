//! Token hygiene: action env scanning plus fetch-exemption shape.
//!
//! Split from `impl_renderer_token_hygiene` to hold the 400-line gate.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, StepRole, WorkflowPolicy};
use velnor_actions_workflow_renderer::{
    RenderError, action_step_with_env, ambient_shell_step, checkout_step, plan_step,
    render_workflow_ir, steps::RESOLVE_QUALIFICATION_OPERATION, steps::internal_step,
};

use super::impl_renderer_fixtures::*;

/// Pinned-shape MBX ref for constructor-built action steps.
fn mbx_pin() -> String {
    format!("jdx/mr-boxington-action@{:040x}", 0)
}

/// Plan job carrying one caller-supplied action step.
fn action_plan_job(step: velnor_actions_contract::Step) -> Result<(String, Job), RenderError> {
    Ok(job(
        "plan",
        "Plan",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, step, plan_step()],
    ))
}

#[test]
fn token_in_action_env_fails_render() -> Result<(), RenderError> {
    // Constructor-legal value (no expression spans) that names a
    // token: the render-time gate must still catch it in `env:`.
    let leaked = action_step_with_env(
        "Setup MBX",
        &mbx_pin(),
        BTreeMap::from([("github-cache-mode".to_owned(), "objects".to_owned())]),
        BTreeMap::from([(
            "ACTIONS_CACHE_MODE".to_owned(),
            "see GITHUB_TOKEN here".to_owned(),
        )]),
    )?;
    render_fails_with(vec![action_plan_job(leaked)?], "token_in_action_env");
    Ok(())
}

#[test]
fn benign_mbx_cache_mode_env_passes() -> Result<(), RenderError> {
    // The genuine MBX restore step pins its push-gated cache mode in
    // step env; the extended gate must not flag that expression.
    let [preflight, restore, version_check] = mbx_tool_steps(&mbx_pin(), "1.5.0", "1.99.0")?;
    let text = render_workflow_ir(
        &fixture_ir(vec![job(
            "plan",
            "Plan",
            Vec::new(),
            vec![
                checkout_step(&checkout_pin())?,
                preflight,
                restore,
                version_check,
                plan_step(),
            ],
        )]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(
        text.contains("github-cache-mode"),
        "benign MBX cache mode must render: {text}"
    );
    Ok(())
}

#[test]
fn qualification_resolver_is_scoped_to_dispatch_in_plan_job() -> Result<(), RenderError> {
    let mut resolver = internal_step(
        "Resolve qualification predecessor",
        RESOLVE_QUALIFICATION_OPERATION,
    )?;
    resolver.condition = Some("github.event_name == 'workflow_dispatch'".to_owned());
    render_workflow_ir(
        &fixture_ir(vec![action_plan_job(resolver)?]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;

    let unguarded = internal_step(
        "Resolve qualification predecessor",
        RESOLVE_QUALIFICATION_OPERATION,
    )?;
    let error = render_workflow_ir(
        &fixture_ir(vec![action_plan_job(unguarded)?]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )
    .expect_err("resolver must require workflow_dispatch condition");
    assert!(
        matches!(error, RenderError::InvalidWorkflow(ref problem) if problem.contains("qualification_resolver_scope")),
        "{error}"
    );
    Ok(())
}

#[test]
fn fetch_ambient_auth_follows_typed_role_not_label() -> Result<(), RenderError> {
    let mut role_owned = ambient_shell_step(
        "Renamed source fetch",
        vec!["true".to_owned()],
        BTreeMap::new(),
    )?;
    role_owned.role = Some(StepRole::CargoSourcesFetch);
    let allowed = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, role_owned, plan_step()],
    );
    render_workflow_ir(
        &fixture_ir(vec![allowed]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;

    let label_only = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            ambient_shell_step(
                "Fetch Cargo sources (nested/Cargo.toml)",
                vec!["true".to_owned()],
                BTreeMap::new(),
            )?,
            plan_step(),
        ],
    );
    render_fails_with(vec![label_only], "missing_scrub");
    Ok(())
}
