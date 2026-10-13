//! MBX collection stays off during producers and resumes only after publication.

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_contract::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};
use velnor_actions_workflow_renderer::steps::{
    MBX_CACHE_MODE_ENV, checkout_step, mbx_objects_step,
};
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir};

use super::impl_renderer_fixtures::*;

/// MBX jobs disable asynchronous GC while preserving trusted-main write policy.
#[test]
fn hosted_action_step_has_isolated_trusted_main_cache_policy() -> Result<(), RenderError> {
    let uses = format!("jdx/mr-boxington-action@{}", "a".repeat(40));
    let mbx = mbx_objects_step(&uses, false, "1.21.1")?;
    let plain = checkout_step(&checkout_pin())?;
    let text = render_workflow_ir(
        &fixture_ir(vec![job("demo", "Demo", Vec::new(), vec![plain, mbx])]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(
        text.contains(&format!(
            "{MBX_CACHE_MODE_ENV}: ${{{{ runner.environment == 'github-hosted' && github.event_name == 'push'"
        )),
        "hosted cache mode is event-gated:\n{text}"
    );
    assert!(
        text.contains("isolate-objects-cache: ${{ runner.environment == 'github-hosted' }}"),
        "isolation activates only on hosted runners:\n{text}"
    );
    assert!(
        text.contains(
            "cache-key-suffix: ${{ runner.environment == 'github-hosted' && github.job || '' }}"
        ),
        "hosted jobs use private primary keys:\n{text}"
    );
    assert!(
        text.contains("github.ref_protected == true && 'write' || 'read'"),
        "untrusted events stay read-only:\n{text}"
    );
    assert!(
        !text.contains("Collect MBX cache before export"),
        "the hosted post action must export before any collection:\n{text}"
    );
    assert!(
        text.contains("MBX_GC_AUTO: \"0\""),
        "MBX jobs disable asynchronous collection through export:\n{text}"
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

/// Lane extraction preserves one shared policy body for both runner profiles.
#[test]
fn mbx_gc_policy_covers_both_runner_lanes() -> Result<(), RenderError> {
    let uses = format!("jdx/mr-boxington-action@{}", "a".repeat(40));
    let mbx = mbx_objects_step(&uses, false, "1.21.1")?;
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
    assert_eq!(text.matches("MBX_GC_AUTO: \"0\"").count(), 2, "{text}");
    assert!(
        text.contains("uses: $/.github/actions/rust-demo"),
        "both lanes keep the shared composite:\n{text}"
    );
    assert!(
        text.contains("MBX_GC_AUTO: \"0\""),
        "both runner lanes disable asynchronous collection until safe cleanup:\n{text}"
    );
    Ok(())
}
