//! Source-only native qualification rendering, independent of runtime admission.

use std::path::Path;

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_workflow_renderer::{
    RenderedFile, RenderedTree, foundation_qualification as document,
};

use crate::OrchestratorError;
use crate::cover_identity::generator::sha256_hex as source_sha256;

#[path = "foundation_qualification_source.rs"]
mod source;

const WORKFLOW_PATH: &str = document::WORKFLOW_PATH;

/// Planned path owned by the same policy gate as source rendering.
pub(crate) fn planned_path(policy: WorkflowPolicy) -> Option<&'static str> {
    (policy == WorkflowPolicy::VelnorRepositoryV1).then_some(WORKFLOW_PATH)
}

/// Register only generator-owned source qualification infrastructure.
pub(crate) fn files(
    prep: &crate::prepare::GenerationPreparation,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    if planned_path(prep.config.workflow.policy).is_none() {
        return Ok(Vec::new());
    }
    crate::prepare::check_velnor_identity(&prep.root, &prep.config)?;
    files_for_policy(
        prep.config.workflow.policy,
        &source::published_foundation_source()?,
    )
}

/// Stage the reviewed first-step action workflow outside the repository.
///
/// Source publication is independent of runtime Foundation qualification. This
/// entrypoint reads config and repository identity, then writes source only. It
/// performs no project discovery, installation, native execution or dispatch.
/// # Errors
/// Requires canonical generator policy/identity, the compiled reviewed source
/// publication tuple, and an empty safe external destination.
pub fn preview_foundation_qualification(
    root: &Path,
    destination: &Path,
) -> Result<Vec<String>, OrchestratorError> {
    preview_with_source(root, destination, &source::published_foundation_source()?)
}

fn files_for_policy(
    policy: WorkflowPolicy,
    published: &source::PublishedFoundationSource,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    if planned_path(policy).is_none() {
        return Ok(Vec::new());
    }
    render(published)
}

fn preview_with_source(
    root: &Path,
    destination: &Path,
    published: &source::PublishedFoundationSource,
) -> Result<Vec<String>, OrchestratorError> {
    let root = crate::resolve_root(root)?;
    let config = crate::config::load_config(&root)?;
    if planned_path(config.workflow.policy).is_none() {
        return Err(OrchestratorError::IdentityRejected {
            problem: "foundation_qualification_requires_velnor_repository_policy".to_owned(),
        });
    }
    crate::prepare::check_velnor_identity(&root, &config)?;
    let tree = RenderedTree {
        files: files_for_policy(config.workflow.policy, published)?,
        symlinks: Vec::new(),
    };
    let preview = crate::generate::guards::prepare_preview_dir(&root, destination)?;
    crate::generate::write_tree(&preview.join(".github"), &tree)?;
    Ok(tree.paths())
}

/// Authenticate publication upstream; delegate only neutral serialization inputs.
fn render(
    published: &source::PublishedFoundationSource,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let action = document::SourceActionReference::new(published.action_reference())?;
    let file = document::render(&action, env!("CARGO_PKG_VERSION")).map_err(|error| {
        if matches!(&error, velnor_actions_workflow_renderer::RenderError::InvalidWorkflow(
            problem
        ) if problem == "foundation_qualification_source_template_identity")
        {
            invalid("source_template_identity")
        } else {
            error.into()
        }
    })?;
    Ok(vec![file])
}

fn invalid(problem: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("foundation_qualification_{problem}"),
    }
}

#[cfg(test)]
#[path = "foundation_qualification_tests.rs"]
mod tests;
