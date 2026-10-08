//! Release-owned paths listed in the deterministic generation plan.

use velnor_actions_workflow_renderer::release_tree::RELEASE_TREE_PATHS;
use velnor_actions_workflow_renderer::rust_binary_release::BINARY_RELEASE_WORKFLOW_PATH;

use crate::plan::push;
use crate::prepare::GenerationPreparation;

/// Append the release paths emitted by `generate` for this configuration.
///
/// The checked plan entrypoint renders the staged tree first, so invalid
/// release configuration has already returned an error.
pub(crate) fn release_file_lines(out: &mut String, prep: &GenerationPreparation) {
    let enabled = crate::release_emit::enabled_release(prep)
        .ok()
        .flatten()
        .is_some();
    let binary_enabled = prep
        .config
        .stacks
        .rust
        .as_ref()
        .is_some_and(|stack| stack.binary_release.enabled);
    let mut paths = Vec::new();
    if enabled {
        paths.extend(RELEASE_TREE_PATHS.iter().copied());
    }
    if binary_enabled {
        paths.push(BINARY_RELEASE_WORKFLOW_PATH);
    }
    paths.sort_unstable();
    for path in paths {
        push(out, &format!("  {path}"));
    }
}
