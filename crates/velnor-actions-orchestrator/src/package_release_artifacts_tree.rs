//! Exact regular-file tree admission for untrusted producer directories.

use std::{collections::BTreeSet, fs, path::Path};

use super::super::{MAX_FILE_COUNT, MAX_PATH_BYTES};
use super::{ExpectedFile, invalid};
use crate::OrchestratorError;

/// Reject any missing, additional, linked, or non-regular producer output.
pub(crate) fn validate_source_tree(
    root: &Path,
    expected: &[ExpectedFile],
) -> Result<(), OrchestratorError> {
    let metadata = fs::symlink_metadata(root)
        .map_err(|error| OrchestratorError::io(root.display().to_string(), error.to_string()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(invalid("producer_output_root_not_regular_directory"));
    }
    let expected_paths = expected
        .iter()
        .map(|file| file.path.as_str())
        .collect::<BTreeSet<_>>();
    let expected_dirs = expected
        .iter()
        .flat_map(|file| parent_paths(&file.path))
        .collect::<BTreeSet<_>>();
    let mut found = Vec::new();
    let mut count = 0usize;
    scan_tree(
        root,
        root,
        &expected_paths,
        &expected_dirs,
        &mut found,
        &mut count,
    )?;
    found.sort();
    let mut expected_sorted = expected_paths.into_iter().collect::<Vec<_>>();
    expected_sorted.sort_unstable();
    if found.iter().map(String::as_str).collect::<Vec<_>>() != expected_sorted {
        return Err(invalid("producer_output_file_set_mismatch"));
    }
    Ok(())
}

fn scan_tree(
    root: &Path,
    directory: &Path,
    expected_files: &BTreeSet<&str>,
    expected_dirs: &BTreeSet<String>,
    found: &mut Vec<String>,
    count: &mut usize,
) -> Result<(), OrchestratorError> {
    let mut entries = fs::read_dir(directory)
        .map_err(|error| OrchestratorError::io(directory.display().to_string(), error.to_string()))?
        .take(MAX_FILE_COUNT.saturating_sub(*count).saturating_add(1))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            OrchestratorError::io(directory.display().to_string(), error.to_string())
        })?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        velnor_actions_mise::CheckDeadline::check_scope()
            .map_err(|_| invalid("package_output_verification_timeout"))?;
        *count = count.saturating_add(1);
        if *count > MAX_FILE_COUNT {
            return Err(invalid("producer_output_entry_limit"));
        }
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| invalid("producer_output_path_escape"))?;
        let relative = normalized_relative(relative)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            OrchestratorError::io(path.display().to_string(), error.to_string())
        })?;
        if metadata.file_type().is_symlink() {
            return Err(invalid("producer_output_symlink"));
        }
        if metadata.is_dir() {
            if !expected_dirs.contains(&relative) {
                return Err(invalid("producer_output_unexpected_directory"));
            }
            scan_tree(root, &path, expected_files, expected_dirs, found, count)?;
        } else if metadata.is_file() {
            if !expected_files.contains(relative.as_str()) {
                return Err(invalid("producer_output_unexpected_file"));
            }
            found.push(relative);
        } else {
            return Err(invalid("producer_output_non_regular_file"));
        }
    }
    Ok(())
}

fn parent_paths(path: &str) -> Vec<String> {
    Path::new(path)
        .ancestors()
        .skip(1)
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| {
            parent
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/")
        })
        .collect()
}

fn normalized_relative(path: &Path) -> Result<String, OrchestratorError> {
    let text = path
        .to_str()
        .ok_or_else(|| invalid("producer_output_path_not_utf8"))?
        .replace(std::path::MAIN_SEPARATOR, "/");
    if text.is_empty() || text.len() > MAX_PATH_BYTES || path.components().count() > 64 {
        return Err(invalid("producer_output_path_invalid"));
    }
    Ok(text)
}
