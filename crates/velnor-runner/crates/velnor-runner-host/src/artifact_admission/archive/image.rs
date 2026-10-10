//! Validated OCI image descriptors, config profile, and provenance referrer.

use std::collections::{BTreeMap, BTreeSet};

use bollard::models::ContainerConfig;
use serde_json::Value;

use crate::error::HostError;

use super::super::hash::{is_lower_hex, is_sha256_digest};
use super::super::manifest::{
    DEFAULT_ENV, IMAGE_ENTRYPOINT, IMAGE_USER, IMAGE_WORKDIR, SOURCE_LABEL,
};
use super::super::strict_json;

const MAX_JSON_BYTES: usize = 8 * 1024 * 1024;
const IMAGE_MEDIA_TYPES: [&str; 2] = [
    "application/vnd.oci.image.manifest.v1+json",
    "application/vnd.docker.distribution.manifest.v2+json",
];
const CONFIG_MEDIA_TYPES: [&str; 2] = [
    "application/vnd.oci.image.config.v1+json",
    "application/vnd.docker.container.image.v1+json",
];
const LAYER_MEDIA_TYPES: [&str; 2] = [
    "application/vnd.oci.image.layer.v1.tar+gzip",
    "application/vnd.docker.image.rootfs.diff.tar.gzip",
];

pub(super) struct ImageDescriptor {
    pub(super) image_manifest_digest: String,
    pub(super) image_manifest_media_type: String,
    pub(super) image_manifest_size: u64,
    pub(super) config_digest: String,
    pub(super) config: ContainerConfig,
}

pub(super) fn validate_image_descriptor(
    descriptor: &Value,
    files: &BTreeMap<String, Vec<u8>>,
    compatible_config: &str,
    compatible_layers: &[String],
    expected_source: &str,
    referenced: &mut BTreeSet<String>,
) -> Result<ImageDescriptor, HostError> {
    let media_type = required_string(descriptor, "mediaType")?;
    if !IMAGE_MEDIA_TYPES.contains(&media_type) {
        return Err(HostError::Identity);
    }
    validate_platform(descriptor)?;
    let (digest, contents) = descriptor_blob(descriptor, files, referenced)?;
    let manifest = parse_json(&contents, "image manifest")?;
    if manifest.get("schemaVersion").and_then(Value::as_u64) != Some(2)
        || manifest.get("mediaType").and_then(Value::as_str) != Some(media_type)
    {
        return Err(HostError::Identity);
    }
    let (config_digest, config) = validate_image_config(
        manifest.get("config").ok_or(HostError::Identity)?,
        files,
        compatible_config,
        expected_source,
        referenced,
    )?;
    let layers = manifest
        .get("layers")
        .and_then(Value::as_array)
        .ok_or(HostError::Identity)?
        .iter()
        .map(|layer| validate_layer(layer, files, referenced))
        .collect::<Result<Vec<_>, _>>()?;
    if layers != compatible_layers {
        return Err(HostError::Identity);
    }
    if let Some(annotations) = descriptor.get("annotations") {
        let annotations = annotations.as_object().ok_or(HostError::Identity)?;
        if annotations
            .get("config.digest")
            .is_some_and(|value| value.as_str() != Some(config_digest.as_str()))
        {
            return Err(HostError::Identity);
        }
    }
    Ok(ImageDescriptor {
        image_manifest_digest: digest,
        image_manifest_media_type: media_type.to_owned(),
        image_manifest_size: u64::try_from(contents.len()).map_err(|_| HostError::Frame)?,
        config_digest,
        config,
    })
}

fn validate_image_config(
    descriptor: &Value,
    files: &BTreeMap<String, Vec<u8>>,
    compatible_config: &str,
    expected_source: &str,
    referenced: &mut BTreeSet<String>,
) -> Result<(String, ContainerConfig), HostError> {
    let (digest, bytes) = descriptor_blob(descriptor, files, referenced)?;
    if digest != compatible_config
        || !CONFIG_MEDIA_TYPES.contains(&required_string(descriptor, "mediaType")?)
    {
        return Err(HostError::Identity);
    }
    let document = parse_json(&bytes, "image config")?;
    if document.get("os").and_then(Value::as_str) != Some("linux")
        || document.get("architecture").and_then(Value::as_str) != Some("amd64")
    {
        return Err(HostError::Identity);
    }
    let config = document.get("config").ok_or(HostError::Identity)?;
    validate_config_profile(config, expected_source)?;
    let config = serde_json::from_value(config.clone()).map_err(|_| HostError::Identity)?;
    Ok((digest, config))
}

fn validate_config_profile(config: &Value, expected_source: &str) -> Result<(), HostError> {
    let object = config.as_object().ok_or(HostError::Identity)?;
    let keys: BTreeSet<&str> = object.keys().map(String::as_str).collect();
    let expected_keys = BTreeSet::from(["User", "Env", "Entrypoint", "WorkingDir", "Labels"]);
    if keys != expected_keys
        || config.get("User").and_then(Value::as_str) != Some(IMAGE_USER)
        || config.get("WorkingDir").and_then(Value::as_str) != Some(IMAGE_WORKDIR)
        || config.get("Entrypoint") != Some(&serde_json::json!([IMAGE_ENTRYPOINT]))
        || config.get("Env") != Some(&serde_json::json!([DEFAULT_ENV]))
        || config.get("Labels") != Some(&serde_json::json!({SOURCE_LABEL: expected_source}))
        || config
            .get("Labels")
            .and_then(|value| value.get(SOURCE_LABEL))
            .and_then(Value::as_str)
            != Some(expected_source)
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn validate_layer(
    layer: &Value,
    files: &BTreeMap<String, Vec<u8>>,
    referenced: &mut BTreeSet<String>,
) -> Result<String, HostError> {
    if !LAYER_MEDIA_TYPES.contains(&required_string(layer, "mediaType")?) {
        return Err(HostError::Identity);
    }
    let (digest, _) = descriptor_blob(layer, files, referenced)?;
    Ok(digest)
}

pub(super) fn validate_referrer(
    descriptor: &Value,
    files: &BTreeMap<String, Vec<u8>>,
    subject: &str,
    referenced: &mut BTreeSet<String>,
) -> Result<(), HostError> {
    if descriptor.get("platform").is_some()
        || required_string(descriptor, "mediaType")? != "application/vnd.oci.image.manifest.v1+json"
        || descriptor
            .get("annotations")
            .and_then(|annotations| annotations.get("io.containerd.manifest.subject"))
            .and_then(Value::as_str)
            != Some(subject)
    {
        return Err(HostError::Identity);
    }
    let (_, contents) = descriptor_blob(descriptor, files, referenced)?;
    let manifest = parse_json(&contents, "referrer manifest")?;
    if manifest.get("mediaType").and_then(Value::as_str)
        != Some("application/vnd.oci.image.manifest.v1+json")
    {
        return Err(HostError::Identity);
    }
    let (_, config_bytes) = descriptor_blob(
        manifest.get("config").ok_or(HostError::Identity)?,
        files,
        referenced,
    )?;
    parse_json(&config_bytes, "referrer config")?;
    let layers = manifest
        .get("layers")
        .and_then(Value::as_array)
        .filter(|layers| !layers.is_empty())
        .ok_or(HostError::Identity)?;
    let mut subject_found = false;
    for layer in layers {
        if required_string(layer, "mediaType")? != "application/vnd.in-toto+json" {
            return Err(HostError::Identity);
        }
        let (_, statement_bytes) = descriptor_blob(layer, files, referenced)?;
        let statement = parse_json(&statement_bytes, "in-toto statement")?;
        let subjects = statement
            .get("subject")
            .and_then(Value::as_array)
            .ok_or(HostError::Identity)?;
        subject_found |= subjects.iter().any(|item| {
            item.get("digest")
                .and_then(|digest| digest.get("sha256"))
                .and_then(Value::as_str)
                == subject.strip_prefix("sha256:")
        });
    }
    if subject_found {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

fn validate_platform(descriptor: &Value) -> Result<(), HostError> {
    if let Some(platform) = descriptor.get("platform")
        && platform != &serde_json::json!({"os": "linux", "architecture": "amd64"})
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn descriptor_blob(
    descriptor: &Value,
    files: &BTreeMap<String, Vec<u8>>,
    referenced: &mut BTreeSet<String>,
) -> Result<(String, Vec<u8>), HostError> {
    let digest = required_string(descriptor, "digest")?;
    if !is_sha256_digest(digest) {
        return Err(HostError::Identity);
    }
    let size = descriptor
        .get("size")
        .and_then(Value::as_u64)
        .ok_or(HostError::Identity)?;
    let hex = digest.strip_prefix("sha256:").ok_or(HostError::Identity)?;
    let path = format!("blobs/sha256/{hex}");
    let contents = files.get(&path).ok_or(HostError::Identity)?;
    if u64::try_from(contents.len()).map_err(|_| HostError::Identity)? != size {
        return Err(HostError::Identity);
    }
    referenced.insert(path);
    Ok((digest.to_owned(), contents.clone()))
}

pub(super) fn blob_path(path: &str) -> Result<String, HostError> {
    let digest = path
        .strip_prefix("blobs/sha256/")
        .ok_or(HostError::Identity)?;
    if !is_lower_hex(digest, 64) {
        return Err(HostError::Identity);
    }
    Ok(format!("sha256:{digest}"))
}

fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, HostError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or(HostError::Identity)
}

pub(super) fn has_annotation(value: &Value, key: &str) -> Result<bool, HostError> {
    match value.get("annotations") {
        None => Ok(false),
        Some(annotations) => annotations
            .as_object()
            .map(|annotations| annotations.contains_key(key))
            .ok_or(HostError::Identity),
    }
}

pub(super) fn file<'a>(
    files: &'a BTreeMap<String, Vec<u8>>,
    name: &str,
) -> Result<&'a [u8], HostError> {
    files
        .get(name)
        .map(Vec::as_slice)
        .ok_or(HostError::Identity)
}

pub(super) fn parse_json(bytes: &[u8], _label: &str) -> Result<Value, HostError> {
    if bytes.is_empty() || bytes.len() > MAX_JSON_BYTES {
        return Err(HostError::Frame);
    }
    strict_json::parse(bytes)
}
