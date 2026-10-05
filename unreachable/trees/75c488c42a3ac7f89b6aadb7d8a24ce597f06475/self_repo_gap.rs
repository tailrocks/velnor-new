//! Attach the actionlint `$/` tool-gap ignore after lane sharing.

use velnor_actions_actionlint::append_self_repo_action_gap;
use velnor_actions_workflow_renderer::render::{RenderedFile, WORKFLOW_PATH};

use crate::OrchestratorError;

/// Ignore actionlint 1.7.12's false positive on `$/` only where it appears.
pub(super) fn note(
    actionlint: &str,
    workflow: &str,
    extra: &[RenderedFile],
) -> Result<String, OrchestratorError> {
    let mut files = Vec::with_capacity(extra.len() + 1);
    files.push((WORKFLOW_PATH, workflow));
    for file in extra {
        files.push((file.path.as_str(), file.bytes.as_str()));
    }
    append_self_repo_action_gap(actionlint, &files).map_err(OrchestratorError::from)
}
