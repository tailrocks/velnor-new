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
mod tests;
