//! Token hygiene: action env scanning plus fetch-exemption shape.
//!
//! Split from `impl_renderer_token_hygiene` to hold the 400-line gate.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, WorkflowPolicy};
use velnor_actions_workflow_renderer::{
    RenderError, action_step_with_env, ambient_shell_step, checkout_step, plan_step,
    render_workflow_ir, shell_step,
    steps::{CompileDriver, mbx_steps_for_driver},
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
        vec![
            checkout_step(&checkout_pin())?,
            shell_step(
                "Prepare pinned Rust",
                vec![
                    "mise".to_owned(),
                    "--no-config".to_owned(),
                    "--no-env".to_owned(),
                    "--no-hooks".to_owned(),
                    "install".to_owned(),
                    "rust@1.98.1".to_owned(),
                ],
                BTreeMap::from([
                    (
                        "MISE_CARGO_HOME".to_owned(),
                        "${{ runner.temp }}/velnor/cargo".to_owned(),
                    ),
                    (
                        "MISE_RUSTUP_HOME".to_owned(),
                        "${{ runner.temp }}/velnor/rustup".to_owned(),
                    ),
                    ("RUSTUP_TOOLCHAIN".to_owned(), "1.98.1".to_owned()),
                ]),
            )?,
            step,
            plan_step(),
        ],
    ))
}

#[test]
fn token_in_action_env_fails_render() -> Result<(), RenderError> {
    // Constructor-legal value (no expression spans) that names a
    // token: the render-time gate must still catch it in `env:`.
    let leaked = action_step_with_env(
        "Setup MBX",
        &mbx_pin(),
        BTreeMap::from([("backend".to_owned(), "local".to_owned())]),
        BTreeMap::from([("NOTE".to_owned(), "see GITHUB_TOKEN here".to_owned())]),
    )?;
    render_fails_with(vec![action_plan_job(leaked)?], "token_in_action_env");
    Ok(())
}

#[test]
fn typed_mbx_preflight_and_objects_cache_pass() -> Result<(), RenderError> {
    let [preflight, restore] = mbx_steps_for_driver(
        &mbx_pin(),
        CompileDriver::Mbx,
        "1.21.1",
        "1.98.1",
        mbx_tool_env("1.98.1"),
    )?
    .ok_or_else(|| RenderError::InvalidWorkflow("missing_mbx_steps".to_owned()))?;
    let mut plan = action_plan_job(restore)?.1;
    plan.steps.insert(2, preflight);
    render_workflow_ir(
        &fixture_ir(vec![("plan".to_owned(), plan)]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    Ok(())
}

#[test]
fn qualification_restore_outputs_allow_only_exact_guard_bindings() -> Result<(), RenderError> {
    shell_step(
        "Qualification cache guard",
        vec!["true".to_owned()],
        BTreeMap::from([
            (
                "MATCHED".to_owned(),
                "${{ steps.mbx-bundle.outputs.cache-matched-key }}".to_owned(),
            ),
            (
                "PREFIX".to_owned(),
                "${{ steps.mbx-cache-key.outputs.prefix }}".to_owned(),
            ),
        ]),
    )?;

    for denied in [
        (
            "MATCHED",
            "${{ steps.mbx-bundle.outputs.cache-matched-key-extra }}",
        ),
        (
            "PREFIX",
            "${{ steps.mbx-cache-key.outputs.restore-prefix }}",
        ),
        ("PREFIX", "${{ steps.mbx-cache-key.outputs.prefix-extra }}"),
    ] {
        let result = shell_step(
            "Qualification cache guard",
            vec!["true".to_owned()],
            BTreeMap::from([(denied.0.to_owned(), denied.1.to_owned())]),
        );
        assert!(
            result.is_err(),
            "unexpectedly accepted env expression: {}",
            denied.1
        );
    }
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
