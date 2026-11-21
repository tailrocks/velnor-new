// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::fs::File;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use nix::errno::Errno;
use nix::fcntl::{AtFlags, OFlag, openat, renameat};
use nix::sys::stat::{Mode, SFlag, fstatat, mkdirat};
use nix::unistd::{UnlinkatFlags, unlinkat};

use crate::error::CiError;

const REGISTRY: &str = "registry";
const INDEX: &str = "index";
const ENTRIES: &str = ".cache";
const CONFIG: &str = "config.json";

pub(super) fn keep(scratch: &Path, cache: &Path) -> Result<(), CiError> {
    let source = scratch.join(REGISTRY).join(INDEX);
    let Ok(read) = std::fs::read_dir(&source) else {
        return Ok(());
    };
    let mut roots = Vec::new();
    for entry in read {
        let entry = entry.map_err(|e| failed(&source, &e))?;
        let name = entry.file_name();
        if name.as_encoded_bytes().starts_with(b".") {
            continue;
        }
        let kind = entry.file_type().map_err(|e| failed(&entry.path(), &e))?;
        if kind.is_symlink() {
            return Err(linked(&entry.path()));
        }
        if kind.is_dir() {
            roots.push(name);
        }
    }
    if roots.is_empty() {
        return Ok(());
    }
    let home = File::options()
        .read(true)
        .custom_flags(libc::O_DIRECTORY)
        .open(cache)
        .map_err(|e| failed(cache, &e))?;
    let target = cache.join(REGISTRY).join(INDEX);
    let registry = descend(home.as_fd(), OsStr::new(REGISTRY), &cache.join(REGISTRY))?;
    let index = descend(registry.as_fd(), OsStr::new(INDEX), &target)?;
    for name in roots {
        copy_root(
            &source.join(&name),
            index.as_fd(),
            &name,
            &target.join(&name),
        )?;
    }
    Ok(())
}

fn copy_root(source: &Path, index: impl AsFd, name: &OsStr, target: &Path) -> Result<(), CiError> {
    let entries = source.join(ENTRIES);
    let config = source.join(CONFIG);
    let has_entries = present(&entries)?;
    let has_config = present(&config)?;
    if !has_entries && !has_config {
        return Ok(());
    }
    let root = descend(index.as_fd(), name, target)?;
    if has_entries {
        copy_directory(&entries, root.as_fd(), &target.join(ENTRIES))?;
    }
    if has_config {
        copy_file(&config, root.as_fd(), &target.join(CONFIG))?;
    }
    Ok(())
}

fn present(path: &Path) -> Result<bool, CiError> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(linked(path)),
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(failed(path, &e)),
    }
}

fn copy_directory(source: &Path, directory: impl AsFd, target: &Path) -> Result<(), CiError> {
    let name = source.file_name().unwrap_or_default();
    let here = descend(directory.as_fd(), name, target)?;
    walk(source, here.as_fd(), target)
}

fn walk(source: &Path, directory: impl AsFd, target: &Path) -> Result<(), CiError> {
    for entry in std::fs::read_dir(source).map_err(|e| failed(source, &e))? {
        let entry = entry.map_err(|e| failed(source, &e))?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| failed(&path, &e))?;
        let name = entry.file_name();
        let destination = target.join(&name);
        if kind.is_symlink() {
            return Err(linked(&path));
        } else if kind.is_dir() {
            let child = descend(directory.as_fd(), &name, &destination)?;
            walk(&path, child.as_fd(), &destination)?;
        } else if kind.is_file() {
            write(&path, directory.as_fd(), &name, &destination)?;
        }
    }
    Ok(())
}

fn copy_file(source: &Path, directory: impl AsFd, target: &Path) -> Result<(), CiError> {
    let name = source.file_name().unwrap_or_default();
    write(source, directory, name, target)
}

fn descend(parent: impl AsFd, name: &OsStr, shown: &Path) -> Result<OwnedFd, CiError> {
    match mkdirat(parent.as_fd(), name, Mode::from_bits_truncate(0o755)) {
        Ok(()) | Err(Errno::EEXIST) => {}
        Err(e) => return Err(failed(shown, &e)),
    }
    let kind = kind(parent.as_fd(), name).map_err(|e| failed(shown, &e))?;
    if kind == Some(SFlag::S_IFLNK) {
        return Err(linked(shown));
    }
    openat(
        parent.as_fd(),
        name,
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| failed(shown, &e))
}

fn write(source: &Path, directory: impl AsFd, name: &OsStr, shown: &Path) -> Result<(), CiError> {
    match kind(directory.as_fd(), name) {
        Ok(None | Some(SFlag::S_IFREG)) => {}
        Ok(Some(_)) => {
            return Err(CiError::new(format!(
                "refusing to replace {}, which is a symbolic link or not a regular file",
                shown.display()
            )));
        }
        Err(e) => return Err(failed(shown, &e)),
    }
    let mut input = File::options()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(source)
        .map_err(|e| failed(source, &e))?;
    if !input.metadata().map_err(|e| failed(source, &e))?.is_file() {
        return Err(linked(source));
    }
    let mut temporary = name.to_os_string();
    temporary.push(format!(".velnor-{}", std::process::id()));
    let fd = openat(
        directory.as_fd(),
        temporary.as_os_str(),
        OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::from_bits_truncate(0o644),
    )
    .map_err(|e| failed(shown, &e))?;
    let mut output = File::from(fd);
    let copied = std::io::copy(&mut input, &mut output)
        .map_err(|e| failed(shown, &e))
        .and_then(|_| {
            renameat(
                directory.as_fd(),
                temporary.as_os_str(),
                directory.as_fd(),
                name,
            )
            .map_err(|e| failed(shown, &e))
        });
    if copied.is_err() {
        unlinkat(
            directory.as_fd(),
            temporary.as_os_str(),
            UnlinkatFlags::NoRemoveDir,
        )
        .unwrap_or(());
    }
    copied
}

fn kind(directory: impl AsFd, name: &OsStr) -> Result<Option<SFlag>, Errno> {
    match fstatat(directory, name, AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(stat) => Ok(Some(
            SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT,
        )),
        Err(Errno::ENOENT) => Ok(None),
        Err(e) => Err(e),
    }
}

fn linked(path: &Path) -> CiError {
    CiError::new(format!(
        "refusing to copy the registry index through a symbolic link at {}",
        path.display()
    ))
}

fn failed(path: &Path, error: &dyn std::fmt::Display) -> CiError {
    CiError::new(format!(
        "cannot keep the registry index entries at {}: {error}",
        path.display()
    ))
}
