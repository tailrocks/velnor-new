//! Protected post-merge `OpenTofu` apply workflow emission.

use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::render::RenderedFile;
use velnor_actions_workflow_renderer::{TofuApplySpec, render_tofu_apply_workflow};

use crate::OrchestratorError;
use crate::pins::resolve_mise_setup;
use crate::prepare::GenerationPreparation;
use crate::workflow::CHECKOUT_USES;

/// Whether the optional protected apply workflow is configured.
pub(crate) fn tofu_apply_enabled(prep: &GenerationPreparation) -> bool {
    prep.config.workflow.tofu_apply.is_some()
}

/// Render the optional apply workflow from closed config, or emit no file.
///
/// # Errors
///
/// Returns pin-resolution or workflow-rendering errors.
pub(crate) fn tofu_apply_files(
    prep: &GenerationPreparation,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let Some(config) = prep.config.workflow.tofu_apply.as_ref() else {
        return Ok(Vec::new());
    };
    let spec = TofuApplySpec {
        config: config.clone(),
        default_branch: prep.default_branch.clone(),
        runs_on: prep.runner_label.clone(),
        checkout_uses: CHECKOUT_USES.to_owned(),
        mise_setup: resolve_mise_setup(&prep.config, &prep.runner_label)?,
        opentofu_version: ToolCatalog::pinned()
            .version(PinnedTool::Opentofu)
            .to_owned(),
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    Ok(vec![render_tofu_apply_workflow(&spec)?])
}
