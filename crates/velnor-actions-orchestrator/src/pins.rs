// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_actionlint::actions::{MISE_ACTION_SHA, MISE_ACTION_VERSION};
use velnor_actions_actionlint::overrides::{
    ActionPinOverride as ApprovedOverride, ApprovedPinCatalog,
};
use velnor_actions_contract::{
    BuildTaskRunner, GeneratorLock, ReleaseTarget, Step, VelnorConfig, VerificationRunner,
};
use velnor_actions_mise::MISE_VERSION;
use velnor_actions_workflow_renderer::{MiseSetup, release_helpers};

use crate::OrchestratorError;
use crate::discover::Discovery;

const MISE_ACTION_KEY: &str = "jdx/mise-action";

use velnor_actions_workflow_renderer::setup::{
    MISE_BINARY_SHA256_LINUX_X64, MISE_BINARY_SHA256_MACOS_ARM64, MISE_BINARY_SHA256_MACOS_X64,
};

#[path = "pins_consumer_evidence.rs"]
mod consumer_evidence;
pub(crate) use consumer_evidence::consumer_evidence_helper;

#[path = "pins_mise_pin_qualification.rs"]
mod mise_pin_qualification;
pub(super) use mise_pin_qualification::resolve as resolve_mise_pin_qualification;

pub(crate) fn resolve_mise_setup(
    config: &VelnorConfig,
    label: &str,
) -> Result<MiseSetup, OrchestratorError> {
    let target =
        ReleaseTarget::for_runner_label(label).ok_or_else(|| OrchestratorError::Contract {
            problem: format!("mise_setup_unsupported_target:{label}"),
        })?;
    resolve_mise_setup_for_release_target(config, target)
}

pub(crate) fn resolve_mise_setup_for_release_target(
    config: &VelnorConfig,
    target: ReleaseTarget,
) -> Result<MiseSetup, OrchestratorError> {
    let sha256 = match target {
        ReleaseTarget::LinuxX86_64 => MISE_BINARY_SHA256_LINUX_X64,
        ReleaseTarget::MacosArm64 => MISE_BINARY_SHA256_MACOS_ARM64,
        ReleaseTarget::MacosX86_64 => MISE_BINARY_SHA256_MACOS_X64,
    };
    Ok(MiseSetup {
        uses: mise_action_uses(config)?,
        version: MISE_VERSION.to_owned(),
        sha256: sha256.to_owned(),
    })
}

pub(crate) fn resolve_verification_mise_setup(
    config: &VelnorConfig,
    runner: VerificationRunner,
) -> Result<MiseSetup, OrchestratorError> {
    let sha256 = match runner {
        VerificationRunner::LinuxX64 => MISE_BINARY_SHA256_LINUX_X64,
        VerificationRunner::MacosArm64
        | VerificationRunner::Macos26Arm64
        | VerificationRunner::Xcode27Arm64 => MISE_BINARY_SHA256_MACOS_ARM64,
    };
    Ok(MiseSetup {
        uses: mise_action_uses(config)?,
        version: MISE_VERSION.to_owned(),
        sha256: sha256.to_owned(),
    })
}

pub(crate) fn resolve_build_task_mise_setup(
    config: &VelnorConfig,
    runner: BuildTaskRunner,
) -> Result<MiseSetup, OrchestratorError> {
    let sha256 = match runner {
        BuildTaskRunner::Macos26Arm64 | BuildTaskRunner::Xcode27Arm64 => {
            MISE_BINARY_SHA256_MACOS_ARM64
        }
    };
    Ok(MiseSetup {
        uses: mise_action_uses(config)?,
        version: MISE_VERSION.to_owned(),
        sha256: sha256.to_owned(),
    })
}

fn mise_action_uses(config: &VelnorConfig) -> Result<String, OrchestratorError> {
    let Some(pin) = config.actions.overrides.get(MISE_ACTION_KEY) else {
        return Ok(format!("{MISE_ACTION_KEY}@{MISE_ACTION_SHA}"));
    };
    let mut catalog = ApprovedPinCatalog::new();
    catalog.insert(MISE_ACTION_KEY, MISE_ACTION_SHA, MISE_ACTION_VERSION)?;
    let request = ApprovedOverride {
        action: MISE_ACTION_KEY.to_owned(),
        sha: pin.sha.clone(),
        version: pin.version.clone(),
    };
    let approved = catalog.validate_override(&request)?;
    Ok(format!("{}@{}", approved.uses_key(), pin.sha))
}

pub(crate) fn consumer_acquire_step(
    label: &str,
    version: &str,
    discovery: &Discovery,
) -> Result<Step, OrchestratorError> {
    consumer_acquire_from(label, version, discovery.consumer_manifest_json.as_deref())
}

pub fn consumer_acquire_step_with_manifest(
    label: &str,
    version: &str,
    json: Option<&str>,
) -> Result<Step, OrchestratorError> {
    consumer_acquire_from(label, version, json)
}

fn consumer_acquire_from(
    label: &str,
    version: &str,
    json: Option<&str>,
) -> Result<Step, OrchestratorError> {
    let target =
        ReleaseTarget::for_runner_label(label).ok_or_else(|| OrchestratorError::Contract {
            problem: format!("unsupported_target_for_runner:{label}"),
        })?;
    consumer_acquire_for_target(target, version, json)
}

pub(crate) fn consumer_acquire_for_runner(
    runner: &velnor_actions_contract::config::CheckRunner,
    version: &str,
    json: Option<&str>,
) -> Result<Step, OrchestratorError> {
    runner.validate("workflow", "check.runner")?;
    consumer_acquire_for_target(runner.platform.release_target(), version, json)
}

pub(crate) fn consumer_acquire_for_target(
    target: ReleaseTarget,
    version: &str,
    json: Option<&str>,
) -> Result<Step, OrchestratorError> {
    release_helpers::consumer_acquire_for_target(target, version, json)
        .map_err(crate::local_release_manifest::map_release_helper_error)
}

pub(crate) fn lock_acquire_step(
    lock: &GeneratorLock,
    label: &str,
    staged: &str,
) -> Result<Step, OrchestratorError> {
    let target =
        ReleaseTarget::for_runner_label(label).ok_or_else(|| OrchestratorError::Contract {
            problem: format!("unsupported_target_for_runner:{label}"),
        })?;
    lock_acquire_for_target(lock, target, staged)
}

pub(crate) fn lock_acquire_for_runner(
    lock: &GeneratorLock,
    runner: &velnor_actions_contract::config::CheckRunner,
    staged: &str,
) -> Result<Step, OrchestratorError> {
    runner.validate("workflow", "check.runner")?;
    lock_acquire_for_target(lock, runner.platform.release_target(), staged)
}

fn lock_acquire_for_target(
    lock: &GeneratorLock,
    target: ReleaseTarget,
    staged: &str,
) -> Result<Step, OrchestratorError> {
    release_helpers::lock_acquire_for_target(lock, target, staged)
        .map_err(crate::local_release_manifest::map_release_helper_error)
}

pub fn acquire_script_argv(
    staged: &str,
    seed_root: &str,
    target: ReleaseTarget,
) -> Result<Vec<String>, OrchestratorError> {
    release_helpers::acquire_script_argv(staged, seed_root, target)
        .map_err(crate::local_release_manifest::map_release_helper_error)
}
