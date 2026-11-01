// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path};

use rustix::fs::{FileType, Mode, OFlags};
use rustix::io::Errno;

const SYMLINK_REJECTED: &str = "symlinks are not allowed";

pub(crate) fn read_regular(root: &Path, relative: &str, cap: usize) -> Result<Vec<u8>, String> {
    let relative = Path::new(relative);
    let mut path = root.to_path_buf();
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err(format!("{} is not a repository path", relative.display()));
        };
        path.push(name);
        if components.peek().is_none() {
            break;
        }
        let file_type = fs::symlink_metadata(&path)
            .map_err(|error| format!("unreadable ({error})"))?
            .file_type();
        if file_type.is_symlink() {
            return Err(format!("{}: {SYMLINK_REJECTED}", path.display()));
        }
        if !file_type.is_dir() {
            return Err(format!("{} is not a directory", path.display()));
        }
    }
    let fd = rustix::fs::open(
        &path,
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| match error {
        Errno::LOOP => format!("{}: {SYMLINK_REJECTED}", path.display()),
        other => format!("unreadable ({})", std::io::Error::from(other)),
    })?;
    let stat = rustix::fs::fstat(&fd)
        .map_err(|error| format!("unreadable ({})", std::io::Error::from(error)))?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
        return Err(format!("{} is not a regular file", path.display()));
    }
    let limit = u64::try_from(cap).unwrap_or(u64::MAX).saturating_add(1);
    let mut bytes = Vec::new();
    File::from(fd)
        .take(limit)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("unreadable ({error})"))?;
    if bytes.len() > cap {
        return Err(format!("exceeds {cap} bytes"));
    }
    Ok(bytes)
}
