use super::{OrchestratorError, mise_action_uses};
use velnor_actions_contract::{ReleaseTarget, VelnorConfig};
use velnor_actions_workflow_renderer::{MisePinQualificationPins, MiseSetup};

const VERSION: &str = "2026.10.19";
const LINUX_X64_SHA256: &str = "3162760f40b6b5f706c60641e4ef5ee7322bc6c54f72fc26a7111b337aa18d9d";
const MACOS_X64_SHA256: &str = "8d5e1591811143f15334f690373cefb58c13bd6cbd0292008748fd777f6c99d8";
const LINUX_X64_RUNNER: &str = "ubuntu-26.04";
const MACOS_X64_RUNNER: &str = "macos-15-intel";

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
