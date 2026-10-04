//! The stock MBX action installs the exact catalog version through local setup.

use std::collections::BTreeMap;

use velnor_actions_contract::{StepKind, WorkflowPolicy};
use velnor_actions_workflow_renderer::steps::{
    CompileDriver, MBX_PREFLIGHT_NAME, MBX_SETUP_NAME, mbx_path_preflight_step, mbx_step_for_driver,
};
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir, shell_step};

use super::impl_renderer_fixtures::*;
use super::impl_renderer_mbx_bundle::mbx_job;

const MBX_ACTION: &str = "jdx/mr-boxington-action@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn setup_pins_the_local_backend_and_exact_version() -> Result<(), RenderError> {
    let setup =
        mbx_step_for_driver(MBX_ACTION, CompileDriver::Mbx, TEST_MBX_VERSION)?.expect("MBX setup");
    assert_eq!(setup.name, MBX_SETUP_NAME);
    let StepKind::Action { uses, with, env } = setup.kind else {
        return Err(RenderError::InvalidWorkflow(
            "mbx_setup_not_action".to_owned(),
        ));
    };
    assert_eq!(uses, MBX_ACTION);
    assert_eq!(
        with.len(),
        2,
        "setup has only its backend and version inputs"
    );
    assert_eq!(with.get("backend").map(String::as_str), Some("local"));
    assert_eq!(
        with.get("version").map(String::as_str),
        Some(TEST_MBX_VERSION)
    );
    assert!(env.is_empty(), "local setup carries no implicit cache mode");
    Ok(())
}

#[test]
fn setup_rejects_floating_refs_and_tool_versions() {
    for uses in [
        "actions/cache/restore@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "jdx/mr-boxington-action@main",
    ] {
        assert!(
            mbx_step_for_driver(uses, CompileDriver::Mbx, TEST_MBX_VERSION).is_err(),
            "invalid action ref {uses}"
        );
    }
    for version in ["latest", "v1.22.0", "1.22", "1.22.0.1", "1.22.x", ""] {
        assert!(
            mbx_step_for_driver(MBX_ACTION, CompileDriver::Mbx, version).is_err(),
            "invalid MBX version {version:?}"
        );
    }
}

#[test]
fn cargo_driver_emits_no_mbx_setup() -> Result<(), RenderError> {
    assert!(mbx_step_for_driver(MBX_ACTION, CompileDriver::Cargo, TEST_MBX_VERSION)?.is_none());
    Ok(())
}

#[test]
fn preflight_requires_the_same_exact_rust_selector_in_env() {
    for rust in ["stable", "1.98", "1.98.1-nightly", "1.98.1;exit 1", ""] {
        assert!(
            mbx_path_preflight_step(TEST_MBX_VERSION, rust, mbx_tool_env(rust)).is_err(),
            "invalid Rust selector {rust:?}"
        );
    }
    let mut env = mbx_tool_env(TEST_RUST_TOOLCHAIN);
    env.insert("RUSTUP_TOOLCHAIN".to_owned(), "1.98.0".to_owned());
    assert!(
        mbx_path_preflight_step(TEST_MBX_VERSION, TEST_RUST_TOOLCHAIN, env).is_err(),
        "environment selector must match the probed toolchain"
    );
}

#[test]
fn full_render_injects_one_preflight_between_tool_install_and_private_root()
-> Result<(), RenderError> {
    let built = mbx_job("demo", TEST_MBX_VERSION)?;
    let text = render_workflow_ir(
        &fixture_ir(vec![built]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    let names = step_names(&text, "demo");
    let prepare = names
        .iter()
        .position(|name| name == "Prepare pinned tools")
        .expect("exact tool install");
    let preflight = names
        .iter()
        .position(|name| name == MBX_PREFLIGHT_NAME)
        .expect("central tool preflight");
    let root = names
        .iter()
        .position(|name| name == "Prepare private MBX store")
        .expect("private store root");
    let setup = names
        .iter()
        .position(|name| name == MBX_SETUP_NAME)
        .expect("stock MBX setup");
    let key = names
        .iter()
        .position(|name| name == "Prepare MBX bundle key")
        .expect("bundle key");
    let restore = names
        .iter()
        .position(|name| name == "Restore MBX single bundle")
        .expect("external bundle restore");
    let import = names
        .iter()
        .position(|name| name == "Import MBX single bundle")
        .expect("external bundle import");
    assert_eq!(
        names
            .iter()
            .filter(|name| name.as_str() == MBX_PREFLIGHT_NAME)
            .count(),
        1,
        "the central renderer emits one preflight per MBX job: {names:?}"
    );
    assert!(prepare < preflight && preflight < root && root < setup);
    assert!(setup < key && key < restore && restore < import);
    let preflight_yaml = &text[text.find(MBX_PREFLIGHT_NAME).expect("preflight YAML")..];
    assert!(
        preflight_yaml.contains("mr-boxington@1.22.0"),
        "{preflight_yaml}"
    );
    assert!(preflight_yaml.contains("rust@1.98.1"), "{preflight_yaml}");
    assert!(preflight_yaml.contains("GITHUB_PATH"), "{preflight_yaml}");
    Ok(())
}

#[test]
fn render_rejects_spoofed_preflight_name() -> Result<(), RenderError> {
    let (id, mut built) = mbx_job("demo", TEST_MBX_VERSION)?;
    built.steps.insert(
        1,
        shell_step(
            MBX_PREFLIGHT_NAME,
            vec!["true".to_owned()],
            BTreeMap::default(),
        )?,
    );
    let result = render_workflow_ir(
        &fixture_ir(vec![(id, built)]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    );
    assert!(
        result.is_err_and(|error| format!("{error:?}").contains("mbx_preflight_mismatch")),
        "a matching step name cannot bypass the renderer's exact preflight"
    );
    Ok(())
}
