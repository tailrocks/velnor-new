//! Archive inventory validation and bounded extraction.

use std::collections::BTreeSet;
use std::path::Path;

use super::{MAX_RESULT_BYTES, zip_directory::central_entry_count};
use velnor_actions_contract_config::ArtifactBuildTask;
use velnor_actions_contract_workflow::{
    ARTIFACT_BUILD_OUTPUTS_DIRECTORY, ARTIFACT_BUILD_RESULT_FILENAME, ArtifactBuildExpectation,
};
use velnor_actions_orchestrator_core::exclusive_write::{
    create_dir_no_symlink, write_exclusive_with,
};
use velnor_actions_orchestrator_core::internal;

/// One preflighted member in the exact expected artifact ZIP inventory.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ArchiveMember {
    index: usize,
    name: String,
    max_bytes: u64,
    declared_bytes: u64,
}

/// Validate every ZIP member and expanded-size declaration before creating any
/// extracted output. The stream is then re-read through per-member byte caps.
pub(crate) fn extract_artifact_archive<R: std::io::Read + std::io::Seek>(
    mut reader: R,
    archive_bytes: u64,
    max_archive_bytes: u64,
    expectation: &ArtifactBuildExpectation,
    task: &ArtifactBuildTask,
    destination: &Path,
) -> Result<(), &'static str> {
    if expectation.outputs != task.outputs {
        return Err("artifact_output_inventory_mismatch");
    }
    task.validate("plan").map_err(|_| "artifact_task_invalid")?;
    if archive_bytes == 0 || archive_bytes > max_archive_bytes {
        return Err("artifact_archive_size_invalid");
    }
    let expected = expected_archive_members(task);
    let max_expanded_bytes = expanded_archive_bound(&expected)?;
    let central_entries = central_entry_count(&mut reader, archive_bytes)?;
    let mut archive = zip::ZipArchive::new(reader).map_err(|_| "artifact_archive_invalid")?;
    validate_archive_inventory_count(central_entries, archive.len())?;
    let members = preflight_archive(&mut archive, &expected, max_expanded_bytes)?;
    extract_preflighted_archive(archive, &members, max_expanded_bytes, destination)
}

fn validate_archive_inventory_count(
    central_entries: u64,
    unique_entries: usize,
) -> Result<(), &'static str> {
    let unique_entries =
        u64::try_from(unique_entries).map_err(|_| "artifact_archive_inventory_mismatch")?;
    if central_entries > unique_entries {
        return Err("artifact_archive_duplicate_member");
    }
    if central_entries != unique_entries {
        return Err("artifact_archive_inventory_mismatch");
    }
    Ok(())
}

fn expected_archive_members(task: &ArtifactBuildTask) -> std::collections::BTreeMap<String, u64> {
    let mut expected = std::collections::BTreeMap::new();
    expected.insert(ARTIFACT_BUILD_RESULT_FILENAME.to_owned(), MAX_RESULT_BYTES);
    for output in &task.outputs {
        expected.insert(
            format!("{ARTIFACT_BUILD_OUTPUTS_DIRECTORY}/{}.bin", output.id),
            output.max_bytes,
        );
    }
    expected
}

fn expanded_archive_bound(
    expected: &std::collections::BTreeMap<String, u64>,
) -> Result<u64, &'static str> {
    expected
        .values()
        .try_fold(0_u64, |total, value| total.checked_add(*value))
        .ok_or("artifact_expanded_size_overflow")
}

fn preflight_archive<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    expected: &std::collections::BTreeMap<String, u64>,
    max_expanded_bytes: u64,
) -> Result<Vec<ArchiveMember>, &'static str> {
    if archive.len() < expected.len() || archive.len() > expected.len().saturating_add(1) {
        return Err("artifact_archive_inventory_mismatch");
    }
    let mut seen = BTreeSet::new();
    let mut members = Vec::with_capacity(expected.len());
    let mut output_directory_seen = false;
    let mut declared_expanded_bytes = 0_u64;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|_| "artifact_archive_member_invalid")?;
        if entry.encrypted() {
            return Err("artifact_archive_encrypted_member");
        }
        if entry.is_dir() {
            validate_output_directory(&entry, &mut output_directory_seen)?;
            continue;
        }
        members.push(preflight_archive_file(
            index,
            &entry,
            expected,
            &mut seen,
            &mut declared_expanded_bytes,
            max_expanded_bytes,
        )?);
    }
    if seen.len() != expected.len() {
        return Err("artifact_archive_inventory_mismatch");
    }
    Ok(members)
}

fn validate_output_directory<R: std::io::Read + ?Sized>(
    entry: &zip::read::ZipFile<'_, R>,
    output_directory_seen: &mut bool,
) -> Result<(), &'static str> {
    if entry.name() != "outputs/"
        || *output_directory_seen
        || entry.size() != 0
        || entry.compressed_size() != 0
        || entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170_000 != 0o040_000)
    {
        return Err("artifact_archive_directory_invalid");
    }
    *output_directory_seen = true;
    Ok(())
}

fn preflight_archive_file<R: std::io::Read + ?Sized>(
    index: usize,
    entry: &zip::read::ZipFile<'_, R>,
    expected: &std::collections::BTreeMap<String, u64>,
    seen: &mut BTreeSet<String>,
    declared_expanded_bytes: &mut u64,
    max_expanded_bytes: u64,
) -> Result<ArchiveMember, &'static str> {
    if entry.is_symlink()
        || entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170_000 != 0 && mode & 0o170_000 != 0o100_000)
    {
        return Err("artifact_archive_nonregular_member");
    }
    if !matches!(
        entry.compression(),
        zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated
    ) {
        return Err("artifact_archive_compression_unsupported");
    }
    let name = entry.name().to_owned();
    let max_bytes = *expected
        .get(&name)
        .ok_or("artifact_archive_inventory_mismatch")?;
    if !seen.insert(name.clone()) {
        return Err("artifact_archive_duplicate_member");
    }
    let declared_bytes = entry.size();
    if declared_bytes == 0 || declared_bytes > max_bytes {
        return Err("artifact_archive_member_size_invalid");
    }
    *declared_expanded_bytes = declared_expanded_bytes
        .checked_add(declared_bytes)
        .ok_or("artifact_expanded_size_overflow")?;
    if *declared_expanded_bytes > max_expanded_bytes {
        return Err("artifact_archive_expanded_size_exceeded");
    }
    Ok(ArchiveMember {
        index,
        name,
        max_bytes,
        declared_bytes,
    })
}

fn extract_preflighted_archive<R: std::io::Read + std::io::Seek>(
    mut archive: zip::ZipArchive<R>,
    members: &[ArchiveMember],
    max_expanded_bytes: u64,
    destination: &Path,
) -> Result<(), &'static str> {
    let outputs_dir = destination.join(ARTIFACT_BUILD_OUTPUTS_DIRECTORY);
    create_dir_no_symlink(destination, &outputs_dir)
        .map_err(|_| "artifact_output_directory_failed")?;
    let mut actual_expanded_bytes = 0_u64;
    for member in members {
        let copied = extract_archive_member(
            &mut archive,
            member,
            &mut actual_expanded_bytes,
            max_expanded_bytes,
            destination,
        )?;
        actual_expanded_bytes = actual_expanded_bytes
            .checked_add(copied)
            .ok_or("artifact_archive_size_overflow")?;
    }
    Ok(())
}

fn extract_archive_member<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    member: &ArchiveMember,
    actual_expanded_bytes: &mut u64,
    max_expanded_bytes: u64,
    destination: &Path,
) -> Result<u64, &'static str> {
    let mut entry = archive
        .by_index(member.index)
        .map_err(|_| "artifact_archive_member_invalid")?;
    let output_path = destination.join(&member.name);
    write_exclusive_with(&output_path, "artifact_download", |target| {
        copy_bounded_archive_member(
            &mut entry,
            target,
            member,
            actual_expanded_bytes,
            max_expanded_bytes,
        )
    })
    .map_err(|_| "artifact_archive_output_write_failed")
}

fn copy_bounded_archive_member(
    entry: &mut impl std::io::Read,
    target: &mut impl std::io::Write,
    member: &ArchiveMember,
    actual_expanded_bytes: &mut u64,
    max_expanded_bytes: u64,
) -> Result<u64, velnor_actions_orchestrator_core::OrchestratorError> {
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = entry
            .read(&mut buffer)
            .map_err(|_| internal("artifact_archive_member_read_failed"))?;
        if count == 0 {
            break;
        }
        let count = u64::try_from(count).map_err(|_| internal("artifact_archive_size_overflow"))?;
        let next = total
            .checked_add(count)
            .ok_or_else(|| internal("artifact_archive_size_overflow"))?;
        let expanded = actual_expanded_bytes
            .checked_add(next)
            .ok_or_else(|| internal("artifact_archive_size_overflow"))?;
        if next > member.max_bytes || expanded > max_expanded_bytes {
            return Err(internal("artifact_archive_expanded_size_exceeded"));
        }
        let count =
            usize::try_from(count).map_err(|_| internal("artifact_archive_size_overflow"))?;
        target
            .write_all(&buffer[..count])
            .map_err(|_| internal("artifact_download_output_write_failed"))?;
        total = next;
    }
    if total != member.declared_bytes {
        return Err(internal("artifact_archive_declared_size_mismatch"));
    }
    Ok(total)
}
