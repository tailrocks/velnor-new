use super::{MAX_WORKFLOW_BYTES, check_workflow_size};
use velnor_actions_workflow_steps::RenderError;

fn marked_workflow_with_byte_size(size: usize) -> Result<String, RenderError> {
    let mut workflow = crate::marker::with_marker("0.1.0", "")?;
    while workflow.len() < size {
        if size - workflow.len() >= 'é'.len_utf8() {
            workflow.push('é');
        } else {
            workflow.push('x');
        }
    }
    Ok(workflow)
}

#[test]
fn workflow_size_limit_includes_exact_boundary() {
    let exact = "x".repeat(MAX_WORKFLOW_BYTES);
    assert!(check_workflow_size(".github/workflows/ci.yml", &exact).is_ok());

    let over = "x".repeat(MAX_WORKFLOW_BYTES + 1);
    let error = check_workflow_size(".github/workflows/ci.yml", &over)
        .expect_err("one byte over must fail");
    assert!(matches!(error, RenderError::InvalidWorkflow(problem)
        if problem == "workflow_too_large:.github/workflows/ci.yml:500001:500000"));
}

#[test]
fn utf8_workflow_limit_counts_marked_rendered_bytes() -> Result<(), RenderError> {
    let exact = marked_workflow_with_byte_size(MAX_WORKFLOW_BYTES)?;
    assert!(exact.chars().count() < MAX_WORKFLOW_BYTES);
    assert_eq!(exact.len(), MAX_WORKFLOW_BYTES);
    assert!(check_workflow_size(".github/workflows/ci.yml", &exact).is_ok());

    let over = marked_workflow_with_byte_size(MAX_WORKFLOW_BYTES + 1)?;
    assert_eq!(over.len(), MAX_WORKFLOW_BYTES + 1);
    let error = check_workflow_size(".github/workflows/ci.yml", &over)
        .expect_err("one UTF-8 byte over must fail");
    assert!(matches!(error, RenderError::InvalidWorkflow(problem)
        if problem == "workflow_too_large:.github/workflows/ci.yml:500001:500000"));
    Ok(())
}

#[test]
fn size_limit_ignores_non_workflow_files() {
    let over = "x".repeat(MAX_WORKFLOW_BYTES + 1);
    assert!(check_workflow_size(".github/actionlint.yaml", &over).is_ok());
    assert!(check_workflow_size("docs/ci.yml", &over).is_ok());
}

#[test]
fn size_limit_recognizes_case_insensitive_workflow_extensions() {
    let over = "x".repeat(MAX_WORKFLOW_BYTES + 1);
    let error = check_workflow_size(".github/workflows/ci.YML", &over)
        .expect_err("case-insensitive workflow extensions must be limited");
    assert!(matches!(error, RenderError::InvalidWorkflow(problem)
        if problem == "workflow_too_large:.github/workflows/ci.YML:500001:500000"));
}
