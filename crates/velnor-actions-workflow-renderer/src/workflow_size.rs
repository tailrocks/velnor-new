//! Byte limit for every generated GitHub Actions workflow document.

use crate::RenderError;

/// Maximum rendered workflow size, including the generator marker.
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
    path.starts_with(".github/workflows/") && (path.ends_with(".yml") || path.ends_with(".yaml"))
}

#[cfg(test)]
mod tests {
    use super::{MAX_WORKFLOW_BYTES, check_workflow_size};
    use crate::RenderError;

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
    fn size_limit_ignores_non_workflow_files() {
        let over = "x".repeat(MAX_WORKFLOW_BYTES + 1);
        assert!(check_workflow_size(".github/actionlint.yaml", &over).is_ok());
        assert!(check_workflow_size("docs/ci.yml", &over).is_ok());
    }
}
