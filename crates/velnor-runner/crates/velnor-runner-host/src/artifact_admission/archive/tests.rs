use std::collections::BTreeMap;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::super::hash::lowercase_hex;
use super::super::manifest::{
    ARCHIVE_FORMAT, DEFAULT_ENV, IMAGE_ENTRYPOINT, IMAGE_TAG, IMAGE_USER, IMAGE_WORKDIR, PLATFORM,
    ProbeManifest, SOURCE_LABEL,
};
use super::validate;
use crate::error::HostError;

fn fixture(
    platform: Option<Value>,
    unreferenced_blob: bool,
) -> Result<(Vec<u8>, ProbeManifest), HostError> {
    let mut blobs = BTreeMap::new();
    let config_digest = config_blob(&mut blobs)?;
    let layer_bytes = b"synthetic layer payload".to_vec();
    let layer_digest = add_blob(&mut blobs, layer_bytes);
    let manifest_value = json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
        "config": {
            "mediaType": "application/vnd.docker.container.image.v1+json",
            "digest": config_digest,
            "size": u64::try_from(blobs[&blob_name(&config_digest)].len())
                .map_err(|_| HostError::Frame)?,
        },
        "layers": [{
            "mediaType": "application/vnd.docker.image.rootfs.diff.tar.gzip",
            "digest": layer_digest,
            "size": u64::try_from(blobs[&blob_name(&layer_digest)].len())
                .map_err(|_| HostError::Frame)?,
        }],
    });
    let image_manifest_bytes =
        serde_json::to_vec(&manifest_value).map_err(|_| HostError::Identity)?;
    let image_manifest_digest = add_blob(&mut blobs, image_manifest_bytes.clone());
    let image_manifest_size =
        u64::try_from(image_manifest_bytes.len()).map_err(|_| HostError::Frame)?;
    let compatibility_manifest = serde_json::to_vec(&json!([{
        "Config": blob_name(&config_digest),
        "RepoTags": [IMAGE_TAG],
        "Layers": [blob_name(&layer_digest)],
    }]))
    .map_err(|_| HostError::Identity)?;
    let mut descriptor = json!({
        "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
        "digest": image_manifest_digest,
        "size": image_manifest_size,
    });
    if let Some(platform) = platform {
        descriptor["platform"] = platform;
    }
    let index_bytes = serde_json::to_vec(&json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "manifests": [descriptor],
    }))
    .map_err(|_| HostError::Identity)?;
    let oci_layout = serde_json::to_vec(&json!({"imageLayoutVersion": "1.0.0"}))
        .map_err(|_| HostError::Identity)?;
    if unreferenced_blob {
        add_blob(&mut blobs, b"unreferenced".to_vec());
    }
    let mut files = BTreeMap::from([
        ("oci-layout".to_owned(), oci_layout),
        ("index.json".to_owned(), index_bytes.clone()),
        ("manifest.json".to_owned(), compatibility_manifest),
    ]);
    files.extend(blobs);
    let archive = tar_bytes(&files)?;
    let manifest = ProbeManifest {
        source_commit: "a".repeat(40),
        workflow_authority_sha: "a".repeat(40),
        platform: PLATFORM.to_owned(),
        archive_format: ARCHIVE_FORMAT.to_owned(),
        oci_index_sha256: lowercase_hex(&Sha256::digest(index_bytes)),
        image_manifest_digest,
        config_digest,
        archive_sha256: lowercase_hex(&Sha256::digest(&archive)),
    };
    Ok((archive, manifest))
}

fn config_blob(blobs: &mut BTreeMap<String, Vec<u8>>) -> Result<String, HostError> {
    let bytes = serde_json::to_vec(&json!({
        "architecture": "amd64",
        "config": {
            "User": IMAGE_USER,
            "Env": [DEFAULT_ENV],
            "Entrypoint": [IMAGE_ENTRYPOINT],
            "WorkingDir": IMAGE_WORKDIR,
            "Labels": {SOURCE_LABEL: "a".repeat(40)},
        },
        "os": "linux",
        "rootfs": {"type": "layers", "diff_ids": []},
    }))
    .map_err(|_| HostError::Identity)?;
    Ok(add_blob(blobs, bytes))
}

fn add_blob(blobs: &mut BTreeMap<String, Vec<u8>>, bytes: Vec<u8>) -> String {
    let digest = lowercase_hex(&Sha256::digest(&bytes));
    blobs.insert(format!("blobs/sha256/{digest}"), bytes);
    format!("sha256:{digest}")
}

fn blob_name(digest: &str) -> String {
    format!("blobs/sha256/{}", digest.trim_start_matches("sha256:"))
}

fn tar_bytes(files: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>, HostError> {
    let mut builder = tar::Builder::new(Vec::new());
    for directory in ["blobs", "blobs/sha256"] {
        append_tar_entry(&mut builder, directory, &[], true)?;
    }
    for (name, bytes) in files {
        append_tar_entry(&mut builder, name, bytes, false)?;
    }
    builder.finish().map_err(|_| HostError::Identity)?;
    builder.into_inner().map_err(|_| HostError::Identity)
}

fn append_tar_entry(
    builder: &mut tar::Builder<Vec<u8>>,
    name: &str,
    bytes: &[u8],
    directory: bool,
) -> Result<(), HostError> {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(if directory {
        tar::EntryType::Directory
    } else {
        tar::EntryType::Regular
    });
    header.set_mode(if directory { 0o755 } else { 0o644 });
    header.set_size(u64::try_from(bytes.len()).map_err(|_| HostError::Frame)?);
    header.set_cksum();
    builder
        .append_data(&mut header, name, bytes)
        .map_err(|_| HostError::Identity)
}

#[test]
fn accepts_one_source_bound_linux_amd64_image() -> Result<(), HostError> {
    let (archive, manifest) = fixture(None, false)?;
    let identity = validate(&archive, &manifest)?;
    if identity.image_manifest_digest != manifest.image_manifest_digest
        || identity.config_digest != manifest.config_digest
        || identity.config.user.as_deref() != Some(IMAGE_USER)
        || identity.config.working_dir.as_deref() != Some(IMAGE_WORKDIR)
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

#[test]
fn rejects_wrong_platform_and_unreferenced_blob() -> Result<(), HostError> {
    let (wrong_platform, manifest) =
        fixture(Some(json!({"os": "linux", "architecture": "arm64"})), false)?;
    assert_eq!(
        validate(&wrong_platform, &manifest),
        Err(HostError::Identity)
    );
    let (extra_blob, manifest) = fixture(None, true)?;
    assert_eq!(validate(&extra_blob, &manifest), Err(HostError::Identity));
    Ok(())
}
