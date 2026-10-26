// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::Duration;

use velnor_actions_contract::strict_json::parse_strict_json_bytes;
use velnor_actions_contract::{
    GitHubEvidenceFetchSpecV1, GitHubEvidenceSnapshotV1, MAX_GITHUB_EVIDENCE_ARCHIVE_BYTES,
    MAX_GITHUB_EVIDENCE_ARCHIVES, MAX_GITHUB_EVIDENCE_BUNDLE_BYTES,
    MAX_GITHUB_EVIDENCE_DOCUMENT_BYTES, MAX_GITHUB_EVIDENCE_DOCUMENTS, canonical_json_bytes,
};
use velnor_actions_mise::CheckDeadline;

use crate::OrchestratorError;
use crate::internal::internal;

#[path = "github_evidence_import_files_inventory.rs"]
mod inventory;
#[path = "github_evidence_import_files_io.rs"]
mod io;

use self::inventory::{
    allowed_directory_prefixes, enumerate_files, expected_bundle_files, real_directory, safe_join,
};
use self::io::{copy_bounded, read_bounded, verify_digest};

const IMPORT_TIMEOUT: Duration = Duration::from_secs(600);
const MAX_EXPECTED_BUNDLE_FILES: usize =
    1 + MAX_GITHUB_EVIDENCE_DOCUMENTS + MAX_GITHUB_EVIDENCE_ARCHIVES;
const MAX_EXPECTED_DIRECTORY_PREFIXES: usize = MAX_EXPECTED_BUNDLE_FILES * 4;
const MAX_BUNDLE_PATH_BYTES: usize = 4_096;

pub(super) fn new_deadline() -> Result<CheckDeadline, OrchestratorError> {
    CheckDeadline::after(IMPORT_TIMEOUT).map_err(|_| internal("github_evidence_import_deadline"))
}

pub(super) fn check_deadline(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    deadline
        .remaining()
        .map(|_| ())
        .map_err(|_| internal("github_evidence_import_deadline"))
}

fn document_byte_limit() -> Result<usize, OrchestratorError> {
    usize::try_from(MAX_GITHUB_EVIDENCE_DOCUMENT_BYTES)
        .map_err(|_| internal("github_evidence_document_bound"))
}

fn verify_bundle_with_inventory(
    root: &Path,
    snapshot: &GitHubEvidenceSnapshotV1,
    expected: &BTreeSet<String>,
    directories: &BTreeSet<&str>,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    check_deadline(deadline)?;
    if !root.is_absolute() || !real_directory(root) {
        return Err(internal("github_evidence_bundle_root"));
    }
    check_deadline(deadline)?;
    let actual = enumerate_files(root, expected, directories, deadline)?;
    if &actual != expected {
        return Err(internal("github_evidence_bundle_file_set"));
    }
    let mut total = 0_u64;
    let mut unpacked_total = 0_u64;
    for document in &snapshot.documents {
        check_deadline(deadline)?;
        let path = safe_join(root, &document.path)?;
        verify_digest(
            &path,
            document.size_bytes,
            &document.sha256,
            MAX_GITHUB_EVIDENCE_DOCUMENT_BYTES,
            deadline,
        )?;
        let bytes = read_bounded(&path, document.size_bytes, deadline)?;
        parse_strict_json_bytes(&bytes, document_byte_limit()?)
            .map_err(|_| internal("github_evidence_document_json"))?;
        check_deadline(deadline)?;
        total = total
            .checked_add(document.size_bytes)
            .filter(|size| *size <= MAX_GITHUB_EVIDENCE_BUNDLE_BYTES)
            .ok_or_else(|| internal("github_evidence_bundle_too_large"))?;
    }
    for artifact in &snapshot.artifacts {
        check_deadline(deadline)?;
        let path = safe_join(root, &artifact.path)?;
        verify_digest(
            &path,
            artifact.size_bytes,
            &artifact.sha256,
            MAX_GITHUB_EVIDENCE_ARCHIVE_BYTES,
            deadline,
        )?;
        check_deadline(deadline)?;
        let mut file = File::open(&path).map_err(|_| internal("github_evidence_archive_read"))?;
        check_deadline(deadline)?;
        let unpacked_result =
            crate::package_release_publish::artifact_zip::validate_evidence_archive(
                &mut file, deadline,
            );
        check_deadline(deadline)?;
        let unpacked = unpacked_result.map_err(|_| internal("github_evidence_archive_invalid"))?;
        unpacked_total = unpacked_total
            .checked_add(unpacked)
            .filter(|size| *size <= MAX_GITHUB_EVIDENCE_BUNDLE_BYTES)
            .ok_or_else(|| internal("github_evidence_archive_unpacked_total"))?;
        total = total
            .checked_add(artifact.size_bytes)
            .filter(|size| *size <= MAX_GITHUB_EVIDENCE_BUNDLE_BYTES)
            .ok_or_else(|| internal("github_evidence_bundle_too_large"))?;
    }
    check_deadline(deadline)?;
    let index_path = safe_join(root, "index.json")?;
    let index = read_bounded(&index_path, MAX_GITHUB_EVIDENCE_DOCUMENT_BYTES, deadline)?;
    check_deadline(deadline)?;
    let canonical =
        canonical_json_bytes(snapshot).map_err(|_| internal("github_evidence_index"))?;
    check_deadline(deadline)?;
    if index != canonical {
        return Err(internal("github_evidence_index_noncanonical"));
    }
    total = total
        .checked_add(index.len() as u64)
        .filter(|size| *size <= MAX_GITHUB_EVIDENCE_BUNDLE_BYTES)
        .ok_or_else(|| internal("github_evidence_bundle_too_large"))?;
    if total > MAX_GITHUB_EVIDENCE_BUNDLE_BYTES {
        return Err(internal("github_evidence_bundle_too_large"));
    }
    check_deadline(deadline)?;
    Ok(())
}

pub(super) fn load_snapshot(
    root: &Path,
    spec: &GitHubEvidenceFetchSpecV1,
    deadline: CheckDeadline,
) -> Result<GitHubEvidenceSnapshotV1, OrchestratorError> {
    check_deadline(deadline)?;
    if !root.is_absolute() || !real_directory(root) {
        return Err(internal("github_evidence_bundle_root"));
    }
    check_deadline(deadline)?;
    let path = safe_join(root, "index.json")?;
    let bytes = read_bounded(&path, MAX_GITHUB_EVIDENCE_DOCUMENT_BYTES, deadline)?;
    check_deadline(deadline)?;
    let value = parse_strict_json_bytes(&bytes, document_byte_limit()?)
        .map_err(|_| internal("github_evidence_index_json"))?;
    check_deadline(deadline)?;
    let snapshot: GitHubEvidenceSnapshotV1 =
        serde_json::from_value(value).map_err(|_| internal("github_evidence_index_shape"))?;
    check_deadline(deadline)?;
    let canonical =
        canonical_json_bytes(&snapshot).map_err(|_| internal("github_evidence_index"))?;
    check_deadline(deadline)?;
    if canonical != bytes {
        return Err(internal("github_evidence_index_noncanonical"));
    }
    snapshot
        .validate_for_spec(spec)
        .map_err(|_| internal("github_evidence_snapshot_invalid"))?;
    check_deadline(deadline)?;
    Ok(snapshot)
}

pub(super) fn stage_bundle(
    root: &Path,
    workspace: &Path,
    snapshot: &GitHubEvidenceSnapshotV1,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    stage_bundle_inner(root, workspace, snapshot, deadline, || {})
}

fn stage_bundle_inner(
    root: &Path,
    workspace: &Path,
    snapshot: &GitHubEvidenceSnapshotV1,
    deadline: CheckDeadline,
    after_create: impl FnOnce(),
) -> Result<(), OrchestratorError> {
    let expected = expected_bundle_files(snapshot, deadline)?;
    let directories = allowed_directory_prefixes(&expected, deadline)?;
    verify_bundle_with_inventory(root, snapshot, &expected, &directories, deadline)?;
    check_deadline(deadline)?;
    if !workspace.is_absolute() || !real_directory(workspace) {
        return Err(internal("github_evidence_workspace"));
    }
    check_deadline(deadline)?;
    let velnor = workspace.join(".velnor");
    if !real_directory(&velnor) {
        return Err(internal("github_evidence_config_directory"));
    }
    check_deadline(deadline)?;
    let destination = velnor.join("evidence");
    let staging = velnor.join("evidence.importing");
    if fs::symlink_metadata(&destination).is_ok() || fs::symlink_metadata(&staging).is_ok() {
        return Err(internal("github_evidence_destination_exists"));
    }
    check_deadline(deadline)?;
    fs::create_dir(&staging).map_err(|_| internal("github_evidence_stage_create"))?;
    let mut guard = StageGuard {
        path: staging.clone(),
        keep: false,
    };
    after_create();
    check_deadline(deadline)?;
    for relative in &expected {
        check_deadline(deadline)?;
        let source = safe_join(root, relative)?;
        let target = safe_join(&staging, relative)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|_| internal("github_evidence_stage_create"))?;
        }
        let size_limit = file_size(snapshot, relative, deadline)?;
        copy_bounded(&source, &target, size_limit, deadline)?;
    }
    verify_bundle_with_inventory(&staging, snapshot, &expected, &directories, deadline)?;
    check_deadline(deadline)?;
    fs::rename(&staging, &destination).map_err(|_| internal("github_evidence_stage_publish"))?;
    guard.keep = true;
    Ok(())
}

fn file_size(
    snapshot: &GitHubEvidenceSnapshotV1,
    relative: &str,
    deadline: CheckDeadline,
) -> Result<u64, OrchestratorError> {
    if relative == "index.json" {
        check_deadline(deadline)?;
        let length = canonical_json_bytes(snapshot)
            .map_err(|_| internal("github_evidence_index"))?
            .len();
        check_deadline(deadline)?;
        return u64::try_from(length).map_err(|_| internal("github_evidence_index"));
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
