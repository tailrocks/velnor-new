// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, fs::File, os::unix::fs::MetadataExt};

use serde_json::Value;
use velnor_runner_image_manifest::{
    attestation_descriptors, attested_index_manifests, split_attested_roles,
    validate_attestation_reference,
};

use super::{
    INDEX_MEDIA_TYPE, MANIFEST_MEDIA_TYPE, add_reference, descriptor, parse_manifest,
    read_referenced, sha256, validate_blob_inventory, verify_blob_hashes,
};
use crate::owned_archive_guard::oci_candidate::{
    OciCandidateError, OciCandidateInspection, Result, UntrustedProvenance,
    archive::{authenticate_archive, load_archive_file},
    check_deadline, inspect_oci_candidate_file_with_config,
    json::parse,
};

mod envelope;
mod output;

pub use envelope::{UntrustedEnvelopeInspection, verify_provenance_envelope};

#[derive(Debug)]
pub struct AttestedProjection {
    pub provenance: UntrustedProvenance,
    pub candidate: OciCandidateInspection,
}

pub(in crate::owned_archive_guard::oci_candidate) fn project_attested_oci_file(
    source: &File,
    destination: &mut File,
) -> Result<AttestedProjection> {
    check_deadline()?;
    validate_destination(source, destination)?;
    let mut archive = load_archive_file(source)?;
    let layout = parse(&archive.layout)?;
    if layout.as_object().is_none_or(|value| {
        value.len() != 1 || value.get("imageLayoutVersion").and_then(Value::as_str) != Some("1.0.0")
    }) {
        return Err(OciCandidateError("oci_layout_identity"));
    }
    let outer = parse(&archive.index)?;
    let outer_manifests =
        attested_index_manifests(&outer, 1).map_err(|error| OciCandidateError(error.0))?;
    let nested_descriptor = descriptor(&outer_manifests[0], true)?;
    if nested_descriptor.media_type != INDEX_MEDIA_TYPE {
        return Err(OciCandidateError("attested_index_media_type"));
    }
    let nested_raw = read_referenced(&mut archive, &nested_descriptor)?;
    let nested = parse(&nested_raw)?;
    let manifests =
        attested_index_manifests(&nested, 2).map_err(|error| OciCandidateError(error.0))?;
    let (platform_value, attestation_value) =
        split_attested_roles(manifests).map_err(|error| OciCandidateError(error.0))?;
    let platform = descriptor(platform_value, true)?;
    let attestation = descriptor(attestation_value, true)?;
    if platform.media_type != MANIFEST_MEDIA_TYPE || attestation.media_type != MANIFEST_MEDIA_TYPE {
        return Err(OciCandidateError("attested_manifest_media_type"));
    }
    validate_attestation_reference(attestation_value, &platform)
        .map_err(|error| OciCandidateError(error.0))?;
    let platform_raw = read_referenced(&mut archive, &platform)?;
    let (configuration, layers) = parse_manifest(&platform_raw)?;
    let attestation_raw = read_referenced(&mut archive, &attestation)?;
    let attestation_manifest = parse(&attestation_raw)?;
    let (attestation_config, statement) =
        attestation_parts(&mut archive, &attestation_manifest, &platform)?;
    let mut all = BTreeMap::new();
    let mut projected = BTreeMap::new();
    for value in [&nested_descriptor, &platform, &attestation, &configuration] {
        add_reference(&mut all, &value.digest, value.size, &value.media_type)?;
    }
    for value in [&platform, &configuration] {
        add_reference(&mut projected, &value.digest, value.size, &value.media_type)?;
    }
    for layer in &layers {
        add_reference(&mut all, &layer.digest, layer.size, &layer.media_type)?;
        add_reference(&mut projected, &layer.digest, layer.size, &layer.media_type)?;
    }
    for value in [&attestation_config.descriptor, &statement.descriptor] {
        add_reference(&mut all, &value.digest, value.size, &value.media_type)?;
    }
    validate_blob_inventory(&archive, &all)?;
    verify_blob_hashes(&mut archive, &all)?;
    let (original_digest, original_size) = authenticate_archive(&mut archive)?;
    let projected_index = projected_index(platform_value)?;
    output::write(&mut archive, destination, &projected_index, &projected)?;
    let candidate = inspect_oci_candidate_file_with_config(destination)?;
    let (final_digest, final_size) = authenticate_archive(&mut archive)?;
    if (original_digest.as_str(), original_size) != (final_digest.as_str(), final_size)
        || candidate.evidence.platform_manifest_digest != format!("sha256:{}", platform.digest)
    {
        return Err(OciCandidateError("projection_identity"));
    }
    Ok(AttestedProjection {
        provenance: UntrustedProvenance {
            original_archive_sha256: original_digest,
            original_archive_size_bytes: original_size,
            original_index: archive.index.clone(),
            buildkit_index: nested_raw,
            attestation_descriptor: attestation_value.clone(),
            attestation_manifest: attestation_raw,
            attestation_config: attestation_config.bytes,
            statement: statement.bytes,
        },
        candidate,
    })
}

fn projected_index(platform: &Value) -> Result<Vec<u8>> {
    let mut descriptor = platform.clone();
    descriptor
        .as_object_mut()
        .ok_or(OciCandidateError("projection_index"))?
        .remove("annotations");
    serde_json::to_vec(&serde_json::json!({"schemaVersion": 2, "mediaType": INDEX_MEDIA_TYPE, "manifests": [descriptor]})).map_err(|_| OciCandidateError("projection_index"))
}

fn validate_destination(source: &File, destination: &File) -> Result<()> {
    let source_meta = source
        .metadata()
        .map_err(|_| OciCandidateError("archive_stat"))?;
    let destination_meta = destination
        .metadata()
        .map_err(|_| OciCandidateError("projection_stat"))?;
    if !destination_meta.is_file()
        || destination_meta.len() != 0
        || destination_meta.nlink() != 1
        || (destination_meta.dev() == source_meta.dev()
            && destination_meta.ino() == source_meta.ino())
    {
        return Err(OciCandidateError("projection_destination"));
    }
    Ok(())
}

struct AttestationPart {
    descriptor: super::OciDescriptor,
    bytes: Vec<u8>,
}

fn attestation_parts(
    archive: &mut crate::owned_archive_guard::oci_candidate::archive::OciArchive,
    value: &Value,
    platform: &super::OciDescriptor,
) -> Result<(AttestationPart, AttestationPart)> {
    let (config, statement) =
        attestation_descriptors(value, platform).map_err(|error| OciCandidateError(error.0))?;
    let config_bytes = read_referenced(archive, &config)?;
    let statement_bytes = read_referenced(archive, &statement)?;
    if config_bytes != b"{}" || statement_bytes.len() > 1024 * 1024 {
        return Err(OciCandidateError("attested_metadata_size"));
    }
    parse(&statement_bytes)?;
    Ok((
        AttestationPart {
            descriptor: config,
            bytes: config_bytes,
        },
        AttestationPart {
            descriptor: statement,
            bytes: statement_bytes,
        },
    ))
}
