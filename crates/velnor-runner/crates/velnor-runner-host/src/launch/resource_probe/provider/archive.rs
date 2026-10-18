//! Bounded OCI/Docker archive parser for the verified probe artifact.

use std::collections::{HashMap, HashSet};
use std::io::Read;

use bollard::models::ContainerConfig;
use flate2::read::GzDecoder;
use serde_json::Value;
use sha2::{Digest, Sha256};

mod load_archive;
mod types;
mod unpack;

use super::release::ResourceManifest;
use super::{IMAGE_TAG, ProviderFailure, hex, valid_hex, valid_sha};
use load_archive::derive as derive_load_archive;
use types::{Descriptor, ImageConfigFile, LegacyManifest, OciIndex, OciManifest, Platform, RootFs};
use unpack::unpack;

const MAX_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;
const MAX_UNPACKED_LAYER_BYTES: u64 = 128 * 1024 * 1024;
const OCI_INDEX_MEDIA_TYPE: &str = "application/vnd.oci.image.index.v1+json";
const OCI_MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";
const OCI_CONFIG_MEDIA_TYPE: &str = "application/vnd.oci.image.config.v1+json";
const OCI_GZIP_LAYER_MEDIA_TYPE: &str = "application/vnd.oci.image.layer.v1.tar+gzip";
const IMAGE_REVISION_LABEL: &str = "org.opencontainers.image.revision";
const IMAGE_PATH: &str = "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

pub(super) struct ParsedArchive {
    pub(super) archive_sha256: String,
    pub(super) archive_index_digest: String,
    pub(super) manifest_digest: String,
    pub(super) manifest_size: u64,
    pub(super) config_digest: String,
    pub(super) config: ContainerConfig,
    pub(super) load_archive: Vec<u8>,
    pub(super) load_archive_sha256: String,
    pub(super) load_archive_index_digest: String,
    pub(super) image_reference: String,
}

struct VerifiedOciImage {
    manifest_digest: String,
    manifest_size: u64,
    config_digest: String,
    config: ContainerConfig,
}

pub(super) fn verify(
    bytes: &[u8],
    manifest: &ResourceManifest,
    source_revision: &str,
) -> Result<ParsedArchive, ProviderFailure> {
    if !valid_sha(source_revision)
        || bytes.len() > MAX_ARCHIVE_BYTES
        || manifest.archive_name != "velnor-resource-probe-linux-amd64.tar"
        || sha256(bytes) != manifest.archive_sha256
    {
        return Err(ProviderFailure::Archive);
    }
    let files = unpack(bytes)?;
    let layout = files.get("oci-layout").ok_or(ProviderFailure::Archive)?;
    verify_layout(layout)?;
    let (index_digest, index) = verify_index(&files, manifest)?;
    let descriptor = index.manifests.first().ok_or(ProviderFailure::Archive)?;
    verify_index_descriptor(descriptor)?;
    let image = verify_oci_image(&files, descriptor, manifest, source_revision)?;
    let load_archive = derive_load_archive(bytes, source_revision, &manifest.archive_sha256)?;
    Ok(ParsedArchive {
        archive_sha256: manifest.archive_sha256.clone(),
        archive_index_digest: index_digest,
        manifest_digest: image.manifest_digest,
        manifest_size: image.manifest_size,
        config_digest: image.config_digest,
        config: image.config,
        load_archive: load_archive.bytes,
        load_archive_sha256: load_archive.archive_sha256,
        load_archive_index_digest: load_archive.index_digest,
        image_reference: load_archive.image_reference,
    })
}

fn verify_index(
    files: &HashMap<String, Vec<u8>>,
    manifest: &ResourceManifest,
) -> Result<(String, OciIndex), ProviderFailure> {
    let index_bytes = files.get("index.json").ok_or(ProviderFailure::Archive)?;
    let index_digest = sha256(index_bytes);
    if index_digest != manifest.oci_index_sha256 {
        return Err(ProviderFailure::Archive);
    }
    let index: OciIndex =
        serde_json::from_slice(index_bytes).map_err(|_| ProviderFailure::Archive)?;
    if index.schema_version != 2
        || index.media_type != OCI_INDEX_MEDIA_TYPE
        || index.manifests.len() != 1
    {
        return Err(ProviderFailure::Archive);
    }
    Ok((index_digest, index))
}

fn verify_index_descriptor(descriptor: &Descriptor) -> Result<(), ProviderFailure> {
    let annotations = descriptor
        .annotations
        .as_ref()
        .ok_or(ProviderFailure::Archive)?;
    if descriptor.media_type != OCI_MANIFEST_MEDIA_TYPE
        || descriptor.platform.as_ref()
            != Some(&Platform {
                architecture: "amd64".to_owned(),
                os: "linux".to_owned(),
            })
        || descriptor.artifact_type.is_some()
        || annotations.len() != 2
        || annotations
            .get("io.containerd.image.name")
            .map(String::as_str)
            != Some("docker.io/library/velnor-resource-probe:linux-amd64")
        || annotations
            .get("org.opencontainers.image.ref.name")
            .map(String::as_str)
            != Some("linux-amd64")
    {
        return Err(ProviderFailure::Archive);
    }
    verify_digest(&descriptor.digest)?;
    Ok(())
}

fn verify_oci_image(
    files: &HashMap<String, Vec<u8>>,
    descriptor: &Descriptor,
    manifest: &ResourceManifest,
    source_revision: &str,
) -> Result<VerifiedOciImage, ProviderFailure> {
    if descriptor.digest != manifest.image_manifest_digest {
        return Err(ProviderFailure::Archive);
    }
    let image_manifest_bytes = blob(files, &descriptor.digest, descriptor.size)?;
    let image_manifest_digest = sha256(image_manifest_bytes);
    if format!("sha256:{image_manifest_digest}") != descriptor.digest {
        return Err(ProviderFailure::Archive);
    }
    let image_manifest: OciManifest =
        serde_json::from_slice(image_manifest_bytes).map_err(|_| ProviderFailure::Archive)?;
    if image_manifest.schema_version != 2
        || image_manifest.media_type != OCI_MANIFEST_MEDIA_TYPE
        || image_manifest.layers.len() != 1
        || image_manifest.artifact_type.is_some()
        || image_manifest.subject.is_some()
    {
        return Err(ProviderFailure::Archive);
    }
    let config = &image_manifest.config;
    let runtime = verify_image_config(files, config, manifest, source_revision)?;
    let layer = image_manifest
        .layers
        .first()
        .ok_or(ProviderFailure::Archive)?;
    verify_layer(files, layer, &runtime.rootfs.diff_ids)?;
    verify_blob_inventory(files, descriptor, config, layer)?;
    verify_legacy_manifest(
        files.get("manifest.json").ok_or(ProviderFailure::Archive)?,
        &config.digest,
        &layer.digest,
    )?;
    Ok(VerifiedOciImage {
        manifest_digest: descriptor.digest.clone(),
        manifest_size: descriptor.size,
        config_digest: config.digest.clone(),
        config: runtime.config,
    })
}

struct VerifiedRuntimeConfig {
    config: ContainerConfig,
    rootfs: RootFs,
}

fn verify_image_config(
    files: &HashMap<String, Vec<u8>>,
    descriptor: &Descriptor,
    manifest: &ResourceManifest,
    source_revision: &str,
) -> Result<VerifiedRuntimeConfig, ProviderFailure> {
    verify_digest(&descriptor.digest)?;
    if descriptor.media_type != OCI_CONFIG_MEDIA_TYPE
        || descriptor.digest != manifest.config_digest
        || descriptor.artifact_type.is_some()
        || descriptor.platform.is_some()
        || descriptor.annotations.is_some()
    {
        return Err(ProviderFailure::Archive);
    }
    let config_bytes = blob(files, &descriptor.digest, descriptor.size)?;
    if format!("sha256:{}", sha256(config_bytes)) != descriptor.digest {
        return Err(ProviderFailure::Archive);
    }
    let image_config: ImageConfigFile =
        serde_json::from_slice(config_bytes).map_err(|_| ProviderFailure::Archive)?;
    let runtime_config = verify_config(&image_config, source_revision)?;
    Ok(VerifiedRuntimeConfig {
        config: runtime_config,
        rootfs: image_config.rootfs,
    })
}

fn verify_layer(
    files: &HashMap<String, Vec<u8>>,
    layer: &Descriptor,
    diff_ids: &[String],
) -> Result<(), ProviderFailure> {
    verify_digest(&layer.digest)?;
    if layer.media_type != OCI_GZIP_LAYER_MEDIA_TYPE
        || layer.artifact_type.is_some()
        || layer.platform.is_some()
        || layer.annotations.is_some()
        || diff_ids.len() != 1
    {
        return Err(ProviderFailure::Archive);
    }
    let layer_bytes = blob(files, &layer.digest, layer.size)?;
    if format!("sha256:{}", sha256(layer_bytes)) != layer.digest {
        return Err(ProviderFailure::Archive);
    }
    verify_diff_id(layer_bytes, &diff_ids[0])
}

fn verify_blob_inventory(
    files: &HashMap<String, Vec<u8>>,
    index: &Descriptor,
    config: &Descriptor,
    layer: &Descriptor,
) -> Result<(), ProviderFailure> {
    let expected_blobs = HashSet::from([
        blob_path(&index.digest)?,
        blob_path(&config.digest)?,
        blob_path(&layer.digest)?,
    ]);
    if files
        .keys()
        .filter(|path| path.starts_with("blobs/sha256/"))
        .any(|path| !expected_blobs.contains(path))
        || expected_blobs.iter().any(|path| !files.contains_key(path))
        || expected_blobs.len() != 3
    {
        return Err(ProviderFailure::Archive);
    }
    Ok(())
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

fn verify_legacy_manifest(
    bytes: &[u8],
    config_digest: &str,
    layer_digest: &str,
) -> Result<(), ProviderFailure> {
    let entries: Vec<LegacyManifest> =
        serde_json::from_slice(bytes).map_err(|_| ProviderFailure::Archive)?;
    if entries.len() != 1
        || entries[0].config != blob_path(config_digest)?
        || entries[0].repo_tags != [IMAGE_TAG]
        || entries[0].layers != [blob_path(layer_digest)?]
    {
        return Err(ProviderFailure::Archive);
    }
    Ok(())
}

fn verify_config(
    image: &ImageConfigFile,
    source_revision: &str,
) -> Result<ContainerConfig, ProviderFailure> {
    if image.architecture != "amd64"
        || image.os != "linux"
        || image.variant.is_some()
        || image.rootfs.kind != "layers"
        || image.rootfs.diff_ids.len() != 1
        || !image.rootfs.diff_ids[0].starts_with("sha256:")
    {
        return Err(ProviderFailure::Archive);
    }
    let expected_labels =
        HashMap::from([(IMAGE_REVISION_LABEL.to_owned(), source_revision.to_owned())]);
    let expected_runtime = ContainerConfig {
        user: Some("65532:65532".to_owned()),
        env: Some(vec![IMAGE_PATH.to_owned()]),
        working_dir: Some("/".to_owned()),
        entrypoint: Some(vec!["/velnor/resource-probe".to_owned()]),
        labels: Some(expected_labels),
        ..Default::default()
    };
    let actual = serde_json::to_value(&image.config).map_err(|_| ProviderFailure::Archive)?;
    let expected = serde_json::to_value(&expected_runtime).map_err(|_| ProviderFailure::Archive)?;
    if actual != expected {
        return Err(ProviderFailure::Archive);
    }
    serde_json::from_value(actual).map_err(|_| ProviderFailure::Archive)
}

fn verify_diff_id(layer: &[u8], expected: &str) -> Result<(), ProviderFailure> {
    let mut decoder = GzDecoder::new(layer);
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 8192];
    loop {
        let read = decoder
            .read(&mut buffer)
            .map_err(|_| ProviderFailure::Archive)?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or(ProviderFailure::Archive)?;
        if total > MAX_UNPACKED_LAYER_BYTES {
            return Err(ProviderFailure::Archive);
        }
        hasher.update(&buffer[..read]);
    }
    if format!("sha256:{}", hex(&hasher.finalize())) == expected {
        Ok(())
    } else {
        Err(ProviderFailure::Archive)
    }
}

fn blob<'a>(
    files: &'a HashMap<String, Vec<u8>>,
    digest: &str,
    size: u64,
) -> Result<&'a [u8], ProviderFailure> {
    let path = blob_path(digest)?;
    let bytes = files.get(&path).ok_or(ProviderFailure::Archive)?;
    if bytes.len() as u64 == size {
        Ok(bytes)
    } else {
        Err(ProviderFailure::Archive)
    }
}

fn blob_path(digest: &str) -> Result<String, ProviderFailure> {
    let hash = digest
        .strip_prefix("sha256:")
        .filter(|value| valid_hex(value, 64))
        .ok_or(ProviderFailure::Archive)?;
    Ok(format!("blobs/sha256/{hash}"))
}

fn verify_digest(digest: &str) -> Result<(), ProviderFailure> {
    blob_path(digest).map(|_| ())
}

fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}
