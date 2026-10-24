//! Captured tap checkout snapshots and formula-only delta enforcement.

use super::{FORMULA_LIMIT, SNAPSHOT_BYTES_LIMIT, SNAPSHOT_FILE_LIMIT, TREE_LIMIT};
use crate::OrchestratorError;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SnapshotEntry {
    pub(crate) mode: u32,
    pub(crate) sha256: String,
    pub(crate) size: u64,
    pub(crate) directory: bool,
}

pub(crate) type Snapshot = BTreeMap<String, SnapshotEntry>;

pub(crate) fn snapshot_tree(root: &Path) -> Result<Snapshot, OrchestratorError> {
    let mut result = BTreeMap::new();
    let mut pending = vec![PathBuf::new()];
    let mut total = 0_u64;
    while let Some(relative_dir) = pending.pop() {
        let directory = root.join(&relative_dir);
        let entries = fs::read_dir(&directory).map_err(|_| internal("tap_snapshot_read"))?;
        for entry in entries {
            let entry = entry.map_err(|_| internal("tap_snapshot_read"))?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| internal("tap_snapshot_path_encoding"))?;
            let relative = relative_dir.join(&name);
            let key = relative
                .to_str()
                .filter(|path| safe_relative_path(path))
                .ok_or_else(|| internal("tap_snapshot_path_invalid"))?
                .to_owned();
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|_| internal("tap_snapshot_metadata"))?;
            if metadata.file_type().is_symlink() {
                return Err(internal("tap_snapshot_symlink"));
            }
            #[cfg(unix)]
            let mode = {
                use std::os::unix::fs::PermissionsExt;
                metadata.permissions().mode() & 0o777
            };
            #[cfg(not(unix))]
            let mode = if metadata.permissions().readonly() {
                0o444
            } else {
                0o644
            };
            if metadata.is_dir() {
                if result.len() >= TREE_LIMIT {
                    return Err(internal("tap_snapshot_too_many_paths"));
                }
                pending.push(relative);
                result.insert(
                    key,
                    SnapshotEntry {
                        mode,
                        sha256: String::new(),
                        size: 0,
                        directory: true,
                    },
                );
                continue;
            }
            if !metadata.is_file() || metadata.len() > SNAPSHOT_FILE_LIMIT {
                return Err(internal("tap_snapshot_nonregular_or_large"));
            }
            total = total.saturating_add(metadata.len());
            if total > SNAPSHOT_BYTES_LIMIT || result.len() >= TREE_LIMIT {
                return Err(internal("tap_snapshot_size_or_count"));
            }
            result.insert(
                key,
                SnapshotEntry {
                    mode,
                    sha256: hash_file(&entry.path())?,
                    size: metadata.len(),
                    directory: false,
                },
            );
        }
    }
    Ok(result)
}

pub(crate) fn validate_tree_snapshot(
    tree: &[velnor_actions_mise::TapTreeEntry],
    snapshot: &Snapshot,
) -> Result<(), OrchestratorError> {
    let actual: Vec<_> = snapshot
        .iter()
        .filter(|(path, entry)| !path.starts_with(".git/") && !entry.directory)
        .map(|(path, entry)| (path.as_str(), entry.mode))
        .collect();
    let expected: Vec<_> = tree
        .iter()
        .map(|entry| {
            (
                entry.path.as_str(),
                if entry.mode == 0o100_755 {
                    0o755
                } else {
                    0o644
                },
            )
        })
        .collect();
    if actual != expected {
        return Err(internal("tap_checkout_differs_from_captured_tree"));
    }
    Ok(())
}

pub(crate) fn read_formula(root: &Path, path: &str) -> Result<Vec<u8>, OrchestratorError> {
    if !safe_relative_path(path) {
        return Err(internal("tap_formula_path_invalid"));
    }
    crate::check_evidence::reject_link_components(root, path)?;
    let file = root.join(path);
    let metadata = fs::symlink_metadata(&file).map_err(|_| internal("tap_formula_missing"))?;
    if !metadata.is_file() || metadata.len() > FORMULA_LIMIT {
        return Err(internal("tap_formula_not_regular_or_large"));
    }
    fs::read(file).map_err(|_| internal("tap_formula_read"))
}

pub(crate) fn hash_file(path: &Path) -> Result<String, OrchestratorError> {
    let mut file = fs::File::open(path).map_err(|_| internal("tap_snapshot_file_open"))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| internal("tap_snapshot_file_read"))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(hex_digest(&digest.finalize()))
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    hex_digest(&Sha256::digest(bytes))
}

fn hex_digest(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(char::from(DIGITS[usize::from(byte >> 4)]));
        result.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    result
}

pub(crate) fn safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && !path.starts_with('/')
        && !path.contains('\\')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn internal(problem: &str) -> OrchestratorError {
    crate::internal::internal(problem)
}
