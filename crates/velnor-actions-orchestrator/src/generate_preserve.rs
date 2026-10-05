//! Preserve repository-owned `.github` content within the staged transaction.
//!
//! Workflows and declared generated formats are generator-owned; all other
//! entries are copied without following symbolic links. Unsupported file types
//! fail before the existing tree is replaced.

use std::io::Read;
use std::path::{Path, PathBuf};

use velnor_actions_contract::{
    DECLARED_GITHUB_FORMATS, MARKER_PREFIX, OLD_MARKER_PREFIX, is_generated_marker_line,
};
use velnor_actions_workflow_renderer::release_tree::RELEASE_TREE_PATHS;

use crate::OrchestratorError;

const TOOL_SEED_ACTION_RELATIVE: &str = "actions/velnor-tool-seed/action.yml";
const SHARED_SCRIPTS_RELATIVE: &str = "scripts/velnor-shared";

/// Capture only the repository root's own directory mode, never a link target.
pub(super) fn root_permissions(
    source: &Path,
) -> Result<Option<std::fs::Permissions>, OrchestratorError> {
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

/// Apply the captured root mode after generation so read-only roots can stage.
pub(super) fn restore_root_permissions(
    destination: &Path,
    permissions: Option<std::fs::Permissions>,
) -> Result<(), OrchestratorError> {
    if let Some(permissions) = permissions {
        std::fs::set_permissions(destination, permissions)
            .map_err(|error| io(destination, &error))?;
    }
    Ok(())
}

/// Restore descendants only after generated output is complete, deepest first.
pub(super) fn restore_directory_permissions(
    directories: Vec<(PathBuf, std::fs::Permissions)>,
) -> Result<(), OrchestratorError> {
    for (directory, permissions) in directories {
        std::fs::set_permissions(&directory, permissions)
            .map_err(|error| io(&directory, &error))?;
    }
    Ok(())
}

/// Refuse every preserved collision before a writer creates any output parent.
pub(super) fn check_generated_collisions(
    destination: &Path,
    tree: &velnor_actions_workflow_renderer::RenderedTree,
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

/// Copy every repository-owned root entry, including hidden and empty trees.
pub(super) fn copy_repository_content(
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
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(io(source, &err)),
    };
    std::fs::create_dir_all(destination).map_err(|err| io(destination, &err))?;
    let mut directories = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| io(source, &err))?;
        copy_entry(
            &entry.path(),
            &destination.join(entry.file_name()),
            Path::new(&entry.file_name()),
            &mut directories,
        )?;
    }
    Ok(directories)
}

/// Ownership derives from output inventories, including disabled release paths.
fn generator_owned(source: &Path, relative: &Path) -> Result<bool, OrchestratorError> {
    let fixed_inventory = relative.starts_with("workflows")
        || DECLARED_GITHUB_FORMATS
            .iter()
            .map(|format| format.path)
            .chain(RELEASE_TREE_PATHS.iter().copied())
            .any(|path| {
                Path::new(path)
                    .strip_prefix(".github")
                    .is_ok_and(|rel| rel == relative)
            });
    if fixed_inventory {
        return Ok(true);
    }
    let shared_script = relative
        .strip_prefix(SHARED_SCRIPTS_RELATIVE)
        .is_ok_and(|suffix| !suffix.as_os_str().is_empty());
    if relative != Path::new(TOOL_SEED_ACTION_RELATIVE) && !shared_script {
        return Ok(false);
    }
    generated_marker_file(source)
}

/// Treat only marked regular files as replaceable generator-owned assets.
fn generated_marker_file(source: &Path) -> Result<bool, OrchestratorError> {
    let metadata = std::fs::symlink_metadata(source).map_err(|err| io(source, &err))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Ok(false);
    }
    let file = std::fs::File::open(source).map_err(|err| io(source, &err))?;
    let prefix_limit = MARKER_PREFIX.len().max(OLD_MARKER_PREFIX.len());
    let mut prefix = Vec::with_capacity(prefix_limit);
    file.take(prefix_limit as u64)
        .read_to_end(&mut prefix)
        .map_err(|err| io(source, &err))?;
    let Ok(first_line) = std::str::from_utf8(&prefix) else {
        return Ok(false);
    };
    Ok(is_generated_marker_line(first_line))
}

/// Preserve files, directory permissions, and links as entries, never targets.
fn copy_entry(
    source: &Path,
    destination: &Path,
    relative: &Path,
    directories: &mut Vec<(PathBuf, std::fs::Permissions)>,
) -> Result<(), OrchestratorError> {
    if generator_owned(source, relative)? {
        return Ok(());
    }
    let metadata = std::fs::symlink_metadata(source).map_err(|err| io(source, &err))?;
    if metadata.is_symlink() {
        copy_symlink(source, destination)?;
    } else if metadata.is_file() {
        std::fs::copy(source, destination).map_err(|err| io(source, &err))?;
    } else if metadata.is_dir() {
        std::fs::create_dir(destination).map_err(|err| io(destination, &err))?;
        for entry in std::fs::read_dir(source).map_err(|err| io(source, &err))? {
            let entry = entry.map_err(|err| io(source, &err))?;
            copy_entry(
                &entry.path(),
                &destination.join(entry.file_name()),
                &relative.join(entry.file_name()),
                directories,
            )?;
        }
        directories.push((destination.to_path_buf(), metadata.permissions()));
    } else {
        return Err(OrchestratorError::UnsafePath {
            path: source.display().to_string(),
            reason: "unsupported_repository_entry".to_owned(),
        });
    }
    Ok(())
}

/// Recreate a symbolic link with its literal target, including dangling links.
fn copy_symlink(source: &Path, destination: &Path) -> Result<(), OrchestratorError> {
    let target = std::fs::read_link(source).map_err(|err| io(source, &err))?;
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, destination).map_err(|err| io(destination, &err))
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt;
        let metadata = std::fs::symlink_metadata(source).map_err(|err| io(source, &err))?;
        let result = if metadata.file_type().is_symlink_dir() {
            std::os::windows::fs::symlink_dir(target, destination)
        } else {
            std::os::windows::fs::symlink_file(target, destination)
        };
        result.map_err(|err| io(destination, &err))
    }
}

/// Retain the exact failing path in filesystem diagnostics.
fn io(path: &Path, error: &std::io::Error) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), error.to_string())
}

#[cfg(test)]
#[path = "generate_preserve_tests.rs"]
mod tests;
