//! Generated-tree limits apply to the base and extra workflow families.

use std::collections::BTreeSet;
use velnor_actions_contract_config::{RoutingWorkflow, WorkflowPolicy};
use velnor_actions_contract_workflow::ScheduleTrigger;
use velnor_actions_workflow_renderer::freshness::{FreshnessSpec, render_freshness_workflow};
use velnor_actions_workflow_renderer::release_tree::render_release_workflow;
use velnor_actions_workflow_renderer::schema2::MbxQualificationPins;
use velnor_actions_workflow_renderer::schema2::Schema2WorkflowRequest;
use velnor_actions_workflow_renderer::setup::MiseSetup;
use velnor_actions_workflow_renderer::{
    MAX_WORKFLOW_BYTES, RenderError, RenderedFile, render_schema2_workflows, render_tree,
    render_tree_with_extra, render_workflow_ir, with_marker,
};

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
fn direct_release_renderer_rejects_an_oversized_workflow() -> Result<(), RenderError> {
    let mut spec = super::impl_renderer_release_tree::spec()?;
    spec.triggers.schedule = Some(oversized_schedule());
    let error = render_release_workflow(&spec, &super::impl_renderer_release_tree::ctx())
        .expect_err("direct release render must enforce the cap");
    assert!(
        error
            .to_string()
            .contains("workflow_too_large:.github/workflows/release.yml")
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
fn direct_schema2_renderer_rejects_an_oversized_workflow() -> Result<(), RenderError> {
    let large_mbx_version = format!("{}.0.0", "9".repeat(MAX_WORKFLOW_BYTES / 4));
    let request = Schema2WorkflowRequest {
        version: VERSION.to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::from([RoutingWorkflow::Qualification]),
        mbx_qualification: Some(MbxQualificationPins {
            mise_setup: MiseSetup {
                uses: "jdx/mise-action@0123456789abcdef0123456789abcdef01234567".to_owned(),
                version: "2026.9.18".to_owned(),
                sha256: "a".repeat(64),
            },
            candidate_action_uses:
                "jdx/mr-boxington-action@0123456789abcdef0123456789abcdef01234567".to_owned(),
            mbx_version: large_mbx_version,
            rust_version: "1.98.0".to_owned(),
        }),
        generator_release: None,
    };
    let error =
        render_schema2_workflows(&request).expect_err("direct routing render must enforce the cap");
    assert!(
        error
            .to_string()
            .contains("workflow_too_large:.github/workflows/qualification.yml")
    );
    Ok(())
}
