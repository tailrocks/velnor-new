//! Over-cap representation fallback for the generated CI workflow.

use crate::{
    RenderError, marker, workflow_size,
    yaml::{Yaml, render_yaml, share_step_run_scalars},
};

/// Render the complete marked document before the caller applies the size guard.
pub(super) fn render_marked_workflow(
    path: &str,
    document: &Yaml,
    generator_version: &str,
) -> Result<String, RenderError> {
    let canonical = marker::with_marker(generator_version, &render_yaml(document))?;
    if !workflow_size::is_workflow_path(path)
        || canonical.len() <= workflow_size::MAX_WORKFLOW_BYTES
    {
        #[cfg(feature = "test-render-capture")]
        if workflow_size::is_workflow_path(path) {
            crate::render::test_render_capture::record(path, canonical.clone(), canonical.clone());
        }
        return Ok(canonical);
    }
    let shared = share_step_run_scalars(document.clone());
    let transformed = marker::with_marker(generator_version, &render_yaml(&shared))?;
    #[cfg(feature = "test-render-capture")]
    let canonical_capture = canonical.clone();
    let selected = if transformed.len() < canonical.len() {
        transformed
    } else {
        canonical
    };
    #[cfg(feature = "test-render-capture")]
    if workflow_size::is_workflow_path(path) {
        crate::render::test_render_capture::record(path, canonical_capture, selected.clone());
    }
    Ok(selected)
}

/// Render the fallback and retain the production size rejection.
pub(crate) fn render_checked_workflow(
    path: &str,
    document: &Yaml,
    generator_version: &str,
) -> Result<String, RenderError> {
    let text = render_marked_workflow(path, document, generator_version)?;
    workflow_size::check_workflow_size(path, &text)?;
    Ok(text)
}

#[cfg(test)]
#[path = "render_fallback_tests.rs"]
mod tests;
