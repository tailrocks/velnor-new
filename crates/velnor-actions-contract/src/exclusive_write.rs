// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::Write;
use std::path::{Component, Path};

#[cfg(unix)]
use rustix::fs::{Mode, OFlags, open, openat};

use crate::helper_error::HelperError;

pub fn write_exclusive(path: &Path, bytes: &[u8], context: &str) -> Result<(), HelperError> {
    write_exclusive_until(path, bytes, context, || Ok::<(), HelperError>(()))
}

pub fn reserve_exclusive(path: &Path, context: &str) -> Result<fs::File, HelperError> {
    let file = open_exclusive(path, context)?;
    if !created_identity(&file, path) {
        return Err(HelperError::internal("symlink_refused"));
    }
    Ok(file)
}

pub fn verify_reserved(file: &fs::File, path: &Path) -> Result<(), HelperError> {
    if created_identity(file, path) {
        Ok(())
    } else {
        Err(HelperError::internal("symlink_refused"))
    }
}

pub fn write_exclusive_until<E: From<HelperError>>(
    path: &Path,
    bytes: &[u8],
    context: &str,
    mut checkpoint: impl FnMut() -> Result<(), E>,
) -> Result<(), E> {
    checkpoint()?;
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(HelperError::internal("symlink_refused").into());
        }
        Ok(_) => return Err(HelperError::internal(&format!("{context}_exists")).into()),
        Err(_) => {}
    }
    let mut file = open_exclusive(path, context)?;
    if !created_identity(&file, path) {
        return Err(HelperError::internal("symlink_refused").into());
    }
    for chunk in bytes.chunks(64 * 1024) {
        checkpoint()?;
        file.write_all(chunk)
            .map_err(|_| HelperError::internal(&format!("{context}_unwritable")))?;
        checkpoint()?;
    }
    Ok(())
}

#[cfg(unix)]
fn open_exclusive(path: &Path, context: &str) -> Result<std::fs::File, HelperError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(Path::new)
        .ok_or_else(|| HelperError::internal(&format!("{context}_unwritable")))?;
    let parent_fd = open(
        parent,
        OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| parent_open_error(parent, context))?;
    let created = openat(
        &parent_fd,
        file_name,
        OFlags::CREATE | OFlags::EXCL | OFlags::WRONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        file_mode(),
    )
    .map_err(|err| child_open_error(err, context))?;
    Ok(std::fs::File::from(created))
}

#[cfg(not(unix))]
fn open_exclusive(path: &Path, context: &str) -> Result<std::fs::File, HelperError> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| HelperError::internal(&format!("{context}_exists")))
}

#[cfg(unix)]
fn file_mode() -> Mode {
    Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::WGRP | Mode::ROTH | Mode::WOTH
}

#[cfg(unix)]
fn parent_open_error(parent: &Path, context: &str) -> HelperError {
    if fs::symlink_metadata(parent).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return HelperError::internal("symlink_refused");
    }
    HelperError::internal(&format!("{context}_unwritable"))
}

#[cfg(unix)]
fn child_open_error(err: rustix::io::Errno, context: &str) -> HelperError {
    if err == rustix::io::Errno::EXIST {
        return HelperError::internal(&format!("{context}_exists"));
    }
    if err == rustix::io::Errno::LOOP {
        return HelperError::internal("symlink_refused");
    }
    HelperError::internal(&format!("{context}_unwritable"))
}

pub fn create_dir_no_symlink(anchor: &Path, dir: &Path) -> Result<(), HelperError> {
    let relative = dir
        .strip_prefix(anchor)
        .map_err(|_| HelperError::internal("anchor_escape"))?;
    let mut parts = Vec::new();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err(HelperError::internal("anchor_escape"));
        };
        parts.push(part);
    }
    let mut current = anchor.to_path_buf();
    for part in parts {
        current.push(part);
        refuse_if_link(&current)?;
        if !current.is_dir() {
            match fs::create_dir(&current) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(err) => {
                    return Err(HelperError::io(
                        current.display().to_string(),
                        err.to_string(),
                    ));
                }
            }
        }
        refuse_if_link(&current)?;
    }
    Ok(())
}

fn refuse_if_link(path: &Path) -> Result<(), HelperError> {
    if fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(HelperError::internal("symlink_refused"));
    }
    Ok(())
}

#[cfg(unix)]
fn created_identity(file: &fs::File, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    if fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return false;
    }
    match (file.metadata(), fs::metadata(path)) {
        (Ok(open), Ok(at_path)) => open.dev() == at_path.dev() && open.ino() == at_path.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn created_identity(_file: &fs::File, path: &Path) -> bool {
    !fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}
