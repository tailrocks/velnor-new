// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::time::Instant;

use serde_json::Value;
use sha2::{Digest, Sha256};
use velnor_runner_image_manifest::{
    ImageCandidateArchive, ImageCandidateOci, ImageKind, OciDescriptor, parse_oci_descriptor,
    parse_oci_manifest, strict_json,
};

use super::ProviderFailure;
use super::archive_read::{hash_projected, read_projected};
use super::model::ArchiveFiles;
use super::projected_layers;

pub(crate) struct ProjectedCandidate {
    pub(crate) archive: ImageCandidateArchive,
    pub(crate) oci: ImageCandidateOci,
    pub(crate) configuration: Value,
    pub(crate) diff_ids: Vec<String>,
}

pub(crate) fn inspect_projected_candidate(
    held: &File,
    role: ImageKind,
    source: &str,
    deadline: Instant,
) -> Result<ProjectedCandidate, crate::error::HostError> {
    inspect(held, role, source, deadline).map_err(|error| match error {
        ProviderFailure::Timeout => crate::error::HostError::DockerTimeout,
        _ => crate::error::HostError::Identity,
    })
}

fn inspect(
    held: &File,
    role: ImageKind,
    source: &str,
    deadline: Instant,
) -> Result<ProjectedCandidate, ProviderFailure> {
    if source.len() != 40
        || !source
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ProviderFailure::Archive);
    }
    let archive_sha256 = hash_projected(held, deadline)?;
    let archive_size_bytes = held.metadata().map_err(|_| ProviderFailure::Archive)?.len();
    let archive = read_projected(held, deadline)?;
    let (index_bytes, manifest) = inspect_index(&archive)?;
    let manifest_bytes = read_blob(held, &archive, &manifest, deadline)?;
    let manifest_value = parse(&manifest_bytes)?;
    let (config, layers) =
        parse_oci_manifest(&manifest_value).map_err(|_| ProviderFailure::Archive)?;
    let config_bytes = read_blob(held, &archive, &config, deadline)?;
    let (configuration, diff_ids) =
        inspect_configuration(&config_bytes, layers.len(), role, source)?;
    let mut references = BTreeMap::new();
    for descriptor in [&manifest, &config].into_iter().chain(layers.iter()) {
        if references
            .insert(
                descriptor.digest.clone(),
                (descriptor.size, &descriptor.media_type),
            )
            .is_some_and(|old| old != (descriptor.size, &descriptor.media_type))
        {
            return Err(ProviderFailure::Archive);
        }
        let blob = archive
            .blobs
            .get(&format!("blobs/sha256/{}", descriptor.digest))
            .ok_or(ProviderFailure::Archive)?;
        if blob.size != descriptor.size {
            return Err(ProviderFailure::Archive);
        }
    }
    if references.len() != archive.blobs.len() {
        return Err(ProviderFailure::Archive);
    }
    projected_layers::verify(held, &archive.blobs, &layers, &diff_ids, deadline)?;
    if hash_projected(held, deadline)? != archive_sha256 {
        return Err(ProviderFailure::Archive);
    }
    Ok(ProjectedCandidate {
        archive: ImageCandidateArchive {
            sha256: archive_sha256,
            size_bytes: archive_size_bytes,
        },
        oci: ImageCandidateOci {
            archive_index_digest: super::hex(&Sha256::digest(index_bytes)),
            archive_index_size_bytes: index_bytes.len() as u64,
            platform_manifest_digest: format!("sha256:{}", manifest.digest),
            platform_manifest_size_bytes: manifest.size,
            image_configuration_digest: format!("sha256:{}", config.digest),
            image_configuration_size_bytes: config.size,
            layer_digests: layers
                .iter()
                .map(|layer| format!("sha256:{}", layer.digest))
                .collect(),
        },
        configuration: Value::Object(configuration),
        diff_ids,
    })
}

fn parse(bytes: &[u8]) -> Result<Value, ProviderFailure> {
    strict_json::parse_strict_json_bytes(bytes, bytes.len()).map_err(|_| ProviderFailure::Archive)
}

fn inspect_configuration(
    config_bytes: &[u8],
    layer_count: usize,
    role: ImageKind,
    source: &str,
) -> Result<(serde_json::Map<String, Value>, Vec<String>), ProviderFailure> {
    let image_config = parse(config_bytes)?;
    let object = image_config.as_object().ok_or(ProviderFailure::Archive)?;
    if object.get("os").and_then(Value::as_str) != Some("linux")
        || object.get("architecture").and_then(Value::as_str) != Some("amd64")
    {
        return Err(ProviderFailure::Archive);
    }
    let diff_ids = object
        .get("rootfs")
        .and_then(Value::as_object)
        .filter(|root| root.get("type").and_then(Value::as_str) == Some("layers"))
        .and_then(|root| root.get("diff_ids"))
        .and_then(Value::as_array)
        .filter(|ids| ids.len() == layer_count)
        .ok_or(ProviderFailure::Archive)?
        .iter()
        .map(|id| {
            id.as_str()
                .map(str::to_owned)
                .ok_or(ProviderFailure::Archive)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let configuration = object
        .get("config")
        .and_then(Value::as_object)
        .cloned()
        .ok_or(ProviderFailure::Archive)?;
    let projected = velnor_runner_image_manifest::project_oci_configuration(&image_config)
        .map_err(|_| ProviderFailure::Archive)?;
    if !velnor_runner_image_manifest::admits_image_configuration(&projected, role, source) {
        return Err(ProviderFailure::Archive);
    }
    Ok((configuration, diff_ids))
}

fn inspect_index(archive: &ArchiveFiles) -> Result<(&[u8], OciDescriptor), ProviderFailure> {
    let layout = parse(
        archive
            .files
            .get("oci-layout")
            .ok_or(ProviderFailure::Archive)?,
    )?;
    if layout.as_object().is_none_or(|value| {
        value.len() != 1 || value.get("imageLayoutVersion").and_then(Value::as_str) != Some("1.0.0")
    }) {
        return Err(ProviderFailure::Archive);
    }
    let index_bytes = archive
        .files
        .get("index.json")
        .ok_or(ProviderFailure::Archive)?;
    let index = parse(index_bytes)?;
    let object = index.as_object().ok_or(ProviderFailure::Archive)?;
    if object.len() != 3
        || object.get("schemaVersion").and_then(Value::as_u64) != Some(2)
        || object.get("mediaType").and_then(Value::as_str)
            != Some("application/vnd.oci.image.index.v1+json")
    {
        return Err(ProviderFailure::Archive);
    }
    let entries = object
        .get("manifests")
        .and_then(Value::as_array)
        .filter(|entries| entries.len() == 1)
        .ok_or(ProviderFailure::Archive)?;
    let value = &entries[0];
    let manifest = parse_oci_descriptor(value, true).map_err(|_| ProviderFailure::Archive)?;
    let platform = value
        .get("platform")
        .and_then(Value::as_object)
        .ok_or(ProviderFailure::Archive)?;
    if manifest.media_type != "application/vnd.oci.image.manifest.v1+json"
        || value.get("annotations").is_some()
        || platform.len() != 2
        || platform.get("os").and_then(Value::as_str) != Some("linux")
        || platform.get("architecture").and_then(Value::as_str) != Some("amd64")
    {
        return Err(ProviderFailure::Archive);
    }
    Ok((index_bytes, manifest))
}

fn read_blob(
    held: &File,
    archive: &ArchiveFiles,
    descriptor: &OciDescriptor,
    deadline: Instant,
) -> Result<Vec<u8>, ProviderFailure> {
    if descriptor.size > 64 * 1024 * 1024 {
        return Err(ProviderFailure::Archive);
    }
    let blob = archive
        .blobs
        .get(&format!("blobs/sha256/{}", descriptor.digest))
        .filter(|blob| blob.size == descriptor.size)
        .ok_or(ProviderFailure::Archive)?;
    let mut file = held.try_clone().map_err(|_| ProviderFailure::Archive)?;
    file.seek(SeekFrom::Start(
        blob.offset.ok_or(ProviderFailure::Archive)?,
    ))
    .map_err(|_| ProviderFailure::Archive)?;
    let mut bytes = Vec::new();
    let mut limited = file.take(descriptor.size);
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        if Instant::now() >= deadline {
            return Err(ProviderFailure::Timeout);
        }
        let count = limited
            .read(&mut buffer)
            .map_err(|_| ProviderFailure::Archive)?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    if Instant::now() >= deadline {
        return Err(ProviderFailure::Timeout);
    }
    if bytes.len() as u64 != descriptor.size
        || super::hex(&Sha256::digest(&bytes)) != descriptor.digest
    {
        return Err(ProviderFailure::Archive);
    }
    Ok(bytes)
}
