//! Compiled pin resolution: Mise setup plus helper provenance.
//!
//! Resolves `[actions.overrides]` against the compiled approved-pair catalog,
//! selects installed-Mise digests by qualified runner target, and builds
//! digest-verified helper acquisition from release or lock provenance. Anything
//! without provenance fails closed; no digest is ever invented.

use velnor_actions_actionlint::actions::{MISE_ACTION_SHA, MISE_ACTION_VERSION};
use velnor_actions_actionlint::overrides::{
    ActionPinOverride as ApprovedOverride, ApprovedPinCatalog,
};
use velnor_actions_contract::{
    GeneratorLock, ReleaseManifest, Step, VelnorConfig, check_release_artifact,
    target_for_runner_label,
};
use velnor_actions_mise::MISE_VERSION;
use velnor_actions_workflow_renderer::{
    HelperProvenance, MiseSetup, MiseSetupSet, STAGED_BINARY_PREFIX, provision_acquire_step,
};

use crate::OrchestratorError;
use crate::discover::Discovery;

/// Override key selecting the Mise setup action pin.
const MISE_ACTION_KEY: &str = "jdx/mise-action";

/// Installed `mise` binary digest for mise 2026.10.2 on Linux x86-64.
///
/// Source: official tag `v2026.10.2`, commit
/// `44ea2537166efbe21b19d808355d9914e830941a`; the Linux x64 archive
/// `mise-v2026.10.2-linux-x64.tar.gz` SHA-256 is
/// `79a2bf0ffc9b8a9a6391344e875b3c3679c15053fda3e8728ddf1790d63db788`.
/// Its extracted `mise/bin/mise` SHA-256 is the digest emitted below. The
/// real Phase 0 probe passed on Linux x86-64. The setup action checks the
/// installed binary, not the archive, and this pin is only for x64 Linux.
const MISE_BINARY_SHA256_LINUX_X64: &str =
    "8f5f6660336f572830e33cd9b378d3131e529a0d4c4f0c553776be90a1ba302a";
/// Installed Mise digest for the official v2026.10.2 macOS ARM64 binary.
///
/// Source: official release asset `mise-v2026.10.2-macos-arm64`, tag commit
/// `44ea2537166efbe21b19d808355d9914e830941a`; its downloaded executable SHA-256
/// is `66d49acecca413c8b334922584982a4907a10588912829873d6c55d0c6d42612`.
/// The `jdx/mise-action` input checks the installed binary digest.
const MISE_BINARY_SHA256_MACOS_ARM64: &str =
    "66d49acecca413c8b334922584982a4907a10588912829873d6c55d0c6d42612";

/// Linux x64 target covered by the compiled Mise digest.
const LINUX_X64_TARGET: &str = "x86_64-unknown-linux-gnu";
/// macOS ARM64 target covered by the compiled Mise digest.
const MACOS_ARM64_TARGET: &str = "aarch64-apple-darwin";
/// Runner label used by the repository's native Apple Silicon Phase0 job.
const MACOS_ARM64_RUNNER_LABEL: &str = "macos-15";

/// Resolve typed Mise setup pins: overrides plus the compiled catalog.
///
/// The `uses` ref comes from `[actions.overrides]` when present (approved
/// pairs only; anything else fails closed) or the compiled default pin.
/// `version` is the compiled Mise release; `sha256` is the verified
/// installed-binary digest for the selected target. Only the repository's
/// supported Linux x64 and native macOS ARM64 runners are accepted.
pub(crate) fn resolve_mise_setup(
    config: &VelnorConfig,
    label: &str,
) -> Result<MiseSetup, OrchestratorError> {
    let target =
        mise_target_for_runner_label(label).ok_or_else(|| OrchestratorError::Contract {
            problem: format!("mise_setup_unsupported_target:{label}"),
        })?;
    Ok(MiseSetup {
        uses: mise_action_uses(config)?,
        version: MISE_VERSION.to_owned(),
        sha256: mise_binary_sha256(target)?.to_owned(),
    })
}

/// Resolve all typed Mise setup records needed by one generated workflow.
///
/// The configured runner's setup is reused; only the other supported native
/// platform is resolved separately. Action overrides therefore apply to both
/// records without duplicating override policy.
pub(crate) fn resolve_mise_setup_set(
    config: &VelnorConfig,
    label: &str,
    configured_setup: &MiseSetup,
) -> Result<MiseSetupSet, OrchestratorError> {
    let configured_target =
        mise_target_for_runner_label(label).ok_or_else(|| OrchestratorError::Contract {
            problem: format!("mise_setup_unsupported_target:{label}"),
        })?;
    let mut setups = vec![(configured_target.to_owned(), configured_setup.clone())];
    for (target, runner_label) in [
        (LINUX_X64_TARGET, "ubuntu-26.04"),
        (MACOS_ARM64_TARGET, MACOS_ARM64_RUNNER_LABEL),
    ] {
        if target != configured_target {
            setups.push((target.to_owned(), resolve_mise_setup(config, runner_label)?));
        }
    }
    Ok(MiseSetupSet::new(setups)?)
}

/// Resolve the exact supported target for a hosted label.
fn mise_target_for_runner_label(label: &str) -> Option<&'static str> {
    if label == MACOS_ARM64_RUNNER_LABEL {
        return Some(MACOS_ARM64_TARGET);
    }
    (target_for_runner_label(label) == Some(LINUX_X64_TARGET)).then_some(LINUX_X64_TARGET)
}

/// Select the digest compiled for one supported Mise target.
fn mise_binary_sha256(target: &str) -> Result<&'static str, OrchestratorError> {
    match target {
        LINUX_X64_TARGET => Ok(MISE_BINARY_SHA256_LINUX_X64),
        MACOS_ARM64_TARGET => Ok(MISE_BINARY_SHA256_MACOS_ARM64),
        _ => Err(OrchestratorError::Contract {
            problem: format!("mise_setup_unsupported_target:{target}"),
        }),
    }
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
    let Some(json) = json else {
        return Err(OrchestratorError::Contract {
            problem: "consumer_requires_release_install:install an official velnor-actions release"
                .to_owned(),
        });
    };
    let manifest = ReleaseManifest::parse_json(json, "release-manifest.json")?;
    manifest.validate("release-manifest.json")?;
    if manifest.version != version {
        return Err(OrchestratorError::Contract {
            problem: format!(
                "release_manifest_version_mismatch:{}:{version}",
                manifest.version
            ),
        });
    }
    let target = target_for_runner_label(label).ok_or_else(|| OrchestratorError::Contract {
        problem: format!("unsupported_target_for_runner:{label}"),
    })?;
    let record = manifest
        .record_for_target(target)
        .ok_or_else(|| OrchestratorError::Contract {
            problem: format!("manifest_missing_target:{target}"),
        })?;
    // Defense in depth: re-bind the consumed record's URL even though
    // `validate` already bound every record (X1).
    check_release_artifact(
        &record.artifact,
        &manifest.version,
        target,
        "release-manifest.json",
        "targets.artifact",
    )?;
    acquire_step(
        &record.artifact,
        &record.sha256,
        &manifest.commit,
        &format!("{STAGED_BINARY_PREFIX}{version}"),
    )
}

/// Lock-backed Acquire step for one runner label (Velnor policy only).
pub(crate) fn lock_acquire_step(
    lock: &GeneratorLock,
    label: &str,
    staged: &str,
) -> Result<Step, OrchestratorError> {
    let target = target_for_runner_label(label).ok_or_else(|| OrchestratorError::Contract {
        problem: format!("unsupported_target_for_runner:{label}"),
    })?;
    let record = lock
        .binary_for_target(target)
        .ok_or_else(|| OrchestratorError::Contract {
            problem: format!("lock_missing_target:{target}"),
        })?;
    acquire_step(
        &record.artifact,
        &record.sha256,
        &lock.generator.commit,
        staged,
    )
}

/// Digest-verified staging step: fetch URL, check SHA-256, record the source
/// commit, make executable. Both provenance paths supply the commit (F3).
fn acquire_step(
    url: &str,
    sha: &str,
    commit: &str,
    staged: &str,
) -> Result<Step, OrchestratorError> {
    let provenance = HelperProvenance::ReleaseAsset {
        url: url.to_owned(),
        sha256: sha.to_owned(),
        commit: commit.to_owned(),
    };
    Ok(provision_acquire_step(&provenance, acquire_argv(staged))?)
}

/// Fixed acquisition argv over the staged path plus asset env references.
///
/// Curl is pinned to HTTPS-only (`--proto '=https'`) over TLS 1.2+ and
/// the staged path is defensively double-quoted (X12).
fn acquire_argv(staged: &str) -> Vec<String> {
    let dir = staged.rsplit_once('/').map_or(staged, |(head, _)| head);
    let script = format!(
        "mkdir -p \"{dir}\" && curl -fsSL --proto '=https' --tlsv1.2 \"$VELNOR_ASSET_URL\" -o \"{staged}\" && echo \"$VELNOR_ASSET_SHA256  {staged}\" | sha256sum -c - && chmod +x \"{staged}\""
    );
    vec!["sh".to_owned(), "-c".to_owned(), script]
}

/// `cfg(test)`-only fixture manifest matching the workspace version.
#[cfg(test)]
fn test_manifest_json() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let targets = ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin", "x86_64-apple-darwin"]
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
