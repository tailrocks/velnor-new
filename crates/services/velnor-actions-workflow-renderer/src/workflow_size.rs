//! Byte limit for every generated GitHub Actions workflow document.

use crate::RenderError;
use std::path::Path;

/// Maximum rendered workflow size, including the marker; decimal 500 KB.
///
/// GitHub Actions documents a 500 KB per-file ceiling. V1 applies an exact
/// 500,000-byte cap so every accepted workflow stays within that published
/// limit.
pub const MAX_WORKFLOW_BYTES: usize = 500_000;

/// Reject an oversized generated workflow after its marker has been added.
///
/// Non-workflow files do not use this limit. The tree assembler applies the
/// same check to every workflow path, including workflows supplied as extras.
/// # Errors
pub(crate) fn check_workflow_size(path: &str, bytes: &str) -> Result<(), RenderError> {
    if !is_workflow_path(path) || bytes.len() <= MAX_WORKFLOW_BYTES {
        return Ok(());
    }
    Err(RenderError::InvalidWorkflow(format!(
        "workflow_too_large:{path}:{}:{MAX_WORKFLOW_BYTES}",
        bytes.len()
    )))
}

/// True for generated `.yml` or `.yaml` files under GitHub's workflow tree.
fn is_workflow_path(path: &str) -> bool {
    path.starts_with(".github/workflows/")
        && Path::new(path).extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("yml") || extension.eq_ignore_ascii_case("yaml")
        })
}

#[cfg(test)]
mod tests {
    use super::{MAX_WORKFLOW_BYTES, check_workflow_size};
    use crate::RenderError;

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
}
