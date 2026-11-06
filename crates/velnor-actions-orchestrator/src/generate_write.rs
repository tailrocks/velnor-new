// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::Path;

use crate::generation_filesystem::{FilesystemObjectType, GenerationFileOpen};

use velnor_actions_workflow_renderer::render::RenderedTree;

use crate::OrchestratorError;

pub(crate) fn write_tree(github_dir: &Path, tree: &RenderedTree) -> Result<(), OrchestratorError> {
    for file in &tree.files {
        let rel = file.path.strip_prefix(".github/").unwrap_or(&file.path);
        let dest = github_dir.join(rel);
        ensure_parent_dirs_safe(github_dir, &dest)?;
        if std::fs::symlink_metadata(&dest).is_ok_and(|meta| meta.is_symlink()) {
            return Err(OrchestratorError::UnsafePath {
                path: dest.display().to_string(),
                reason: "symlink_refused".to_owned(),
            });
        }
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&dest)
            .map_err(|err| {
                if err.kind() == std::io::ErrorKind::AlreadyExists {
                    OrchestratorError::OverwriteRefused {
                        path: dest.display().to_string(),
                    }
                } else {
                    OrchestratorError::io(dest.display().to_string(), err.to_string())
                }
            })?
            .write_all(file.bytes.as_bytes())
            .map_err(|err| OrchestratorError::io(dest.display().to_string(), err.to_string()))?;
    }
    for link in &tree.symlinks {
        let rel = link.path.strip_prefix(".github/").unwrap_or(&link.path);
        let dest = github_dir.join(rel);
        ensure_parent_dirs_safe(github_dir, &dest)?;
        if std::fs::symlink_metadata(&dest).is_ok() {
            return Err(OrchestratorError::OverwriteRefused {
                path: dest.display().to_string(),
            });
        }
        create_symlink(&link.target, &dest)?;
    }
    Ok(())
}

pub(crate) fn write_tree_at(
    github_dir: &super::fd::Directory,
    tree: &RenderedTree,
) -> Result<(), OrchestratorError> {
    for file in &tree.files {
        let relative =
            file.path
                .strip_prefix(".github/")
                .ok_or_else(|| OrchestratorError::Contract {
                    problem: "generated_path_outside_github".to_owned(),
                })?;
        let (parent, name) = ensure_parents_at(github_dir, Path::new(relative))?;
        if parent.stat_opt(&name)?.is_some() {
            return Err(OrchestratorError::OverwriteRefused {
                path: parent.child_path(&name).display().to_string(),
            });
        }
        let mut output = parent.open_file(&name, GenerationFileOpen::WriteCreateNew)?;
        output
            .write_all(file.bytes.as_bytes())
            .map_err(|error| crate::generation_filesystem::io(&parent.child_path(&name), &error))?;
    }
    for link in &tree.symlinks {
        let relative =
            link.path
                .strip_prefix(".github/")
                .ok_or_else(|| OrchestratorError::Contract {
                    problem: "generated_path_outside_github".to_owned(),
                })?;
        let (parent, name) = ensure_parents_at(github_dir, Path::new(relative))?;
        if parent.stat_opt(&name)?.is_some() {
            return Err(OrchestratorError::OverwriteRefused {
                path: parent.child_path(&name).display().to_string(),
            });
        }
        parent.symlink(OsStr::new(&link.target), &name)?;
    }
    Ok(())
}

fn ensure_parents_at(
    root: &super::fd::Directory,
    relative: &Path,
) -> Result<(super::fd::Directory, OsString), OrchestratorError> {
    let components: Vec<_> = relative
        .components()
        .map(|component| match component {
            std::path::Component::Normal(value) => Ok(value.to_os_string()),
            _ => Err(OrchestratorError::Contract {
                problem: "generated_path_not_relative".to_owned(),
            }),
        })
        .collect::<Result<_, _>>()?;
    let (leaf, parents) = components
        .split_last()
        .ok_or_else(|| OrchestratorError::Contract {
            problem: "generated_path_empty".to_owned(),
        })?;
    let mut current = root.try_clone()?;
    for name in parents {
        match current.stat_opt(name)? {
            Some(stat) if stat.object_type == FilesystemObjectType::Directory => {
                let child = current.open_dir(name)?;
                let opened = child.metadata()?;
                if (opened.device, opened.inode) != (stat.device, stat.inode) {
                    return Err(OrchestratorError::UnsafePath {
                        path: current.child_path(name).display().to_string(),
                        reason: "directory_identity_changed".to_owned(),
                    });
                }
                current = child;
            }
            Some(stat) if stat.object_type == FilesystemObjectType::Symlink => {
                return Err(OrchestratorError::UnsafePath {
                    path: current.child_path(name).display().to_string(),
                    reason: "symlink_refused".to_owned(),
                });
            }
            Some(_) => {
                return Err(OrchestratorError::OverwriteRefused {
                    path: current.child_path(name).display().to_string(),
                });
            }
            None => {
                match current.create_dir_raw(name, 0o777) {
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => {
                        return Err(crate::generation_filesystem::io(
                            &current.child_path(name),
                            &error,
                        ));
                    }
                }
                let child = current.open_dir(name)?;
                current = child;
            }
        }
    }
    Ok((current, leaf.clone()))
}

fn ensure_parent_dirs_safe(github_dir: &Path, dest: &Path) -> Result<(), OrchestratorError> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| OrchestratorError::io(parent.display().to_string(), err.to_string()))?;
        let mut current = Some(parent);
        while let Some(level) = current {
            if !level.starts_with(github_dir) {
                break;
            }
            if std::fs::symlink_metadata(level).is_ok_and(|meta| meta.is_symlink()) {
                return Err(OrchestratorError::UnsafePath {
                    path: level.display().to_string(),
                    reason: "symlink_refused".to_owned(),
                });
            }
            current = if level == github_dir {
                None
            } else {
                level.parent()
            };
        }
    }
    Ok(())
}

fn create_symlink(target: &str, dest: &Path) -> Result<(), OrchestratorError> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, dest)
            .map_err(|err| OrchestratorError::io(dest.display().to_string(), err.to_string()))
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(target, dest).map_err(|err| {
            OrchestratorError::io(
                dest.display().to_string(),
                format!("symlink_failed:{err} (Windows requires Developer Mode or SeCreateSymbolicLinkPrivilege)"),
            )
        })
    }
}
