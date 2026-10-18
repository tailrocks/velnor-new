//! Streaming verifier and deterministic tag binding for worker image archives.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use bollard::models::ContainerConfig;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tar::{Archive, Builder, EntryType, Header};

use super::super::{ProviderFailure, hex, valid_hex, valid_sha};
use super::types::{Descriptor, ImageConfigFile, LegacyManifest, OciIndex, OciManifest, Platform};

const MAX_WORKER_ARCHIVE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_WORKER_ENTRIES: usize = 64;
const MAX_METADATA_BYTES: u64 = 1024 * 1024;
const MAX_ARCHIVE_METADATA_BYTES: u64 = 2 * 1024 * 1024;
const OCI_INDEX_MEDIA_TYPE: &str = "application/vnd.oci.image.index.v1+json";
const OCI_MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";
const OCI_CONFIG_MEDIA_TYPE: &str = "application/vnd.oci.image.config.v1+json";
const OCI_GZIP_LAYER_MEDIA_TYPE: &str = "application/vnd.oci.image.layer.v1.tar+gzip";
const IN_TOTO_MEDIA_TYPE: &str = "application/vnd.in-toto+json";
const SLSA_PREDICATE: &str = "https://slsa.dev/provenance/v1";
const BUILD_KIT_TYPE: &str =
    "https://github.com/moby/buildkit/blob/master/docs/attestations/slsa-definitions.md";
const SOURCE_REPOSITORY_URL: &str = "https://github.com/tailrocks/velnor-new";
const PLATFORM: &str = "linux/amd64";
const PATH_ENV: &str = "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
const BASE_LABELS: [&str; 4] = [
    "org.opencontainers.image.created",
    "org.opencontainers.image.description",
    "org.opencontainers.image.title",
    "org.opencontainers.image.version",
];

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::launch::resource_probe::provider) enum WorkerImage {
    Runner,
    Dind,
}

impl WorkerImage {
    pub(in crate::launch::resource_probe::provider) fn archive_name(self) -> &'static str {
        match self {
            Self::Runner => "velnor-runner-linux-amd64.tar",
            Self::Dind => "velnor-dind-linux-amd64.tar",
        }
    }

    pub(in crate::launch::resource_probe::provider) fn image_name(self) -> &'static str {
        match self {
            Self::Runner => "velnor-runner",
            Self::Dind => "velnor-dind",
        }
    }

    fn entrypoint(self) -> &'static str {
        match self {
            Self::Runner => "/usr/local/bin/velnor-runner-entrypoint",
            Self::Dind => "/usr/local/bin/velnor-dind-entrypoint",
        }
    }

    fn source_context(self) -> &'static str {
        match self {
            Self::Runner => "images/runner/ubuntu-26.04",
            Self::Dind => "images/dind",
        }
    }

    fn user(self) -> Option<&'static str> {
        match self {
            Self::Runner => Some("runner"),
            Self::Dind => None,
        }
    }

    fn working_dir(self) -> Option<&'static str> {
        match self {
            Self::Runner => Some("/home/runner"),
            Self::Dind => None,
        }
    }

    pub(in crate::launch::resource_probe::provider) fn release_tag(self) -> String {
        format!("{}:linux-amd64", self.image_name())
    }

    fn docker_image_name(self) -> String {
        format!("docker.io/library/{}", self.image_name())
    }
}

/// Identities parsed from the signed archive, separate from a daemon's image ID.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::launch::resource_probe::provider) struct ParsedWorkerArchive {
    pub(in crate::launch::resource_probe::provider) archive_sha256: String,
    pub(in crate::launch::resource_probe::provider) archive_index_digest: String,
    pub(in crate::launch::resource_probe::provider) image_index_digest: String,
    pub(in crate::launch::resource_probe::provider) image_index_size: u64,
    pub(in crate::launch::resource_probe::provider) manifest_digest: String,
    pub(in crate::launch::resource_probe::provider) manifest_size: u64,
    pub(in crate::launch::resource_probe::provider) config_digest: String,
    pub(in crate::launch::resource_probe::provider) config: ContainerConfig,
    pub(in crate::launch::resource_probe::provider) diff_ids: Vec<String>,
    pub(in crate::launch::resource_probe::provider) repo_tag: String,
    blob_inventory: BTreeMap<String, u64>,
}

impl ParsedWorkerArchive {
    pub(super) fn same_image_payload(&self, other: &Self) -> bool {
        self.image_index_digest == other.image_index_digest
            && self.image_index_size == other.image_index_size
            && self.manifest_digest == other.manifest_digest
            && self.manifest_size == other.manifest_size
            && self.config_digest == other.config_digest
            && self.config == other.config
            && self.diff_ids == other.diff_ids
            && self.blob_inventory == other.blob_inventory
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::launch::resource_probe::provider) struct DerivedLoadArchive {
    pub(in crate::launch::resource_probe::provider) path: PathBuf,
    pub(in crate::launch::resource_probe::provider) sha256: String,
    pub(in crate::launch::resource_probe::provider) archive_index_digest: String,
    pub(in crate::launch::resource_probe::provider) repo_tag: String,
    pub(in crate::launch::resource_probe::provider) parsed: ParsedWorkerArchive,
}

struct ArchiveFiles {
    files: HashMap<String, Vec<u8>>,
    blobs: HashMap<String, Blob>,
}

struct Blob {
    digest: String,
    size: u64,
    metadata: Option<Vec<u8>>,
}

struct VerifiedSelection {
    image_index_digest: String,
    image_index_size: u64,
    manifest_digest: String,
    manifest_size: u64,
    config_digest: String,
    config: ContainerConfig,
    diff_ids: Vec<String>,
    repo_tag: String,
    blob_inventory: BTreeMap<String, u64>,
}

pub(in crate::launch::resource_probe::provider) fn verify(
    path: &Path,
    image: WorkerImage,
    source_sha: &str,
    expected_archive_sha256: &str,
    expected_repo_tag: &str,
) -> Result<ParsedWorkerArchive, ProviderFailure> {
    if !valid_sha(source_sha) || !valid_hex(expected_archive_sha256, 64) {
        return Err(ProviderFailure::Archive);
    }
    let archive_sha256 = hash_archive(path)?;
    if archive_sha256 != expected_archive_sha256 {
        return Err(ProviderFailure::Archive);
    }
    let files = read_archive(path)?;
    let verified = verify_files(&files, image, source_sha, expected_repo_tag)?;
    let archive_index = files
        .files
        .get("index.json")
        .ok_or(ProviderFailure::Archive)?;
    Ok(ParsedWorkerArchive {
        archive_sha256,
        archive_index_digest: sha256(archive_index),
        image_index_digest: verified.image_index_digest,
        image_index_size: verified.image_index_size,
        manifest_digest: verified.manifest_digest,
        manifest_size: verified.manifest_size,
        config_digest: verified.config_digest,
        config: verified.config,
        diff_ids: verified.diff_ids,
        repo_tag: verified.repo_tag,
        blob_inventory: verified.blob_inventory,
    })
}

pub(in crate::launch::resource_probe::provider) fn derive_load_archive(
    source_path: &Path,
    output_path: &Path,
    image: WorkerImage,
    source_sha: &str,
    original: &ParsedWorkerArchive,
) -> Result<DerivedLoadArchive, ProviderFailure> {
    if !valid_sha(source_sha)
        || source_path == output_path
        || !output_path.is_absolute()
        || output_path.exists()
        || hash_archive(source_path)? != original.archive_sha256
    {
        return Err(ProviderFailure::Archive);
    }
    let tag = format!("v{source_sha}-{}", original.archive_sha256);
    let repo_tag = format!("{}:{tag}", image.image_name());
    if tag.len() > 128 || !valid_docker_reference(image, &repo_tag) {
        return Err(ProviderFailure::Archive);
    }
    let result = (|| {
        write_derived_archive(source_path, output_path, image, &repo_tag, &tag)?;
        let sha256 = hash_archive(output_path)?;
        let parsed = verify(output_path, image, source_sha, &sha256, &repo_tag)?;
        if !original.same_image_payload(&parsed) || parsed.repo_tag != repo_tag {
            return Err(ProviderFailure::Archive);
        }
        Ok(DerivedLoadArchive {
            path: output_path.to_path_buf(),
            sha256,
            archive_index_digest: parsed.archive_index_digest.clone(),
            repo_tag: repo_tag.clone(),
            parsed,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(output_path);
    }
    result
}

fn hash_archive(path: &Path) -> Result<String, ProviderFailure> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ProviderFailure::Archive)?;
    if !metadata.file_type().is_file()
        || metadata.len() == 0
        || metadata.len() > MAX_WORKER_ARCHIVE_BYTES
    {
        return Err(ProviderFailure::Archive);
    }
    let mut file = File::open(path).map_err(|_| ProviderFailure::Archive)?;
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| ProviderFailure::Archive)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(count).map_err(|_| ProviderFailure::Archive)?)
            .ok_or(ProviderFailure::Archive)?;
        if total > MAX_WORKER_ARCHIVE_BYTES {
            return Err(ProviderFailure::Archive);
        }
        hasher.update(&buffer[..count]);
    }
    if total != metadata.len() {
        return Err(ProviderFailure::Archive);
    }
    Ok(hex(&hasher.finalize()))
}

fn read_archive(path: &Path) -> Result<ArchiveFiles, ProviderFailure> {
    let file = File::open(path).map_err(|_| ProviderFailure::Archive)?;
    let mut archive = Archive::new(file);
    let entries = archive.entries().map_err(|_| ProviderFailure::Archive)?;
    let mut files = HashMap::new();
    let mut blobs = HashMap::new();
    let mut directories = HashSet::new();
    let mut total = 0_u64;
    let mut entry_count = 0_usize;
    for entry in entries {
        let mut entry = entry.map_err(|_| ProviderFailure::Archive)?;
        entry_count = entry_count.checked_add(1).ok_or(ProviderFailure::Archive)?;
        if entry_count > MAX_WORKER_ENTRIES {
            return Err(ProviderFailure::Archive);
        }
        let raw_path = entry
            .path()
            .map_err(|_| ProviderFailure::Archive)?
            .to_str()
            .ok_or(ProviderFailure::Archive)?
            .to_owned();
        let kind = entry.header().entry_type();
        let path = if kind == EntryType::Directory {
            raw_path.strip_suffix('/').unwrap_or(&raw_path).to_owned()
        } else {
            if raw_path.ends_with('/') {
                return Err(ProviderFailure::Archive);
            }
            raw_path
        };
        if !safe_path(&path) {
            return Err(ProviderFailure::Archive);
        }
        match kind {
            EntryType::Directory => {
                if !["blobs", "blobs/sha256"].contains(&path.as_str()) || !directories.insert(path)
                {
                    return Err(ProviderFailure::Archive);
                }
            }
            EntryType::Regular => {
                let size = entry.size();
                total = total.checked_add(size).ok_or(ProviderFailure::Archive)?;
                if total > MAX_WORKER_ARCHIVE_BYTES {
                    return Err(ProviderFailure::Archive);
                }
                if is_fixed_file(&path) {
                    let max = match path.as_str() {
                        "oci-layout" => 1024,
                        "index.json" => 64 * 1024,
                        "manifest.json" => 256 * 1024,
                        _ => return Err(ProviderFailure::Archive),
                    };
                    let bytes = read_bounded_entry(&mut entry, size, max)?;
                    if files.insert(path, bytes).is_some() {
                        return Err(ProviderFailure::Archive);
                    }
                } else {
                    let digest = path
                        .strip_prefix("blobs/sha256/")
                        .filter(|value| valid_hex(value, 64))
                        .ok_or(ProviderFailure::Archive)?;
                    let metadata_limit = (size <= MAX_METADATA_BYTES).then_some(size);
                    let (actual, metadata) = hash_entry(&mut entry, size, metadata_limit)?;
                    if actual != digest
                        || blobs
                            .insert(
                                path,
                                Blob {
                                    digest: actual,
                                    size,
                                    metadata,
                                },
                            )
                            .is_some()
                    {
                        return Err(ProviderFailure::Archive);
                    }
                }
            }
            _ => return Err(ProviderFailure::Archive),
        }
    }
    if directories != HashSet::from(["blobs".to_owned(), "blobs/sha256".to_owned()])
        || !["oci-layout", "index.json", "manifest.json"]
            .iter()
            .all(|name| files.contains_key(*name))
        || files.len() != 3
        || blobs.is_empty()
    {
        return Err(ProviderFailure::Archive);
    }
    Ok(ArchiveFiles { files, blobs })
}

fn verify_files(
    archive: &ArchiveFiles,
    image: WorkerImage,
    source_sha: &str,
    expected_repo_tag: &str,
) -> Result<VerifiedSelection, ProviderFailure> {
    verify_layout(
        archive
            .files
            .get("oci-layout")
            .ok_or(ProviderFailure::Archive)?,
    )?;
    let index_bytes = archive
        .files
        .get("index.json")
        .ok_or(ProviderFailure::Archive)?;
    let index: OciIndex =
        serde_json::from_slice(index_bytes).map_err(|_| ProviderFailure::Archive)?;
    if index.schema_version != 2
        || index.media_type != OCI_INDEX_MEDIA_TYPE
        || index.manifests.len() != 1
    {
        return Err(ProviderFailure::Archive);
    }
    let image_index = index.manifests.first().ok_or(ProviderFailure::Archive)?;
    verify_descriptor(image_index)?;
    if image_index.media_type != OCI_INDEX_MEDIA_TYPE
        || image_index.platform.is_some()
        || image_index.artifact_type.is_some()
        || image_index.annotations.as_ref() != Some(&outer_annotations(image, expected_repo_tag))
    {
        return Err(ProviderFailure::Archive);
    }
    let image_index_bytes = get_blob(archive, image_index)?;
    let image_index_json: OciIndex =
        serde_json::from_slice(image_index_bytes).map_err(|_| ProviderFailure::Archive)?;
    if image_index_json.schema_version != 2
        || image_index_json.media_type != OCI_INDEX_MEDIA_TYPE
        || image_index_json.manifests.len() != 2
    {
        return Err(ProviderFailure::Archive);
    }
    let (platform_manifest, attestation_manifest) = select_children(&image_index_json.manifests)?;
    let manifest_bytes = get_blob(archive, platform_manifest)?;
    let manifest: OciManifest =
        serde_json::from_slice(manifest_bytes).map_err(|_| ProviderFailure::Archive)?;
    let (config, diff_ids, config_digest) = verify_platform_image(archive, image, &manifest)?;
    verify_buildkit_attestation(
        archive,
        image,
        source_sha,
        platform_manifest,
        attestation_manifest,
    )?;
    let repo_tag = verify_legacy_manifest(
        archive
            .files
            .get("manifest.json")
            .ok_or(ProviderFailure::Archive)?,
        image,
        &config_digest,
        &manifest.layers,
        expected_repo_tag,
    )?;
    let expected_blobs = referenced_blobs(
        image_index,
        platform_manifest,
        attestation_manifest,
        &manifest,
        archive,
    )?;
    if expected_blobs.len() != archive.blobs.len()
        || archive
            .blobs
            .keys()
            .any(|path| !expected_blobs.contains(path))
    {
        return Err(ProviderFailure::Archive);
    }
    let blob_inventory = archive
        .blobs
        .iter()
        .map(|(path, blob)| (format!("{}:{}", path, blob.digest), blob.size))
        .collect();
    Ok(VerifiedSelection {
        image_index_digest: image_index.digest.clone(),
        image_index_size: image_index.size,
        manifest_digest: platform_manifest.digest.clone(),
        manifest_size: platform_manifest.size,
        config_digest,
        config,
        diff_ids,
        repo_tag,
        blob_inventory,
    })
}

fn verify_platform_image(
    archive: &ArchiveFiles,
    image: WorkerImage,
    manifest: &OciManifest,
) -> Result<(ContainerConfig, Vec<String>, String), ProviderFailure> {
    if manifest.schema_version != 2
        || manifest.media_type != OCI_MANIFEST_MEDIA_TYPE
        || manifest.artifact_type.is_some()
        || manifest.subject.is_some()
        || manifest.layers.is_empty()
        || manifest.layers.len() > 64
    {
        return Err(ProviderFailure::Archive);
    }
    let config_descriptor = &manifest.config;
    verify_descriptor(config_descriptor)?;
    if config_descriptor.media_type != OCI_CONFIG_MEDIA_TYPE
        || config_descriptor.artifact_type.is_some()
        || config_descriptor.annotations.is_some()
        || config_descriptor.platform.is_some()
    {
        return Err(ProviderFailure::Archive);
    }
    let config_bytes = get_blob(archive, config_descriptor)?;
    let image_config: ImageConfigFile =
        serde_json::from_slice(config_bytes).map_err(|_| ProviderFailure::Archive)?;
    if image_config.architecture != "amd64"
        || image_config.os != "linux"
        || image_config.variant.is_some()
        || image_config.rootfs.kind != "layers"
        || image_config.rootfs.diff_ids.len() != manifest.layers.len()
        || image_config
            .rootfs
            .diff_ids
            .iter()
            .any(|digest| !valid_sha256(digest))
    {
        return Err(ProviderFailure::Archive);
    }
    for layer in &manifest.layers {
        verify_descriptor(layer)?;
        if layer.media_type != OCI_GZIP_LAYER_MEDIA_TYPE
            || layer.artifact_type.is_some()
            || layer.annotations.is_some()
            || layer.platform.is_some()
        {
            return Err(ProviderFailure::Archive);
        }
        verify_blob(archive, layer)?;
    }
    verify_runtime_config(&image_config.config, image)?;
    let config = serde_json::from_value(image_config.config.clone())
        .map_err(|_| ProviderFailure::Archive)?;
    Ok((
        config,
        image_config.rootfs.diff_ids,
        config_descriptor.digest.clone(),
    ))
}

fn verify_runtime_config(value: &Value, image: WorkerImage) -> Result<(), ProviderFailure> {
    let object = value.as_object().ok_or(ProviderFailure::Archive)?;
    let expected_fields = match image {
        WorkerImage::Runner => ["User", "Env", "Entrypoint", "WorkingDir", "Labels"].as_slice(),
        WorkerImage::Dind => ["Env", "Entrypoint", "Labels"].as_slice(),
    };
    if object.len() != expected_fields.len()
        || object
            .keys()
            .any(|field| !expected_fields.contains(&field.as_str()))
        || value.get("User").and_then(Value::as_str) != image.user()
        || value.get("WorkingDir").and_then(Value::as_str) != image.working_dir()
        || value
            .get("Env")
            .and_then(Value::as_array)
            .is_none_or(|env| {
                env.len() != 1 || env.first().and_then(Value::as_str) != Some(PATH_ENV)
            })
        || value
            .get("Entrypoint")
            .and_then(Value::as_array)
            .is_none_or(|entrypoint| {
                entrypoint.len() != 1
                    || entrypoint.first().and_then(Value::as_str) != Some(image.entrypoint())
            })
    {
        return Err(ProviderFailure::Archive);
    }
    let labels = value
        .get("Labels")
        .and_then(Value::as_object)
        .ok_or(ProviderFailure::Archive)?;
    if labels.len() != BASE_LABELS.len()
        || labels
            .keys()
            .any(|label| !BASE_LABELS.contains(&label.as_str()))
        || labels
            .get("org.opencontainers.image.created")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || labels
            .get("org.opencontainers.image.description")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || labels
            .get("org.opencontainers.image.title")
            .and_then(Value::as_str)
            != Some("ubuntu")
        || labels
            .get("org.opencontainers.image.version")
            .and_then(Value::as_str)
            != Some("26.04")
    {
        return Err(ProviderFailure::Archive);
    }
    Ok(())
}

fn verify_buildkit_attestation(
    archive: &ArchiveFiles,
    image: WorkerImage,
    source_sha: &str,
    platform_manifest: &Descriptor,
    descriptor: &Descriptor,
) -> Result<(), ProviderFailure> {
    let expected_annotations = HashMap::from([
        (
            "vnd.docker.reference.digest".to_owned(),
            platform_manifest.digest.clone(),
        ),
        (
            "vnd.docker.reference.type".to_owned(),
            "attestation-manifest".to_owned(),
        ),
    ]);
    verify_descriptor(descriptor)?;
    if descriptor.media_type != OCI_MANIFEST_MEDIA_TYPE
        || descriptor.platform.as_ref()
            != Some(&Platform {
                architecture: "unknown".to_owned(),
                os: "unknown".to_owned(),
            })
        || descriptor.artifact_type.is_some()
        || descriptor.annotations.as_ref() != Some(&expected_annotations)
    {
        return Err(ProviderFailure::Archive);
    }
    let manifest_bytes = get_blob(archive, descriptor)?;
    let manifest: OciManifest =
        serde_json::from_slice(manifest_bytes).map_err(|_| ProviderFailure::Archive)?;
    if manifest.schema_version != 2
        || manifest.media_type != OCI_MANIFEST_MEDIA_TYPE
        || manifest.artifact_type.is_some()
        || manifest.subject.is_some()
        || manifest.layers.len() != 1
    {
        return Err(ProviderFailure::Archive);
    }
    let config = &manifest.config;
    verify_descriptor(config)?;
    if config.media_type != OCI_CONFIG_MEDIA_TYPE
        || config.artifact_type.is_some()
        || config.annotations.is_some()
        || config.platform.is_some()
    {
        return Err(ProviderFailure::Archive);
    }
    let layer = manifest.layers.first().ok_or(ProviderFailure::Archive)?;
    verify_descriptor(layer)?;
    if manifest.layers.len() != 1
        || layer.media_type != IN_TOTO_MEDIA_TYPE
        || layer.artifact_type.is_some()
        || layer.platform.is_some()
        || layer.annotations.as_ref().is_none_or(|annotations| {
            annotations.len() != 1
                || annotations
                    .get("in-toto.io/predicate-type")
                    .map(String::as_str)
                    != Some(SLSA_PREDICATE)
        })
    {
        return Err(ProviderFailure::Archive);
    }
    let config_bytes = get_blob(archive, config)?;
    let config_value: Value =
        serde_json::from_slice(config_bytes).map_err(|_| ProviderFailure::Archive)?;
    if !valid_attestation_config(&config_value, &layer.digest) {
        return Err(ProviderFailure::Archive);
    }
    let statement = get_blob(archive, layer)?;
    verify_buildkit_statement(statement, image, source_sha, platform_manifest)
}

fn verify_buildkit_statement(
    bytes: &[u8],
    image: WorkerImage,
    source_sha: &str,
    platform_manifest: &Descriptor,
) -> Result<(), ProviderFailure> {
    let statement: Value = serde_json::from_slice(bytes).map_err(|_| ProviderFailure::Archive)?;
    verify_build_source(&statement, image, source_sha, platform_manifest)
}

fn verify_build_source(
    statement: &Value,
    image: WorkerImage,
    source_sha: &str,
    platform_manifest: &Descriptor,
) -> Result<(), ProviderFailure> {
    let subject = statement
        .get("subject")
        .and_then(Value::as_array)
        .ok_or(ProviderFailure::Archive)?;
    let expected_subject = format!(
        "pkg:docker/{}@linux-amd64?platform=linux%2Famd64",
        image.image_name()
    );
    if subject.len() != 1
        || subject[0].get("name").and_then(Value::as_str) != Some(expected_subject.as_str())
        || subject[0].pointer("/digest/sha256").and_then(Value::as_str)
            != platform_manifest.digest.strip_prefix("sha256:")
    {
        return Err(ProviderFailure::Archive);
    }
    let vcs = statement
        .pointer("/predicate/runDetails/metadata/buildkit_metadata/vcs")
        .and_then(Value::as_object)
        .ok_or(ProviderFailure::Archive)?;
    let build = statement
        .pointer("/predicate/buildDefinition")
        .and_then(Value::as_object)
        .ok_or(ProviderFailure::Archive)?;
    let external = build
        .get("externalParameters")
        .and_then(Value::as_object)
        .ok_or(ProviderFailure::Archive)?;
    let config_source = external
        .get("configSource")
        .and_then(Value::as_object)
        .ok_or(ProviderFailure::Archive)?;
    let request = external
        .get("request")
        .and_then(Value::as_object)
        .ok_or(ProviderFailure::Archive)?;
    let locals = request
        .get("locals")
        .and_then(Value::as_array)
        .ok_or(ProviderFailure::Archive)?;
    let local_names = locals
        .iter()
        .map(|local| {
            let local = local.as_object().ok_or(ProviderFailure::Archive)?;
            if local.len() != 1 {
                return Err(ProviderFailure::Archive);
            }
            local
                .get("name")
                .and_then(Value::as_str)
                .ok_or(ProviderFailure::Archive)
        })
        .collect::<Result<HashSet<_>, _>>()?;
    let builder_platform = build
        .get("internalParameters")
        .and_then(Value::as_object)
        .and_then(|parameters| parameters.get("builderPlatform"))
        .and_then(Value::as_str);
    let context = vcs.get("localdir:context").and_then(Value::as_str);
    if statement.get("_type").and_then(Value::as_str) != Some("https://in-toto.io/Statement/v0.1")
        || statement.get("predicateType").and_then(Value::as_str) != Some(SLSA_PREDICATE)
        || build.get("buildType").and_then(Value::as_str) != Some(BUILD_KIT_TYPE)
        || config_source.get("path").and_then(Value::as_str) != Some("Dockerfile")
        || request.get("frontend").and_then(Value::as_str) != Some("dockerfile.v0")
        || locals.len() != 2
        || local_names != HashSet::from(["context", "dockerfile"])
        || external.len() != 2
        || config_source.len() != 1
        || request.len() != 2
        || build.len() != 4
        || builder_platform != Some(PLATFORM)
        || context != Some(image.source_context())
        || vcs.len() != 4
        || vcs.get("revision").and_then(Value::as_str) != Some(source_sha)
        || vcs.get("source").and_then(Value::as_str) != Some(SOURCE_REPOSITORY_URL)
        || vcs.get("localdir:context").and_then(Value::as_str)
            != vcs.get("localdir:dockerfile").and_then(Value::as_str)
    {
        return Err(ProviderFailure::Archive);
    }
    Ok(())
}

fn valid_attestation_config(value: &Value, layer_digest: &str) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let Some(rootfs) = value.get("rootfs").and_then(Value::as_object) else {
        return false;
    };
    let diff_ids = rootfs.get("diff_ids").and_then(Value::as_array);
    object.len() == 4
        && object
            .keys()
            .all(|key| ["architecture", "os", "config", "rootfs"].contains(&key.as_str()))
        && value.get("architecture").and_then(Value::as_str) == Some("unknown")
        && value.get("os").and_then(Value::as_str) == Some("unknown")
        && value
            .get("config")
            .and_then(Value::as_object)
            .is_some_and(|config| config.is_empty())
        && rootfs.len() == 2
        && rootfs.get("type").and_then(Value::as_str) == Some("layers")
        && diff_ids.is_some_and(|ids| ids.len() == 1 && ids[0].as_str() == Some(layer_digest))
}

fn verify_legacy_manifest(
    bytes: &[u8],
    image: WorkerImage,
    config_digest: &str,
    layers: &[Descriptor],
    expected_repo_tag: &str,
) -> Result<String, ProviderFailure> {
    let manifests: Vec<LegacyManifest> =
        serde_json::from_slice(bytes).map_err(|_| ProviderFailure::Archive)?;
    let manifest = manifests.first().ok_or(ProviderFailure::Archive)?;
    let expected_config = blob_path(config_digest)?;
    let expected_layers = layers
        .iter()
        .map(|layer| blob_path(&layer.digest))
        .collect::<Result<Vec<_>, _>>()?;
    if manifests.len() != 1
        || manifest.config != expected_config
        || manifest.repo_tags.len() != 1
        || manifest.repo_tags.first().map(String::as_str) != Some(expected_repo_tag)
        || expected_repo_tag.split(':').next() != Some(image.image_name())
        || manifest.layers != expected_layers
    {
        return Err(ProviderFailure::Archive);
    }
    Ok(expected_repo_tag.to_owned())
}

fn select_children(
    descriptors: &[Descriptor],
) -> Result<(&Descriptor, &Descriptor), ProviderFailure> {
    let mut platform = None;
    let mut attestation = None;
    for descriptor in descriptors {
        match descriptor.platform.as_ref() {
            Some(value) if value.os == "linux" && value.architecture == "amd64" => {
                if platform.replace(descriptor).is_some()
                    || descriptor.annotations.is_some()
                    || descriptor.artifact_type.is_some()
                    || descriptor.media_type != OCI_MANIFEST_MEDIA_TYPE
                {
                    return Err(ProviderFailure::Archive);
                }
            }
            Some(value) if value.os == "unknown" && value.architecture == "unknown" => {
                if attestation.replace(descriptor).is_some()
                    || descriptor.media_type != OCI_MANIFEST_MEDIA_TYPE
                {
                    return Err(ProviderFailure::Archive);
                }
            }
            _ => return Err(ProviderFailure::Archive),
        }
        verify_descriptor(descriptor)?;
    }
    Ok((
        platform.ok_or(ProviderFailure::Archive)?,
        attestation.ok_or(ProviderFailure::Archive)?,
    ))
}

fn referenced_blobs(
    image_index: &Descriptor,
    image_manifest: &Descriptor,
    attestation_manifest: &Descriptor,
    manifest: &OciManifest,
    archive: &ArchiveFiles,
) -> Result<HashSet<String>, ProviderFailure> {
    let mut expected = HashSet::new();
    for descriptor in [image_index, image_manifest, attestation_manifest] {
        expected.insert(blob_path(&descriptor.digest)?);
    }
    for descriptor in [&manifest.config].into_iter().chain(manifest.layers.iter()) {
        expected.insert(blob_path(&descriptor.digest)?);
    }
    let attestation = get_manifest(archive, attestation_manifest)?;
    expected.insert(blob_path(&attestation.config.digest)?);
    for descriptor in &attestation.layers {
        expected.insert(blob_path(&descriptor.digest)?);
    }
    Ok(expected)
}

fn get_manifest<'a>(
    archive: &'a ArchiveFiles,
    descriptor: &Descriptor,
) -> Result<OciManifest, ProviderFailure> {
    let bytes = get_blob(archive, descriptor)?;
    serde_json::from_slice(bytes).map_err(|_| ProviderFailure::Archive)
}

fn get_blob<'a>(
    archive: &'a ArchiveFiles,
    descriptor: &Descriptor,
) -> Result<&'a [u8], ProviderFailure> {
    let path = verify_blob(archive, descriptor)?;
    let blob = archive.blobs.get(&path).ok_or(ProviderFailure::Archive)?;
    blob.metadata.as_deref().ok_or(ProviderFailure::Archive)
}

fn verify_blob(archive: &ArchiveFiles, descriptor: &Descriptor) -> Result<String, ProviderFailure> {
    verify_descriptor(descriptor)?;
    let path = blob_path(&descriptor.digest)?;
    let blob = archive.blobs.get(&path).ok_or(ProviderFailure::Archive)?;
    if blob.digest
        != descriptor
            .digest
            .strip_prefix("sha256:")
            .unwrap_or_default()
        || blob.size != descriptor.size
    {
        return Err(ProviderFailure::Archive);
    }
    Ok(path)
}

fn verify_descriptor(descriptor: &Descriptor) -> Result<(), ProviderFailure> {
    let digest = descriptor
        .digest
        .strip_prefix("sha256:")
        .filter(|digest| valid_hex(digest, 64))
        .ok_or(ProviderFailure::Archive)?;
    if descriptor.size == 0 || digest.is_empty() {
        Err(ProviderFailure::Archive)
    } else {
        Ok(())
    }
}

fn verify_layout(bytes: &[u8]) -> Result<(), ProviderFailure> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| ProviderFailure::Archive)?;
    value
        .as_object()
        .is_some_and(|object| {
            object.len() == 1
                && object.get("imageLayoutVersion").and_then(Value::as_str) == Some("1.0.0")
        })
        .then_some(())
        .ok_or(ProviderFailure::Archive)
}

fn outer_annotations(image: WorkerImage, tag: &str) -> HashMap<String, String> {
    let Some((name, tag)) = tag.split_once(':') else {
        return HashMap::new();
    };
    if name != image.image_name() || tag.is_empty() {
        return HashMap::new();
    }
    HashMap::from([
        (
            "io.containerd.image.name".to_owned(),
            format!("{}:{tag}", image.docker_image_name()),
        ),
        (
            "org.opencontainers.image.ref.name".to_owned(),
            tag.to_owned(),
        ),
    ])
}

fn blob_path(digest: &str) -> Result<String, ProviderFailure> {
    let digest = digest
        .strip_prefix("sha256:")
        .filter(|value| valid_hex(value, 64))
        .ok_or(ProviderFailure::Archive)?;
    Ok(format!("blobs/sha256/{digest}"))
}

fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && !path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
}

fn is_fixed_file(path: &str) -> bool {
    ["oci-layout", "index.json", "manifest.json"].contains(&path)
}

fn read_bounded_entry<R: Read>(
    reader: &mut R,
    size: u64,
    limit: u64,
) -> Result<Vec<u8>, ProviderFailure> {
    if size > limit || size > MAX_ARCHIVE_METADATA_BYTES {
        return Err(ProviderFailure::Archive);
    }
    let capacity = usize::try_from(size).map_err(|_| ProviderFailure::Archive)?;
    let mut bytes = Vec::with_capacity(capacity);
    let mut bounded = reader.take(size);
    bounded
        .read_to_end(&mut bytes)
        .map_err(|_| ProviderFailure::Archive)?;
    if u64::try_from(bytes.len()).map_err(|_| ProviderFailure::Archive)? != size {
        return Err(ProviderFailure::Archive);
    }
    Ok(bytes)
}

fn hash_entry<R: Read>(
    reader: &mut R,
    size: u64,
    metadata_limit: Option<u64>,
) -> Result<(String, Option<Vec<u8>>), ProviderFailure> {
    let mut hasher = Sha256::new();
    let mut metadata = metadata_limit
        .map(|limit| usize::try_from(limit).map(Vec::with_capacity))
        .transpose()
        .map_err(|_| ProviderFailure::Archive)?;
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    let mut bounded = reader.take(size);
    loop {
        let read = bounded
            .read(&mut buffer)
            .map_err(|_| ProviderFailure::Archive)?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(read).map_err(|_| ProviderFailure::Archive)?)
            .ok_or(ProviderFailure::Archive)?;
        if total > size {
            return Err(ProviderFailure::Archive);
        }
        hasher.update(&buffer[..read]);
        if let Some(metadata) = &mut metadata {
            metadata.extend_from_slice(&buffer[..read]);
        }
    }
    if total != size {
        return Err(ProviderFailure::Archive);
    }
    Ok((hex(&hasher.finalize()), metadata))
}

fn valid_sha256(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|digest| valid_hex(digest, 64))
}

fn valid_docker_reference(image: WorkerImage, reference: &str) -> bool {
    let Some((name, tag)) = reference.split_once(':') else {
        return false;
    };
    name == image.image_name()
        && !tag.is_empty()
        && tag.len() <= 128
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
        && tag.as_bytes()[0].is_ascii_alphanumeric()
}

fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn write_derived_archive(
    source_path: &Path,
    output_path: &Path,
    image: WorkerImage,
    repo_tag: &str,
    ref_name: &str,
) -> Result<(), ProviderFailure> {
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(output_path)
        .map_err(|_| ProviderFailure::Archive)?;
    let result = copy_archive_with_owned_reference(source_path, output, image, repo_tag, ref_name);
    result
}

fn copy_archive_with_owned_reference(
    source_path: &Path,
    output: File,
    image: WorkerImage,
    repo_tag: &str,
    ref_name: &str,
) -> Result<(), ProviderFailure> {
    let source = File::open(source_path).map_err(|_| ProviderFailure::Archive)?;
    let mut archive = Archive::new(source);
    let mut builder = Builder::new(output);
    for entry in archive.entries().map_err(|_| ProviderFailure::Archive)? {
        let mut entry = entry.map_err(|_| ProviderFailure::Archive)?;
        let raw_path = entry
            .path()
            .map_err(|_| ProviderFailure::Archive)?
            .to_str()
            .ok_or(ProviderFailure::Archive)?
            .to_owned();
        let kind = entry.header().entry_type();
        let path = if kind == EntryType::Directory {
            raw_path
                .strip_suffix('/')
                .ok_or(ProviderFailure::Archive)?
                .to_owned()
        } else {
            raw_path
        };
        if !safe_path(&path) {
            return Err(ProviderFailure::Archive);
        }
        match kind {
            EntryType::Directory => {
                let header = entry.header().clone();
                builder
                    .append(&header, std::io::empty())
                    .map_err(|_| ProviderFailure::Archive)?;
            }
            EntryType::Regular if path == "index.json" => {
                let size = entry.size();
                let bytes = read_bounded_entry(&mut entry, size, 64 * 1024)?;
                let value = rewrite_outer_index(&bytes, image, repo_tag, ref_name)?;
                append_rewritten(&mut builder, entry.header(), &value)?;
            }
            EntryType::Regular if path == "manifest.json" => {
                let size = entry.size();
                let bytes = read_bounded_entry(&mut entry, size, 256 * 1024)?;
                let value = rewrite_legacy_manifest(&bytes, repo_tag)?;
                append_rewritten(&mut builder, entry.header(), &value)?;
            }
            EntryType::Regular => {
                let header = entry.header().clone();
                builder
                    .append(&header, &mut entry)
                    .map_err(|_| ProviderFailure::Archive)?;
            }
            _ => return Err(ProviderFailure::Archive),
        }
    }
    let mut output = builder.into_inner().map_err(|_| ProviderFailure::Archive)?;
    output.flush().map_err(|_| ProviderFailure::Archive)?;
    output.sync_all().map_err(|_| ProviderFailure::Archive)
}

fn rewrite_outer_index(
    bytes: &[u8],
    image: WorkerImage,
    repo_tag: &str,
    ref_name: &str,
) -> Result<Vec<u8>, ProviderFailure> {
    let mut value: Value = serde_json::from_slice(bytes).map_err(|_| ProviderFailure::Archive)?;
    let descriptor = value
        .pointer_mut("/manifests/0")
        .and_then(Value::as_object_mut)
        .ok_or(ProviderFailure::Archive)?;
    if descriptor
        .get("annotations")
        .and_then(Value::as_object)
        .is_none()
    {
        return Err(ProviderFailure::Archive);
    }
    let annotations = descriptor
        .get_mut("annotations")
        .and_then(Value::as_object_mut)
        .ok_or(ProviderFailure::Archive)?;
    let expected_tag = image.release_tag();
    let expected_ref_name = expected_tag
        .split_once(':')
        .map(|(_, tag)| tag)
        .ok_or(ProviderFailure::Archive)?;
    if annotations
        .get("io.containerd.image.name")
        .and_then(Value::as_str)
        != Some(format!("{}:{expected_ref_name}", image.docker_image_name()).as_str())
        || annotations
            .get("org.opencontainers.image.ref.name")
            .and_then(Value::as_str)
            != Some(expected_ref_name)
    {
        return Err(ProviderFailure::Archive);
    }
    let (name, tag) = repo_tag.split_once(':').ok_or(ProviderFailure::Archive)?;
    if name != image.image_name() || tag != ref_name || !valid_docker_reference(image, repo_tag) {
        return Err(ProviderFailure::Archive);
    }
    annotations.insert(
        "io.containerd.image.name".to_owned(),
        Value::String(format!("{}:{tag}", image.docker_image_name())),
    );
    annotations.insert(
        "org.opencontainers.image.ref.name".to_owned(),
        Value::String(ref_name.to_owned()),
    );
    serde_json::to_vec(&value).map_err(|_| ProviderFailure::Archive)
}

fn rewrite_legacy_manifest(bytes: &[u8], repo_tag: &str) -> Result<Vec<u8>, ProviderFailure> {
    let mut value: Value = serde_json::from_slice(bytes).map_err(|_| ProviderFailure::Archive)?;
    let entries = value.as_array_mut().ok_or(ProviderFailure::Archive)?;
    if entries.len() != 1 {
        return Err(ProviderFailure::Archive);
    }
    let manifest = entries[0].as_object_mut().ok_or(ProviderFailure::Archive)?;
    manifest.insert(
        "RepoTags".to_owned(),
        Value::Array(vec![Value::String(repo_tag.to_owned())]),
    );
    serde_json::to_vec(&value).map_err(|_| ProviderFailure::Archive)
}

fn append_rewritten<W: Write>(
    builder: &mut Builder<W>,
    original: &Header,
    bytes: &[u8],
) -> Result<(), ProviderFailure> {
    let size = u64::try_from(bytes.len()).map_err(|_| ProviderFailure::Archive)?;
    let mut header = original.clone();
    header.set_size(size);
    header.set_cksum();
    builder
        .append(&header, bytes)
        .map_err(|_| ProviderFailure::Archive)
}

#[cfg(test)]
fn new_header(kind: EntryType, size: u64) -> Result<Header, ProviderFailure> {
    let mut header = Header::new_gnu();
    header.set_entry_type(kind);
    header.set_size(size);
    header.set_mode(if kind == EntryType::Directory {
        0o755
    } else {
        0o444
    });
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();
    Ok(header)
}

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
