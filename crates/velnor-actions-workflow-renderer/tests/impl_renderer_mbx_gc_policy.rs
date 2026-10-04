//! Automatic MBX collection must cover setup, build, and test steps.

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_contract::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};
use velnor_actions_workflow_renderer::steps::{checkout_step, mbx_objects_step};
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir};

use super::impl_renderer_fixtures::*;

/// Rendered GC policy reaches MBX only and composes with push-only saves.
#[test]
fn hosted_mbx_policy_keeps_gc_without_overriding_action_cache_lifecycle() -> Result<(), RenderError>
{
    let uses = format!("jdx/mr-boxington-action@{}", "a".repeat(40));
    let mbx = mbx_objects_step(&uses, false, "1.22.0")?;
    let plain = checkout_step(&checkout_pin())?;
    let text = render_workflow_ir(
        &fixture_ir(vec![job("demo", "Demo", Vec::new(), vec![plain, mbx])]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(
        !text.contains("ACTIONS_CACHE_MODE"),
        "action owns cache lifecycle:\n{text}"
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
        !cargo_text.contains("MBX single bundle"),
        "Cargo-only jobs have no MBX lifecycle steps:\n{cargo_text}"
    );
    Ok(())
}

/// Lane extraction keeps hosted GC enabled and leaves scale-set policy intact.
#[test]
fn mbx_gc_policy_is_scoped_to_hosted_mbx_jobs() -> Result<(), RenderError> {
    let uses = format!("jdx/mr-boxington-action@{}", "a".repeat(40));
    let mbx = mbx_objects_step(&uses, false, "1.22.0")?;
    let hosted = job(
        &format!("rust-demo{HOSTED_SUFFIX}"),
        "Rust demo hosted",
        Vec::new(),
        vec![mbx.clone()],
    );
    let mut local = job(
        &format!("rust-demo{SCALE_SUFFIX}"),
        "Rust demo scale set",
        Vec::new(),
        vec![mbx],
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
