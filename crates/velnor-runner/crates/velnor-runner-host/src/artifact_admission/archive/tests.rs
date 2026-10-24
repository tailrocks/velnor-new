use std::collections::BTreeMap;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::super::docker::validate_inspect;
use super::super::hash::lowercase_hex;
use super::super::manifest::{
    ARCHIVE_FORMAT, DEFAULT_ENV, IMAGE_ENTRYPOINT, IMAGE_TAG, IMAGE_USER, IMAGE_WORKDIR, PLATFORM,
    ProbeManifest, SOURCE_LABEL,
};
use super::validate;
use crate::error::HostError;

#[path = "tests/edge_cases.rs"]
mod edge_cases;

#[path = "release_tests.rs"]
mod release_tests;

fn fixture(
    platform: Option<Value>,
    unreferenced_blob: bool,
) -> Result<(Vec<u8>, ProbeManifest), HostError> {
    fixture_with_working_dir(platform, unreferenced_blob, Some(IMAGE_WORKDIR))
}

fn fixture_with_working_dir(
    platform: Option<Value>,
    unreferenced_blob: bool,
    working_dir: Option<&str>,
) -> Result<(Vec<u8>, ProbeManifest), HostError> {
    let mut blobs = BTreeMap::new();
    let config_digest = config_blob(&mut blobs, working_dir)?;
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

fn config_blob(
    blobs: &mut BTreeMap<String, Vec<u8>>,
    working_dir: Option<&str>,
) -> Result<String, HostError> {
    let mut config = json!({
        "User": IMAGE_USER,
        "Env": [DEFAULT_ENV],
        "Entrypoint": [IMAGE_ENTRYPOINT],
        "Labels": {SOURCE_LABEL: "a".repeat(40)},
    });
    if let Some(working_dir) = working_dir {
        config["WorkingDir"] = json!(working_dir);
    }
    let bytes = serde_json::to_vec(&json!({
        "architecture": "amd64",
        "config": config,
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
    tar_bytes_with_duplicate(files, None)
}

fn tar_bytes_with_duplicate(
    files: &BTreeMap<String, Vec<u8>>,
    duplicate_path: Option<&str>,
) -> Result<Vec<u8>, HostError> {
    let mut builder = tar::Builder::new(Vec::new());
    for directory in ["blobs", "blobs/sha256"] {
        append_tar_entry(&mut builder, directory, &[], true)?;
    }
    for (name, bytes) in files {
        append_tar_entry(&mut builder, name, bytes, false)?;
        if duplicate_path == Some(name.as_str()) {
            append_tar_entry(&mut builder, name, bytes, false)?;
        }
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

fn validate_loaded_image(
    identity: &super::ArchiveIdentity,
    manifest: &ProbeManifest,
    image_id: &str,
    descriptor: Option<Value>,
    working_dir: &str,
) -> Result<(), HostError> {
    let mut inspect = json!({
        "Id": image_id,
        "Os": "linux",
        "Architecture": "amd64",
        "Config": {
            "User": IMAGE_USER,
            "Env": [DEFAULT_ENV],
            "Entrypoint": [IMAGE_ENTRYPOINT],
            "WorkingDir": working_dir,
            "Labels": {SOURCE_LABEL: manifest.source_commit},
        },
    });
    if let Some(descriptor) = descriptor {
        inspect["Descriptor"] = descriptor;
    }
    let inspect = serde_json::from_value(inspect).map_err(|_| HostError::Identity)?;
    validate_inspect(inspect, manifest, identity).map(|_| ())
}

fn refresh_archive(
    files: &BTreeMap<String, Vec<u8>>,
    manifest: &mut ProbeManifest,
    duplicate_path: Option<&str>,
) -> Result<Vec<u8>, HostError> {
    let index = files.get("index.json").ok_or(HostError::Identity)?;
    manifest.oci_index_sha256 = lowercase_hex(&Sha256::digest(index));
    let archive = tar_bytes_with_duplicate(files, duplicate_path)?;
    manifest.archive_sha256 = lowercase_hex(&Sha256::digest(&archive));
    Ok(archive)
}

fn fixture_with_referrer(
    bind_to_selected_image: bool,
) -> Result<(Vec<u8>, ProbeManifest), HostError> {
    let (archive, mut manifest) = fixture(None, false)?;
    let mut files = super::tar_archive::read_archive(&archive)?;
    let mut index: Value =
        serde_json::from_slice(files.get("index.json").ok_or(HostError::Identity)?)
            .map_err(|_| HostError::Identity)?;
    let config_bytes = b"{}".to_vec();
    let config_digest = add_blob(&mut files, config_bytes.clone());
    let statement = serde_json::to_vec(&json!({
        "subject": [{"digest": {"sha256": manifest.image_manifest_digest[7..]}}],
    }))
    .map_err(|_| HostError::Identity)?;
    let statement_digest = add_blob(&mut files, statement.clone());
    let referrer = serde_json::to_vec(&json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": {
            "mediaType": "application/vnd.oci.empty.v1+json",
            "digest": config_digest,
            "size": config_bytes.len(),
        },
        "layers": [{
            "mediaType": "application/vnd.in-toto+json",
            "digest": statement_digest,
            "size": statement.len(),
        }],
    }))
    .map_err(|_| HostError::Identity)?;
    let referrer_digest = add_blob(&mut files, referrer.clone());
    let subject = if bind_to_selected_image {
        manifest.image_manifest_digest.clone()
    } else {
        format!("sha256:{}", "0".repeat(64))
    };
    let descriptor = json!({
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "digest": referrer_digest,
        "size": referrer.len(),
        "annotations": {"io.containerd.manifest.subject": subject},
    });
    index["manifests"]
        .as_array_mut()
        .ok_or(HostError::Identity)?
        .push(descriptor);
    files.insert(
        "index.json".to_owned(),
        serde_json::to_vec(&index).map_err(|_| HostError::Identity)?,
    );
    let archive = refresh_archive(&files, &mut manifest, None)?;
    Ok((archive, manifest))
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

    let (archive, mut manifest) = fixture(None, false)?;
    let mut files = super::tar_archive::read_archive(&archive)?;
    files.insert("unexpected.txt".to_owned(), b"unexpected".to_vec());
    let extra_member = refresh_archive(&files, &mut manifest, None)?;
    assert_eq!(validate(&extra_member, &manifest), Err(HostError::Identity));
    Ok(())
}

#[test]
fn rejects_duplicate_archive_paths_and_duplicate_json_keys() -> Result<(), HostError> {
    let (archive, mut manifest) = fixture(None, false)?;
    let files = super::tar_archive::read_archive(&archive)?;
    let duplicate_path = refresh_archive(&files, &mut manifest, Some("manifest.json"))?;
    assert_eq!(
        validate(&duplicate_path, &manifest),
        Err(HostError::Identity)
    );

    let (archive, mut manifest) = fixture(None, false)?;
    let mut files = super::tar_archive::read_archive(&archive)?;
    files.insert(
        "oci-layout".to_owned(),
        br#"{"imageLayoutVersion":"1.0.0","imageLayoutVersion":"1.0.0"}"#.to_vec(),
    );
    let duplicate_key = refresh_archive(&files, &mut manifest, None)?;
    assert_eq!(
        validate(&duplicate_key, &manifest),
        Err(HostError::Identity)
    );
    Ok(())
}

#[test]
fn accepts_only_referrers_bound_to_the_selected_image() -> Result<(), HostError> {
    let (bound, manifest) = fixture_with_referrer(true)?;
    validate(&bound, &manifest)?;
    let (unbound, manifest) = fixture_with_referrer(false)?;
    assert_eq!(validate(&unbound, &manifest), Err(HostError::Identity));
    Ok(())
}
