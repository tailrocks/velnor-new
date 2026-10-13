//! Latest verified Mise pins for consumer binary release jobs.

use super::{OrchestratorError, mise_action_uses};
use velnor_actions_contract::{ReleaseTarget, VelnorConfig};
use velnor_actions_workflow_renderer::MiseSetup;
use velnor_actions_workflow_renderer::setup::{
    MISE_BINARY_SHA256_LINUX_X64, MISE_BINARY_SHA256_MACOS_ARM64,
};

/// Resolve the latest verified Mise binary without changing the generator pin.
pub(crate) fn resolve_mise_setup_for_consumer_binary_release(
    config: &VelnorConfig,
    target: ReleaseTarget,
) -> Result<MiseSetup, OrchestratorError> {
    let sha256 = match target {
        ReleaseTarget::LinuxX86_64 => MISE_BINARY_SHA256_LINUX_X64,
        ReleaseTarget::MacosArm64 => MISE_BINARY_SHA256_MACOS_ARM64,
        ReleaseTarget::MacosX86_64 => {
            return Err(OrchestratorError::Contract {
                problem: "consumer_binary_release_unsupported_mise_target:macos-x64".to_owned(),
            });
        }
    };
    Ok(MiseSetup {
        uses: mise_action_uses(config)?,
        version: "2026.10.9".to_owned(),
        sha256: sha256.to_owned(),
    })
}
