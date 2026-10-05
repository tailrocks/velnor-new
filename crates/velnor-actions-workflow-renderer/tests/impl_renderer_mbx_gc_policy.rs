//! Automatic MBX collection must cover setup, build, and test steps.

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_contract::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};
use velnor_actions_workflow_renderer::steps::{MBX_CACHE_MODE_ENV, checkout_step};
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir};

use super::impl_renderer_fixtures::*;

/// Hosted Linux MBX jobs disable collection and shared OUT_DIR.
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
        text.contains(&format!(
            "{MBX_CACHE_MODE_ENV}: ${{{{ runner.environment == 'github-hosted' && github.event_name == 'push'"
        )),
        "hosted cache mode is event-gated:\n{text}"
    );
    assert!(
        text.contains("github.ref_protected == true && 'write' || 'read'"),
        "untrusted events stay read-only:\n{text}"
    );
    assert!(
        text.contains("isolate-objects-cache: ${{ runner.environment == 'github-hosted' && runner.os == 'Linux' }}"),
        "isolation activates only on hosted Linux runners:\n{text}"
    );
    assert!(
        text.contains(
            "cache-key-suffix: ${{ runner.environment == 'github-hosted' && github.job || '' }}"
        ),
        "hosted jobs use private primary keys:\n{text}"
    );
    assert!(
        text.contains("MBX_GC_AUTO: \"0\""),
        "hosted Linux MBX jobs disable asynchronous collection:\n{text}"
    );
    assert!(
        text.contains("MBX_SHARE_OUT_DIR: \"0\""),
        "hosted Linux MBX jobs disable shared OUT_DIR:\n{text}"
    );
    let policy = text.find("MBX_SHARE_OUT_DIR: \"0\"").expect("share env");
    let restore = text.find("name: Restore MBX objects").expect("restore");
    assert!(policy < restore, "job env precedes restore:\n{text}");

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
        !cargo_text.contains("MBX_SHARE_OUT_DIR"),
        "Cargo-only jobs do not receive MBX environment:\n{cargo_text}"
    );
    assert!(
        !cargo_text.contains("Export MBX single bundle"),
        "Cargo-only jobs do not export an MBX bundle:\n{cargo_text}"
    );
    Ok(())
}

/// Hosted Linux receives both env keys. Scale Set receives neither.
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
    assert_eq!(text.matches("MBX_GC_AUTO: \"0\"").count(), 1, "{text}");
    assert_eq!(
        text.matches("MBX_SHARE_OUT_DIR: \"0\"").count(),
        1,
        "{text}"
    );
    let local_start = text.find("rust-demo__local:").expect("scale-set job");
    assert!(
        !text[local_start..].contains("MBX_GC_AUTO"),
        "Scale Set keeps its existing collection policy:\n{text}"
    );
    assert!(
        !text[local_start..].contains("MBX_SHARE_OUT_DIR"),
        "Scale Set keeps its existing OUT_DIR policy:\n{text}"
    );
    Ok(())
}
