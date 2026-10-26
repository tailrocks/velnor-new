// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{
    RenderError, marker, workflow_size,
    yaml::{Yaml, render_yaml, share_workflow_nodes},
};

pub(super) fn render_marked_workflow(
    path: &str,
    document: &Yaml,
    generator_version: &str,
) -> Result<String, RenderError> {
    render_marked_workflow_with(path, document, generator_version, |path, document| {
        crate::workflow_timeout::validate(path, document)
    })
}

fn render_marked_workflow_with(
    path: &str,
    document: &Yaml,
    generator_version: &str,
    validate: impl FnOnce(&str, &Yaml) -> Result<(), RenderError>,
) -> Result<String, RenderError> {
    let mut bound = document.clone();
    crate::project_tool_resolver::bind_workflow(&mut bound)?;
    let document = &bound;
    validate(path, document)?;
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
    let shared = share_workflow_nodes(document.clone())?;
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

pub(crate) fn render_checked_workflow(
    path: &str,
    document: &Yaml,
    generator_version: &str,
) -> Result<String, RenderError> {
    let text = render_marked_workflow(path, document, generator_version)?;
    workflow_size::check_workflow_size(path, &text)?;
    Ok(text)
}

pub(crate) fn render_checked_homebrew_adoption_caller(
    path: &str,
    document: &Yaml,
    generator_version: &str,
    scope: &crate::workflow_timeout::HomebrewAdoptionCalleeScope,
) -> Result<String, RenderError> {
    let text = render_marked_workflow_with(path, document, generator_version, |path, document| {
        crate::workflow_timeout::validate_homebrew_adoption_caller(path, document, scope)
    })?;
    workflow_size::check_workflow_size(path, &text)?;
    Ok(text)
}
