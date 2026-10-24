//! Opt-in `upgrades.yml` emission (`[upgrades] enabled = true`).
//!
//! Generator output only: the workflow exists exactly when the config
//! enables it, and `generate` retires it when the table is removed.

use velnor_actions_mise::PinnedTool;
use velnor_actions_workflow_renderer::render::RenderedFile;
use velnor_actions_workflow_renderer::upgrades::{UpgradesSpec, render_upgrades_workflow};

use crate::OrchestratorError;
use crate::prepare::GenerationPreparation;
use crate::workflow::CHECKOUT_USES;

/// True when `generate` emits `upgrades.yml`; shared with `plan` listing.
pub(crate) fn upgrades_enabled(prep: &GenerationPreparation) -> bool {
    prep.config
        .upgrades
        .as_ref()
        .is_some_and(|upgrades| upgrades.enabled)
}

/// Render `upgrades.yml`, or nothing when the table is absent or disabled.
///
/// # Errors
///
/// Returns render errors for invalid spec scalars or unsupported runners.
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
        // The GitHub CLI runs with a token, so it is always Velnor's pin.
        gh_version: velnor_actions_mise::ToolCatalog::pinned()
            .version(PinnedTool::Gh)
            .to_owned(),
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        // `generate` fail-closed-execs these validators (the same set the
        // plan job installs for `Check generated files`).
        generate_tools: [PinnedTool::Jactionlint, PinnedTool::Shellcheck]
            .into_iter()
            .map(|tool| velnor_actions_mise::ToolCatalog::pinned().tool_spec(tool))
            .collect(),
        // The project Rust is resolved at run time, so the script reads it
        // from `VELNOR_TOOL_RUST` like every other generated check.
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

#[cfg(test)]
#[path = "upgrades_emit_tests.rs"]
mod verifier_output_tests;
