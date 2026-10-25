use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufReader, Cursor, Read},
    path::Path,
    time::Instant,
};

use flate2::bufread::GzDecoder;
use tar::EntryType;

use super::{
    ArtifactKind, ExpectedFile, MAX_TOTAL_DECOMPRESSED_BYTES, VerifiedPackageArtifact, invalid,
};
use crate::OrchestratorError;
#[path = "package_release_artifacts_content.rs"]
mod content;
use content::{check_deadline, inspect_optional_identity_envelope, inspect_sbom};

#[path = "package_release_artifacts_gzip.rs"]
mod gzip;

const MAX_ENTRIES_PER_ARCHIVE: usize = 4096;
const MAX_EXECUTABLE_BYTES: u64 = 256 * 1024 * 1024;

pub(crate) fn extract_tar_gz_members(
    bytes: &[u8],
    expected_members: &BTreeSet<String>,
    started: Instant,
) -> Result<BTreeMap<String, Vec<u8>>, OrchestratorError> {
    if expected_members.is_empty()
        || bytes.is_empty()
        || bytes.len() as u64 > super::MAX_PAYLOAD_BYTES
    {
        return Err(invalid("preview_tar_extract_input_invalid"));
    }
    let compressed = Cursor::new(bytes);
    let gzip_reader = GzDecoder::new(compressed);
    let mut archive = tar::Archive::new(gzip_reader);
    archive.set_preserve_permissions(false);
    let entries = archive
        .entries()
        .map_err(|_| invalid("preview_tar_gzip_invalid"))?
        .raw(true);
    let mut found = BTreeMap::new();
    let mut decompressed_total = 0u64;
    for entry in entries {
        check_deadline(started)?;
        if found.len() >= MAX_ENTRIES_PER_ARCHIVE {
            return Err(invalid("preview_tar_entry_limit"));
        }
        let entry = entry.map_err(|_| invalid("preview_tar_entry_invalid"))?;
        if entry.header().entry_type() != EntryType::Regular {
            return Err(invalid("preview_tar_non_regular_entry"));
        }
        let raw_path = entry.path_bytes();
        let name = std::str::from_utf8(raw_path.as_ref())
            .map_err(|_| invalid("preview_tar_path_not_utf8"))?;
        if name.is_empty()
            || name.len() > 255
            || name.contains('/')
            || name.contains('\\')
            || name == "."
            || name == ".."
            || !expected_members.contains(name)
            || found.contains_key(name)
        {
            return Err(invalid("preview_tar_member_inventory_invalid"));
        }
        let name = name.to_owned();
        let size = entry
            .header()
            .size()
            .map_err(|_| invalid("preview_tar_size_invalid"))?;
        if size == 0 || size > MAX_EXECUTABLE_BYTES {
            return Err(invalid("preview_tar_member_size_invalid"));
        }
        let size_usize =
            usize::try_from(size).map_err(|_| invalid("preview_tar_member_size_invalid"))?;
        let mut member = Vec::with_capacity(size_usize);
        entry
            .take(size.saturating_add(1))
            .read_to_end(&mut member)
            .map_err(|_| invalid("preview_tar_member_read_failed"))?;
        if member.len() as u64 != size {
            return Err(invalid("preview_tar_member_size_mismatch"));
        }
        decompressed_total = decompressed_total.saturating_add(size);
        if decompressed_total > MAX_TOTAL_DECOMPRESSED_BYTES {
            return Err(invalid("preview_tar_decompression_limit"));
        }
        found.insert(name, member);
    }
    let mut tar_reader = archive.into_inner();
    gzip::drain_tar_terminator(&mut tar_reader, &mut decompressed_total, started)?;
    let mut compressed = tar_reader.into_inner();
    let mut trailing = [0u8; 1];
    if compressed
        .read(&mut trailing)
        .map_err(|_| invalid("preview_gzip_trailing_read_failed"))?
        != 0
    {
        return Err(invalid("preview_gzip_trailing_bytes"));
    }
    if found.keys().cloned().collect::<BTreeSet<_>>() != *expected_members {
        return Err(invalid("preview_tar_member_inventory_mismatch"));
    }
    Ok(found)
}

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
