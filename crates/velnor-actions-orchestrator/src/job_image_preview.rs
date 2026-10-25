//! Generate the job image candidate workflow without project task discovery.

use std::path::Path;

use velnor_actions_contract::{ExecutionMode, WorkflowPolicy};
use velnor_actions_workflow_renderer::RenderedTree;

use crate::{OrchestratorError, config::load_config, generate, prepare::check_velnor_identity};

/// Write only the job image candidate workflow outside the source repository.
///
/// # Errors
///
/// Incorrect source identity, configuration, or output paths cause an error.
pub fn preview_job_image_candidate(
    root: &Path,
    destination: &Path,
    dispatch: Option<ExecutionMode>,
) -> Result<Vec<String>, OrchestratorError> {
    let root = crate::resolve_root(root)?;
    let config = load_config(&root)?;
    if config.workflow.policy != WorkflowPolicy::VelnorRepositoryV1 {
        return Err(OrchestratorError::IdentityRejected {
            problem: "job_image_candidate_requires_velnor_repository_policy".to_owned(),
        });
    }
    check_velnor_identity(&root, &config)?;
    let catalog = crate::tool_catalog_resolve::resolve_tool_catalog(&root, &config)?;
    let file = crate::routing::job_image_candidate_file(
        &config,
        &catalog.for_emission(),
        env!("CARGO_PKG_VERSION"),
        dispatch,
    )?;
    let tree = RenderedTree {
        files: vec![file],
        symlinks: Vec::new(),
    };
    let preview = generate::guards::prepare_preview_dir(&root, destination)?;
    generate::write_tree(&preview.join(".github"), &tree)?;
    Ok(tree.paths())
}

#[cfg(test)]
#[path = "job_image_preview_tests.rs"]
mod tests;
