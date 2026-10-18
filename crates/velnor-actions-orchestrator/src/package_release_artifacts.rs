//! Independent admission of the untrusted producer output before signing.
//!
//! This module never extracts or executes payloads. It parses only the closed
//! package-output handoff, copies expected regular files into a fresh directory
//! while hashing them, and inspects the staged tar streams and `CycloneDX` files.

use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use velnor_actions_contract::{
    PackageReleasePlanProduct,
    config::RustBinaryPreviewCompanion,
    package_release::{PackageReleaseAsset, PackageReleasePayloadFormat},
    parse_strict_json,
};

use crate::OrchestratorError;

#[path = "package_release_artifacts_archive.rs"]
mod archive;
#[path = "package_release_artifacts_checksums.rs"]
mod checksums;
#[path = "package_release_artifacts_inventory.rs"]
mod inventory;
#[cfg(test)]
#[path = "package_release_artifacts_real_tests.rs"]
mod real_tests;
#[cfg(test)]
#[path = "package_release_artifacts_tests.rs"]
mod tests;

const HANDOFF_SCHEMA: &str = "velnor.package-output.v1";
const MAX_HANDOFF_BYTES: u64 = 4 * 1024 * 1024;
const MAX_SBOM_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PAYLOAD_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_ARTIFACT_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_FILE_COUNT: usize = 4096;
const MAX_PATH_BYTES: usize = 4096;
const MAX_PATH_DEPTH: usize = 64;
const MAX_TOTAL_BYTES: u64 = 6 * 1024 * 1024 * 1024;
const MAX_TOTAL_DECOMPRESSED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const VERIFY_TIMEOUT: Duration = Duration::from_secs(120);

/// Untrusted producer output contract, versioned independently from the public receipt.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackageOutputManifest {
    /// Exact handoff schema discriminator.
    pub schema: String,
    /// Full source commit expected by the trusted workflow.
    pub source_commit: String,
    /// Source ref expected by the trusted workflow.
    pub source_ref: String,
    /// Exact Velnor-computed release version.
    pub version: String,
    /// Producer payload inventory; rows sorted by payload name.
    pub payloads: Vec<PackageOutputPayload>,
    /// Producer companion inventory; rows sorted by companion ID.
    pub companions: Vec<PackageOutputCompanion>,
    /// Producer SBOM inventory; rows sorted by payload name.
    pub sboms: Vec<PackageOutputSbom>,
}

/// Producer-declared payload identity and untrusted digest.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackageOutputPayload {
    /// Trusted product ID expected for this payload.
    pub product_id: String,
    /// Exact target triple.
    pub target: String,
    /// Actual byte representation, cross-checked against the trusted plan.
    pub format: PackageReleasePayloadFormat,
    /// Payload name derived by Velnor.
    pub name: String,
    /// SHA-256 claimed by the producer, checked against copied bytes.
    pub sha256: String,
}

/// Producer-declared companion identity and untrusted digest.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackageOutputCompanion {
    /// Trusted companion ID.
    pub id: String,
    /// Producer output path relative to the output directory.
    pub path: String,
    /// Public asset name declared in source configuration.
    pub name: String,
    /// SHA-256 claimed by the producer, checked against copied bytes.
    pub sha256: String,
}

/// Producer-declared `CycloneDX` payload binding and untrusted digest.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackageOutputSbom {
    /// Payload filename bound by the SBOM.
    pub payload_name: String,
    /// Exact sidecar filename derived by Velnor.
    pub name: String,
    /// SHA-256 claimed by the producer, checked against copied bytes.
    pub sha256: String,
}

/// Validated artifact copied into a fresh Velnor-owned staging directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedPackageArtifact {
    /// Release asset filename.
    pub name: String,
    /// Velnor-owned staged file path to the exact validated bytes.
    pub path: PathBuf,
    /// Number of staged bytes.
    pub size: u64,
    /// Lowercase SHA-256 of staged bytes.
    pub sha256: String,
    /// Receipt-ready identity derived from trusted declarations and copied bytes.
    pub release_asset: PackageReleaseAsset,
}

/// Complete independently verified producer inventory, ready for signer input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedPackageOutput {
    /// Exact authenticated source commit carried by the handoff.
    pub source_commit: String,
    /// Exact source ref carried by the handoff.
    pub source_ref: String,
    /// Exact computed release version carried by the handoff.
    pub version: String,
    /// Archive, companion, and SBOM files copied to Velnor-owned staging.
    pub artifacts: Vec<VerifiedPackageArtifact>,
}

/// Trusted identity expected from the untrusted package-output handoff.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PackageOutputIdentity<'a> {
    /// Exact expected source commit.
    pub source_commit: &'a str,
    /// Exact expected source ref.
    pub source_ref: &'a str,
    /// Exact computed release version.
    pub version: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArtifactKind {
    Payload,
    Companion,
    Sbom,
    Handoff,
}

#[derive(Debug, Clone)]
struct ExpectedFile {
    path: String,
    name: String,
    kind: ArtifactKind,
    product_id: Option<String>,
    target: Option<String>,
    companion_id: Option<String>,
    archive_subject: Option<String>,
    payload_format: Option<PackageReleasePayloadFormat>,
    bins: Vec<String>,
}

/// Validate and copy the exact producer byte inventory before any signing.
///
/// `producer_dir` is untrusted output. `staging_dir` must be a new path owned by
/// Velnor. The handoff's identity is compared to trusted workflow inputs; its
/// names and digests never determine what files are admitted.
pub(crate) fn verify_package_output(
    producer_dir: &Path,
    manifest_path: &str,
    staging_dir: &Path,
    products: &[PackageReleasePlanProduct],
    companions: &[RustBinaryPreviewCompanion],
    identity: PackageOutputIdentity<'_>,
) -> Result<VerifiedPackageOutput, OrchestratorError> {
    let started = Instant::now();
    validate_expected_identity(
        identity.source_commit,
        identity.source_ref,
        identity.version,
    )?;
    let manifest_rel = validate_relative_path(manifest_path)?;
    let manifest_abs = producer_dir.join(&manifest_rel);
    inventory::ensure_no_symlink_path(producer_dir, &manifest_rel)?;
    let manifest_bytes = inventory::read_limited(&manifest_abs, MAX_HANDOFF_BYTES, started)?;
    let manifest_value = parse_strict_json(
        std::str::from_utf8(&manifest_bytes).map_err(|_| invalid("handoff_not_utf8"))?,
    )
    .map_err(|_| invalid("handoff_invalid_json"))?;
    let manifest: PackageOutputManifest =
        serde_json::from_value(manifest_value).map_err(|_| invalid("handoff_schema_invalid"))?;
    validate_manifest_identity(
        &manifest,
        identity.source_commit,
        identity.source_ref,
        identity.version,
    )?;

    let expected = expected_files(products, companions, &manifest_rel)?;
    inventory::compare_manifest_inventory(&manifest, &expected)?;
    inventory::validate_source_tree(producer_dir, &expected)?;
    inventory::create_fresh_staging(staging_dir)?;
    let result = (|| {
        let copied = inventory::copy_expected_files(
            producer_dir,
            staging_dir,
            &expected,
            &manifest,
            started,
        )?;
        archive::validate_copied_content(
            &copied,
            &expected,
            identity.source_commit,
            identity.source_ref,
            identity.version,
            started,
        )?;
        Ok(copied)
    })();
    let copied = match result {
        Ok(copied) => copied,
        Err(error) => {
            fs::remove_dir_all(staging_dir).map_err(|cleanup| {
                OrchestratorError::io(staging_dir.display().to_string(), cleanup.to_string())
            })?;
            return Err(error);
        }
    };
    Ok(VerifiedPackageOutput {
        source_commit: identity.source_commit.to_owned(),
        source_ref: identity.source_ref.to_owned(),
        version: identity.version.to_owned(),
        artifacts: copied,
    })
}

fn validate_expected_identity(
    source_commit: &str,
    source_ref: &str,
    version: &str,
) -> Result<(), OrchestratorError> {
    if source_commit.len() != 40
        || !source_commit
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || !source_ref.starts_with("refs/heads/")
        || source_ref.len() > 300
        || version.is_empty()
        || version.len() > 128
        || !version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
    {
        return Err(invalid("trusted_package_identity_invalid"));
    }
    Ok(())
}

fn validate_manifest_identity(
    manifest: &PackageOutputManifest,
    source_commit: &str,
    source_ref: &str,
    version: &str,
) -> Result<(), OrchestratorError> {
    if manifest.schema != HANDOFF_SCHEMA
        || manifest.source_commit != source_commit
        || manifest.source_ref != source_ref
        || manifest.version != version
    {
        return Err(invalid("handoff_source_or_version_mismatch"));
    }
    Ok(())
}

fn expected_files(
    products: &[PackageReleasePlanProduct],
    companions: &[RustBinaryPreviewCompanion],
    manifest_path: &str,
) -> Result<Vec<ExpectedFile>, OrchestratorError> {
    let mut files = Vec::new();
    for product in products {
        for target in &product.targets {
            let payload = match product.format {
                PackageReleasePayloadFormat::TarGz => format!("{}-{target}.tar.gz", product.id),
                PackageReleasePayloadFormat::RawBinary => format!("{}-{target}", product.bins[0]),
            };
            files.push(ExpectedFile {
                path: payload.clone(),
                name: payload.clone(),
                kind: ArtifactKind::Payload,
                product_id: Some(product.id.clone()),
                target: Some(target.clone()),
                companion_id: None,
                archive_subject: Some(payload.clone()),
                payload_format: Some(product.format),
                bins: product.bins.clone(),
            });
            files.push(ExpectedFile {
                path: format!("{payload}.sbom.json"),
                name: format!("{payload}.sbom.json"),
                kind: ArtifactKind::Sbom,
                product_id: Some(product.id.clone()),
                target: Some(target.clone()),
                companion_id: None,
                archive_subject: Some(payload),
                payload_format: None,
                bins: Vec::new(),
            });
        }
    }
    for companion in companions {
        files.push(ExpectedFile {
            path: validate_relative_path(&companion.path)?,
            name: companion.asset_name.clone(),
            kind: ArtifactKind::Companion,
            product_id: None,
            target: None,
            companion_id: Some(companion.id.clone()),
            archive_subject: Some(companion.asset_name.clone()),
            payload_format: None,
            bins: Vec::new(),
        });
    }
    files.push(ExpectedFile {
        path: manifest_path.to_owned(),
        name: manifest_path.to_owned(),
        kind: ArtifactKind::Handoff,
        product_id: None,
        target: None,
        companion_id: None,
        archive_subject: None,
        payload_format: None,
        bins: Vec::new(),
    });
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let unique = files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<BTreeSet<_>>();
    if unique.len() != files.len() || files.len() > MAX_FILE_COUNT {
        return Err(invalid("expected_package_files_duplicate_or_too_many"));
    }
    let unique_names = files
        .iter()
        .filter(|file| file.kind != ArtifactKind::Handoff)
        .map(|file| file.name.as_str())
        .collect::<BTreeSet<_>>();
    if unique_names.len() != files.len().saturating_sub(1) {
        return Err(invalid("expected_package_asset_names_duplicate"));
    }
    Ok(files)
}

fn validate_relative_path(path: &str) -> Result<String, OrchestratorError> {
    if path.is_empty()
        || path.len() > MAX_PATH_BYTES
        || path.contains('\\')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(invalid("package_output_path_invalid"));
    }
    let candidate = Path::new(path);
    if candidate.is_absolute() || candidate.components().count() > MAX_PATH_DEPTH {
        return Err(invalid("package_output_path_invalid"));
    }
    let mut parts = Vec::new();
    for component in candidate.components() {
        let Component::Normal(part) = component else {
            return Err(invalid("package_output_path_invalid"));
        };
        let part = part
            .to_str()
            .ok_or_else(|| invalid("package_output_path_invalid"))?;
        if part.is_empty() || part == "." || part == ".." || part.contains('\\') {
            return Err(invalid("package_output_path_invalid"));
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return Err(invalid("package_output_path_invalid"));
    }
    Ok(parts.join("/"))
}

fn invalid(problem: &str) -> OrchestratorError {
    OrchestratorError::Internal {
        problem: problem.to_owned(),
    }
}
