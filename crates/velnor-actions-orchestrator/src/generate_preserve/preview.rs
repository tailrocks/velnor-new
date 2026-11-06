// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use crate::OrchestratorError;

use velnor_actions_workflow_renderer::render::RenderedTree;

use super::super::write_tree;
use super::owned::{copy_entry, io};

pub(in crate::generate) fn write_preview(
    source: &Path,
    destination: &Path,
    tree: &RenderedTree,
) -> Result<(), OrchestratorError> {
    let permissions = root_permissions(source)?;
    let directories = copy_repository_content(source, destination)?;
    check_generated_collisions(destination, tree)?;
    write_tree(destination, tree)?;
    restore_directory_permissions(directories)?;
    restore_root_permissions(destination, permissions)
}

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

fn restore_directory_permissions(
    directories: Vec<(PathBuf, std::fs::Permissions)>,
) -> Result<(), OrchestratorError> {
    for (directory, permissions) in directories {
        std::fs::set_permissions(&directory, permissions)
            .map_err(|error| io(&directory, &error))?;
    }
    Ok(())
}

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
