//! Non-extracting tar and `CycloneDX` inspection of copied producer assets.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufReader, Read},
    path::Path,
    time::Instant,
};

use flate2::bufread::GzDecoder;
use serde_json::Value;
use tar::EntryType;
use velnor_actions_contract::parse_strict_json;

use super::{
    ArtifactKind, ExpectedFile, MAX_TOTAL_DECOMPRESSED_BYTES, VerifiedPackageArtifact, invalid,
};
use crate::OrchestratorError;

#[path = "package_release_artifacts_gzip.rs"]
mod gzip;

const MAX_ENTRIES_PER_ARCHIVE: usize = 4096;
const MAX_EXECUTABLE_BYTES: u64 = 256 * 1024 * 1024;

/// Inspect every copied payload and its SBOM without materializing archive members.
pub(super) fn validate_copied_content(
    artifacts: &[VerifiedPackageArtifact],
    expected: &[ExpectedFile],
    source_commit: &str,
    source_ref: &str,
    version: &str,
    started: Instant,
) -> Result<(), OrchestratorError> {
    let by_name = artifacts
        .iter()
        .map(|artifact| (artifact.name.as_str(), artifact))
        .collect::<BTreeMap<_, _>>();
    let expected_by_name = expected
        .iter()
        .filter(|file| file.kind != ArtifactKind::Handoff)
        .map(|file| (file.name.as_str(), file))
        .collect::<BTreeMap<_, _>>();
    if by_name.len() != artifacts.len() || expected_by_name.len() != by_name.len() {
        return Err(invalid("staged_package_inventory_mismatch"));
    }
    let mut decompressed_total = 0u64;
    for file in expected {
        match file.kind {
            ArtifactKind::Payload => {
                let artifact = by_name
                    .get(file.name.as_str())
                    .ok_or_else(|| invalid("staged_payload_missing"))?;
                let (target, bins) = (file.target.as_deref().unwrap_or_default(), &file.bins);
                match file.payload_format {
                    Some(velnor_actions_contract::package_release::PackageReleasePayloadFormat::TarGz) => {
                        inspect_archive(
                            &artifact.path,
                            target,
                            bins,
                            &mut decompressed_total,
                            started,
                        )?;
                    }
                    None => return Err(invalid("payload_format_missing")),
                }
            }
            ArtifactKind::Sbom => {
                let artifact = by_name
                    .get(file.name.as_str())
                    .ok_or_else(|| invalid("staged_sbom_missing"))?;
                let archive_name = file
                    .archive_subject
                    .as_deref()
                    .ok_or_else(|| invalid("sbom_archive_binding_missing"))?;
                let archive = by_name
                    .get(archive_name)
                    .ok_or_else(|| invalid("sbom_archive_missing"))?;
                inspect_sbom(&artifact.path, archive_name, &archive.sha256, started)?;
            }
            ArtifactKind::Companion => {
                let artifact = by_name
                    .get(file.name.as_str())
                    .ok_or_else(|| invalid("staged_companion_missing"))?;
                inspect_optional_identity_envelope(
                    &artifact.path,
                    source_commit,
                    source_ref,
                    version,
                    started,
                )?;
            }
            ArtifactKind::Handoff => {}
        }
    }
    Ok(())
}

#[path = "package_release_artifacts_binary.rs"]
mod binary;

fn inspect_archive(
    path: &Path,
    target: &str,
    bins: &[String],
    decompressed_total: &mut u64,
    started: Instant,
) -> Result<(), OrchestratorError> {
    let file = File::open(path)
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))?;
    let compressed = BufReader::new(file);
    let gzip_reader = GzDecoder::new(compressed);
    let tar_reader = BufReader::new(gzip_reader);
    let mut archive = tar::Archive::new(tar_reader);
    archive.set_preserve_permissions(false);
    let entries = archive
        .entries()
        .map_err(|_| invalid("preview_tar_gzip_invalid"))?
        .raw(true);
    let expected = bins.iter().cloned().collect::<BTreeSet<_>>();
    let mut found = BTreeSet::new();
    let mut count = 0usize;
    for entry in entries {
        check_deadline(started)?;
        count += 1;
        if count > MAX_ENTRIES_PER_ARCHIVE {
            return Err(invalid("preview_tar_entry_limit"));
        }
        let entry = entry.map_err(|_| invalid("preview_tar_entry_invalid"))?;
        inspect_entry(
            entry,
            target,
            &expected,
            &mut found,
            decompressed_total,
            started,
        )?;
    }
    let mut tar_reader = archive.into_inner();
    gzip::drain_tar_terminator(&mut tar_reader, decompressed_total, started)?;
    let gzip_reader = tar_reader.into_inner();
    let mut compressed = gzip_reader.into_inner();
    let mut trailing = [0u8; 1];
    if compressed
        .read(&mut trailing)
        .map_err(|_| invalid("preview_gzip_trailing_read_failed"))?
        != 0
    {
        return Err(invalid("preview_gzip_trailing_bytes"));
    }
    if found != expected {
        return Err(invalid("preview_tar_executable_inventory_mismatch"));
    }
    Ok(())
}

fn inspect_entry<R: Read>(
    mut entry: tar::Entry<'_, R>,
    target: &str,
    expected: &BTreeSet<String>,
    found: &mut BTreeSet<String>,
    decompressed_total: &mut u64,
    started: Instant,
) -> Result<(), OrchestratorError> {
    if entry.header().entry_type() != EntryType::Regular {
        return Err(invalid("preview_tar_non_regular_entry"));
    }
    let raw_path = entry.path_bytes();
    let name =
        std::str::from_utf8(raw_path.as_ref()).map_err(|_| invalid("preview_tar_path_not_utf8"))?;
    if name.is_empty()
        || name.len() > 255
        || name.contains('/')
        || name.contains('\\')
        || name == "."
        || name == ".."
        || !expected.contains(name)
        || !found.insert(name.to_owned())
    {
        return Err(invalid("preview_tar_member_inventory_invalid"));
    }
    let mode = entry
        .header()
        .mode()
        .map_err(|_| invalid("preview_tar_mode_invalid"))?;
    if mode != 0o755 {
        return Err(invalid("preview_tar_executable_mode_invalid"));
    }
    let size = entry
        .header()
        .size()
        .map_err(|_| invalid("preview_tar_size_invalid"))?;
    if size == 0 || size > MAX_EXECUTABLE_BYTES {
        return Err(invalid("preview_tar_executable_size_invalid"));
    }
    let mut prefix = Vec::with_capacity(64);
    let mut read_total = 0u64;
    let mut buffer = vec![0u8; 64 * 1024].into_boxed_slice();
    loop {
        check_deadline(started)?;
        let read = entry
            .read(&mut buffer)
            .map_err(|_| invalid("preview_tar_member_read_failed"))?;
        if read == 0 {
            break;
        }
        read_total = read_total.saturating_add(read as u64);
        *decompressed_total = decompressed_total.saturating_add(read as u64);
        if prefix.len() < 64 {
            let take = (64 - prefix.len()).min(read);
            prefix.extend_from_slice(&buffer[..take]);
        }
        if read_total > size || *decompressed_total > MAX_TOTAL_DECOMPRESSED_BYTES {
            return Err(invalid("preview_tar_decompression_limit"));
        }
    }
    if read_total != size || !binary::is_expected_executable(&prefix, target) {
        return Err(invalid("preview_tar_executable_format_mismatch"));
    }
    Ok(())
}

fn inspect_sbom(
    path: &Path,
    archive_name: &str,
    archive_sha256: &str,
    started: Instant,
) -> Result<(), OrchestratorError> {
    let bytes = super::inventory::read_limited(path, super::MAX_SBOM_BYTES, started)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| invalid("preview_sbom_not_utf8"))?;
    let document = parse_strict_json(text).map_err(|_| invalid("preview_sbom_invalid_json"))?;
    let root = document
        .as_object()
        .ok_or_else(|| invalid("preview_sbom_root_invalid"))?;
    if root.get("bomFormat").and_then(Value::as_str) != Some("CycloneDX") {
        return Err(invalid("preview_sbom_format_invalid"));
    }
    let metadata = root
        .get("metadata")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("preview_sbom_metadata_invalid"))?;
    let component = metadata
        .get("component")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("preview_sbom_component_invalid"))?;
    if component.get("type").and_then(Value::as_str) != Some("file")
        || component.get("name").and_then(Value::as_str) != Some(archive_name)
    {
        return Err(invalid("preview_sbom_archive_name_mismatch"));
    }
    let hashes = component
        .get("hashes")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("preview_sbom_hashes_missing"))?;
    let matches = hashes
        .iter()
        .filter(|hash| {
            hash.get("alg").and_then(Value::as_str) == Some("SHA-256")
                && hash.get("content").and_then(Value::as_str) == Some(archive_sha256)
        })
        .count();
    if matches != 1 || hashes.len() != 1 {
        return Err(invalid("preview_sbom_archive_digest_mismatch"));
    }
    Ok(())
}

fn inspect_optional_identity_envelope(
    path: &Path,
    source_commit: &str,
    source_ref: &str,
    version: &str,
    started: Instant,
) -> Result<(), OrchestratorError> {
    if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
        return Ok(());
    }
    let metadata = std::fs::metadata(path)
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))?;
    if metadata.len() > super::MAX_SBOM_BYTES {
        return Ok(());
    }
    let bytes = super::inventory::read_limited(path, super::MAX_SBOM_BYTES, started)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| invalid("companion_json_not_utf8"))?;
    let Ok(value) = parse_strict_json(text) else {
        return Err(invalid("companion_json_invalid"));
    };
    let Some(object) = value.as_object() else {
        return Ok(());
    };
    for (field, expected) in [
        ("source_commit", source_commit),
        ("source_ref", source_ref),
        ("version", version),
    ] {
        if let Some(value) = object.get(field)
            && value.as_str() != Some(expected)
        {
            return Err(invalid("companion_identity_mismatch"));
        }
    }
    Ok(())
}

fn check_deadline(started: Instant) -> Result<(), OrchestratorError> {
    if velnor_actions_mise::CheckDeadline::check_scope().is_err()
        || started.elapsed() > super::VERIFY_TIMEOUT
    {
        return Err(invalid("package_output_verification_timeout"));
    }
    Ok(())
}
