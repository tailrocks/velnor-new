//! Generated-tree limits apply to the base and extra workflow families.

use velnor_actions_workflow_renderer::{
    MAX_WORKFLOW_BYTES, RenderError, RenderedFile, render_tree, render_tree_with_extra, with_marker,
};

const VERSION: &str = "0.1.0";

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
