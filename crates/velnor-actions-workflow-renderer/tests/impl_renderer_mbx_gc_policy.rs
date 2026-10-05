//! Automatic MBX collection must cover setup, build, and test steps.

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_contract::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};
use velnor_actions_workflow_renderer::steps::{MBX_CACHE_MODE_ENV, checkout_step};
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir};

use super::impl_renderer_fixtures::*;

/// Rendered GC policy reaches MBX only and composes with push-only saves.
#[test]
fn action_step_env_renders_only_when_present() -> Result<(), RenderError> {
    let uses = format!("jdx/mr-boxington-action@{}", "a".repeat(40));
    let [preflight, mbx] = mbx_tool_steps(&uses, "1.21.1", "1.98.1")?;
    let plain = checkout_step(&checkout_pin())?;
    let text = render_workflow_ir(
        &fixture_ir(vec![job(
            "demo",
            "Demo",
            Vec::new(),
            vec![plain, preflight, mbx],
        )]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(
        text.contains(&format!("{MBX_CACHE_MODE_ENV}: read")),
        "action post stays restore-only:\n{text}"
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
    let mbx = mbx_tool_steps(&uses, "1.21.1", "1.98.1")?;
    let checkout = checkout_step(&checkout_pin())?;
    let hosted = job(
        &format!("rust-demo{HOSTED_SUFFIX}"),
        "Rust demo hosted",
        Vec::new(),
        vec![checkout.clone(), mbx[0].clone(), mbx[1].clone()],
    );
    let mut local = job(
        &format!("rust-demo{SCALE_SUFFIX}"),
        "Rust demo scale set",
        Vec::new(),
        vec![checkout, mbx[0].clone(), mbx[1].clone()],
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
