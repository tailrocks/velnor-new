//! Local object export preserves the established lane-specific GC policy.

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_contract::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};
use velnor_actions_workflow_renderer::steps::{MBX_CACHE_MODE_ENV, checkout_step};
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir};

use super::impl_renderer_fixtures::*;

#[test]
fn hosted_export_disables_gc_while_scale_set_keeps_its_existing_policy() -> Result<(), RenderError>
{
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
        2,
        "{text}"
    );
    assert_eq!(text.matches("backend: local").count(), 2, "{text}");
    assert_eq!(
        text.matches("ACTIONS_CACHE_MODE: read").count(),
        2,
        "{text}"
    );
    assert_eq!(
        text.matches("name: Restore MBX single bundle").count(),
        2,
        "{text}"
    );
    assert_eq!(
        text.matches("name: Save MBX single bundle").count(),
        2,
        "{text}"
    );
    assert!(!text.contains("mbx gc"), "{text}");

    let hosted_at = text.find("rust-demo__hosted:").expect("hosted job");
    let local_at = text.find("rust-demo__local:").expect("scale-set job");
    let hosted = &text[hosted_at..local_at];
    let local = &text[local_at..];
    assert!(hosted.contains("MBX_GC_AUTO: \"0\""), "{hosted}");
    assert!(!local.contains("MBX_GC_AUTO"), "{local}");
    assert!(hosted.contains("MBX_SHARE_OUT_DIR: \"0\""), "{hosted}");
    assert!(local.contains("MBX_SHARE_OUT_DIR: \"0\""), "{local}");
    assert!(
        hosted.contains(&format!("{MBX_CACHE_MODE_ENV}: read")),
        "{hosted}"
    );
    assert!(
        local.contains(&format!("{MBX_CACHE_MODE_ENV}: read")),
        "{local}"
    );
    Ok(())
}

#[test]
fn cargo_only_jobs_receive_no_mbx_cache_policy() -> Result<(), RenderError> {
    let plain = checkout_step(&checkout_pin())?;
    let text = render_workflow_ir(
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
    assert!(!text.contains("MBX_GC_AUTO"), "{text}");
    assert!(!text.contains("MBX_SHARE_OUT_DIR"), "{text}");
    assert!(!text.contains("Restore MBX single bundle"), "{text}");
    Ok(())
}
