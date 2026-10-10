//! Latest verified Mise pins for consumer binary release jobs.

use super::{OrchestratorError, mise_action_uses};
use velnor_actions_contract::{ReleaseTarget, VelnorConfig};
use velnor_actions_workflow_renderer::MiseSetup;

/// Resolve the official per-target Mise binary pin for consumer release jobs.
pub(crate) fn resolve_mise_setup_for_consumer_binary_release(
    config: &VelnorConfig,
    target: ReleaseTarget,
) -> Result<MiseSetup, OrchestratorError> {
    let sha256 = match target {
        ReleaseTarget::LinuxX86_64 => {
            "6eb1b890e90818417ca34c90dbbd47881917d5cd199f31b63b062ea9c6b18d85"
        }
        ReleaseTarget::MacosArm64 => {
            "f5171e341518a57e8c4e9280e28443e35d66212c51164c83be76794e0a78b014"
        }
        ReleaseTarget::MacosX86_64 => {
            return Err(OrchestratorError::Contract {
                problem: "consumer_binary_release_unsupported_mise_target:macos-x64".to_owned(),
            });
        }
    };
    Ok(MiseSetup {
        uses: mise_action_uses(config)?,
        version: "2026.10.7".to_owned(),
        sha256: sha256.to_owned(),
    })
}
