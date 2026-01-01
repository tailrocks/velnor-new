//! Latest verified Mise pins for consumer binary release jobs.

use super::{OrchestratorError, mise_action_uses};
use velnor_actions_contract::{ReleaseTarget, VelnorConfig};
use velnor_actions_workflow_renderer::MiseSetup;

/// Resolve the latest verified Mise binary without changing the generator pin.
pub(crate) fn resolve_mise_setup_for_consumer_binary_release(
    config: &VelnorConfig,
    target: ReleaseTarget,
) -> Result<MiseSetup, OrchestratorError> {
    let sha256 = match target {
        ReleaseTarget::LinuxX86_64 => {
            "8a223b5f8ca71100220a3e5bef259614c348e7b1d80e6b15c2a9c9aa3affe5e4"
        }
        ReleaseTarget::MacosArm64 => {
            "41c4028257d30f5f5742c99247c461f417143d6c7301f167a0c185247c8f206e"
        }
        ReleaseTarget::MacosX86_64 => {
            return Err(OrchestratorError::Contract {
                problem: "consumer_binary_release_unsupported_mise_target:macos-x64".to_owned(),
            });
        }
    };
    Ok(MiseSetup {
        uses: mise_action_uses(config)?,
        version: "2026.10.5".to_owned(),
        sha256: sha256.to_owned(),
    })
}
