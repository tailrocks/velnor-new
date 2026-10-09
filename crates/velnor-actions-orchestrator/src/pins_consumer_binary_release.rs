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
            "3f44343eebc7e0d6623bcea46e304864f02dff648edd75c82871b53cc697b366"
        }
        ReleaseTarget::MacosArm64 => {
            "bbcea7b0f844d026424a4c8335357a15a2f5c9e9132c9408de990d9be6f26101"
        }
        ReleaseTarget::MacosX86_64 => {
            return Err(OrchestratorError::Contract {
                problem: "consumer_binary_release_unsupported_mise_target:macos-x64".to_owned(),
            });
        }
    };
    Ok(MiseSetup {
        uses: mise_action_uses(config)?,
        version: "2026.10.6".to_owned(),
        sha256: sha256.to_owned(),
    })
}
