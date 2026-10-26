// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{OrchestratorError, internal};
use std::path::{Component, Path, PathBuf};
use velnor_actions_mise::CheckDeadline;

pub(super) fn absolute_path(path: &Path) -> Result<(), OrchestratorError> {
    if !path.is_absolute()
        || !path
            .to_str()
            .is_some_and(|s| s.len() <= 4096 && !s.chars().any(char::is_control))
        || path
            .components()
            .any(|part| !matches!(part, Component::RootDir | Component::Normal(_)))
    {
        return Err(internal("qualification_requires_absolute_regular_path"));
    }
    Ok(())
}

pub(super) fn resolved_parent(path: &Path) -> Result<PathBuf, OrchestratorError> {
    absolute_path(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| internal("qualification_path_parent"))?;
    let name = path
        .file_name()
        .ok_or_else(|| internal("qualification_path_name"))?;
    let parent = parent.canonicalize().map_err(|e| io(parent, e))?;
    if !parent.is_dir() {
        return Err(internal("qualification_parent_not_directory"));
    }
    Ok(parent.join(name))
}

pub(super) fn new_paths(
    work: &Path,
    output: &Path,
) -> Result<(PathBuf, PathBuf), OrchestratorError> {
    let work = resolved_parent(work)?;
    let output = resolved_parent(output)?;
    if output.starts_with(&work) || work.starts_with(&output) {
        return Err(internal("qualification_paths_overlap"));
    }
    for path in [&work, &output] {
        match std::fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err(internal("qualification_requires_new_paths")),
        }
    }
    Ok((work, output))
}

pub(super) fn create_home(work: &Path) -> Result<(), OrchestratorError> {
    #[cfg(unix)]
    use std::os::unix::fs::DirBuilderExt as _;
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    builder.mode(0o700);
    builder.create(work).map_err(|e| io(work, e))?;
    for name in ["bin", "cargo", "rustup", "tools"] {
        directory(&work.join(name))?;
    }
    Ok(())
}

pub(super) fn directory(path: &Path) -> Result<(), OrchestratorError> {
    std::fs::create_dir(path).map_err(|e| io(path, e))
}

pub(super) fn read(
    path: &Path,
    limit: u64,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    let path = resolved_parent(path)?;
    super::acquisition::read_with_deadline(&path, limit, deadline)
        .map_err(|problem| internal(&format!("qualification_local_read:{problem}")))
}

pub(super) fn write(
    path: &Path,
    bytes: &[u8],
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    crate::exclusive_write::write_exclusive_until(path, bytes, "qualification_candidate", || {
        super::checkpoint(deadline)
    })
}

fn io(path: &Path, error: impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), error.to_string())
}
