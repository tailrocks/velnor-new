// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    OciCandidateError, OciCandidateEvidence, OciCandidateInspection, OciImageConfiguration, Result,
    archive::{OciArchive, authenticate_archive, hash_blob},
    check_deadline, config,
    json::parse,
    layer::{LayerDescriptor, verify_layers},
};
#[path = "graph_support.rs"]
mod support;
use support::{
    Descriptor, add_reference, descriptor, is_layer_media_type, read_referenced, reject_unknown,
    sha256,
};

const INDEX_MEDIA_TYPE: &str = "application/vnd.oci.image.index.v1+json";
const MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";
const CONFIG_MEDIA_TYPE: &str = "application/vnd.oci.image.config.v1+json";

#[derive(Clone, Copy)]
enum ConfigurationProjection {
    Ignore,
    Include,
}

pub(super) fn inspect_graph(archive: OciArchive) -> Result<OciCandidateEvidence> {
    inspect_graph_inner(archive, ConfigurationProjection::Ignore)
        .map(|(inspection, _)| inspection.evidence)
}

pub(super) fn inspect_graph_preserving_archive(
    archive: OciArchive,
) -> Result<(OciCandidateEvidence, OciArchive)> {
    inspect_graph_inner(archive, ConfigurationProjection::Ignore)
        .map(|(inspection, archive)| (inspection.evidence, archive))
}

pub(super) fn inspect_graph_with_config(archive: OciArchive) -> Result<OciCandidateInspection> {
    inspect_graph_inner(archive, ConfigurationProjection::Include).map(|(inspection, _)| inspection)
}

fn inspect_graph_inner(
    mut archive: OciArchive,
    projection: ConfigurationProjection,
) -> Result<(OciCandidateInspection, OciArchive)> {
    check_deadline()?;
    let layout = parse(&archive.layout)?;
    if layout.as_object().is_none_or(|map| {
        map.len() != 1
            || map
                .get("imageLayoutVersion")
                .and_then(serde_json::Value::as_str)
                != Some("1.0.0")
    }) {
        return Err(OciCandidateError("oci_layout_identity"));
    }
    let index = parse(&archive.index)?;
    let index_obj = index
        .as_object()
        .ok_or(OciCandidateError("oci_index_shape"))?;
    reject_unknown(
        index_obj,
        &["schemaVersion", "mediaType", "manifests", "annotations"],
    )?;
    if index_obj
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        != Some(2)
        || index_obj
            .get("mediaType")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(INDEX_MEDIA_TYPE)
            != INDEX_MEDIA_TYPE
    {
        return Err(OciCandidateError("oci_index_identity"));
    }
    let manifests = index_obj
        .get("manifests")
        .and_then(serde_json::Value::as_array)
        .ok_or(OciCandidateError("oci_index_manifests"))?;
    if manifests.len() != 1 {
        return Err(OciCandidateError("oci_platform_count"));
    }
    let descriptor = descriptor(&manifests[0], true)?;
    if descriptor.media_type != MANIFEST_MEDIA_TYPE
        || manifests[0]
            .get("platform")
            .and_then(serde_json::Value::as_object)
            .is_none_or(|platform| {
                platform.get("os").and_then(serde_json::Value::as_str) != Some("linux")
                    || platform
                        .get("architecture")
                        .and_then(serde_json::Value::as_str)
                        != Some("amd64")
                    || platform.len() != 2
            })
    {
        return Err(OciCandidateError("oci_platform_identity"));
    }
    let manifest_raw = read_referenced(&mut archive, &descriptor)?;
    check_deadline()?;
    inspect_manifest(archive, &descriptor, &manifest_raw, projection)
}

fn inspect_manifest(
    archive: OciArchive,
    manifest_descriptor: &Descriptor,
    manifest_raw: &[u8],
    projection: ConfigurationProjection,
) -> Result<(OciCandidateInspection, OciArchive)> {
    let manifest = parse(manifest_raw)?;
    let manifest_obj = manifest
        .as_object()
        .ok_or(OciCandidateError("oci_manifest_shape"))?;
    reject_unknown(
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
        return Err(OciCandidateError("oci_manifest_identity"));
    }
    let config = descriptor(
        manifest_obj
            .get("config")
            .ok_or(OciCandidateError("oci_config_missing"))?,
        false,
    )?;
    if config.media_type != CONFIG_MEDIA_TYPE {
        return Err(OciCandidateError("oci_config_media_type"));
    }
    let layers_value = manifest_obj
        .get("layers")
        .and_then(serde_json::Value::as_array)
        .filter(|layers| !layers.is_empty())
        .ok_or(OciCandidateError("oci_layers_missing"))?;
    let layers = layers_value
        .iter()
        .map(|value| descriptor(value, false).map(Into::into))
        .collect::<Result<Vec<LayerDescriptor>>>()?;
    if layers
        .iter()
        .any(|layer| !is_layer_media_type(&layer.media_type))
    {
        return Err(OciCandidateError("oci_layer_media_type"));
    }
    inspect_config_and_blobs(archive, manifest_descriptor, &config, &layers, projection)
}

fn inspect_config_and_blobs(
    mut archive: OciArchive,
    manifest_descriptor: &Descriptor,
    config: &Descriptor,
    layers: &[LayerDescriptor],
    projection: ConfigurationProjection,
) -> Result<(OciCandidateInspection, OciArchive)> {
    let (expected_diff_ids, configuration) =
        read_expected_diff_ids(&mut archive, config, layers.len(), projection)?;
    let referenced = collect_references(manifest_descriptor, config, layers)?;
    validate_blob_inventory(&archive, &referenced)?;
    let expected = expected_diff_ids
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let digests = layer_digests(layers);
    verify_layers(&mut archive, layers, &expected)?;
    verify_blob_hashes(&mut archive, &referenced)?;
    let (archive_digest, archive_size) = authenticate_archive(&mut archive)?;
    let evidence = candidate_evidence(
        &archive,
        manifest_descriptor,
        config,
        digests,
        archive_digest,
        archive_size,
    )?;
    Ok((
        OciCandidateInspection {
            evidence,
            configuration,
        },
        archive,
    ))
}

fn collect_references(
    manifest: &Descriptor,
    config: &Descriptor,
    layers: &[LayerDescriptor],
) -> Result<std::collections::BTreeMap<String, (u64, String)>> {
    let mut referenced = std::collections::BTreeMap::new();
    add_reference(
        &mut referenced,
        &manifest.digest,
        manifest.size,
        &manifest.media_type,
    )?;
    add_reference(
        &mut referenced,
        &config.digest,
        config.size,
        &config.media_type,
    )?;
    for layer in layers {
        check_deadline()?;
        add_reference(
            &mut referenced,
            &layer.digest,
            layer.size,
            &layer.media_type,
        )?;
    }
    Ok(referenced)
}

fn validate_blob_inventory(
    archive: &OciArchive,
    referenced: &std::collections::BTreeMap<String, (u64, String)>,
) -> Result<()> {
    if archive
        .blobs
        .keys()
        .any(|digest| !referenced.contains_key(digest))
    {
        return Err(OciCandidateError("oci_unreferenced_blob"));
    }
    if referenced
        .keys()
        .any(|digest| !archive.blobs.contains_key(digest))
    {
        return Err(OciCandidateError("oci_blob_missing"));
    }
    if referenced.iter().any(|(digest, (size, _))| {
        archive
            .blobs
            .get(digest)
            .is_none_or(|blob| blob.size != *size)
    }) {
        return Err(OciCandidateError("oci_blob_size"));
    }
    Ok(())
}

fn verify_blob_hashes(
    archive: &mut OciArchive,
    referenced: &std::collections::BTreeMap<String, (u64, String)>,
) -> Result<()> {
    for (digest, (expected_size, _)) in referenced {
        check_deadline()?;
        let blob = archive
            .blobs
            .get(digest)
            .ok_or(OciCandidateError("oci_blob_missing"))?;
        let (actual_digest, actual_size) = hash_blob(&mut archive.file, blob)?;
        if actual_digest != *digest || actual_size != *expected_size {
            return Err(OciCandidateError("oci_blob_digest"));
        }
    }
    Ok(())
}

fn layer_digests(layers: &[LayerDescriptor]) -> Vec<String> {
    layers
        .iter()
        .map(|layer| format!("sha256:{}", layer.digest))
        .collect()
}

fn candidate_evidence(
    archive: &OciArchive,
    manifest_descriptor: &Descriptor,
    config: &Descriptor,
    layer_digests: Vec<String>,
    archive_digest: String,
    archive_size: u64,
) -> Result<OciCandidateEvidence> {
    Ok(OciCandidateEvidence {
        archive_sha256: archive_digest,
        archive_size_bytes: archive_size,
        archive_index_digest: sha256(&archive.index),
        archive_index_size_bytes: u64::try_from(archive.index.len())
            .map_err(|_| OciCandidateError("oci_index_size"))?,
        platform_manifest_digest: format!("sha256:{}", manifest_descriptor.digest),
        platform_manifest_size_bytes: manifest_descriptor.size,
        image_configuration_digest: format!("sha256:{}", config.digest),
        image_configuration_size_bytes: config.size,
        layer_digests,
    })
}

fn read_expected_diff_ids(
    archive: &mut OciArchive,
    config: &Descriptor,
    layer_count: usize,
    projection: ConfigurationProjection,
) -> Result<(Vec<String>, OciImageConfiguration)> {
    let config_raw = read_referenced(archive, config)?;
    let image_config = parse(&config_raw)?;
    let object = image_config
        .as_object()
        .ok_or(OciCandidateError("oci_config_shape"))?;
    if object.get("os").and_then(serde_json::Value::as_str) != Some("linux")
        || object
            .get("architecture")
            .and_then(serde_json::Value::as_str)
            != Some("amd64")
    {
        return Err(OciCandidateError("oci_config_platform"));
    }
    let values = object
        .get("rootfs")
        .and_then(serde_json::Value::as_object)
        .filter(|rootfs| rootfs.get("type").and_then(serde_json::Value::as_str) == Some("layers"))
        .and_then(|rootfs| rootfs.get("diff_ids"))
        .and_then(serde_json::Value::as_array)
        .filter(|values| values.len() == layer_count)
        .ok_or(OciCandidateError("oci_diff_ids"))?;
    let diff_ids = values
        .iter()
        .map(|value| {
            check_deadline()?;
            value
                .as_str()
                .map(str::to_owned)
                .ok_or(OciCandidateError("oci_diff_id_shape"))
        })
        .collect::<Result<Vec<_>>>()?;
    check_deadline()?;
    let projected = match projection {
        ConfigurationProjection::Ignore => OciImageConfiguration::default(),
        ConfigurationProjection::Include => config::project(&image_config)?,
    };
    Ok((diff_ids, projected))
}
