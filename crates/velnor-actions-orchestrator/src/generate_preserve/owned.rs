// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::OrchestratorError;

use velnor_actions_contract::{
    DECLARED_GITHUB_FORMATS, MARKER_PREFIX, OLD_MARKER_PREFIX, TASK_EXECUTION_MANIFEST_PATH,
    is_generated_marker_line,
};
use velnor_actions_workflow_renderer::release_tree::RELEASE_TREE_PATHS;

pub(in crate::generate) fn generator_owned(relative: &Path) -> bool {
    if relative.starts_with("workflows") {
        return true;
    }
    DECLARED_GITHUB_FORMATS
        .iter()
        .map(|format| format.path)
        .chain(RELEASE_TREE_PATHS.iter().copied())
        .chain(std::iter::once(TASK_EXECUTION_MANIFEST_PATH))
        .any(|path| {
            Path::new(path)
                .strip_prefix(".github")
                .is_ok_and(|rel| rel == relative)
        })
}

pub(in crate::generate) fn generated_shared_action(
    relative: &Path,
    source: &Path,
    metadata: &std::fs::Metadata,
) -> Result<bool, OrchestratorError> {
    if relative.components().count() != 3
        || !relative.starts_with("actions")
        || relative.file_name().is_none_or(|name| name != "action.yml")
        || !metadata.is_file()
    {
        return Ok(false);
    }
    first_line_is_generated_marker(source)
}

pub(in crate::generate) fn generated_marked_script(
    relative: &Path,
    source: &Path,
    metadata: &std::fs::Metadata,
) -> Result<bool, OrchestratorError> {
    if relative.components().count() != 2 || !relative.starts_with("scripts") || !metadata.is_file()
    {
        return Ok(false);
    }
    first_line_is_generated_marker(source)
}

pub(in crate::generate) fn generated_marked_dco(
    relative: &Path,
    source: &Path,
    metadata: &std::fs::Metadata,
) -> Result<bool, OrchestratorError> {
    if relative != Path::new("dco.yml") || !metadata.is_file() {
        return Ok(false);
    }
    first_line_is_generated_marker(source)
}

pub(in crate::generate) fn retired_marked_lint_config(
    relative: &Path,
    source: &Path,
    metadata: &std::fs::Metadata,
) -> Result<bool, OrchestratorError> {
    if relative != Path::new("actionlint.yaml") || !metadata.is_file() {
        return Ok(false);
    }
    first_line_is_generated_marker(source)
}

fn first_line_is_generated_marker(source: &Path) -> Result<bool, OrchestratorError> {
    let file = fs::File::open(source).map_err(|error| io(source, &error))?;
    let mut reader = std::io::BufReader::new(file);
    let limit = MARKER_PREFIX.len().max(OLD_MARKER_PREFIX.len());
    let mut first = Vec::with_capacity(limit);
    let mut byte = [0u8; 1];
    for _ in 0..limit {
        if reader.read(&mut byte).map_err(|error| io(source, &error))? == 0 || byte[0] == b'\n' {
            break;
        }
        first.push(byte[0]);
    }
    let first = std::str::from_utf8(&first).ok();
    Ok(first.is_some_and(is_generated_marker_line))
}

pub(super) fn copy_entry(
    source: &Path,
    destination: &Path,
    relative: &Path,
    directories: &mut Vec<(PathBuf, std::fs::Permissions)>,
) -> Result<bool, OrchestratorError> {
    let metadata = std::fs::symlink_metadata(source).map_err(|error| io(source, &error))?;
    if generator_owned(relative)
        || generated_shared_action(relative, source, &metadata)?
        || generated_marked_script(relative, source, &metadata)?
        || generated_marked_dco(relative, source, &metadata)?
        || retired_marked_lint_config(relative, source, &metadata)?
    {
        return Ok(false);
    }
    if metadata.is_symlink() {
        copy_symlink(source, destination)?;
        Ok(true)
    } else if metadata.is_file() {
        std::fs::copy(source, destination).map_err(|error| io(source, &error))?;
        Ok(true)
    } else if metadata.is_dir() {
        std::fs::create_dir(destination).map_err(|error| io(destination, &error))?;
        let mut had_entry = false;
        let mut preserved_entry = false;
        for entry in std::fs::read_dir(source).map_err(|error| io(source, &error))? {
            let entry = entry.map_err(|error| io(source, &error))?;
            let name = entry.file_name();
            had_entry = true;
            preserved_entry |= copy_entry(
                &entry.path(),
                &destination.join(&name),
                &relative.join(&name),
                directories,
            )?;
        }
        if had_entry && !preserved_entry {
            std::fs::remove_dir(destination).map_err(|error| io(destination, &error))?;
            return Ok(false);
        }
        directories.push((destination.to_path_buf(), metadata.permissions()));
        Ok(true)
    } else {
        Err(OrchestratorError::UnsafePath {
            path: source.display().to_string(),
            reason: "unsupported_repository_entry".to_owned(),
        })
    }
}

fn copy_symlink(source: &Path, destination: &Path) -> Result<(), OrchestratorError> {
    let metadata = std::fs::symlink_metadata(source).map_err(|error| io(source, &error))?;
    let target = std::fs::read_link(source).map_err(|error| io(source, &error))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};

        symlink(target, destination).map_err(|error| io(destination, &error))?;
        let mode = metadata.permissions().mode() & 0o7777;
        let copied_mode = std::fs::symlink_metadata(destination)
            .map_err(|error| io(destination, &error))?
            .permissions()
            .mode()
            & 0o7777;
        if mode != copied_mode {
            let raw_mode = checked_raw_mode::<rustix::fs::RawMode>(mode, destination)?;
            rustix::fs::chmodat(
                rustix::fs::CWD,
                destination,
                rustix::fs::Mode::from_raw_mode(raw_mode),
                rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
            )
            .map_err(|error| {
                OrchestratorError::io(destination.display().to_string(), error.to_string())
            })?;
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt;
        let result = if metadata.file_type().is_symlink_dir() {
            std::os::windows::fs::symlink_dir(target, destination)
        } else {
            std::os::windows::fs::symlink_file(target, destination)
        };
        result.map_err(|error| io(destination, &error))
    }
}

#[cfg(unix)]
fn checked_raw_mode<RawMode>(mode: u32, path: &Path) -> Result<RawMode, OrchestratorError>
where
    RawMode: TryFrom<u32>,
    RawMode::Error: std::fmt::Display,
{
    mode.try_into().map_err(|error| {
        OrchestratorError::io(
            path.display().to_string(),
            format!("symlink_mode_invalid:{error}"),
        )
    })
}

pub(super) fn io(path: &Path, error: &std::io::Error) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), error.to_string())
}
