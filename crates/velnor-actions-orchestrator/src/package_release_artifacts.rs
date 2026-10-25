use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use velnor_actions_contract::{
    PackageReleasePlanProduct,
    config::RustBinaryReleaseCompanion,
    package_release::{PackageReleaseAsset, PackageReleasePayloadFormat},
    parse_strict_json,
};

use crate::OrchestratorError;

#[path = "package_release_artifacts_archive.rs"]
mod archive;
pub(crate) use archive::extract_tar_gz_members;
#[path = "package_release_artifacts_checksums.rs"]
mod checksums;
#[path = "package_release_artifacts_identity.rs"]
mod identity;
#[path = "package_release_artifacts_inventory.rs"]
mod inventory;
#[path = "package_release_artifacts_public_content.rs"]
mod public_content;
use identity::{validate_expected_identity, validate_manifest_identity};
pub(crate) use public_content::validate_authenticated_release_content;
pub(crate) mod preview_payload;

const HANDOFF_SCHEMA: &str = "velnor.package-output.v1";
pub(crate) const MAX_HANDOFF_BYTES: u64 = 4 * 1024 * 1024;
pub(crate) const MAX_SBOM_BYTES: u64 = 32 * 1024 * 1024;
pub(crate) const MAX_PAYLOAD_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_ARTIFACT_BYTES: u64 = 1024 * 1024 * 1024;
pub(crate) const MAX_FILE_COUNT: usize = 4096;
const MAX_PATH_BYTES: usize = 4096;
const MAX_PATH_DEPTH: usize = 64;
pub(crate) const MAX_TOTAL_BYTES: u64 = 6 * 1024 * 1024 * 1024;
const MAX_TOTAL_DECOMPRESSED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const VERIFY_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackageOutputManifest {
    pub schema: String,
    pub source_commit: String,
    pub source_ref: String,
    pub version: String,
    pub payloads: Vec<PackageOutputPayload>,
    pub companions: Vec<PackageOutputCompanion>,
    pub sboms: Vec<PackageOutputSbom>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackageOutputPayload {
    pub product_id: String,
    pub target: String,
    pub format: PackageReleasePayloadFormat,
    pub name: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackageOutputCompanion {
    pub id: String,
    pub path: String,
    pub name: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackageOutputSbom {
    pub payload_name: String,
    pub name: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedPackageArtifact {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub sha256: String,
    pub release_asset: PackageReleaseAsset,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedPackageOutput {
    pub source_commit: String,
    pub source_ref: String,
    pub version: String,
    pub artifacts: Vec<VerifiedPackageArtifact>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PackageOutputIdentity<'a> {
    pub source_commit: &'a str,
    pub source_ref: &'a str,
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

pub(crate) fn verify_package_output(
    producer_dir: &Path,
    manifest_path: &str,
    staging_dir: &Path,
    products: &[PackageReleasePlanProduct],
    companions: &[RustBinaryReleaseCompanion],
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

fn expected_files(
    products: &[PackageReleasePlanProduct],
    companions: &[RustBinaryReleaseCompanion],
    manifest_path: &str,
) -> Result<Vec<ExpectedFile>, OrchestratorError> {
    let mut files = Vec::new();
    for product in products {
        for target in &product.targets {
            let payload = match product.format {
                PackageReleasePayloadFormat::TarGz => format!("{}-{target}.tar.gz", product.id),
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
