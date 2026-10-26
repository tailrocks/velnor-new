// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::{fs, io::Write as _, path::Path};

const LIMIT: u64 = 1024 * 1024;

pub(super) fn ensure_directory(path: &Path) -> Result<(), OrchestratorError> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .map_err(|_| internal("stable_tag_directory_create"))?;
    let metadata = fs::symlink_metadata(path).map_err(|_| internal("stable_tag_directory"))?;
    if !metadata.file_type().is_dir() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(internal("stable_tag_directory_not_private"));
    }
    Ok(())
}

pub(super) fn read_metadata(path: &Path) -> Result<Vec<u8>, OrchestratorError> {
    super::super::release_policy_admission::metadata::read(path)
}

pub(super) fn write_new(name: &str, bytes: &[u8]) -> Result<(), OrchestratorError> {
    write_private(&super::fixed_path(name)?, bytes)
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), OrchestratorError> {
    if bytes.is_empty() || bytes.len() as u64 > LIMIT {
        return Err(internal("stable_tag_metadata_bound"));
    }
    let parent = path
        .parent()
        .ok_or_else(|| internal("stable_tag_directory"))?;
    ensure_directory(parent)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| internal("stable_tag_output_create"))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| internal("stable_tag_output_write"))?;
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| internal("stable_tag_output_sync"))
}

pub(super) fn write_resolved_plan(bytes: &[u8]) -> Result<(), OrchestratorError> {
    let temp = std::path::PathBuf::from(super::required("RUNNER_TEMP")?);
    if !temp.is_absolute() {
        return Err(internal("stable_tag_runner_temp"));
    }
    write_private(&temp.join("velnor/package-release-plan.json"), bytes)
}
