//! Token hygiene: action env scanning plus fetch-exemption shape.
//!
//! Split from `impl_renderer_token_hygiene` to hold the 400-line gate.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, WorkflowPolicy};
use velnor_actions_workflow_renderer::{
    RenderError, action_step_with_env, ambient_shell_step, checkout_step, plan_step,
    render_workflow_ir, shell_step, steps::mbx_objects_step,
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
fn local_mbx_backend_passes() -> Result<(), RenderError> {
    let restore = mbx_objects_step(&mbx_pin(), false, "1.5.0")?;
    render_workflow_ir(
        &fixture_ir(vec![action_plan_job(restore)?]),
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
                "CACHE_HIT".to_owned(),
                "${{ steps.mbx-bundle.outputs.cache-hit }}".to_owned(),
            ),
            (
                "EXPECTED_KEY".to_owned(),
                "${{ steps.mbx-bundle-key.outputs.primary }}".to_owned(),
            ),
        ]),
    )?;

    for denied in [
        (
            "CACHE_HIT",
            "${{ steps.mbx-bundle.outputs.cache-hit-extra }}",
        ),
        (
            "EXPECTED_KEY",
            "${{ steps.mbx-bundle-key.outputs.restore-prefix }}",
        ),
        (
            "EXPECTED_KEY",
            "${{ steps.mbx-bundle-key.outputs.primary-suffix }}",
        ),
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
fn qualification_restore_miss_identity_accepts_only_exact_outputs() -> Result<(), RenderError> {
    let expressions = [
        (
            "RESTORE_PRIMARY_KEY",
            "${{ steps.mbx-bundle.outputs.cache-primary-key }}",
        ),
        ("RESTORE_CONCLUSION", "${{ steps.mbx-bundle.conclusion }}"),
    ];
    for (key, expression) in expressions {
        shell_step(
            "Qualification restore identity",
            vec!["true".to_owned()],
            BTreeMap::from([(key.to_owned(), expression.to_owned())]),
        )?;
    }
    for denied in [
        (
            "RESTORE_PRIMARY_KEY",
            "${{ steps.mbx-bundle.outputs.cache-primary-key-suffix }}",
        ),
        (
            "RESTORE_CONCLUSION",
            "${{ steps.mbx-bundle.conclusion-extra }}",
        ),
        (
            "RESTORE_CONCLUSION",
            "${{ steps.mbx-bundle.outputs.conclusion }}",
        ),
    ] {
        let result = shell_step(
            "Qualification restore identity",
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
