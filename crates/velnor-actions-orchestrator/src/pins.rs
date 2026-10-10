//! Compiled pin resolution: Mise setup plus helper provenance.
//!
//! Resolves `[actions.overrides]` against the compiled approved-pair catalog,
//! selects the installed Mise digest for each supported runner target, and
//! builds digest-verified helper acquisition from release or lock provenance.
//! Anything without provenance fails closed; no digest is ever invented.

use velnor_actions_actionlint::actions::{MISE_ACTION_SHA, MISE_ACTION_VERSION};
use velnor_actions_actionlint::overrides::{
    ActionPinOverride as ApprovedOverride, ApprovedPinCatalog,
};
use velnor_actions_contract::{
    BuildTaskRunner, GeneratorLock, RELEASE_MANIFEST_FILENAME, ReleaseManifest, ReleaseTarget,
    Step, VelnorConfig, VerificationRunner, check_release_artifact,
};
use velnor_actions_mise::MISE_VERSION;
use velnor_actions_workflow_renderer::{MiseSetup, STAGED_BINARY_PREFIX};

use crate::OrchestratorError;
use crate::discover::Discovery;

#[path = "pins_acquire.rs"]
mod pins_acquire;
pub(super) use pins_acquire::{acquire_script_argv, acquire_step};

/// Override key selecting the Mise setup action pin.
const MISE_ACTION_KEY: &str = "jdx/mise-action";

use velnor_actions_workflow_renderer::setup::{
    MISE_BINARY_SHA256_LINUX_X64, MISE_BINARY_SHA256_MACOS_ARM64, MISE_BINARY_SHA256_MACOS_X64,
};

#[path = "pins_consumer_binary_release.rs"]
mod consumer_binary_release;
pub(super) use consumer_binary_release::resolve_mise_setup_for_consumer_binary_release;
#[path = "pins_mise_pin_qualification.rs"]
mod mise_pin_qualification;
pub(super) use mise_pin_qualification::resolve as resolve_mise_pin_qualification;
#[path = "pins_rust_toolchain_qualification.rs"]
mod rust_toolchain_qualification;
pub(super) use rust_toolchain_qualification::resolve as resolve_rust_toolchain_qualification;

/// Resolve typed Mise setup pins: overrides plus the compiled catalog.
///
/// The `uses` ref comes from `[actions.overrides]` when present (approved
/// pairs only; anything else fails closed) or the compiled default pin.
/// `version` is the compiled Mise release; `sha256` is the verified
/// installed-binary digest selected for the runner architecture. Unsupported
/// targets fail closed.
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

/// Resolve a release runner's Mise setup pins from the named platform ID.
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

/// Resolve cache-off Mise setup for an isolated verification runner.
pub(crate) fn resolve_verification_mise_setup(
    config: &VelnorConfig,
    runner: VerificationRunner,
) -> Result<MiseSetup, OrchestratorError> {
    let sha256 = match runner {
        VerificationRunner::LinuxX64 => MISE_BINARY_SHA256_LINUX_X64,
        VerificationRunner::MacosArm64 | VerificationRunner::Macos26Arm64 => {
            MISE_BINARY_SHA256_MACOS_ARM64
        }
    };
    Ok(MiseSetup {
        uses: mise_action_uses(config)?,
        version: MISE_VERSION.to_owned(),
        sha256: sha256.to_owned(),
    })
}

/// Resolve cache-off Mise setup for the native macOS build runner.
pub(crate) fn resolve_build_task_mise_setup(
    config: &VelnorConfig,
    runner: BuildTaskRunner,
) -> Result<MiseSetup, OrchestratorError> {
    let sha256 = match runner {
        BuildTaskRunner::Macos26Arm64 => MISE_BINARY_SHA256_MACOS_ARM64,
    };
    Ok(MiseSetup {
        uses: mise_action_uses(config)?,
        version: MISE_VERSION.to_owned(),
        sha256: sha256.to_owned(),
    })
}

/// Resolve the Mise action ref: an approved override or the compiled default.
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

/// Consumer Acquire step from the committed release manifest.
///
/// Bootstrap contract §2: a repo without the committed manifest file
/// fails consumer generation with a provenance diagnostic recommending
/// an official release; it never emits an unverified URL or digest.
pub(crate) fn consumer_acquire_step(
    label: &str,
    version: &str,
    discovery: &Discovery,
) -> Result<Step, OrchestratorError> {
    consumer_acquire_from(label, version, discovery.consumer_manifest_json.as_deref())
}

/// Consumer Acquire step from an explicit manifest (pure; `None` fails).
///
/// A repo without the committed manifest file fails consumer generation
/// with a provenance diagnostic recommending an official release (§2).
///
/// # Errors
///
/// Returns a contract error without provenance, on version or target
/// mismatch, or on malformed manifests.
pub fn consumer_acquire_step_with_manifest(
    label: &str,
    version: &str,
    json: Option<&str>,
) -> Result<Step, OrchestratorError> {
    consumer_acquire_from(label, version, json)
}

/// Consumer Acquire step from an explicit manifest (pure; `None` fails).
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

/// Acquire the exact helper asset for a validated check runner.
/// # Errors
/// Rejects invalid runner profiles or missing artifact provenance.
pub(crate) fn consumer_acquire_for_runner(
    runner: &velnor_actions_contract::config::CheckRunner,
    version: &str,
    json: Option<&str>,
) -> Result<Step, OrchestratorError> {
    runner.validate("workflow", "check.runner")?;
    consumer_acquire_for_target(runner.platform.release_target(), version, json)
}

fn consumer_acquire_for_target(
    target: ReleaseTarget,
    version: &str,
    json: Option<&str>,
) -> Result<Step, OrchestratorError> {
    let Some(json) = json else {
        return Err(OrchestratorError::Contract {
            problem: "consumer_requires_release_install:install an official velnor-actions release"
                .to_owned(),
        });
    };
    let manifest = ReleaseManifest::parse_json(json, RELEASE_MANIFEST_FILENAME)?;
    manifest.validate(RELEASE_MANIFEST_FILENAME)?;
    if manifest.version != version {
        return Err(OrchestratorError::Contract {
            problem: format!(
                "release_manifest_version_mismatch:{}:{version}",
                manifest.version
            ),
        });
    }
    let record =
        manifest
            .record_for_target(target.triple())
            .ok_or_else(|| OrchestratorError::Contract {
                problem: format!("manifest_missing_target:{}", target.triple()),
            })?;
    // Defense in depth: re-bind the consumed record's URL even though
    // `validate` already bound every record (X1).
    check_release_artifact(
        &record.artifact,
        &manifest.version,
        &manifest.commit,
        target.triple(),
        RELEASE_MANIFEST_FILENAME,
        "targets.artifact",
    )?;
    acquire_step(
        &record.artifact,
        &record.sha256,
        &manifest.commit,
        &format!("{STAGED_BINARY_PREFIX}{version}"),
        target,
    )
}

/// Lock-backed Acquire step for one runner label (Velnor policy only).
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
    let record =
        lock.binary_for_target(target.triple())
            .ok_or_else(|| OrchestratorError::Contract {
                problem: format!("lock_missing_target:{}", target.triple()),
            })?;
    acquire_step(
        &record.artifact,
        &record.sha256,
        &lock.generator.commit,
        staged,
        target,
    )
}

/// `cfg(test)`-only fixture manifest matching the workspace version.
#[cfg(test)]
fn test_manifest_json() -> String {
    test_manifest_json_for_version(env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
fn test_manifest_json_for_version(version: &str) -> String {
    let targets = velnor_actions_contract::SUPPORTED_TARGETS
        .iter()
        .map(|target| {
            format!(
                "{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{target}\",\"sha256\":\"{}\"}}",
                "a".repeat(64)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{}\",\"targets\":[{targets}]}}",
        "a".repeat(40)
    )
}

#[cfg(test)]
#[path = "pins_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "pins_tests_b.rs"]
mod tests_b;

#[cfg(test)]
#[path = "pins_mise_pin_qualification_tests.rs"]
mod mise_pin_qualification_tests;
#[cfg(test)]
#[path = "pins_manifest_tests.rs"]
mod pins_manifest_tests;
#[cfg(test)]
#[path = "pins_rust_toolchain_qualification_tests.rs"]
mod rust_toolchain_qualification_tests;
