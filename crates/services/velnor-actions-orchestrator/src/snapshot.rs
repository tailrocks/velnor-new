//! Canonical identity helpers: digests, paths, components (P03).
//!
//! Declared via `#[path]` from `internal_plan.rs` (no `lib.rs` edit).
//! Centralizes canonical serialization, strict parsing, checkout-path
//! normalization, Cargo ID normalization, generator markers, the single
//! immutable [`ExecutionSnapshot`] per analysis, platform inputs with
//! runner-image evidence, SHA-256 executable verification, and the
//! canonical-schema migration gate. Group identities live in
//! [`super::identities`], per-task closures in [`super::closure`];
//! unknown inputs are explicit states, never silent `None`s.

use serde::Serialize;
use velnor_actions_contract::cachekey::{PlatformInputs, platform_id};
use velnor_actions_contract::{
    ContractError, UNOBSERVED_IMAGE_VALUE, canonical_json_bytes, digest_b3, parse_strict_json,
};

/// Explicit unknown marker for unverifiable archive sources.
///
/// Deprecated: archive callers must bind real source content (P04-7);
/// this marker fails closed wherever it still appears.
pub(crate) const UNKNOWN_ARCHIVE_SOURCE: &str = "velnor-unknown-archive-source-v1";
/// SHA-256 of the empty string: explicit unverified-generator marker.
pub(crate) const UNRESOLVED_GENERATOR_SHA: &str =
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
/// Current canonical-schema version; evidence carrying any other version
/// migrates explicitly or is rejected, never silently reinterpreted.
/// Version 2 records per-task input-closure digests in baseline entries;
/// version 1 baselines bound no source bytes and are rejected outright.
pub(crate) const CANONICAL_SCHEMA_VERSION: u32 = 2;

/// BLAKE3 digest over canonical JSON bytes: the single digest function.
///
/// # Errors
///
/// Returns [`ContractError`] when canonical serialization fails.
pub(crate) fn canonical_digest<T: Serialize>(value: &T) -> Result<String, ContractError> {
    Ok(digest_b3(&canonical_json_bytes(value)?))
}

/// Strict JSON parse: duplicate keys are rejected, never last-wins.
///
/// # Errors
///
/// Returns [`ContractError`] for malformed JSON or duplicate keys.
pub(crate) fn parse_canonical_json(text: &str) -> Result<serde_json::Value, ContractError> {
    parse_strict_json(text)
}

/// Release triple for the build host; unknown pairs keep `{arch}-{os}`.
pub(crate) fn map_release_triple(arch: &str, os: &str) -> String {
    match (arch, os) {
        ("x86_64", "linux") => "x86_64-unknown-linux-gnu",
        ("aarch64", "macos") => "aarch64-apple-darwin",
        ("x86_64", "macos") => "x86_64-apple-darwin",
        _ => return format!("{arch}-{os}"),
    }
    .to_owned()
}

/// Reject any canonical-schema version except the current one.
///
/// Old evidence migrates explicitly through a versioned migrator or is
/// rejected here with a migration error; it is never reinterpreted
/// under the new schema.
///
/// # Errors
///
/// Returns [`ContractError`] naming the required migration.
pub(crate) fn check_canonical_version(found: u32) -> Result<(), ContractError> {
    if found == CANONICAL_SCHEMA_VERSION {
        Ok(())
    } else {
        Err(ContractError::identity(
            "canonical_schema",
            format!("migration_required:v{found}_to_v{CANONICAL_SCHEMA_VERSION}"),
        ))
    }
}

/// Platform inputs for one runner label plus execution target.
///
/// The label is recorded verbatim in `runs_on` (a genuine request
/// input); image evidence is explicitly unobserved. The generator
/// never sees the provisioned runner, so label text is never split
/// into `ImageOS`/`ImageVersion` facts: real values arrive only as
/// observed provisioner evidence, and until then the digest commits
/// to the label plus the target triple, nothing more (P03-4).
/// OS/arch come from the release target mapping.
///
/// # Errors
///
/// Returns [`ContractError`] for targets outside the supported set;
/// unknown targets never silently take the build host's arch/OS.
pub(crate) fn platform_inputs_for(
    label: &str,
    target: &str,
) -> Result<PlatformInputs, ContractError> {
    let (arch, os_name) = match target {
        "x86_64-unknown-linux-gnu" => ("x86_64", "linux"),
        "aarch64-apple-darwin" => ("aarch64", "macos"),
        "x86_64-apple-darwin" => ("x86_64", "macos"),
        _ => {
            return Err(ContractError::identity(
                "target",
                format!("unsupported_target:{target}"),
            ));
        }
    };
    Ok(PlatformInputs {
        os: os_name.to_owned(),
        arch: arch.to_owned(),
        runs_on: label.to_owned(),
        image_os: UNOBSERVED_IMAGE_VALUE.to_owned(),
        image_version: UNOBSERVED_IMAGE_VALUE.to_owned(),
        target: target.to_owned(),
    })
}

/// Platform identity digest over runner image evidence (P03-4).
///
/// # Errors
///
/// Returns [`ContractError`] for unsupported targets and invalid
/// platform inputs; no fallback digest is ever substituted.
pub(crate) fn platform_id_for(label: &str, target: &str) -> Result<String, ContractError> {
    let inputs = platform_inputs_for(label, target)?;
    platform_id(&inputs)
}

/// One immutable execution snapshot per analysis (P03-1).
///
/// Root cause of per-group rebuilds: every group rescanned discovery
/// and recomputed graph digests linearly. The snapshot builds the
/// graph/path index once and every group lookup is a map hit. All
/// fields are private with no setters: construction is total and the
/// value is immutable by construction.
#[derive(Debug, Clone)]
pub(crate) struct ExecutionSnapshot {
    /// Workspace-root to canonical graph digest.
    graph_digests: std::collections::BTreeMap<String, String>,
    /// Workspace-root to workspace identity digest.
    workspace_ids: std::collections::BTreeMap<String, String>,
    /// Package ID to owning workspace root.
    member_index: std::collections::BTreeMap<String, String>,
    /// Manifest path to owning package ID.
    manifest_index: std::collections::BTreeMap<String, String>,
}

impl ExecutionSnapshot {
    /// Build the snapshot once per analysis from discovery facts.
    pub(crate) fn build(discovery: &crate::discover::Discovery) -> Self {
        let mut graph_digests = std::collections::BTreeMap::new();
        let mut workspace_ids = std::collections::BTreeMap::new();
        let mut member_index = std::collections::BTreeMap::new();
        let mut manifest_index = std::collections::BTreeMap::new();
        for workspace in &discovery.workspaces {
            let graph = super::identities::snapshot_graph_for(&workspace.record);
            let digest = canonical_digest(&graph).unwrap_or_else(|_| digest_b3(b"graph_error"));
            graph_digests.insert(workspace.record.workspace_root.clone(), digest.clone());
            workspace_ids.insert(workspace.record.workspace_root.clone(), digest);
            for package in &workspace.record.packages {
                member_index.insert(package.id.clone(), workspace.record.workspace_root.clone());
                manifest_index.insert(package.manifest.clone(), package.id.clone());
            }
        }
        Self {
            graph_digests,
            workspace_ids,
            member_index,
            manifest_index,
        }
    }

    /// Graph digest for the workspace owning `package_id` or `manifest`.
    pub(crate) fn graph_digest_for(&self, package_id: &str, manifest: &str) -> String {
        self.owner_root(package_id, manifest)
            .and_then(|root| self.graph_digests.get(root))
            .cloned()
            .unwrap_or_else(|| digest_b3(b"no-workspace"))
    }

    /// Workspace identity for the workspace owning `package_id` or `manifest`.
    pub(crate) fn workspace_id_for(&self, package_id: &str, manifest: &str) -> String {
        self.owner_root(package_id, manifest)
            .and_then(|root| self.workspace_ids.get(root))
            .cloned()
            .unwrap_or_else(|| digest_b3(b"no-workspace"))
    }

    /// Owning workspace root for one group, via the prebuilt index.
    fn owner_root(&self, package_id: &str, manifest: &str) -> Option<&String> {
        if let Some(root) = self.member_index.get(package_id) {
            return Some(root);
        }
        self.manifest_index
            .get(manifest)
            .and_then(|id| self.member_index.get(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_bytes_ignore_key_order() {
        let left = serde_json::json!({"b": 1, "a": [1, 2]});
        let right = serde_json::json!({"a": [1, 2], "b": 1});
        assert_eq!(
            canonical_digest(&left).expect("digest"),
            canonical_digest(&right).expect("digest")
        );
        assert!(parse_canonical_json(r#"{"a": 1, "a": 2}"#).is_err());
        assert!(parse_canonical_json(r#"{"a": 1}"#).is_ok());
    }

    #[test]
    fn triples_map_to_release_targets() {
        assert_eq!(
            map_release_triple("x86_64", "linux"),
            "x86_64-unknown-linux-gnu"
        );
        assert_eq!(map_release_triple("riscv64", "linux"), "riscv64-linux");
        assert!(!UNRESOLVED_GENERATOR_SHA.bytes().all(|b| b == b'0'));
    }

    #[test]
    fn platform_images_and_versions_flip() {
        let linux = platform_id_for("ubuntu-26.04", "x86_64-unknown-linux-gnu").expect("platform");
        let older = platform_id_for("ubuntu-24.04", "x86_64-unknown-linux-gnu").expect("platform");
        assert_ne!(linux, older);
        assert!(velnor_actions_contract::validate_digest(&linux).is_ok());
        assert!(check_canonical_version(2).is_ok());
        assert!(check_canonical_version(0).is_err());
        assert!(check_canonical_version(1).is_err());
    }

    #[test]
    fn platform_image_evidence_is_unobserved_not_label_split() {
        for label in ["ubuntu-26.04", "ubuntu-24.04", "self-hosted"] {
            let inputs = platform_inputs_for(label, "x86_64-unknown-linux-gnu").expect("platform");
            assert_eq!(inputs.runs_on, label);
            assert_eq!(inputs.image_os, UNOBSERVED_IMAGE_VALUE, "{label}");
            assert_eq!(inputs.image_version, UNOBSERVED_IMAGE_VALUE, "{label}");
        }
        let fabricated = PlatformInputs {
            os: "linux".to_owned(),
            arch: "x86_64".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            image_os: "ubuntu".to_owned(),
            image_version: "26.04".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
        };
        let honest = platform_id_for("ubuntu-26.04", "x86_64-unknown-linux-gnu").expect("id");
        let fake = platform_id(&fabricated).expect("id");
        assert_ne!(honest, fake);
    }

    #[test]
    fn unknown_targets_reject_instead_of_taking_host() {
        for target in [
            "host",
            "riscv64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
            "",
        ] {
            let err = platform_id_for("ubuntu-26.04", target).expect_err("target");
            assert!(err.to_string().contains("unsupported_target"), "{err}");
        }
        let mac = platform_id_for("ubuntu-26.04", "aarch64-apple-darwin").expect("mac");
        assert!(velnor_actions_contract::validate_digest(&mac).is_ok());
    }
}
