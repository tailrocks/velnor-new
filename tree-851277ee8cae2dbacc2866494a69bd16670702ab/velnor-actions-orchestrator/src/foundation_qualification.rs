//! Source-only native qualification rendering, independent of runtime admission.

use std::path::Path;

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_workflow_renderer::{RenderedFile, RenderedTree, marker};

use crate::OrchestratorError;
use crate::cover_identity::generator::sha256_hex as source_sha256;

#[path = "foundation_qualification_source.rs"]
mod source;

const WORKFLOW_PATH: &str = ".github/workflows/foundation-qualification.yml";
const TEMPLATE: &str = include_str!("foundation_qualification_workflow.yml.in");
const TEMPLATE_SHA256: &str = "01acba73d0dd12facb25f0985e9141ab16f6fb8941fdc72f54aa10f31914b6a7";
const ACTION_MARKER: &str = "__FOUNDATION_ACTION_REF__";

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
    render(published, TEMPLATE)
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

/// Equivalent to the publication owner's single-marker build API.
/// The argument is issued only by the compiled source publication owner.
fn render(
    published: &source::PublishedFoundationSource,
    template: &str,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    if source_sha256(template.as_bytes()) != TEMPLATE_SHA256
        || template.matches(ACTION_MARKER).count() != 1
    {
        return Err(invalid("source_template_identity"));
    }
    let body = template.replace(ACTION_MARKER, published.action_reference());
    Ok(vec![RenderedFile {
        path: WORKFLOW_PATH.to_owned(),
        bytes: marker::with_marker(env!("CARGO_PKG_VERSION"), &body)?,
    }])
}

fn invalid(problem: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("foundation_qualification_{problem}"),
    }
}

#[cfg(test)]
#[path = "foundation_qualification_tests.rs"]
mod tests;
