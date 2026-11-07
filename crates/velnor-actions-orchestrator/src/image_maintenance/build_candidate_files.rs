// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::source;
use crate::{OrchestratorError, internal::internal};
use std::{
    fs,
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};

pub(super) fn copy_source_file(
    source_path: &Path,
    destination: &Path,
    expected_sha: &str,
) -> Result<(), OrchestratorError> {
    let hash_from = |path: &Path| {
        source::hash_source_file(
            path.parent()
                .ok_or_else(|| internal("image_candidate_copy_path"))?,
            path.file_name()
                .and_then(std::ffi::OsStr::to_str)
                .ok_or_else(|| internal("image_candidate_copy_path"))?,
        )
    };
    if hash_from(source_path)? != expected_sha {
        return Err(internal("image_candidate_copy_source_changed"));
    }
    fs::copy(source_path, destination).map_err(|error| {
        OrchestratorError::io(destination.display().to_string(), error.to_string())
    })?;
    if hash_from(destination)? != expected_sha {
        return Err(internal("image_candidate_copy_changed"));
    }
    Ok(())
}

pub(super) fn write_staged(path: &Path, bytes: &[u8]) -> Result<(), OrchestratorError> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))
}

pub(super) fn remove_our_archive_link(stage: &Path, output: &Path) {
    let stage_metadata = fs::metadata(stage).ok();
    let output_metadata = fs::symlink_metadata(output).ok();
    if stage_metadata
        .zip(output_metadata)
        .is_some_and(|(left, right)| left.dev() == right.dev() && left.ino() == right.ino())
    {
        drop(fs::remove_file(output));
    }
}
