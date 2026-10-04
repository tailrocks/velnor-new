//! Source-bound generator release manifest assembly and reconciliation.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use velnor_actions_contract::{
    RELEASE_MANIFEST_FILENAME, ReleaseManifest, SUPPORTED_TARGETS, TargetRecord, asset_filename,
    require_release_version,
};

use crate::OrchestratorError;
use crate::exclusive_write::write_exclusive;
use crate::internal::internal;
use crate::retrieve_reports::read_staged_bytes;

/// Private operation that assembles the canonical release manifest.
pub const GENERATOR_RELEASE_MANIFEST_ASSEMBLE_OP: &str = "assemble-generator-release-manifest-v1";
/// Private operation that verifies a manifest against actual release assets.
pub const GENERATOR_RELEASE_MANIFEST_VERIFY_OP: &str = "verify-generator-release-manifest-v1";

/// Environment key naming the directory of exact generator release assets.
pub const GENERATOR_RELEASE_ASSET_DIR_ENV: &str = "VELNOR_RELEASE_ASSET_DIR";
/// Environment key carrying the exact Cargo package version.
pub const GENERATOR_RELEASE_VERSION_ENV: &str = "VELNOR_RELEASE_VERSION";
/// Environment key carrying the immutable official release tag.
pub const GENERATOR_RELEASE_TAG_ENV: &str = "VELNOR_RELEASE_TAG";
/// Environment key carrying the canonical owner/repository identity.
pub const GENERATOR_RELEASE_REPOSITORY_ENV: &str = "VELNOR_RELEASE_REPOSITORY";
/// Environment key carrying the source commit from release eligibility.
pub const GENERATOR_RELEASE_SOURCE_ENV: &str = "VELNOR_SOURCE_SHA";

const MAX_ASSET_BYTES: u64 = 536_870_912;
const MAX_SIDECAR_BYTES: u64 = 512;
const MAX_MANIFEST_BYTES: u64 = 65_536;

#[cfg(test)]
#[path = "generator_release_manifest_tests.rs"]
mod tests;

#[derive(Debug)]
struct ReleaseContext {
    asset_dir: PathBuf,
    version: String,
    tag: String,
    repository: String,
    source: String,
}

/// Assemble a manifest from binaries and sidecars staged by the release workflow.
///
/// The caller binds this asset directory to same-run build artifacts and the
/// eligibility source; this local data operation does not query GitHub or attest.
///
/// # Errors
///
/// Returns an error for incomplete or unexpected assets, mismatched sidecars,
/// invalid release identity, or an existing manifest that would be replaced.
pub fn assemble_generator_release_manifest() -> Result<(), OrchestratorError> {
    let context = release_context_from_env()?;
    assemble_for(&context)
}

fn assemble_for(context: &ReleaseContext) -> Result<(), OrchestratorError> {
    validate_context(context)?;
    validate_inventory(context, false)?;
    let records = release_records(context)?;
    let manifest = release_manifest(context, records);
    manifest.validate(RELEASE_MANIFEST_FILENAME)?;
    let bytes = serde_json::to_vec(&manifest).map_err(|_| internal("manifest_encode_failed"))?;
    let path = context.asset_dir.join(RELEASE_MANIFEST_FILENAME);
    write_exclusive(&path, &bytes, "generator_release_manifest")
}

/// Verify canonical manifest bytes against actual binaries and sidecars.
///
/// # Errors
///
/// Returns an error for invalid manifest bytes, unexpected assets, or any
/// difference in release identity, target, URL, digest, or canonical bytes.
pub fn verify_generator_release_manifest() -> Result<(), OrchestratorError> {
    let context = release_context_from_env()?;
    verify_for(&context)
}

fn verify_for(context: &ReleaseContext) -> Result<(), OrchestratorError> {
    validate_context(context)?;
    validate_inventory(context, true)?;
    let records = release_records(context)?;
    let expected = release_manifest(context, records);
    expected.validate(RELEASE_MANIFEST_FILENAME)?;
    let expected_bytes =
        serde_json::to_vec(&expected).map_err(|_| internal("manifest_encode_failed"))?;
    let path = context.asset_dir.join(RELEASE_MANIFEST_FILENAME);
    let bytes = read_asset(&path, MAX_MANIFEST_BYTES, "manifest")?;
    let actual = ReleaseManifest::parse_json(
        std::str::from_utf8(&bytes).map_err(|_| internal("manifest_not_utf8"))?,
        RELEASE_MANIFEST_FILENAME,
    )?;
    actual.validate(RELEASE_MANIFEST_FILENAME)?;
    if bytes != expected_bytes {
        return Err(internal("manifest_asset_binding_mismatch"));
    }
    Ok(())
}

fn release_context_from_env() -> Result<ReleaseContext, OrchestratorError> {
    Ok(ReleaseContext {
        asset_dir: PathBuf::from(nonempty_env(GENERATOR_RELEASE_ASSET_DIR_ENV)?),
        version: nonempty_env(GENERATOR_RELEASE_VERSION_ENV)?,
        tag: nonempty_env(GENERATOR_RELEASE_TAG_ENV)?,
        repository: nonempty_env(GENERATOR_RELEASE_REPOSITORY_ENV)?,
        source: nonempty_env(GENERATOR_RELEASE_SOURCE_ENV)?,
    })
}

fn validate_context(context: &ReleaseContext) -> Result<(), OrchestratorError> {
    require_release_version(&context.version, RELEASE_MANIFEST_FILENAME)?;
    if context.repository != velnor_actions_contract::EXPECTED_REPOSITORY {
        return Err(internal("release_repository_mismatch"));
    }
    if context.tag != format!("v{}", context.version) {
        return Err(internal("release_tag_version_mismatch"));
    }
    if !velnor_actions_contract::ids::is_lower_hex_len(&context.source, 40) {
        return Err(internal("release_source_sha_malformed"));
    }
    let metadata = fs::symlink_metadata(&context.asset_dir).map_err(|err| {
        OrchestratorError::io(context.asset_dir.display().to_string(), err.to_string())
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(internal("release_asset_directory_invalid"));
    }
    Ok(())
}

fn validate_inventory(
    context: &ReleaseContext,
    has_manifest: bool,
) -> Result<(), OrchestratorError> {
    let mut expected = BTreeSet::new();
    for target in SUPPORTED_TARGETS {
        let binary = asset_filename(&context.version, target);
        expected.insert(binary.clone());
        expected.insert(format!("{binary}.sha256"));
    }
    if has_manifest {
        expected.insert(RELEASE_MANIFEST_FILENAME.to_owned());
    }
    let mut actual = BTreeSet::new();
    let entries = fs::read_dir(&context.asset_dir).map_err(|err| {
        OrchestratorError::io(context.asset_dir.display().to_string(), err.to_string())
    })?;
    for entry in entries {
        let entry = entry.map_err(|err| {
            OrchestratorError::io(context.asset_dir.display().to_string(), err.to_string())
        })?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| internal("release_asset_name_not_utf8"))?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|err| {
            OrchestratorError::io(entry.path().display().to_string(), err.to_string())
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(internal("release_asset_not_regular_file"));
        }
        if !expected.contains(&name) || !actual.insert(name) {
            return Err(internal("release_asset_set_mismatch"));
        }
    }
    if actual != expected {
        return Err(internal("release_asset_set_mismatch"));
    }
    Ok(())
}

fn release_records(context: &ReleaseContext) -> Result<Vec<TargetRecord>, OrchestratorError> {
    SUPPORTED_TARGETS
        .iter()
        .map(|target| {
            let name = asset_filename(&context.version, target);
            let binary = read_asset(&context.asset_dir.join(&name), MAX_ASSET_BYTES, "binary")?;
            if binary.is_empty() {
                return Err(internal("release_binary_empty"));
            }
            let digest = sha256_lower(&binary);
            let sidecar_name = format!("{name}.sha256");
            let sidecar = read_asset(
                &context.asset_dir.join(&sidecar_name),
                MAX_SIDECAR_BYTES,
                "sidecar",
            )?;
            let expected_sidecar = format!("{digest}  {name}\n");
            if sidecar != expected_sidecar.as_bytes() {
                return Err(internal("release_sidecar_mismatch"));
            }
            Ok(TargetRecord {
                target: (*target).to_owned(),
                artifact: format!(
                    "https://github.com/{}/releases/download/{}/{}",
                    context.repository, context.tag, name
                ),
                sha256: digest,
            })
        })
        .collect()
}

fn release_manifest(context: &ReleaseContext, targets: Vec<TargetRecord>) -> ReleaseManifest {
    ReleaseManifest {
        schema: ReleaseManifest::SCHEMA,
        version: context.version.clone(),
        repository: context.repository.clone(),
        commit: context.source.clone(),
        targets,
    }
}

fn read_asset(path: &Path, limit: u64, kind: &str) -> Result<Vec<u8>, OrchestratorError> {
    read_staged_bytes(path, limit).map_err(|reason| internal(&format!("release_{kind}_{reason}")))
}

fn nonempty_env(key: &str) -> Result<String, OrchestratorError> {
    env::var(key)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal(&format!("release_missing_env:{key}")))
}

fn sha256_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
