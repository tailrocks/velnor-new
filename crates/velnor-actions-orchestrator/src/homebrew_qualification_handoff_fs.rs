// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::sha256;
use crate::{OrchestratorError, internal::internal};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

pub(super) fn safe_file(name: &str) -> bool {
    let Some((label, file)) = name.split_once('/') else {
        return false;
    };
    matches!(label, "A" | "D")
        && !file.is_empty()
        && file != "."
        && file != ".."
        && file
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

pub(super) fn read_artifact(
    directory: &Path,
    digest: &str,
    size: u64,
    limit: u64,
) -> Result<Vec<u8>, OrchestratorError> {
    let metadata = fs::symlink_metadata(directory).map_err(|e| io(directory, &e))?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || size == 0
        || size > limit
        || !super::is_sha256(digest)
    {
        return Err(internal("homebrew_handoff_artifact_identity"));
    }
    let mut entries = fs::read_dir(directory).map_err(|e| io(directory, &e))?;
    let path = entries
        .next()
        .ok_or_else(|| internal("homebrew_handoff_artifact_missing"))?
        .map_err(|e| io(directory, &e))?
        .path();
    if entries.next().is_some() {
        return Err(internal("homebrew_handoff_artifact_inventory"));
    }
    let bytes = read_regular(&path, size)?;
    if bytes.len() as u64 != size || sha256(&bytes) != digest {
        return Err(internal("homebrew_handoff_archive_digest"));
    }
    Ok(bytes)
}

pub(super) fn read_regular(path: &Path, limit: u64) -> Result<Vec<u8>, OrchestratorError> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits().cast_signed())
        .open(path)
        .map_err(|e| io(path, &e))?;
    let metadata = file.metadata().map_err(|e| io(path, &e))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > limit {
        return Err(internal("homebrew_handoff_file_limit"));
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| io(path, &e))?;
    if bytes.len() as u64 != metadata.len() || bytes.len() as u64 > limit {
        return Err(internal("homebrew_handoff_file_changed"));
    }
    Ok(bytes)
}

pub(super) fn io(path: &Path, error: &std::io::Error) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), error.to_string())
}
pub(super) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), OrchestratorError> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| io(path, &e))?
        .write_all(bytes)
        .map_err(|e| io(path, &e))
}
