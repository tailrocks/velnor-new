// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use serde_json::Value;

use super::super::types::{Descriptor, OciManifest, Platform};
use super::ProviderFailure;
use super::layout::{get_blob, sha256, verify_descriptor};
use super::model::{
    ArchiveFiles, Blob, IN_TOTO_MEDIA_TYPE, OCI_CONFIG_MEDIA_TYPE, OCI_MANIFEST_MEDIA_TYPE,
    SLSA_PREDICATE, WorkerImage,
};
use crate::error::HostError;

const OCI_EMPTY_CONFIG_MEDIA_TYPE: &str = "application/vnd.oci.empty.v1+json";
const OCI_EMPTY_CONFIG_DIGEST: &str =
    "sha256:44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a";
const OCI_EMPTY_CONFIG_SIZE: u64 = 2;
const DOCKER_ATTESTATION_ARTIFACT_TYPE: &str =
    "application/vnd.docker.attestation.manifest.v1+json";

#[expect(
    clippy::too_many_arguments,
    reason = "public attestation verifier takes each OCI envelope component explicitly"
)]
pub fn verify_worker_attestation_manifest(
    image: WorkerImage,
    source_sha: &str,
    platform_digest: &str,
    platform_size: u64,
    attestation_descriptor_json: &Value,
    attestation_manifest_bytes: &[u8],
    config_bytes: &[u8],
    statement_bytes: &[u8],
) -> Result<(), HostError> {
    let platform_manifest = Descriptor {
        media_type: OCI_MANIFEST_MEDIA_TYPE.to_owned(),
        digest: platform_digest.to_owned(),
        size: platform_size,
        annotations: None,
        platform: Some(Platform {
            architecture: "amd64".to_owned(),
            os: "linux".to_owned(),
        }),
        artifact_type: None,
    };
    let descriptor: Descriptor = serde_json::from_value(attestation_descriptor_json.clone())
        .map_err(|_| HostError::Docker)?;
    let mut blobs = HashMap::new();
    for raw in [attestation_manifest_bytes, config_bytes, statement_bytes] {
        let digest = sha256(raw);
        blobs.insert(
            format!("blobs/sha256/{digest}"),
            Blob {
                digest,
                size: raw.len() as u64,
                metadata: Some(raw.to_vec()),
            },
        );
    }
    let archive = ArchiveFiles {
        files: HashMap::new(),
        blobs,
    };
    verify_buildkit_attestation(&archive, image, source_sha, &platform_manifest, &descriptor)
        .map_err(|_| HostError::Docker)
}

pub(super) fn verify_buildkit_attestation(
    archive: &ArchiveFiles,
    image: WorkerImage,
    source_sha: &str,
    platform_manifest: &Descriptor,
    descriptor: &Descriptor,
) -> Result<(), ProviderFailure> {
    verify_descriptor(descriptor)?;
    let manifest_bytes = get_blob(archive, descriptor)?;
    let manifest: OciManifest =
        serde_json::from_slice(manifest_bytes).map_err(|_| ProviderFailure::Archive)?;
    let config_bytes = get_blob(archive, &manifest.config)?;
    let config_value: Value =
        serde_json::from_slice(config_bytes).map_err(|_| ProviderFailure::Archive)?;
    verify_attestation_envelope(platform_manifest, descriptor, &manifest, &config_value)?;
    let layer = manifest.layers.first().ok_or(ProviderFailure::Archive)?;
    let statement = get_blob(archive, layer)?;
    verify_buildkit_statement(statement, image, source_sha, platform_manifest)
}

fn verify_attestation_envelope(
    platform_manifest: &Descriptor,
    descriptor: &Descriptor,
    manifest: &OciManifest,
    config_value: &Value,
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
    let descriptor_artifact_ok = descriptor.artifact_type.is_none()
        || descriptor.artifact_type.as_deref() == Some(DOCKER_ATTESTATION_ARTIFACT_TYPE);
    if descriptor.media_type != OCI_MANIFEST_MEDIA_TYPE
        || descriptor.platform.as_ref()
            != Some(&Platform {
                architecture: "unknown".to_owned(),
                os: "unknown".to_owned(),
            })
        || !descriptor_artifact_ok
        || descriptor.annotations.as_ref() != Some(&expected_annotations)
    {
        return Err(ProviderFailure::Archive);
    }
    if manifest.schema_version != 2
        || manifest.media_type != OCI_MANIFEST_MEDIA_TYPE
        || manifest.layers.len() != 1
    {
        return Err(ProviderFailure::Archive);
    }
    let config = &manifest.config;
    verify_descriptor(config)?;
    if config.artifact_type.is_some() || config.annotations.is_some() || config.platform.is_some() {
        return Err(ProviderFailure::Archive);
    }
    let layer = manifest.layers.first().ok_or(ProviderFailure::Archive)?;
    verify_descriptor(layer)?;
    if layer.media_type != IN_TOTO_MEDIA_TYPE
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
    match manifest.artifact_type.as_deref() {
        None => {
            if descriptor.artifact_type.is_some()
                || manifest.subject.is_some()
                || config.media_type != OCI_CONFIG_MEDIA_TYPE
                || !valid_attestation_config(config_value, &layer.digest)
            {
                return Err(ProviderFailure::Archive);
            }
        }
        Some(DOCKER_ATTESTATION_ARTIFACT_TYPE) => {
            if config.media_type != OCI_EMPTY_CONFIG_MEDIA_TYPE
                || config.digest != OCI_EMPTY_CONFIG_DIGEST
                || config.size != OCI_EMPTY_CONFIG_SIZE
                || !config_value
                    .as_object()
                    .is_some_and(serde_json::Map::is_empty)
                || !valid_attestation_subject(manifest.subject.as_ref(), platform_manifest)
            {
                return Err(ProviderFailure::Archive);
            }
        }
        Some(_) => return Err(ProviderFailure::Archive),
    }
    Ok(())
}

fn valid_attestation_subject(subject: Option<&Value>, platform_manifest: &Descriptor) -> bool {
    let Some(subject_value) = subject else {
        return false;
    };
    let Ok(subject) = serde_json::from_value::<Descriptor>(subject_value.clone()) else {
        return false;
    };
    subject.media_type == platform_manifest.media_type
        && subject.digest == platform_manifest.digest
        && subject.size == platform_manifest.size
        && subject.artifact_type.is_none()
        && subject.annotations.is_none()
}

fn verify_buildkit_statement(
    bytes: &[u8],
    image: WorkerImage,
    source_sha: &str,
    platform_manifest: &Descriptor,
) -> Result<(), ProviderFailure> {
    let statement: Value = serde_json::from_slice(bytes).map_err(|_| ProviderFailure::Archive)?;
    super::source::verify_build_source(&statement, image, source_sha, platform_manifest)
}

pub(super) fn valid_attestation_config(value: &Value, layer_digest: &str) -> bool {
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
            .is_some_and(serde_json::Map::is_empty)
        && rootfs.len() == 2
        && rootfs.get("type").and_then(Value::as_str) == Some("layers")
        && diff_ids.is_some_and(|ids| ids.len() == 1 && ids[0].as_str() == Some(layer_digest))
}
