use velnor_actions_contract::cachekey::mbx_cache_generation;
use velnor_actions_contract::{Step, StepKind, WorkflowPolicy};
use velnor_actions_workflow_renderer::steps::{
    CompileDriver, MBX_CACHE_MODE_ENV, MBX_PREFLIGHT_NAME, mbx_steps_for_driver,
};
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir};

use super::impl_renderer_fixtures::{
    TEST_MBX_VERSION, TEST_RUST_TOOLCHAIN, fixture_ctx, fixture_ir, job, mbx_tool_env, step_names,
};

const MBX_ACTION: &str = "jdx/mr-boxington-action@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn steps(
    version: &str,
    rust: &str,
) -> Result<[Step; 2], velnor_actions_workflow_renderer::RenderError> {
    mbx_steps_for_driver(
        MBX_ACTION,
        CompileDriver::Mbx,
        version,
        rust,
        mbx_tool_env(rust),
    )?
    .ok_or_else(|| {
        velnor_actions_workflow_renderer::RenderError::InvalidWorkflow(
            "mbx_steps_missing".to_owned(),
        )
    })
}

#[test]
fn action_uses_exact_cache_inputs_after_preflight() {
    let [preflight, action] = steps(TEST_MBX_VERSION, TEST_RUST_TOOLCHAIN).expect("MBX steps");
    assert_eq!(preflight.name, MBX_PREFLIGHT_NAME);
    let StepKind::Action { uses, with, env } = action.kind else {
        panic!("restore must be an action");
    };
    assert_eq!(uses, MBX_ACTION);
    assert_eq!(
        with.get("github-cache-mode").map(String::as_str),
        Some("objects")
    );
    assert_eq!(
        with.get("toolchain").map(String::as_str),
        Some(TEST_RUST_TOOLCHAIN)
    );
    assert!(
        !with.contains_key("version"),
        "preflight owns exact MBX identity"
    );
    assert_eq!(
        with.get("cache-generation").map(String::as_str),
        Some(mbx_cache_generation(TEST_MBX_VERSION).as_str())
    );
    assert_eq!(
        env.get(MBX_CACHE_MODE_ENV).map(String::as_str),
        Some("read")
    );
    assert_eq!(env.get("RUSTUP_HOME"), env.get("MISE_RUSTUP_HOME"));
    assert_eq!(env.get("CARGO_HOME"), env.get("MISE_CARGO_HOME"));
    for key in [
        "save-on-pull-request",
        "save-on-workflow-dispatch",
        "save-on-protected-branch",
        "mode",
    ] {
        assert!(!with.contains_key(key), "unsupported action input {key}");
    }
}

#[test]
fn action_construction_rejects_floating_refs_and_tool_versions() {
    let mut env = mbx_tool_env(TEST_RUST_TOOLCHAIN);
    for uses in [
        "actions/cache/restore@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "jdx/mr-boxington-action@main",
    ] {
        assert!(
            mbx_steps_for_driver(
                uses,
                CompileDriver::Mbx,
                TEST_MBX_VERSION,
                TEST_RUST_TOOLCHAIN,
                env.clone(),
            )
            .is_err(),
            "invalid action ref {uses}"
        );
    }
    for version in ["latest", "v1.21.1", "1.21", "1.21.1.0", "1.21.x", ""] {
        assert!(
            steps(version, TEST_RUST_TOOLCHAIN).is_err(),
            "invalid MBX version {version:?}"
        );
    }
    for rust in ["stable", "1.98", "1.98.1-nightly", "1.98.1;exit 1"] {
        env = mbx_tool_env(rust);
        assert!(
            mbx_steps_for_driver(MBX_ACTION, CompileDriver::Mbx, TEST_MBX_VERSION, rust, env,)
                .is_err(),
            "invalid Rust toolchain {rust:?}"
        );
    }
}

#[test]
fn cargo_driver_emits_neither_preflight_nor_mbx_action() {
    assert!(
        mbx_steps_for_driver(
            MBX_ACTION,
            CompileDriver::Cargo,
            TEST_MBX_VERSION,
            TEST_RUST_TOOLCHAIN,
            mbx_tool_env(TEST_RUST_TOOLCHAIN),
        )
        .expect("Cargo driver")
        .is_none()
    );
}

#[test]
fn rendered_action_receives_preflight_paths_and_matching_tool_homes() -> Result<(), RenderError> {
    let pair = steps(TEST_MBX_VERSION, TEST_RUST_TOOLCHAIN)?;
    let text = render_workflow_ir(
        &fixture_ir(vec![job("demo", "Demo", Vec::new(), pair.into())]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    let names = step_names(&text, "demo");
    let preflight = names
        .iter()
        .position(|name| name == "Verify MBX and Rust toolchains")
        .expect("rendered preflight");
    let action = names
        .iter()
        .position(|name| name == "Restore MBX objects")
        .expect("rendered MBX action");
    assert!(preflight < action, "rendered order: {names:?}");
    let rustup_home = "$".to_owned() + "{{ runner.temp }}/velnor/rustup";
    let cargo_home = "$".to_owned() + "{{ runner.temp }}/velnor/cargo";
    for key in ["MISE_RUSTUP_HOME", "RUSTUP_HOME"] {
        assert!(text.contains(&format!("{key}: {rustup_home}")), "{key}");
    }
    for key in ["MISE_CARGO_HOME", "CARGO_HOME"] {
        assert!(text.contains(&format!("{key}: {cargo_home}")), "{key}");
    }
    assert!(
        text.contains("GITHUB_PATH"),
        "rendered preflight exports PATH"
    );
    assert!(
        text.contains("rust@1.98.1"),
        "rendered preflight probes the catalog toolchain"
    );
    Ok(())
}
