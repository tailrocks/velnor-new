use std::collections::HashMap;

use serde_json::Value;

use super::super::types::{Descriptor, OciManifest, Platform};
use super::ProviderFailure;
use super::layout::{get_blob, verify_descriptor};
use super::model::{
    ArchiveFiles, IN_TOTO_MEDIA_TYPE, OCI_CONFIG_MEDIA_TYPE, OCI_MANIFEST_MEDIA_TYPE,
    SLSA_PREDICATE, WorkerImage,
};

pub(super) fn verify_buildkit_attestation(
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
