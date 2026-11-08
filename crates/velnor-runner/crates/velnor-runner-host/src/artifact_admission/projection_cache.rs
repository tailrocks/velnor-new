// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::hash::{is_lower_hex, lowercase_hex, sha256};
use crate::error::HostError;
use serde_json::Value;
use velnor_runner_image_manifest::{
    ImageEntry, ImageKind, admits_image_configuration, parse_oci_manifest,
    project_oci_configuration, strict_json,
};

const CONFIG_LIMIT: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryProjection {
    manifest_size: u64,
    configuration: Value,
    diff_ids: Vec<String>,
    role: ImageKind,
    source: String,
    registry_digest: String,
    config_digest: String,
}

impl RegistryProjection {
    pub fn from_oci(
        image: &ImageEntry,
        role: ImageKind,
        source: &str,
        manifest: &[u8],
        config_bytes: &[u8],
    ) -> Result<Self, HostError> {
        if image.repository != role.repository()
            || image.registry_digest != image.platform_manifest_digest
            || !digest(&image.registry_digest)
            || !digest(&image.config_digest)
            || !is_lower_hex(source, 40)
            || format!("sha256:{}", lowercase_hex(&sha256(manifest))) != image.registry_digest
        {
            return Err(HostError::Identity);
        }
        let value = strict_json::parse_strict_json_bytes(manifest, 4 * 1024 * 1024)
            .map_err(|_| HostError::Identity)?;
        let (config, layers) = parse_oci_manifest(&value).map_err(|_| HostError::Identity)?;
        if format!("sha256:{}", config.digest) != image.config_digest
            || config_bytes.len() as u64 != config.size
            || lowercase_hex(&sha256(config_bytes)) != config.digest
        {
            return Err(HostError::Identity);
        }
        let (configuration, diff_ids) = validate_config(config_bytes, layers.len(), role, source)?;
        Ok(Self {
            manifest_size: manifest.len() as u64,
            configuration,
            diff_ids,
            role,
            source: source.to_owned(),
            registry_digest: image.registry_digest.clone(),
            config_digest: image.config_digest.clone(),
        })
    }

    pub(crate) fn manifest_size(&self) -> u64 {
        self.manifest_size
    }

    pub(crate) fn configuration(&self) -> &Value {
        &self.configuration
    }

    pub(crate) fn diff_ids(&self) -> &[String] {
        &self.diff_ids
    }

    pub(crate) fn matches(
        &self,
        role: ImageKind,
        source: &str,
        registry_digest: &str,
        config_digest: &str,
    ) -> bool {
        self.role == role
            && self.source == source
            && self.registry_digest == registry_digest
            && self.config_digest == config_digest
    }
}

pub(super) fn validate_config(
    bytes: &[u8],
    layer_count: usize,
    role: ImageKind,
    source: &str,
) -> Result<(Value, Vec<String>), HostError> {
    let image_config = strict_json::parse_strict_json_bytes(bytes, CONFIG_LIMIT)
        .map_err(|_| HostError::Identity)?;
    let object = image_config.as_object().ok_or(HostError::Identity)?;
    if object.get("os").and_then(Value::as_str) != Some("linux")
        || object.get("architecture").and_then(Value::as_str) != Some("amd64")
    {
        return Err(HostError::Identity);
    }
    let diff_ids = object
        .get("rootfs")
        .and_then(Value::as_object)
        .filter(|root| root.get("type").and_then(Value::as_str) == Some("layers"))
        .and_then(|root| root.get("diff_ids"))
        .and_then(Value::as_array)
        .filter(|values| values.len() == layer_count)
        .ok_or(HostError::Identity)?
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|value| digest(value))
                .map(str::to_owned)
                .ok_or(HostError::Identity)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let projection = project_oci_configuration(&image_config).map_err(|_| HostError::Identity)?;
    if !admits_image_configuration(&projection, role, source) {
        return Err(HostError::Identity);
    }
    let configuration = object
        .get("config")
        .and_then(Value::as_object)
        .cloned()
        .ok_or(HostError::Identity)?;
    Ok((Value::Object(configuration), diff_ids))
}

fn digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|value| is_lower_hex(value, 64))
}

#[derive(Default)]
pub(super) struct ProjectionCache {
    entries: Vec<(ImageEntry, RegistryProjection)>,
}

impl ProjectionCache {
    pub(super) fn get(
        &self,
        image: &ImageEntry,
        role: ImageKind,
        source: &str,
    ) -> Option<RegistryProjection> {
        self.entries
            .iter()
            .find(|(cached_image, projection)| {
                cached_image == image
                    && projection.matches(
                        role,
                        source,
                        &image.registry_digest,
                        &image.config_digest,
                    )
            })
            .map(|(_, projection)| projection.clone())
    }

    pub(super) fn insert(&mut self, image: &ImageEntry, projection: RegistryProjection) {
        self.entries
            .retain(|(_, cached)| cached.role != projection.role);
        self.entries.push((image.clone(), projection));
    }
}
