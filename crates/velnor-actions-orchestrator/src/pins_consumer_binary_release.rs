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
            "c72477ae6c3a8e7d817a322f43da70e4643e75ee5c21265f1c19c894273b7428"
        }
        ReleaseTarget::MacosArm64 => {
            "dd77de9a7574e38d8409fb06fa85e1581415896639d075987791d8c4e5a57b73"
        }
        ReleaseTarget::MacosX86_64 => {
            return Err(OrchestratorError::Contract {
                problem: "consumer_binary_release_unsupported_mise_target:macos-x64".to_owned(),
            });
        }
    };
    Ok(MiseSetup {
        uses: mise_action_uses(config)?,
        version: "2026.10.8".to_owned(),
        sha256: sha256.to_owned(),
    })
}
