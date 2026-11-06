// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::io::{Seek, SeekFrom};
use std::path::Path;

use crate::OrchestratorError;
use crate::generation_filesystem::{
    FilesystemMetadata, FilesystemObjectType, GenerationFile, GenerationFileOpen,
};

use velnor_actions_contract::{MARKER_PREFIX, OLD_MARKER_PREFIX, is_generated_marker_line};
use velnor_actions_workflow_renderer::render::RenderedTree;

use super::super::fd::Directory;
use super::owned::generator_owned;

pub(in crate::generate) fn stage_in_place(
    source: Option<&Directory>,
    destination: &Directory,
    tree: &RenderedTree,
    root_mode: Option<u32>,
) -> Result<(), OrchestratorError> {
    let mut directories = Vec::new();
    if let Some(source) = source {
        copy_repository_content_at(source, destination, &mut directories)?;
    }
    check_generated_collisions_at(destination, tree)?;
    super::super::write_tree_at(destination, tree)?;
    for (directory, mode) in directories {
        directory.set_mode(mode)?;
    }
    if let Some(mode) = root_mode {
        destination.set_mode(mode)?;
    }
    Ok(())
}

fn copy_repository_content_at(
    source: &Directory,
    destination: &Directory,
    directories: &mut Vec<(Directory, u32)>,
) -> Result<(), OrchestratorError> {
    for name in source.entries()? {
        copy_entry_at(source, destination, &name, Path::new(&name), directories)?;
    }
    Ok(())
}

fn copy_entry_at(
    source: &Directory,
    destination: &Directory,
    name: &OsStr,
    relative: &Path,
    directories: &mut Vec<(Directory, u32)>,
) -> Result<bool, OrchestratorError> {
    let source_path = source.child_path(name);
    let metadata = source.stat(name)?;
    if generator_owned(relative) {
        return Ok(false);
    }
    match metadata.object_type {
        FilesystemObjectType::Symlink => {
            let target = source.read_link(name)?;
            destination.symlink(&target, name)?;
            Ok(true)
        }
        FilesystemObjectType::RegularFile => {
            let mut input = source.open_file(name, GenerationFileOpen::ReadOnly)?;
            let opened = input.metadata()?;
            if identity(&opened) != identity(&metadata) {
                return Err(unsafe_path(&source_path, "source_identity_changed"));
            }
            if generated_marker_source(relative, &metadata, &mut *input)? {
                return Ok(false);
            }
            input
                .seek(SeekFrom::Start(0))
                .map_err(|error| crate::generation_filesystem::io(&source_path, &error))?;
            let mut output = destination.open_file(name, GenerationFileOpen::WriteCreateNew)?;
            let destination_path = destination.child_path(name);
            std::io::copy(&mut input, &mut output)
                .map_err(|error| crate::generation_filesystem::io(&destination_path, &error))?;
            output.set_mode(metadata.mode)?;
            Ok(true)
        }
        FilesystemObjectType::Directory => {
            let source_child = source.open_dir(name)?;
            let opened = source_child.metadata()?;
            if identity(&opened) != identity(&metadata) {
                return Err(unsafe_path(&source_path, "source_identity_changed"));
            }
            destination.create_dir(name, 0o700)?;
            let destination_child = destination.open_dir(name)?;
            let mut had_entry = false;
            let mut preserved_entry = false;
            for child_name in source_child.entries()? {
                had_entry = true;
                preserved_entry |= copy_entry_at(
                    &source_child,
                    &destination_child,
                    &child_name,
                    &relative.join(&child_name),
                    directories,
                )?;
            }
            if had_entry && !preserved_entry {
                destination.remove_dir(name)?;
                return Ok(false);
            }
            directories.push((destination_child, metadata.mode));
            Ok(true)
        }
        FilesystemObjectType::Other => {
            Err(unsafe_path(&source_path, "unsupported_repository_entry"))
        }
    }
}

fn generated_marker_source(
    relative: &Path,
    metadata: &FilesystemMetadata,
    file: &mut dyn GenerationFile,
) -> Result<bool, OrchestratorError> {
    let is_candidate = (relative.components().count() == 3
        && relative.starts_with("actions")
        && relative
            .file_name()
            .is_some_and(|name| name == "action.yml"))
        || (relative.components().count() == 2 && relative.starts_with("scripts"))
        || relative == Path::new("dco.yml")
        || relative == Path::new("actionlint.yaml");
    if !is_candidate || metadata.object_type != FilesystemObjectType::RegularFile {
        return Ok(false);
    }
    let limit = MARKER_PREFIX.len().max(OLD_MARKER_PREFIX.len());
    let mut first = vec![0; limit];
    let mut read = 0;
    while read < limit {
        let count = file
            .read(&mut first[read..])
            .map_err(|error| crate::generation_filesystem::io(Path::new(".github"), &error))?;
        if count == 0 {
            break;
        }
        let end = read + count;
        if let Some(newline) = first[read..end].iter().position(|byte| *byte == b'\n') {
            read += newline;
            break;
        }
        read = end;
    }
    first.truncate(read);
    let line = std::str::from_utf8(&first).ok();
    Ok(line.is_some_and(is_generated_marker_line))
}

fn check_generated_collisions_at(
    destination: &Directory,
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
        let components: Vec<_> = Path::new(relative).components().collect();
        let mut current = destination.try_clone()?;
        for (index, component) in components.iter().enumerate() {
            let std::path::Component::Normal(name) = component else {
                return Err(OrchestratorError::Contract {
                    problem: "generated_path_not_relative".to_owned(),
                });
            };
            let Some(metadata) = current.stat_opt(name)? else {
                break;
            };
            match metadata.object_type {
                FilesystemObjectType::Symlink => {
                    return Err(unsafe_path(&current.child_path(name), "symlink_refused"));
                }
                FilesystemObjectType::Directory if index + 1 != components.len() => {
                    let child = current.open_dir(name)?;
                    let opened = child.metadata()?;
                    if identity(&opened) != identity(&metadata) {
                        return Err(unsafe_path(child.path(), "directory_identity_changed"));
                    }
                    current = child;
                }
                _ => {
                    return Err(OrchestratorError::OverwriteRefused {
                        path: current.child_path(name).display().to_string(),
                    });
                }
            }
        }
    }
    Ok(())
}

fn identity(metadata: &FilesystemMetadata) -> (u64, u64) {
    (metadata.device, metadata.inode)
}

fn unsafe_path(path: &Path, reason: &str) -> OrchestratorError {
    OrchestratorError::UnsafePath {
        path: path.display().to_string(),
        reason: reason.to_owned(),
    }
}
