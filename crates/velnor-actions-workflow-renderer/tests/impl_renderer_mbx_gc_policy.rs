//! Automatic MBX collection must cover setup, build, and test steps.

use std::collections::BTreeMap;

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_contract::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};
use velnor_actions_workflow_renderer::render_workflow_ir;
use velnor_actions_workflow_renderer::steps::{checkout_step, mbx_objects_step};
use velnor_actions_workflow_renderer::{RenderError, shell_step};

use super::impl_renderer_fixtures::*;

/// Rendered local setup and GC policy reach MBX jobs only.
#[test]
fn local_backend_and_gc_policy_render_only_when_present() -> Result<(), RenderError> {
    let uses = format!("jdx/mr-boxington-action@{}", "a".repeat(40));
    let mbx = mbx_objects_step(&uses, false, TEST_MBX_VERSION)?;
    let plain = checkout_step(&checkout_pin())?;
    let text = render_workflow_ir(
        &fixture_ir(vec![job(
            "demo",
            "Demo",
            Vec::new(),
            vec![pinned_tools_step(TEST_MBX_VERSION)?, plain, mbx],
        )]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(
        text.contains("backend: local"),
        "MBX setup uses local backend:\n{text}"
    );
    assert!(
        text.contains("name: Restore MBX single bundle"),
        "explicit restore exists:\n{text}"
    );
    assert!(
        !text.contains("github-cache-mode: objects"),
        "no hidden action restore:\n{text}"
    );
    assert!(
        !text.contains("&& 'write'"),
        "push writes must not come from the action post:\n{text}"
    );
    assert!(
        text.contains("MBX_GC_AUTO: \"1\""),
        "MBX jobs enable GC:\n{text}"
    );

    let plain = checkout_step(&checkout_pin())?;
    let cargo_text = render_workflow_ir(
        &fixture_ir(vec![job(
            "cargo-only",
            "Cargo only",
            Vec::new(),
            vec![plain],
        )]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(
        !cargo_text.contains("MBX_GC_AUTO"),
        "Cargo-only jobs do not receive MBX policy:\n{cargo_text}"
    );
    assert!(
        !cargo_text.contains("Export MBX single bundle"),
        "Cargo-only jobs do not export an MBX bundle:\n{cargo_text}"
    );
    Ok(())
}

/// Lane extraction keeps hosted GC enabled and leaves scale-set policy intact.
#[test]
fn mbx_gc_policy_is_scoped_to_hosted_mbx_jobs() -> Result<(), RenderError> {
    let uses = format!("jdx/mr-boxington-action@{}", "a".repeat(40));
    let mbx = mbx_objects_step(&uses, false, TEST_MBX_VERSION)?;
    let checkout = checkout_step(&checkout_pin())?;
    let hosted = job(
        &format!("rust-demo{HOSTED_SUFFIX}"),
        "Rust demo hosted",
        Vec::new(),
        vec![
            checkout.clone(),
            pinned_tools_step(TEST_MBX_VERSION)?,
            mbx.clone(),
        ],
    );
    let mut local = job(
        &format!("rust-demo{SCALE_SUFFIX}"),
        "Rust demo scale set",
        Vec::new(),
        vec![checkout, pinned_tools_step(TEST_MBX_VERSION)?, mbx],
    );
    local.1.runs_on = ScaleSetSelector::try_new(
        SCALE_SET_NAME,
        &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
    )
    .expect("valid scale-set selector")
    .token();
    let text = render_workflow_ir(
        &fixture_ir(vec![hosted, local]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert_eq!(text.matches("MBX_GC_AUTO: \"1\"").count(), 1, "{text}");
    Ok(())
}

fn pinned_tools_step(version: &str) -> Result<velnor_actions_contract::Step, RenderError> {
    shell_step(
        "Prepare pinned Rust",
        vec![
            "mise".to_owned(),
            "--no-config".to_owned(),
            "--no-env".to_owned(),
            "--no-hooks".to_owned(),
            "install".to_owned(),
            "rust@1.98.1".to_owned(),
            format!("mr-boxington@{version}"),
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
    )
}
