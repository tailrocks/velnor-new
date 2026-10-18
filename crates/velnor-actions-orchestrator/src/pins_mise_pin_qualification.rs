//! Candidate Mise release pins used only by the hosted `mise-pin` mode.

use super::{OrchestratorError, mise_action_uses};
use velnor_actions_contract::{ReleaseTarget, VelnorConfig};
use velnor_actions_workflow_renderer::{MisePinQualificationPins, MiseSetup};

const VERSION: &str = "2026.10.13";
const LINUX_X64_SHA256: &str = "94c45523c7b6760b7da704f16051171ca32e8447a849a02266a439a4455e9a12";
const MACOS_X64_SHA256: &str = "4e4a996f4747eb59d55497fdd4b0d60d3add9a06c0dc5d65a70402015ee445d0";
const LINUX_X64_RUNNER: &str = "ubuntu-26.04";
const MACOS_X64_RUNNER: &str = "macos-15-intel";

/// Resolve the unqualified 2026.10.13 release pins for both x64 hosts.
pub(crate) fn resolve(
    config: &VelnorConfig,
) -> Result<MisePinQualificationPins, OrchestratorError> {
    let linux = ReleaseTarget::for_runner_label(LINUX_X64_RUNNER)
        .ok_or_else(|| unsupported_runner(LINUX_X64_RUNNER))?;
    let macos = ReleaseTarget::for_runner_label(MACOS_X64_RUNNER)
        .ok_or_else(|| unsupported_runner(MACOS_X64_RUNNER))?;
    Ok(MisePinQualificationPins {
        linux_x86_64_setup: setup(config, linux)?,
        macos_x86_64_setup: setup(config, macos)?,
    })
}

fn setup(config: &VelnorConfig, target: ReleaseTarget) -> Result<MiseSetup, OrchestratorError> {
    let sha256 = match target {
        ReleaseTarget::LinuxX86_64 => LINUX_X64_SHA256,
        ReleaseTarget::MacosX86_64 => MACOS_X64_SHA256,
        ReleaseTarget::MacosArm64 => {
            return Err(OrchestratorError::Contract {
                problem: "mise_pin_qualification_unsupported_target:macos-arm64".to_owned(),
            });
        }
    };
    Ok(MiseSetup {
        uses: mise_action_uses(config)?,
        version: VERSION.to_owned(),
        sha256: sha256.to_owned(),
    })
}

fn unsupported_runner(label: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("mise_pin_qualification_unsupported_runner:{label}"),
    }
}
