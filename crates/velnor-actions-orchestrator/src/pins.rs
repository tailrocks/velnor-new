//! Compiled pin resolution: Mise setup plus helper provenance.
//!
//! Resolves `[actions.overrides]` against the compiled approved-pair catalog,
//! gates the installed-mise digest to the single x64-Linux runner target, and
//! builds digest-verified helper acquisition from release or lock provenance.
//! Anything without provenance fails closed; no digest is ever invented.

use velnor_actions_actionlint::actions::{MISE_ACTION_SHA, MISE_ACTION_VERSION};
use velnor_actions_actionlint::overrides::{
    ActionPinOverride as ApprovedOverride, ApprovedPinCatalog,
};
use velnor_actions_contract::{
    GeneratorLock, ReleaseManifest, Step, VelnorConfig, target_for_runner_label,
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

/// Runner target the compiled mise digest covers.
const LINUX_X64_TARGET: &str = "x86_64-unknown-linux-gnu";

/// Resolve typed Mise setup pins: overrides plus the compiled catalog.
///
/// The `uses` ref comes from `[actions.overrides]` when present (approved
/// pairs only; anything else fails closed) or the compiled default pin.
/// `version` is the compiled Mise release; `sha256` is the verified
/// installed-binary digest, so non-x64-Linux labels fail closed rather than
/// emitting a digest for the wrong architecture.
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

/// Consumer Acquire step from the release manifest of this exact version.
///
/// Bootstrap contract §2: a source build (no embedded manifest) fails
/// consumer generation with a provenance diagnostic recommending an
/// official release; it never emits an unverified URL or placeholder digest.
pub(crate) fn consumer_acquire_step(
    label: &str,
    version: &str,
    discovery: &Discovery,
) -> Result<Step, OrchestratorError> {
    consumer_acquire_from(label, version, release_manifest_json(discovery).as_deref())
}

/// Consumer Acquire step from an explicit manifest (pure; `None` fails).
///
/// A source build (no embedded manifest) fails consumer generation with a
/// provenance diagnostic recommending an official release (boot §2).
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
    acquire_step(
        &record.artifact,
        &record.sha256,
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
    acquire_step(&record.artifact, &record.sha256, staged)
}

/// Digest-verified staging step: fetch URL, check SHA-256, make executable.
fn acquire_step(url: &str, sha: &str, staged: &str) -> Result<Step, OrchestratorError> {
    let provenance = HelperProvenance::ReleaseAsset {
        url: url.to_owned(),
        sha256: sha.to_owned(),
    };
    Ok(provision_acquire_step(&provenance, acquire_argv(staged))?)
}

/// Fixed acquisition argv over the staged path plus asset env references.
fn acquire_argv(staged: &str) -> Vec<String> {
    let dir = staged.rsplit_once('/').map_or(staged, |(head, _)| head);
    let script = format!(
        "mkdir -p {dir} && curl -fsSL \"$VELNOR_ASSET_URL\" -o {staged} && echo \"$VELNOR_ASSET_SHA256  {staged}\" | sha256sum -c - && chmod +x {staged}"
    );
    vec!["sh".to_owned(), "-c".to_owned(), script]
}

/// Embedded release-manifest JSON: compile-time release provenance.
///
/// Baked `VELNOR_RELEASE_MANIFEST_JSON` wins; otherwise the debug-only
/// discovery fixture applies. Release builds have no fixture path, so a
/// source build always fails the consumer gate.
fn release_manifest_json(discovery: &Discovery) -> Option<String> {
    if let Some(baked) = option_env!("VELNOR_RELEASE_MANIFEST_JSON") {
        return Some(baked.to_owned());
    }
    discovery.consumer_manifest_json.clone()
}

/// `cfg(test)`-only fixture manifest matching the workspace version.
#[cfg(test)]
fn test_manifest_json() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let targets = ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin", "x86_64-apple-darwin"]
        .iter()
        .map(|target| {
            format!(
                "{{\"target\":\"{target}\",\"artifact\":\"https://example.invalid/releases/download/{version}/velnor-actions-{version}-{target}\",\"sha256\":\"{}\"}}",
                "a".repeat(64)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"tailrocks/velnor-new\",\"targets\":[{targets}]}}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use velnor_actions_contract::config::{ActionPinOverride, ActionsConfig};
    use velnor_actions_contract::{
        DiscoveryConfig, GeneratorValidation, ResourcesConfig, StacksConfig, TestShardingConfig,
        WorkflowConfig, WorkflowPolicy,
    };

    /// Config carrying exactly the given action-pin overrides.
    fn config_with(overrides: BTreeMap<String, ActionPinOverride>) -> VelnorConfig {
        VelnorConfig {
            schema: 1,
            workflow: WorkflowConfig {
                name: "CI".to_owned(),
                policy: WorkflowPolicy::ConsumerV1,
                default_branch: None,
                generator_validation: GeneratorValidation::Bootstrap,
                max_parallel_jobs: 2,
                runner_label: None,
            },
            resources: ResourcesConfig {
                compiler_process_budget: 2,
                test_process_budget: 2,
            },
            test_sharding: TestShardingConfig {
                default_shards: 1,
                by_manifest: BTreeMap::new(),
            },
            stacks: StacksConfig {
                ignore: Vec::new(),
                rust: None,
            },
            discovery: DiscoveryConfig {
                exclude: Vec::new(),
            },
            actions: ActionsConfig { overrides },
        }
    }

    #[test]
    fn source_build_consumer_generation_fails_with_provenance() {
        let err = consumer_acquire_from("ubuntu-26.04", "0.1.0", None);
        assert!(err.is_err_and(|err| {
            err.to_string()
                .contains("consumer_requires_release_install")
        }));
    }

    #[test]
    fn consumer_manifest_mismatch_and_bad_target_fail() {
        let err = consumer_acquire_from("ubuntu-26.04", "9.9.9", Some(&test_manifest_json()));
        assert!(err.is_err_and(|err| err.to_string().contains("version_mismatch")));
        let err = consumer_acquire_from("ubuntu-26.04-arm", "0.1.0", Some(&test_manifest_json()));
        assert!(err.is_err_and(|err| err.to_string().contains("unsupported_target_for_runner")));
        let err = consumer_acquire_from("ubuntu-26.04", "0.1.0", Some("not json"));
        assert!(err.is_err());
    }

    #[test]
    fn fixture_manifest_embeds_runner_target_record() {
        let step = consumer_acquire_from(
            "ubuntu-26.04",
            env!("CARGO_PKG_VERSION"),
            Some(&test_manifest_json()),
        )
        .map(|step| step.name);
        assert_eq!(
            step.map_err(|err| err.to_string()),
            Ok("Acquire Velnor".to_owned())
        );
    }

    #[test]
    fn mise_setup_defaults_to_compiled_pins() {
        let setup = resolve_mise_setup(&config_with(BTreeMap::new()), "ubuntu-26.04")
            .map_err(|err| err.to_string());
        assert_eq!(
            setup,
            Ok(MiseSetup {
                uses: format!("jdx/mise-action@{MISE_ACTION_SHA}"),
                version: MISE_VERSION.to_owned(),
                sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
            })
        );
    }

    #[test]
    fn mise_setup_accepts_approved_override_only() {
        let approved = BTreeMap::from([(
            MISE_ACTION_KEY.to_owned(),
            ActionPinOverride {
                sha: MISE_ACTION_SHA.to_owned(),
                version: MISE_ACTION_VERSION.to_owned(),
            },
        )]);
        let setup = resolve_mise_setup(&config_with(approved), "ubuntu-26.04");
        assert!(setup.is_ok_and(|setup| setup.uses.ends_with(MISE_ACTION_SHA)));
        for pin in [
            ActionPinOverride {
                sha: "0".repeat(40),
                version: MISE_ACTION_VERSION.to_owned(),
            },
            ActionPinOverride {
                sha: MISE_ACTION_SHA.to_owned(),
                version: "v9.9.9".to_owned(),
            },
            ActionPinOverride {
                sha: "short".to_owned(),
                version: MISE_ACTION_VERSION.to_owned(),
            },
        ] {
            let overrides = BTreeMap::from([(MISE_ACTION_KEY.to_owned(), pin)]);
            assert!(resolve_mise_setup(&config_with(overrides), "ubuntu-26.04").is_err());
        }
    }

    #[test]
    fn mise_setup_rejects_non_linux_runners() {
        for label in ["ubuntu-26.04-arm", "windows-latest", "ubuntu-latest"] {
            let err = resolve_mise_setup(&config_with(BTreeMap::new()), label);
            assert!(
                err.is_err_and(|err| err.to_string().contains("mise_setup_unsupported_target")),
                "{label}"
            );
        }
    }
}
