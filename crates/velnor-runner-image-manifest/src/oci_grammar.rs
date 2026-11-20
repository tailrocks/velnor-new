// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OciGrammarError(pub &'static str);
type Result<T> = std::result::Result<T, OciGrammarError>;
const MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";
const CONFIG_MEDIA_TYPE: &str = "application/vnd.oci.image.config.v1+json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OciDescriptor {
    pub digest: String,
    pub size: u64,
    pub media_type: String,
}

pub fn parse_oci_descriptor(value: &serde_json::Value, strict: bool) -> Result<OciDescriptor> {
    let object = value
        .as_object()
        .ok_or(OciGrammarError("oci_descriptor_shape"))?;
    if !["mediaType", "digest", "size"]
        .iter()
        .all(|key| object.contains_key(*key))
    {
        return Err(OciGrammarError("oci_descriptor_required"));
    }
    if strict {
        reject_unknown_oci_fields(
            object,
            &["mediaType", "digest", "size", "platform", "annotations"],
        )?;
    }
    let digest = object
        .get("digest")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| value.strip_prefix("sha256:"))
        .filter(|value| is_digest(value))
        .ok_or(OciGrammarError("oci_descriptor_digest"))?
        .to_owned();
    let size = object
        .get("size")
        .and_then(serde_json::Value::as_u64)
        .filter(|size| *size > 0)
        .ok_or(OciGrammarError("oci_descriptor_size"))?;
    let media_type = object
        .get("mediaType")
        .and_then(serde_json::Value::as_str)
        .ok_or(OciGrammarError("oci_descriptor_media_type"))?
        .to_owned();
    Ok(OciDescriptor {
        digest,
        size,
        media_type,
    })
}

pub fn reject_unknown_oci_fields(
    object: &serde_json::Map<String, serde_json::Value>,
    allowed: &[&str],
) -> Result<()> {
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(OciGrammarError("oci_unknown_field"));
    }
    Ok(())
}

#[must_use]
pub fn is_oci_layer_media_type(value: &str) -> bool {
    matches!(
        value,
        "application/vnd.oci.image.layer.v1.tar"
            | "application/vnd.oci.image.layer.v1.tar+gzip"
            | "application/vnd.oci.image.layer.nondistributable.v1.tar"
            | "application/vnd.oci.image.layer.nondistributable.v1.tar+gzip"
    )
}

fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn parse_oci_manifest(
    manifest: &serde_json::Value,
) -> Result<(OciDescriptor, Vec<OciDescriptor>)> {
    let manifest_obj = manifest
        .as_object()
        .ok_or(OciGrammarError("oci_manifest_shape"))?;
    reject_unknown_oci_fields(
        manifest_obj,
        &[
            "schemaVersion",
            "mediaType",
            "config",
            "layers",
            "annotations",
        ],
    )?;
    if manifest_obj
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        != Some(2)
        || manifest_obj
            .get("mediaType")
            .and_then(serde_json::Value::as_str)
            != Some(MANIFEST_MEDIA_TYPE)
    {
        return Err(OciGrammarError("oci_manifest_identity"));
    }
    let config = parse_oci_descriptor(
        manifest_obj
            .get("config")
            .ok_or(OciGrammarError("oci_config_missing"))?,
        false,
    )?;
    if config.media_type != CONFIG_MEDIA_TYPE {
        return Err(OciGrammarError("oci_config_media_type"));
    }
    let layers_value = manifest_obj
        .get("layers")
        .and_then(serde_json::Value::as_array)
        .filter(|layers| !layers.is_empty())
        .ok_or(OciGrammarError("oci_layers_missing"))?;
    let layers = layers_value
        .iter()
        .map(|value| parse_oci_descriptor(value, false))
        .collect::<Result<Vec<OciDescriptor>>>()?;
    if layers
        .iter()
        .any(|layer| !is_oci_layer_media_type(&layer.media_type))
    {
        return Err(OciGrammarError("oci_layer_media_type"));
    }
    Ok((config, layers))
}
