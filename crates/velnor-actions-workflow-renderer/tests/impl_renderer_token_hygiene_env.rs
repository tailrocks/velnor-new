//! Token hygiene: action env scanning plus typed fetch authority.
//!
//! Split from `impl_renderer_token_hygiene` to hold the 400-line gate.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, StepRole, WorkflowPolicy};
use velnor_actions_workflow_renderer::{
    RenderError, action_step_with_env, ambient_shell_step, checkout_step, plan_step,
    render_workflow_ir, steps::MBX_CACHE_MODE_ENV,
};

use super::impl_renderer_fixtures::*;

/// Pinned-shape MBX ref for constructor-built action steps.
fn mbx_pin() -> String {
    format!("jdx/mr-boxington-action@{:040x}", 0)
}

/// Plan job carrying one caller-supplied action step.
fn action_plan_job(
    steps: Vec<velnor_actions_contract::Step>,
) -> Result<(String, Job), RenderError> {
    Ok(job(
        "plan",
        "Plan",
        Vec::new(),
        [
            vec![checkout_step(&checkout_pin())?],
            steps,
            vec![plan_step()],
        ]
        .concat(),
    ))
}

#[test]
fn token_in_action_env_fails_render() -> Result<(), RenderError> {
    // Constructor-legal value (no expression spans) that names a
    // token: the render-time gate must still catch it in `env:`.
    let leaked = action_step_with_env(
        "Restore MBX objects",
        &mbx_pin(),
        BTreeMap::from([("github-cache-mode".to_owned(), "objects".to_owned())]),
        BTreeMap::from([(
            MBX_CACHE_MODE_ENV.to_owned(),
            "see GITHUB_TOKEN here".to_owned(),
        )]),
    )?;
    render_fails_with(vec![action_plan_job(vec![leaked])?], "token_in_action_env");
    Ok(())
}

#[test]
fn benign_mbx_cache_mode_env_passes() -> Result<(), RenderError> {
    // The genuine MBX restore step pins its push-gated cache mode in
    // step env; the extended gate must not flag that expression.
    let restore = mbx_tool_steps(&mbx_pin(), "1.5.0", "1.98.1")?;
    render_workflow_ir(
        &fixture_ir(vec![action_plan_job(restore.into())?]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    Ok(())
}

#[test]
fn ambient_fetch_auth_uses_typed_role_not_display_name() -> Result<(), RenderError> {
    // A typed fetch retains its generator-owned ambient-auth exemption
    // after its presentation name changes.
    let mut allowed_step = ambient_shell_step(
        "Renamed source preparation",
        vec!["true".to_owned()],
        BTreeMap::new(),
    )?;
    allowed_step.role = Some(StepRole::CargoSourcesFetch);
    let allowed = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, allowed_step, plan_step()],
    );
    render_workflow_ir(
        &fixture_ir(vec![allowed]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;

    // Old and forged labels carry no authority by themselves.
    for name in [
        "Fetch Cargo sources (nested/Cargo.toml)",
        "Fetch Cargo sources (a/b/Cargo.toml)",
        "Fetch Cargo sources (x",
        "Fetch Cargo sources (nested/Cargo.toml",
        "Fetch Cargo sources (nested/Cargo.toml))",
        "Fetch Cargo sources (nested/Cargo.toml) extra",
        "Fetch Cargo sources (Cargo.lock)",
        "Fetch Cargo sources ()",
        "Fetch Cargo sources (../escape/Cargo.toml)",
        "Fetch Cargo sources ($HOME/Cargo.toml)",
        "Fetch Cargo sources (a\"b/Cargo.toml)",
    ] {
        let forged = job(
            "plan",
            "Plan",
            Vec::new(),
            vec![
                checkout_step(&checkout_pin())?,
                ambient_shell_step(name, vec!["true".to_owned()], BTreeMap::new())?,
                plan_step(),
            ],
        );
        render_fails_with(vec![forged], "missing_scrub");
    }
    Ok(())
}
