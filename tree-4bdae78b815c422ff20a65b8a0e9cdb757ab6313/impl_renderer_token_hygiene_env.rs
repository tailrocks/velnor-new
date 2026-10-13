//! Token hygiene: action env scanning plus fetch-exemption shape.
//!
//! Split from `impl_renderer_token_hygiene` to hold the 400-line gate.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, WorkflowPolicy};
use velnor_actions_workflow_renderer::{
    RenderError, action_step_with_env, ambient_shell_step, checkout_step, plan_step,
    render_workflow_ir, steps::mbx_objects_step,
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
        "Restore MBX objects",
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
fn mbx_action_step_has_no_renderer_cache_mode_env() -> Result<(), RenderError> {
    // The action owns restore/save behavior; regular CI has no renderer
    // override for its post phase.
    let restore = mbx_objects_step(&mbx_pin(), false, "1.5.0")?;
    let text = render_workflow_ir(
        &fixture_ir(vec![action_plan_job(restore)?]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(!text.contains("ACTIONS_CACHE_MODE"), "{text}");
    Ok(())
}

#[test]
fn nested_fetch_exemption_requires_generator_shape() -> Result<(), RenderError> {
    // Genuine nested names stay exempt with unscrubbed ambient env.
    for name in [
        "Fetch Cargo sources (nested/Cargo.toml)",
        "Fetch Cargo sources (a/b/Cargo.toml)",
    ] {
        let allowed = job(
            "plan",
            "Plan",
            Vec::new(),
            vec![
                checkout_step(&checkout_pin())?,
                ambient_shell_step(name, vec!["true".to_owned()], BTreeMap::new())?,
                plan_step(),
            ],
        );
        render_workflow_ir(
            &fixture_ir(vec![allowed]),
            WorkflowPolicy::ConsumerV1,
            None,
            &fixture_ctx(),
        )?;
    }
    // Crafted lookalikes are NOT exempt: unscrubbed env fails coverage.
    for name in [
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
