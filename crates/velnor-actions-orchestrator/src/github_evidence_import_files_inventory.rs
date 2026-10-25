use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use velnor_actions_contract::GitHubEvidenceSnapshotV1;

use super::{
    MAX_BUNDLE_PATH_BYTES, MAX_EXPECTED_BUNDLE_FILES, MAX_EXPECTED_DIRECTORY_PREFIXES,
    check_deadline, document_byte_limit, internal,
};
use crate::OrchestratorError;

pub(super) fn expected_bundle_files(
    snapshot: &GitHubEvidenceSnapshotV1,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<BTreeSet<String>, OrchestratorError> {
    check_deadline(deadline)?;
    if snapshot.documents.len() > velnor_actions_contract::MAX_GITHUB_EVIDENCE_DOCUMENTS
        || snapshot.artifacts.len() > velnor_actions_contract::MAX_GITHUB_EVIDENCE_ARCHIVES
    {
        return Err(internal("github_evidence_bundle_file_count"));
    }
    let expected_count = snapshot
        .documents
        .len()
        .checked_add(snapshot.artifacts.len())
        .and_then(|count| count.checked_add(1))
        .filter(|count| *count <= MAX_EXPECTED_BUNDLE_FILES)
        .ok_or_else(|| internal("github_evidence_bundle_file_count"))?;

    let mut path_bytes = "index.json".len();
    for document in &snapshot.documents {
        check_deadline(deadline)?;
        path_bytes = path_bytes
            .checked_add(document.path.len())
            .ok_or_else(|| internal("github_evidence_bundle_path_bound"))?;
    }
    for artifact in &snapshot.artifacts {
        check_deadline(deadline)?;
        path_bytes = path_bytes
            .checked_add(artifact.path.len())
            .ok_or_else(|| internal("github_evidence_bundle_path_bound"))?;
    }
    if path_bytes > document_byte_limit()? {
        return Err(internal("github_evidence_bundle_path_bound"));
    }

    let expected = snapshot.expected_bundle_files();
    if expected.len() != expected_count {
        return Err(internal("github_evidence_bundle_file_set"));
    }
    for path in &expected {
        check_deadline(deadline)?;
        if path.len() > MAX_BUNDLE_PATH_BYTES || !safe_relative(path) {
            return Err(internal("github_evidence_bundle_path_bound"));
        }
    }
    Ok(expected)
}

pub(super) fn allowed_directory_prefixes(
    expected: &BTreeSet<String>,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<BTreeSet<&str>, OrchestratorError> {
    let mut directories = BTreeSet::new();
    for file in expected {
        check_deadline(deadline)?;
        for (end, _) in file.match_indices('/') {
            check_deadline(deadline)?;
            let prefix = &file[..end];
            if !directories.contains(prefix) && directories.len() >= MAX_EXPECTED_DIRECTORY_PREFIXES
            {
                return Err(internal("github_evidence_bundle_directory_bound"));
            }
            directories.insert(prefix);
        }
    }
    Ok(directories)
}

pub(super) fn enumerate_files(
    root: &Path,
    expected: &BTreeSet<String>,
    allowed_directories: &BTreeSet<&str>,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<BTreeSet<String>, OrchestratorError> {
    let mut files = BTreeSet::new();
    let mut stack = vec![root.to_path_buf()];
    let mut directory_count = 0_usize;
    while let Some(directory) = stack.pop() {
        check_deadline(deadline)?;
        let entries =
            fs::read_dir(&directory).map_err(|_| internal("github_evidence_bundle_read"))?;
        check_deadline(deadline)?;
        for entry in entries {
            check_deadline(deadline)?;
            let entry = entry.map_err(|_| internal("github_evidence_bundle_read"))?;
            let path = entry.path();
            let metadata =
                fs::symlink_metadata(&path).map_err(|_| internal("github_evidence_bundle_read"))?;
            check_deadline(deadline)?;
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
                if !allowed_directories.contains(text.as_str()) {
                    return Err(internal("github_evidence_bundle_extra_directory"));
                }
                if directory_count >= allowed_directories.len() {
                    return Err(internal("github_evidence_bundle_directory_bound"));
                }
                directory_count += 1;
                stack.push(path);
            } else if metadata.file_type().is_file() {
                if !expected.contains(&text) {
                    return Err(internal("github_evidence_bundle_extra_file"));
                }
                if files.contains(&text) {
                    return Err(internal("github_evidence_bundle_duplicate_path"));
                }
                if files.len() >= expected.len() {
                    return Err(internal("github_evidence_bundle_extra_file"));
                }
                if !files.insert(text) {
                    return Err(internal("github_evidence_bundle_duplicate_path"));
                }
            } else {
                return Err(internal("github_evidence_bundle_special_file"));
            }
        }
    }
    check_deadline(deadline)?;
    Ok(files)
}

pub(super) fn safe_join(root: &Path, relative: &str) -> Result<PathBuf, OrchestratorError> {
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

pub(super) fn real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_dir())
}
