//! Automatic MBX collection must cover setup, build, and test steps.

use velnor_actions_contract_config::WorkflowPolicy;
use velnor_actions_contract_config::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_contract_workflow::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};
use velnor_actions_workflow_renderer::steps::{MBX_CACHE_MODE_ENV, checkout_step};
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir};

use super::impl_renderer_fixtures::*;

/// Rendered MBX policy reaches the action and build payload.
#[test]
fn action_step_env_renders_only_when_present() -> Result<(), RenderError> {
    let uses = format!("jdx/mr-boxington-action@{}", "a".repeat(40));
    let [preflight, mbx, version_check] = mbx_tool_steps(&uses, "1.21.1", "1.98.1")?;
    let plain = checkout_step(&checkout_pin())?;
    let text = render_workflow_ir(
        &fixture_ir(vec![job(
            "demo",
            "Demo",
            Vec::new(),
            vec![plain, preflight, mbx, version_check],
        )]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(
        text.contains("github.ref_protected == true && 'write' || 'read'"),
        "only protected default-branch pushes write:\n{text}"
    );
    assert!(
        text.contains("MBX_GC_AUTO: \"1\""),
        "MBX jobs enable GC:\n{text}"
    );
    assert!(text.contains("MBX_SHARE_OUT_DIR: \"0\""), "{text}");
    assert!(text.contains(&format!("{MBX_CACHE_MODE_ENV}:")), "{text}");

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
        !cargo_text.contains("jdx/mr-boxington-action")
            && !cargo_text.contains("MBX_SHARE_OUT_DIR"),
        "Cargo-only jobs do not carry native MBX cache policy:\n{cargo_text}"
    );
    Ok(())
}

/// Lane extraction preserves the same GC and `OUT_DIR` policy on both runners.
#[test]
fn mbx_job_policy_applies_to_hosted_and_scale_set_lanes() -> Result<(), RenderError> {
    let uses = format!("jdx/mr-boxington-action@{}", "a".repeat(40));
    let mbx = mbx_tool_steps(&uses, "1.21.1", "1.98.1")?;
    let checkout = checkout_step(&checkout_pin())?;
    let hosted = job(
        &format!("rust-demo{HOSTED_SUFFIX}"),
        "Rust demo hosted",
        Vec::new(),
        vec![
            checkout.clone(),
            mbx[0].clone(),
            mbx[1].clone(),
            mbx[2].clone(),
        ],
    );
    let mut local = job(
        &format!("rust-demo{SCALE_SUFFIX}"),
        "Rust demo scale set",
        Vec::new(),
        vec![checkout, mbx[0].clone(), mbx[1].clone(), mbx[2].clone()],
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
    assert_eq!(text.matches("MBX_GC_AUTO: \"1\"").count(), 2, "{text}");
    assert_eq!(
        text.matches("MBX_SHARE_OUT_DIR: \"0\"").count(),
        2,
        "{text}"
    );
    assert_eq!(
        text.matches("MBX_CACHE_DIR: ${{ runner.temp }}/velnor/mbx")
            .count(),
        6,
        "preflight, action main/post, and version guard use one path per job: {text}"
    );
    assert!(
        !text.contains("isolate-objects-cache"),
        "v1.6 has no isolation input"
    );
    Ok(())
}
