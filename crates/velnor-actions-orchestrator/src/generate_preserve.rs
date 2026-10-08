//! Preserve repository-owned `.github` entries inside generation staging.
//!
//! Generated workflows, declared formats, and marked shared actions remain
//! generator-owned. Other entries copy without following links; unsupported
//! entry types fail closed.

use std::path::{Path, PathBuf};
use std::{fs, io::Read};

use velnor_actions_contract::{
    DECLARED_GITHUB_FORMATS, MARKER_PREFIX, OLD_MARKER_PREFIX, RETIRED_GITHUB_PATHS,
    is_generated_marker_line,
};
use velnor_actions_workflow_renderer::release_tree::RELEASE_TREE_PATHS;
use velnor_actions_workflow_renderer::render::RenderedTree;

use crate::OrchestratorError;

/// Prepare the in-place replacement tree before the existing output is swapped.
pub(super) fn stage_in_place(
    source: &Path,
    staging_root: &Path,
    tree: &RenderedTree,
) -> Result<PathBuf, OrchestratorError> {
    let permissions = root_permissions(source)?;
    // A read-only root must exchange with a sibling on macOS.
    let staged = if permissions.is_some() {
        staging_root.to_path_buf()
    } else {
        staging_root.join(".github")
    };
    let directories = copy_repository_content(source, &staged)?;
    check_generated_collisions(&staged, tree)?;
    super::write_tree(&staged, tree)?;
    restore_directory_permissions(directories)?;
    restore_root_permissions(&staged, permissions)?;
    Ok(staged)
}

/// Populate the already-reserved preview tree from the repository and render.
pub(super) fn write_preview(
    source: &Path,
    destination: &Path,
    tree: &RenderedTree,
) -> Result<(), OrchestratorError> {
    let permissions = root_permissions(source)?;
    let directories = copy_repository_content(source, destination)?;
    check_generated_collisions(destination, tree)?;
    super::write_tree(destination, tree)?;
    restore_directory_permissions(directories)?;
    restore_root_permissions(destination, permissions)
}

/// Capture the repository root's own mode, never a link target.
fn root_permissions(source: &Path) -> Result<Option<std::fs::Permissions>, OrchestratorError> {
    match std::fs::symlink_metadata(source) {
        Ok(metadata) if metadata.is_dir() && !metadata.is_symlink() => {
            Ok(Some(metadata.permissions()))
        }
        Ok(_) => Err(OrchestratorError::UnsafePath {
            path: source.display().to_string(),
            reason: "repository_root_not_directory".to_owned(),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io(source, &error)),
    }
}

/// Restore descendants deepest first, after every file has been written.
fn restore_directory_permissions(
    directories: Vec<(PathBuf, std::fs::Permissions)>,
) -> Result<(), OrchestratorError> {
    for (directory, permissions) in directories {
        std::fs::set_permissions(&directory, permissions)
            .map_err(|error| io(&directory, &error))?;
    }
    Ok(())
}

/// Restore the source root mode after staged generation completes.
fn restore_root_permissions(
    destination: &Path,
    permissions: Option<std::fs::Permissions>,
) -> Result<(), OrchestratorError> {
    if let Some(permissions) = permissions {
        std::fs::set_permissions(destination, permissions)
            .map_err(|error| io(destination, &error))?;
    }
    Ok(())
}

/// Refuse any preserved path that overlaps generated output before writing.
fn check_generated_collisions(
    destination: &Path,
    tree: &RenderedTree,
) -> Result<(), OrchestratorError> {
    for path in tree
        .files
        .iter()
        .map(|file| &file.path)
        .chain(tree.symlinks.iter().map(|link| &link.path))
    {
        let relative =
            path.strip_prefix(".github/")
                .ok_or_else(|| OrchestratorError::Contract {
                    problem: "generated_path_outside_github".to_owned(),
                })?;
        let mut current = destination.to_path_buf();
        let components: Vec<_> = Path::new(relative).components().collect();
        for (index, component) in components.iter().enumerate() {
            if !matches!(component, std::path::Component::Normal(_)) {
                return Err(OrchestratorError::Contract {
                    problem: "generated_path_not_relative".to_owned(),
                });
            }
            current.push(component.as_os_str());
            match std::fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.is_symlink() => {
                    return Err(OrchestratorError::UnsafePath {
                        path: current.display().to_string(),
                        reason: "symlink_refused".to_owned(),
                    });
                }
                Ok(metadata) if index + 1 == components.len() || !metadata.is_dir() => {
                    return Err(OrchestratorError::OverwriteRefused {
                        path: current.display().to_string(),
                    });
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(error) => return Err(io(&current, &error)),
            }
        }
    }
    Ok(())
}

/// Copy repository-owned entries, including hidden files and empty trees.
fn copy_repository_content(
    source: &Path,
    destination: &Path,
) -> Result<Vec<(PathBuf, std::fs::Permissions)>, OrchestratorError> {
    if std::fs::symlink_metadata(source).is_ok_and(|metadata| metadata.is_symlink()) {
        return Err(OrchestratorError::UnsafePath {
            path: source.display().to_string(),
            reason: "symlink_refused".to_owned(),
        });
    }
    let entries = match std::fs::read_dir(source) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io(source, &error)),
    };
    std::fs::create_dir_all(destination).map_err(|error| io(destination, &error))?;
    let mut directories = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| io(source, &error))?;
        let name = entry.file_name();
        copy_entry(
            &entry.path(),
            &destination.join(&name),
            Path::new(&name),
            &mut directories,
        )?;
    }
    Ok(directories)
}

/// Keep workflow, declared-format, release, and retired outputs generator-owned.
/// Retired paths stay owned so the preserve-copy skips them and the swap
/// deletes stale copies; nothing re-emits them.
fn generator_owned(relative: &Path) -> bool {
    relative.starts_with("workflows")
        || DECLARED_GITHUB_FORMATS
            .iter()
            .map(|format| format.path)
            .chain(RELEASE_TREE_PATHS.iter().copied())
            .chain(RETIRED_GITHUB_PATHS.iter().copied())
            .any(|path| {
                Path::new(path)
                    .strip_prefix(".github")
                    .is_ok_and(|rel| rel == relative)
            })
}

/// Recognize generated shared actions by exact path shape and marker.
fn generated_shared_action(
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

/// Recognize generated helper scripts by exact path shape and marker.
///
/// Unmarked files under `scripts/` stay repository-owned; only a marked
/// file is generator-owned and skipped during preserve-copy.
fn generated_marked_script(
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

/// Copy one repository entry without following symbolic links.
fn copy_entry(
    source: &Path,
    destination: &Path,
    relative: &Path,
    directories: &mut Vec<(PathBuf, std::fs::Permissions)>,
) -> Result<bool, OrchestratorError> {
    let metadata = std::fs::symlink_metadata(source).map_err(|error| io(source, &error))?;
    if generator_owned(relative)
        || generated_shared_action(relative, source, &metadata)?
        || generated_marked_script(relative, source, &metadata)?
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

/// Recreate a symbolic link with its literal target, including dangling links.
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

/// Convert Unix permission bits to Rustix's platform-specific raw mode type.
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

/// Attach the path that failed to preserve or publish output.
fn io(path: &Path, error: &std::io::Error) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), error.to_string())
}

#[cfg(test)]
#[path = "generate_preserve_tests.rs"]
mod tests;
