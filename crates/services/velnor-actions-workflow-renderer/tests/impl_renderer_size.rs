//! Generated-tree limits apply to the base and extra workflow families.

use velnor_actions_contract_config::WorkflowPolicy;
use velnor_actions_contract_workflow::ScheduleTrigger;
use velnor_actions_workflow_jobs::freshness::{FreshnessSpec, render_freshness_workflow};
use velnor_actions_workflow_renderer::{render_tree, render_tree_with_extra, render_workflow_ir};
use velnor_actions_workflow_steps::RenderError;
use velnor_actions_workflow_tree::{MAX_WORKFLOW_BYTES, RenderedFile, with_marker};

const VERSION: &str = "0.1.0";

fn oversized_schedule() -> ScheduleTrigger {
    ScheduleTrigger {
        cron: vec!["0 6 * * 1".to_owned(); 30_000],
    }
}

fn workflow_with_size(size: usize) -> Result<String, RenderError> {
    let header = with_marker(VERSION, "")?;
    if size < header.len() {
        return Err(RenderError::InvalidWorkflow(
            "test_size_below_marker".to_owned(),
        ));
    }
    Ok(format!("{header}{}", "x".repeat(size - header.len())))
}

#[test]
fn base_workflow_boundary_passes_and_one_byte_over_fails() -> Result<(), RenderError> {
    let actionlint = with_marker(VERSION, "config-variables: []\n")?;
    let exact = workflow_with_size(MAX_WORKFLOW_BYTES)?;
    assert_eq!(exact.len(), MAX_WORKFLOW_BYTES);
    assert!(render_tree(&exact, &actionlint, VERSION).is_ok());

    let over = workflow_with_size(MAX_WORKFLOW_BYTES + 1)?;
    assert_eq!(over.len(), MAX_WORKFLOW_BYTES + 1);
    let error =
        render_tree(&over, &actionlint, VERSION).expect_err("base workflow over the cap must fail");
    assert!(matches!(error, RenderError::InvalidWorkflow(problem)
        if problem == "workflow_too_large:.github/workflows/ci.yml:500001:500000"));
    Ok(())
}

#[test]
fn extra_release_workflow_boundary_is_checked_too() -> Result<(), RenderError> {
    let workflow = with_marker(VERSION, "name: CI\n")?;
    let actionlint = with_marker(VERSION, "config-variables: []\n")?;
    let exact = RenderedFile {
        path: ".github/workflows/release.yml".to_owned(),
        bytes: workflow_with_size(MAX_WORKFLOW_BYTES)?,
    };
    assert!(render_tree_with_extra(&workflow, &actionlint, &[exact], VERSION).is_ok());

    let over = RenderedFile {
        path: ".github/workflows/release.yml".to_owned(),
        bytes: workflow_with_size(MAX_WORKFLOW_BYTES + 1)?,
    };
    let error = render_tree_with_extra(&workflow, &actionlint, &[over], VERSION)
        .expect_err("release workflow over the cap must fail");
    assert!(matches!(error, RenderError::InvalidWorkflow(problem)
        if problem == "workflow_too_large:.github/workflows/release.yml:500001:500000"));
    Ok(())
}

#[test]
fn direct_ci_renderer_rejects_an_oversized_workflow() -> Result<(), RenderError> {
    let mut ir = super::impl_renderer_tree::fixture_ir()?;
    ir.name = "x".repeat(MAX_WORKFLOW_BYTES + 1);
    let error = render_workflow_ir(
        &ir,
        WorkflowPolicy::ConsumerV1,
        None,
        &super::impl_renderer_tree::fixture_ctx(),
    )
    .expect_err("direct CI render must enforce the cap");
    assert!(
        error
            .to_string()
            .contains("workflow_too_large:.github/workflows/ci.yml")
    );
    Ok(())
}

#[test]
fn direct_freshness_renderer_rejects_an_oversized_workflow() {
    let spec = FreshnessSpec {
        schedule: ScheduleTrigger {
            cron: oversized_schedule().cron,
        },
        runs_on: "ubuntu-26.04".to_owned(),
        checkout_uses: "actions/checkout@0123456789abcdef0123456789abcdef01234567".to_owned(),
        generator_version: VERSION.to_owned(),
    };
    let error =
        render_freshness_workflow(&spec).expect_err("direct freshness render must enforce the cap");
    assert!(
        error
            .to_string()
            .contains("workflow_too_large:.github/workflows/freshness.yml")
    );
}

#[test]
fn tree_assembly_rejects_hand_edited_workflow_markers() -> Result<(), RenderError> {
    let generated = with_marker(VERSION, "run: generated\n")?;
    let actionlint = with_marker(VERSION, "config-variables: []\n")?;
    assert!(render_tree(&generated, &actionlint, VERSION).is_ok());

    let hand_edited = format!("run: hand-edited\n{generated}");
    let error = render_tree(&hand_edited, &actionlint, VERSION)
        .expect_err("a hand-edited workflow must not bypass generated-tree validation");
    assert!(matches!(error, RenderError::BadMarker { .. }), "{error}");
    Ok(())
}
