//! Safe, bounded validation of one downloaded provider/task artifact.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Read as _;
use std::path::{Path, PathBuf};

use rustix::fs::{FileType, OFlags};
use velnor_actions_contract::{canonical::Blake3Accumulator, parse_strict_json};
use velnor_actions_contract_config::ArtifactBuildTask;
use velnor_actions_contract_workflow::{
    ARTIFACT_BUILD_OUTPUTS_DIRECTORY, ARTIFACT_BUILD_RESULT_FILENAME, ArtifactBuildExpectation,
    ArtifactBuildResult, DownloadedArtifactOutput,
};
use velnor_actions_orchestrator_core::staged_reads::read_staged_bytes;

#[path = "artifact_build_download/archive.rs"]
mod archive;
pub(super) use self::archive::extract_artifact_archive;
#[path = "artifact_build_download/zip_directory.rs"]
mod zip_directory;
#[cfg(test)]
use self::zip_directory::central_entry_count;

pub(super) const MAX_RESULT_BYTES: u64 = 1 << 20;

/// Validate the exact uploaded tree and hash each declared file through a bounded handle.
pub(super) fn inspect_download(
    expectation: &ArtifactBuildExpectation,
    task: &ArtifactBuildTask,
    root: &Path,
) -> Result<(ArtifactBuildResult, Vec<DownloadedArtifactOutput>), &'static str> {
    let result_path = root.join(ARTIFACT_BUILD_RESULT_FILENAME);
    require_exact_entries(
        root,
        &[
            ARTIFACT_BUILD_RESULT_FILENAME,
            ARTIFACT_BUILD_OUTPUTS_DIRECTORY,
        ],
    )?;
    let result_bytes = read_staged_bytes(&result_path, MAX_RESULT_BYTES)?;
    let result_text = std::str::from_utf8(&result_bytes).map_err(|_| "result_not_utf8")?;
    let result_json = parse_strict_json(result_text).map_err(|_| "result_malformed_json")?;
    let result: ArtifactBuildResult =
        serde_json::from_value(result_json).map_err(|_| "result_malformed_shape")?;
    result
        .validate_for(task, &expectation.identity)
        .map_err(|_| "result_identity_or_inventory_mismatch")?;

    let output_dir = root.join(ARTIFACT_BUILD_OUTPUTS_DIRECTORY);
    let expected_names: Vec<String> = expectation
        .outputs
        .iter()
        .map(|output| format!("{}.bin", output.id))
        .collect();
    let expected_refs: Vec<&str> = expected_names.iter().map(String::as_str).collect();
    require_exact_entries(&output_dir, &expected_refs)?;

    let mut downloaded = Vec::with_capacity(expectation.outputs.len());
    for output in &expectation.outputs {
        let path = output_dir.join(format!("{}.bin", output.id));
        let (size_bytes, digest) = digest_regular_file(&path, output.max_bytes)?;
        downloaded.push(DownloadedArtifactOutput {
            output_id: output.id.clone(),
            path: output.path.clone(),
            size_bytes,
            digest,
        });
    }
    Ok((result, downloaded))
}

/// Create a fresh destination. Reuse or symlinked paths are refused.
pub(super) fn create_download_dir(
    run_dir: &Path,
    artifact_name: &str,
) -> Result<PathBuf, &'static str> {
    require_directory(run_dir)?;
    let parent = run_dir.join("artifact-builds");
    match fs::create_dir(&parent) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            require_directory(&parent)?;
        }
        Err(_) => return Err("artifact_download_parent_unavailable"),
    }
    let destination = parent.join(artifact_name);
    fs::create_dir(&destination).map_err(|_| "artifact_download_destination_exists")?;
    Ok(destination)
}

fn require_exact_entries(directory: &Path, expected: &[&str]) -> Result<(), &'static str> {
    require_directory(directory)?;
    let expected_names: BTreeSet<&str> = expected.iter().copied().collect();
    if expected_names.len() != expected.len() {
        return Err("artifact_expected_inventory_duplicate");
    }
    let mut names = BTreeSet::new();
    for entry in fs::read_dir(directory).map_err(|_| "artifact_directory_unreadable")? {
        let entry = entry.map_err(|_| "artifact_directory_unreadable")?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "artifact_non_utf8_name")?;
        if !expected_names.contains(name.as_str()) || names.len() >= expected_names.len() {
            return Err("artifact_entry_inventory_mismatch");
        }
        let metadata =
            fs::symlink_metadata(entry.path()).map_err(|_| "artifact_entry_unreadable")?;
        if metadata.file_type().is_symlink() {
            return Err("artifact_symlink_rejected");
        }
        names.insert(name);
    }
    if names.len() != expected_names.len() {
        return Err("artifact_entry_inventory_mismatch");
    }
    Ok(())
}

fn require_directory(path: &Path) -> Result<(), &'static str> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "artifact_directory_missing")?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("artifact_directory_not_regular");
    }
    Ok(())
}

fn digest_regular_file(path: &Path, max_bytes: u64) -> Result<(u64, String), &'static str> {
    let fd = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| "artifact_output_open_failed")?;
    if rustix::fs::fstat(&fd)
        .map(|stat| FileType::from_raw_mode(stat.st_mode))
        .map_err(|_| "artifact_output_stat_failed")?
        != FileType::RegularFile
    {
        return Err("artifact_output_not_regular");
    }
    let mut file = File::from(fd);
    let mut digest = Blake3Accumulator::new();
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| "artifact_output_read_failed")?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(read).map_err(|_| "artifact_output_size_overflow")?)
            .ok_or("artifact_output_size_overflow")?;
        if total > max_bytes {
            return Err("artifact_output_oversize");
        }
        digest.update(&buffer[..read]);
    }
    if total == 0 {
        return Err("artifact_output_empty");
    }
    Ok((total, digest.finalize()))
}

#[cfg(test)]
#[path = "artifact_build_download/tests.rs"]
mod tests;
