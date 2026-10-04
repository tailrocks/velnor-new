//! Fixed immutable Rust release proof source closure, owned by the composer.
use std::collections::BTreeMap;
use velnor_actions_contract::{CompiledSupportSource, ContractError, generated_source};

#[path = "release_source_snapshot_sources.rs"]
mod snapshot;

/// Fixed anonymous package helper path.
pub(super) const PACKAGE_SCRIPT_PATH: &str = ".github/velnor/release_package.sh";
/// Fixed credentialed forge preflight helper path.
pub(super) const FORGE_PREFLIGHT_SCRIPT_PATH: &str = ".github/velnor/release_forge_preflight.sh";
/// Fixed reconciliation execution helper path.
pub(super) const RECONCILE_SCRIPT_PATH: &str = ".github/velnor/release_reconcile.sh";

/// Generation selects one fixed immutable execution closure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProofKind {
    SourceSnapshot,
    PreparedPackage,
    PackageVerify,
    AnonymousPackage,
    ForgePreflight,
    Reconcile,
    RegistryArtifactProof,
    RegistryPublishOidc,
    RegistryPublishBootstrap,
    ForgePublish,
    PrepareAnonymous,
    PrepareForge,
}

/// Fixed operation inventory for the validated release authentication and preparation policy.
pub(crate) fn proof_record_kinds(
    authentication: velnor_actions_contract::config::ReleaseAuthentication,
    release_pr: bool,
) -> Vec<ProofKind> {
    use velnor_actions_contract::config::ReleaseAuthentication;
    let registry = match authentication {
        ReleaseAuthentication::TrustedPublishing => ProofKind::RegistryPublishOidc,
        ReleaseAuthentication::BootstrapToken => ProofKind::RegistryPublishBootstrap,
    };
    let mut kinds = vec![
        ProofKind::SourceSnapshot,
        ProofKind::AnonymousPackage,
        ProofKind::ForgePreflight,
        ProofKind::RegistryArtifactProof,
        registry,
        ProofKind::ForgePublish,
        ProofKind::Reconcile,
    ];
    if release_pr {
        kinds.extend([ProofKind::PrepareAnonymous, ProofKind::PrepareForge]);
    }
    kinds
}

/// Fixed distinct preparation footprints; common tools have one compiled record.
pub(crate) fn preparation_record_kinds(release_pr: bool) -> Vec<ProofKind> {
    let mut kinds = vec![ProofKind::AnonymousPackage, ProofKind::ForgePreflight];
    if release_pr {
        kinds.push(ProofKind::PrepareAnonymous);
    }
    kinds
}

const PURE_MODULES: &[&str] = &[
    "release_reconcile_common.py",
    "release_reconcile_cargo.py",
    "release_package_contract.py",
    "release_reconcile_registry.py",
    "release_publish_metadata.py",
    "release_publish_manifest.py",
    "release_publish_verify.py",
    "release_publish_artifact.py",
];

const PACKAGE_MODULES: &[&str] = &[
    "release_source_validation.py",
    "release_reconcile_common.py",
    "release_reconcile_cargo.py",
    "release_package_contract.py",
    "release_publish_metadata.py",
    "release_publish_manifest.py",
    "release_preflight_cargo.py",
    "release_package.py",
];
const PREPARE_ANONYMOUS_MODULES: &[&str] = &[
    "release_source_validation.py",
    "release_reconcile_common.py",
    "release_prepare_bytes.py",
    "release_prepare_notes.py",
    "release_prepare_summary.py",
    "release_prepare_anonymous.py",
];
const PREPARE_FORGE_MODULES: &[&str] = &[
    "release_reconcile_common.py",
    "release_reconcile_forge.py",
    "release_prepare_bytes.py",
    "release_prepare_notes.py",
    "release_prepare_forge_workspace.py",
    "release_prepare_forge_metadata.py",
    "release_prepare_forge.py",
];

fn execution_modules(kind: ProofKind) -> Result<(Vec<&'static str>, &'static str), ContractError> {
    use ProofKind::*;
    Ok(match kind {
        SourceSnapshot => {
            return Err(ContractError::identity(
                "release_source_snapshot",
                "separate_source_owner",
            ));
        }
        PreparedPackage | PackageVerify => {
            return Err(ContractError::identity(
                "release_source_intent",
                "compiled_inputs_required",
            ));
        }
        AnonymousPackage => (PACKAGE_MODULES.to_vec(), "package_main"),
        RegistryPublishOidc | RegistryPublishBootstrap => {
            let mut modules = PURE_MODULES.to_vec();
            modules.extend([
                "release_publish_transport.py",
                "release_publish_auth.py",
                "release_publish_registry.py",
                "release_publish_entry.py",
            ]);
            let entry = if kind == RegistryPublishOidc {
                "trusted_publish_main"
            } else {
                "bootstrap_publish_main"
            };
            (modules, entry)
        }
        PrepareAnonymous => (
            PREPARE_ANONYMOUS_MODULES.to_vec(),
            "preparation_anonymous_main",
        ),
        PrepareForge => (PREPARE_FORGE_MODULES.to_vec(), "preparation_forge_main"),
        ForgePreflight | RegistryArtifactProof | ForgePublish | Reconcile => {
            let mut modules = PURE_MODULES.to_vec();
            modules.extend(["release_reconcile_forge.py", "release_publish_proof.py"]);
            let entry = match kind {
                ForgePreflight => {
                    modules.push("release_forge_preflight.py");
                    "forge_preflight_main"
                }
                RegistryArtifactProof => "registry_artifact_proof_main",
                ForgePublish => {
                    modules.extend([
                        "release_forge_publish_read.py",
                        "release_forge_publish_verify.py",
                        "release_forge_publish_api.py",
                        "release_forge_publish.py",
                    ]);
                    "forge_publish_main"
                }
                Reconcile => {
                    modules.extend([
                        "release_forge_publish_read.py",
                        "release_forge_publish_verify.py",
                        "release_reconcile_entry.py",
                    ]);
                    "reconcile_main"
                }
                _ => {
                    return Err(ContractError::identity(
                        "rust_release",
                        "compiled_entry_kind",
                    ));
                }
            };
            (modules, entry)
        }
    })
}

/// Emit generic companion sources for complete generated-tree inventory.
/// # Errors
/// Rejects invalid generator markers or fixed support source identities.
pub(super) fn support_sources(version: &str) -> Result<Vec<CompiledSupportSource>, ContractError> {
    [
        (
            ".github/velnor/release_reconcile_common.py",
            include_str!("release_reconcile_common.py"),
        ),
        (
            ".github/velnor/release_reconcile_forge.py",
            include_str!("release_reconcile_forge.py"),
        ),
        (
            ".github/velnor/release_forge_preflight.py",
            include_str!("release_forge_preflight.py"),
        ),
        (
            ".github/velnor/release_reconcile_entry.py",
            include_str!("release_reconcile_entry.py"),
        ),
        (
            ".github/velnor/release_publish_proof.py",
            include_str!("release_publish_proof.py"),
        ),
        (
            ".github/velnor/release_forge_publish_read.py",
            include_str!("release_forge_publish_read.py"),
        ),
        (
            ".github/velnor/release_forge_publish_verify.py",
            include_str!("release_forge_publish_verify.py"),
        ),
        (
            ".github/velnor/release_forge_publish_api.py",
            include_str!("release_forge_publish_api.py"),
        ),
        (
            ".github/velnor/release_forge_publish.py",
            include_str!("release_forge_publish.py"),
        ),
        (
            ".github/velnor/release_prepare_bytes.py",
            include_str!("release_prepare_bytes.py"),
        ),
        (
            ".github/velnor/release_prepare_notes.py",
            include_str!("release_prepare_notes.py"),
        ),
        (
            ".github/velnor/release_prepare_summary.py",
            include_str!("release_prepare_summary.py"),
        ),
        (
            ".github/velnor/release_prepare_anonymous.py",
            include_str!("release_prepare_anonymous.py"),
        ),
        (
            ".github/velnor/release_prepare_forge_workspace.py",
            include_str!("release_prepare_forge_workspace.py"),
        ),
        (
            ".github/velnor/release_prepare_forge_metadata.py",
            include_str!("release_prepare_forge_metadata.py"),
        ),
        (
            ".github/velnor/release_prepare_forge.py",
            include_str!("release_prepare_forge.py"),
        ),
    ]
    .into_iter()
    .map(|(path, source)| CompiledSupportSource::compiled(path, source, version))
    .collect()
}

/// Compose all generation sources and immutable execution wrappers.
/// # Errors
/// Rejects invalid markers or source identities from any compiled owner.
pub(super) fn complete_support_sources(
    inputs: &super::release_steps::JobInputs<'_>,
    version: &str,
) -> Result<Vec<CompiledSupportSource>, crate::OrchestratorError> {
    let mut sources = super::release_admission::support_sources(version)?;
    sources.extend(support_sources(version)?);
    sources.extend(snapshot::support_sources(inputs, version)?);
    sources.extend(velnor_actions_rust::release_support_sources::support_sources(version)?);
    for kind in proof_record_kinds(inputs.release.authentication, true) {
        let path = execution_path(kind);
        let body = if kind == ProofKind::SourceSnapshot {
            snapshot::execution_body(inputs, version)?
        } else {
            execution_body(version, kind)?
        };
        sources.push(CompiledSupportSource::compiled(path, &body, version)?);
    }
    Ok(sources)
}

fn execution_path(kind: ProofKind) -> &'static str {
    use ProofKind::*;
    match kind {
        SourceSnapshot => ".github/velnor/release_source_snapshot.sh",
        PreparedPackage => ".github/velnor/release_prepared_package.sh",
        PackageVerify => ".github/velnor/release_package_verify.sh",
        AnonymousPackage => PACKAGE_SCRIPT_PATH,
        ForgePreflight => FORGE_PREFLIGHT_SCRIPT_PATH,
        Reconcile => RECONCILE_SCRIPT_PATH,
        RegistryArtifactProof => ".github/velnor/release_registry_artifact_proof.sh",
        RegistryPublishOidc | RegistryPublishBootstrap => {
            ".github/velnor/release_registry_publish.sh"
        }
        ForgePublish => ".github/velnor/release_forge_publish.sh",
        PrepareAnonymous => ".github/velnor/release_prepare_anonymous.sh",
        PrepareForge => ".github/velnor/release_prepare_forge.sh",
    }
}

/// Complete marked Bash bytes for the source-bound operation compiler.
/// # Errors
/// Rejects malformed fixed source records and invalid generator markers.
pub(super) fn execution_source(
    inputs: &super::release_steps::JobInputs<'_>,
    version: &str,
    kind: ProofKind,
) -> Result<String, crate::OrchestratorError> {
    if matches!(kind, ProofKind::SourceSnapshot) {
        return snapshot::execution_source(inputs, version);
    }
    Ok(generated_source(version, &execution_body(version, kind)?)?)
}

fn execution_body(version: &str, kind: ProofKind) -> Result<String, ContractError> {
    if matches!(kind, ProofKind::SourceSnapshot) {
        return Err(ContractError::identity(
            "release_source_snapshot",
            "compiled_inputs_required",
        ));
    }
    let mut sources = support_sources(version)?;
    sources.extend(velnor_actions_rust::release_support_sources::support_sources(version)?);
    let (modules, entry) = execution_modules(kind)?;
    let mut closure = BTreeMap::new();
    for name in &modules {
        let path = format!(".github/velnor/{name}");
        let source = sources
            .iter()
            .find(|source| source.path() == path)
            .ok_or_else(|| ContractError::identity("rust_release", "compiled_module_missing"))?;
        closure.insert(*name, source.source());
    }
    let sources = serde_json::to_string(&closure)
        .map_err(|_| ContractError::identity("rust_release", "compiled_module_encoding"))?;
    let names = serde_json::to_string(&modules)
        .map_err(|_| ContractError::identity("rust_release", "compiled_order_encoding"))?;
    Ok(format!(
        "set -euo pipefail\npython3 -I -S - \"$@\" <<'VELNOR_RELEASE_COMPILED_BODY'\nCOMPILED_SOURCES = {sources}\n{}\nVELNOR_RELEASE_COMPILED_BODY\n",
        sealed_launcher(&names, entry),
    ))
}

fn sealed_launcher(names: &str, entry: &str) -> String {
    format!(
        r#"import os
from pathlib import Path
workspace = Path(os.environ['GITHUB_WORKSPACE'])
if not workspace.is_absolute() or workspace.resolve() != workspace:
    raise SystemExit('release_workspace_noncanonical')
namespace = {{'__name__': 'velnor_release_compiled'}}
for filename in {names}:
    exec(compile(COMPILED_SOURCES[filename], filename, 'exec'), namespace)
namespace['{entry}']()
"#
    )
}
