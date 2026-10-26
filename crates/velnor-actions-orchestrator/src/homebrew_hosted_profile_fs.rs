// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use velnor_actions_mise::CheckDeadline;

use super::support::{io_error, run_command};
use crate::{OrchestratorError, homebrew::command::HomebrewCommand, internal::internal};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PrefixIdentity {
    pub(super) device: u64,
    pub(super) inode: u64,
}

pub(super) fn directory_identity(path: &Path) -> Result<PrefixIdentity, OrchestratorError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| io_error(path, &error))?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != crate::filesystem_owner::effective_uid()
    {
        return Err(internal("homebrew_profile_prefix_identity"));
    }
    Ok(PrefixIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

pub(super) fn path_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

pub(super) fn validate_trusted_host_tools(paths: &[&str]) -> Result<(), OrchestratorError> {
    for raw in paths {
        let path = Path::new(raw);
        let canonical = path
            .canonicalize()
            .map_err(|error| io_error(path, &error))?;
        let metadata = fs::metadata(&canonical).map_err(|error| io_error(&canonical, &error))?;
        if !metadata.is_file()
            || metadata.uid() != 0
            || metadata.mode() & 0o022 != 0
            || metadata.mode() & 0o111 == 0
        {
            return Err(internal("homebrew_profile_host_tool_identity"));
        }
        let mut parent = canonical.parent();
        while let Some(directory) = parent {
            let parent_metadata =
                fs::metadata(directory).map_err(|error| io_error(directory, &error))?;
            if !parent_metadata.is_dir()
                || parent_metadata.uid() != 0
                || parent_metadata.mode() & 0o022 != 0
            {
                return Err(internal("homebrew_profile_host_tool_parent"));
            }
            if directory == Path::new("/") {
                break;
            }
            parent = directory.parent();
        }
    }
    Ok(())
}

pub(super) fn validate_no_brew_env_files(prefix: &Path) -> Result<(), OrchestratorError> {
    for path in [
        Path::new("/etc/homebrew/brew.env"),
        &prefix.join("etc/homebrew/brew.env"),
    ] {
        match fs::symlink_metadata(path) {
            Ok(_) => return Err(internal("homebrew_profile_brew_env_present")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(path, &error)),
        }
    }
    Ok(())
}

pub(super) fn resolve_path_executable(name: &str) -> Result<Option<PathBuf>, OrchestratorError> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(name);
        match fs::metadata(&candidate) {
            Ok(metadata) if metadata.is_file() => {
                return candidate
                    .canonicalize()
                    .map(Some)
                    .map_err(|error| io_error(&candidate, &error));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(&candidate, &error)),
        }
    }
    Ok(None)
}

pub(super) fn move_prefix(
    source: &Path,
    destination: &Path,
    home: &Path,
    temp: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    run_command(
        HomebrewCommand::MovePrefix {
            source: source.to_path_buf(),
            destination: destination.to_path_buf(),
        },
        home,
        temp,
        deadline,
        "sudo",
    )
    .map(|_| ())
}
