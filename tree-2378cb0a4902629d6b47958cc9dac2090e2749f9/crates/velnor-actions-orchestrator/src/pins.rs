//! Compiled pin resolution: Mise setup plus helper provenance.
//!
//! Resolves `[actions.overrides]` against the compiled approved-pair catalog,
//! gates normal setup to the x64-Linux target and verification setup to its
//! platform-specific digest, then builds digest-verified helper acquisition
//! from release or lock provenance. Missing provenance fails closed.

use velnor_actions_actionlint::actions::{MISE_ACTION_SHA, MISE_ACTION_VERSION};
use velnor_actions_actionlint::overrides::{
    ActionPinOverride as ApprovedOverride, ApprovedPinCatalog,
};
use velnor_actions_contract::{
    GeneratorLock, ReleaseManifest, Step, VelnorConfig, VerificationRunner, check_release_artifact,
    target_for_runner_label,
};
use velnor_actions_mise::MISE_VERSION;
use velnor_actions_workflow_renderer::{
    HelperProvenance, MiseSetup, STAGED_BINARY_PREFIX, provision_acquire_step,
};

use crate::OrchestratorError;
use crate::discover::Discovery;

/// Override key selecting the Mise setup action pin.
const MISE_ACTION_KEY: &str = "jdx/mise-action";

/// Installed `mise` binary digest for mise 2026.9.18 on Linux x86-64.
///
/// Source: `SHASUMS256.txt` of the jdx/mise `v2026.9.18` release, verified
/// 2026-09-30 by downloading `mise-v2026.9.18-linux-x64.tar.zst`, extracting
/// `mise/bin/mise`, and hashing the extracted binary. The setup action
/// compares its `sha256` input against the installed binary, not the archive,
/// so only this digest is emitted, and only for x64-Linux runners.
const MISE_BINARY_SHA256_LINUX_X64: &str =
    "d24fe0bf7e613824ad99f7b8dac3f2b381a37b9f75f84dd250855217095a8de4";

/// Installed `mise` binary digest for mise 2026.9.18 on Apple ARM64.
///
/// Source: the `mise-v2026.9.18-macos-arm64` entry in the official
/// `SHASUMS256.txt`, fetched over HTTPS on 2026-10-05.
const MISE_BINARY_SHA256_MACOS_ARM64: &str =
    "484c135bd4329975d608d3f77e26c2ece5d2f5590f18ca71f44440294f8cfa6f";

/// Runner target the compiled mise digest covers.
const LINUX_X64_TARGET: &str = "x86_64-unknown-linux-gnu";

/// Resolve typed Mise setup pins: overrides plus the compiled catalog.
///
/// The `uses` ref comes from `[actions.overrides]` when present (approved
/// pairs only; anything else fails closed) or the compiled default pin.
/// `version` is the compiled Mise release; `sha256` is the verified
/// installed-binary digest, so this general-purpose setup path rejects labels
/// outside x64 Linux rather than emitting a digest for the wrong architecture.
pub(crate) fn resolve_mise_setup(
    config: &VelnorConfig,
    label: &str,
) -> Result<MiseSetup, OrchestratorError> {
    if target_for_runner_label(label) != Some(LINUX_X64_TARGET) {
        return Err(OrchestratorError::Contract {
            problem: format!("mise_setup_unsupported_target:{label}"),
        });
    }
    Ok(MiseSetup {
        uses: mise_action_uses(config)?,
        version: MISE_VERSION.to_owned(),
        sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
    })
}

/// Resolve cache-off Mise setup for an isolated verification runner.
pub(crate) fn resolve_verification_mise_setup(
    config: &VelnorConfig,
    runner: VerificationRunner,
) -> Result<MiseSetup, OrchestratorError> {
    let sha256 = match runner {
        VerificationRunner::LinuxX64 => MISE_BINARY_SHA256_LINUX_X64,
        VerificationRunner::MacosArm64 => MISE_BINARY_SHA256_MACOS_ARM64,
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
    Ok(provision_acquire_step(
        &provenance,
        acquire_script_argv(staged, GENERATOR_SEED_ROOT)?,
    )?)
}

/// Host path of the read-only generator seed. Not a job input.
const GENERATOR_SEED_ROOT: &str = "/opt/velnor/seed";

/// Fixed acquisition argv. A matching seed file is copied. Otherwise curl.
///
/// Curl stays HTTPS-only (`--proto '=https'`) over TLS 1.2+. The staged
/// path stays double-quoted. A hash mismatch does not copy the seed.
///
/// # Errors
///
/// Returns [`OrchestratorError::Contract`] when the seed root or staged
/// path is outside its closed safe-path grammar.
pub fn acquire_script_argv(
    staged: &str,
    seed_root: &str,
) -> Result<Vec<String>, OrchestratorError> {
    if !absolute_token(seed_root) {
        return Err(OrchestratorError::Contract {
            problem: format!("bad_seed_root:{seed_root}"),
        });
    }
    if !staged_path_token(staged) {
        return Err(OrchestratorError::Contract {
            problem: format!("bad_staged_path:{staged}"),
        });
    }
    let dir = staged.rsplit_once('/').map_or(staged, |(head, _)| head);
    let name = staged.rsplit_once('/').map_or(staged, |(_, tail)| tail);
    if !file_token(name) {
        return Err(OrchestratorError::Contract {
            problem: format!("bad_staged_name:{name}"),
        });
    }
    let seed = format!("{seed_root}/generator/{name}");
    let script = format!(
        "mkdir -p \"{dir}\" && if [ -f \"{seed}\" ] && echo \"$VELNOR_ASSET_SHA256  {seed}\" | sha256sum -c -; then cp \"{seed}\" \"{staged}\"; else curl -fsSL --proto '=https' --tlsv1.2 \"$VELNOR_ASSET_URL\" -o \"{staged}\" && echo \"$VELNOR_ASSET_SHA256  {staged}\" | sha256sum -c -; fi && chmod +x \"{staged}\""
    );
    Ok(vec!["sh".to_owned(), "-c".to_owned(), script])
}

fn absolute_token(value: &str) -> bool {
    value.starts_with('/')
        && !value.contains("..")
        && !value.contains("//")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'))
}

/// Allow static absolute test paths or the one emitted runner-temp prefix.
fn staged_path_token(value: &str) -> bool {
    value
        .strip_prefix(STAGED_BINARY_PREFIX)
        .map_or_else(|| absolute_token(value), file_token)
}

fn file_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// `cfg(test)`-only fixture manifest matching the workspace version.
#[cfg(test)]
fn test_manifest_json() -> String {
    let version = env!("CARGO_PKG_VERSION");
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
