use velnor_actions_mise::PinnedTool;
use velnor_actions_workflow_renderer::render::RenderedFile;
use velnor_actions_workflow_renderer::upgrades::{UpgradesSpec, render_upgrades_workflow};

use crate::OrchestratorError;
use crate::prepare::GenerationPreparation;
use crate::workflow::CHECKOUT_USES;

pub(crate) fn upgrades_enabled(prep: &GenerationPreparation) -> bool {
    prep.config
        .upgrades
        .as_ref()
        .is_some_and(|upgrades| upgrades.enabled)
}

pub(crate) fn upgrades_files(
    prep: &GenerationPreparation,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let Some(config) = prep.config.upgrades.as_ref().filter(|u| u.enabled) else {
        return Ok(Vec::new());
    };
    let spec = UpgradesSpec {
        config: config.clone(),
        runs_on: prep.runner_label.clone(),
        checkout_uses: CHECKOUT_USES.to_owned(),
        mise_setup: crate::pins::resolve_mise_setup(&prep.config, &prep.runner_label)?,
        gh_version: velnor_actions_mise::ToolCatalog::pinned()
            .version(PinnedTool::Gh)
            .to_owned(),
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        generate_tools: [PinnedTool::Jactionlint, PinnedTool::Shellcheck]
            .into_iter()
            .map(|tool| velnor_actions_mise::ToolCatalog::pinned().tool_spec(tool))
            .collect(),
        verify_rust_script: crate::workflow::toolchain_script(
            "${VELNOR_TOOL_RUST}",
            crate::workflow::ToolchainPhase::Installed,
            velnor_actions_mise::catalog::RUST_CHANNEL_MANIFESTS,
        ),
        jactionlint_verify: crate::workflow::workflow_policy::jactionlint_verify_step::script(
            &velnor_actions_mise::ToolCatalog::pinned(),
        ),
    };
    Ok(vec![render_upgrades_workflow(&spec)?])
}
