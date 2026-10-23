//! Exact file-set, streaming digest, and atomic local-input staging checks.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};
use velnor_actions_contract::strict_json::parse_strict_json_bytes;
use velnor_actions_contract::{
    GitHubEvidenceFetchSpecV1, GitHubEvidenceSnapshotV1, MAX_GITHUB_EVIDENCE_ARCHIVE_BYTES,
    MAX_GITHUB_EVIDENCE_BUNDLE_BYTES, MAX_GITHUB_EVIDENCE_DOCUMENT_BYTES, canonical_json_bytes,
};

use crate::OrchestratorError;
use crate::internal::internal;

fn document_byte_limit() -> Result<usize, OrchestratorError> {
    usize::try_from(MAX_GITHUB_EVIDENCE_DOCUMENT_BYTES)
        .map_err(|_| internal("github_evidence_document_bound"))
}

pub(super) fn verify_bundle(
    root: &Path,
    snapshot: &GitHubEvidenceSnapshotV1,
) -> Result<(), OrchestratorError> {
    if !root.is_absolute() || !real_directory(root) {
        return Err(internal("github_evidence_bundle_root"));
    }
    let expected = snapshot.expected_bundle_files();
    let actual = enumerate_files(root, &expected)?;
    if actual != expected {
        return Err(internal("github_evidence_bundle_file_set"));
    }
    let mut total = 0_u64;
    let mut unpacked_total = 0_u64;
    let archive_deadline =
        velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(600))
            .map_err(|_| internal("github_evidence_archive_deadline"))?;
    for document in &snapshot.documents {
        let path = safe_join(root, &document.path)?;
        verify_digest(
            &path,
            document.size_bytes,
            &document.sha256,
            MAX_GITHUB_EVIDENCE_DOCUMENT_BYTES,
        )?;
        let bytes = read_bounded(&path, document.size_bytes)?;
        parse_strict_json_bytes(&bytes, document_byte_limit()?)
            .map_err(|_| internal("github_evidence_document_json"))?;
        total = total.saturating_add(document.size_bytes);
    }
    for artifact in &snapshot.artifacts {
        let path = safe_join(root, &artifact.path)?;
        verify_digest(
            &path,
            artifact.size_bytes,
            &artifact.sha256,
            MAX_GITHUB_EVIDENCE_ARCHIVE_BYTES,
        )?;
        let mut file = File::open(&path).map_err(|_| internal("github_evidence_archive_read"))?;
        let unpacked = crate::package_release_publish::artifact_zip::validate_evidence_archive(
            &mut file,
            archive_deadline,
        )
        .map_err(|_| internal("github_evidence_archive_invalid"))?;
        unpacked_total = unpacked_total
            .checked_add(unpacked)
            .filter(|size| *size <= MAX_GITHUB_EVIDENCE_BUNDLE_BYTES)
            .ok_or_else(|| internal("github_evidence_archive_unpacked_total"))?;
        total = total.saturating_add(artifact.size_bytes);
    }
    let index_path = safe_join(root, "index.json")?;
    let index = read_bounded(&index_path, MAX_GITHUB_EVIDENCE_DOCUMENT_BYTES)?;
    if index != canonical_json_bytes(snapshot).map_err(|_| internal("github_evidence_index"))? {
        return Err(internal("github_evidence_index_noncanonical"));
    }
    total = total.saturating_add(index.len() as u64);
    if total > MAX_GITHUB_EVIDENCE_BUNDLE_BYTES {
        return Err(internal("github_evidence_bundle_too_large"));
    }
    Ok(())
}

pub(super) fn load_snapshot(
    root: &Path,
    spec: &GitHubEvidenceFetchSpecV1,
) -> Result<GitHubEvidenceSnapshotV1, OrchestratorError> {
    if !root.is_absolute() || !real_directory(root) {
        return Err(internal("github_evidence_bundle_root"));
    }
    let path = safe_join(root, "index.json")?;
    let bytes = read_bounded(&path, MAX_GITHUB_EVIDENCE_DOCUMENT_BYTES)?;
    let value = parse_strict_json_bytes(&bytes, document_byte_limit()?)
        .map_err(|_| internal("github_evidence_index_json"))?;
    let snapshot: GitHubEvidenceSnapshotV1 =
        serde_json::from_value(value).map_err(|_| internal("github_evidence_index_shape"))?;
    if canonical_json_bytes(&snapshot).map_err(|_| internal("github_evidence_index"))? != bytes {
        return Err(internal("github_evidence_index_noncanonical"));
    }
    snapshot
        .validate_for_spec(spec)
        .map_err(|_| internal("github_evidence_snapshot_invalid"))?;
    Ok(snapshot)
}

pub(super) fn stage_bundle(
    root: &Path,
    workspace: &Path,
    snapshot: &GitHubEvidenceSnapshotV1,
) -> Result<(), OrchestratorError> {
    if !workspace.is_absolute() || !real_directory(workspace) {
        return Err(internal("github_evidence_workspace"));
    }
    let velnor = workspace.join(".velnor");
    if !real_directory(&velnor) {
        return Err(internal("github_evidence_config_directory"));
    }
    let destination = velnor.join("evidence");
    let staging = velnor.join("evidence.importing");
    if fs::symlink_metadata(&destination).is_ok() || fs::symlink_metadata(&staging).is_ok() {
        return Err(internal("github_evidence_destination_exists"));
    }
    fs::create_dir(&staging).map_err(|_| internal("github_evidence_stage_create"))?;
    let mut guard = StageGuard {
        path: staging.clone(),
        keep: false,
    };
    for relative in snapshot.expected_bundle_files() {
        let source = safe_join(root, &relative)?;
        let target = safe_join(&staging, &relative)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|_| internal("github_evidence_stage_create"))?;
        }
        let size_limit = file_size(snapshot, &relative)?;
        copy_bounded(&source, &target, size_limit)?;
    }
    verify_bundle(&staging, snapshot)?;
    fs::rename(&staging, &destination).map_err(|_| internal("github_evidence_stage_publish"))?;
    guard.keep = true;
    Ok(())
}

fn file_size(
    snapshot: &GitHubEvidenceSnapshotV1,
    relative: &str,
) -> Result<u64, OrchestratorError> {
    if relative == "index.json" {
        return u64::try_from(
            canonical_json_bytes(snapshot)
                .map_err(|_| internal("github_evidence_index"))?
                .len(),
        )
        .map_err(|_| internal("github_evidence_index"));
    }
    snapshot
        .documents
        .iter()
        .find(|document| document.path == relative)
        .map(|document| document.size_bytes)
        .or_else(|| {
            snapshot
                .artifacts
                .iter()
                .find(|artifact| artifact.path == relative)
                .map(|artifact| artifact.size_bytes)
        })
        .ok_or_else(|| internal("github_evidence_file_unexpected"))
}

fn enumerate_files(
    root: &Path,
    expected: &BTreeSet<String>,
) -> Result<BTreeSet<String>, OrchestratorError> {
    let mut files = BTreeSet::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in
            fs::read_dir(&directory).map_err(|_| internal("github_evidence_bundle_read"))?
        {
            let entry = entry.map_err(|_| internal("github_evidence_bundle_read"))?;
            let path = entry.path();
            let metadata =
                fs::symlink_metadata(&path).map_err(|_| internal("github_evidence_bundle_read"))?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| internal("github_evidence_bundle_path"))?;
            let text = relative
                .to_str()
                .ok_or_else(|| internal("github_evidence_bundle_path"))?
                .replace(std::path::MAIN_SEPARATOR, "/");
            if !safe_relative(&text) {
                return Err(internal("github_evidence_bundle_path"));
            }
            if metadata.file_type().is_dir() {
                if !expected
                    .iter()
                    .any(|file| file.starts_with(&format!("{text}/")))
                {
                    return Err(internal("github_evidence_bundle_extra_directory"));
                }
                stack.push(path);
            } else if metadata.file_type().is_file() {
                if !files.insert(text) {
                    return Err(internal("github_evidence_bundle_duplicate_path"));
                }
            } else {
                return Err(internal("github_evidence_bundle_special_file"));
            }
        }
    }
    Ok(files)
}

fn verify_digest(
    path: &Path,
    expected_size: u64,
    expected_digest: &str,
    maximum: u64,
) -> Result<(), OrchestratorError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| internal("github_evidence_file_missing"))?;
    if !metadata.file_type().is_file()
        || expected_size == 0
        || expected_size > maximum
        || metadata.len() != expected_size
    {
        return Err(internal("github_evidence_file_size"));
    }
    let mut file = File::open(path).map_err(|_| internal("github_evidence_file_read"))?;
    let mut hash = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| internal("github_evidence_file_read"))?;
        if count == 0 {
            break;
        }
        size = size.saturating_add(count as u64);
        if size > maximum {
            return Err(internal("github_evidence_file_size"));
        }
        hash.update(&buffer[..count]);
    }
    if size != expected_size || hex_lower(&hash.finalize()) != expected_digest {
        return Err(internal("github_evidence_file_digest"));
    }
    Ok(())
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, OrchestratorError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| internal("github_evidence_file_missing"))?;
    if !metadata.file_type().is_file() || metadata.len() > limit {
        return Err(internal("github_evidence_file_size"));
    }
    let file = File::open(path).map_err(|_| internal("github_evidence_file_read"))?;
    let capacity =
        usize::try_from(metadata.len()).map_err(|_| internal("github_evidence_file_size"))?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| internal("github_evidence_file_read"))?;
    if bytes.len() as u64 != metadata.len() || bytes.len() as u64 > limit {
        return Err(internal("github_evidence_file_size"));
    }
    Ok(bytes)
}

fn safe_join(root: &Path, relative: &str) -> Result<PathBuf, OrchestratorError> {
    if !safe_relative(relative) {
        return Err(internal("github_evidence_bundle_path"));
    }
    Ok(root.join(relative))
}

fn safe_relative(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('\\')
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_dir())
}

fn copy_bounded(source: &Path, target: &Path, expected_size: u64) -> Result<(), OrchestratorError> {
    let metadata =
        fs::symlink_metadata(source).map_err(|_| internal("github_evidence_file_missing"))?;
    if !metadata.file_type().is_file() || metadata.len() != expected_size {
        return Err(internal("github_evidence_file_size"));
    }
    let mut input = File::open(source).map_err(|_| internal("github_evidence_file_read"))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|_| internal("github_evidence_stage_write"))?;
    let mut copied = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = input
            .read(&mut buffer)
            .map_err(|_| internal("github_evidence_file_read"))?;
        if count == 0 {
            break;
        }
        copied = copied.saturating_add(count as u64);
        if copied > expected_size {
            return Err(internal("github_evidence_file_size"));
        }
        output
            .write_all(&buffer[..count])
            .map_err(|_| internal("github_evidence_stage_write"))?;
    }
    if copied != expected_size {
        return Err(internal("github_evidence_file_size"));
    }
    output
        .sync_all()
        .map_err(|_| internal("github_evidence_stage_write"))
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        result.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
    }
    result
}

struct StageGuard {
    path: PathBuf,
    keep: bool,
}

impl Drop for StageGuard {
    fn drop(&mut self) {
        if !self.keep {
            drop(fs::remove_dir_all(&self.path));
        }
    }
}
