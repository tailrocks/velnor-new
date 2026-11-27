// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use rustix::fs::{Dir, FileType, Mode, OFlags, StatVfsMountFlags, fstatvfs, open, openat};
use rustix::io::Errno;

use crate::exec::ExecError;

fn directory() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
}

fn io(path: &Path) -> impl FnOnce(Errno) -> ExecError {
    let context = format!("scan {}", path.display());
    move |error| ExecError::io(context, error.into())
}

fn visit(parent: &OwnedFd, path: &Path, found: &mut Vec<PathBuf>) -> Result<(), ExecError> {
    for entry in Dir::read_from(parent).map_err(io(path))? {
        let entry = entry.map_err(io(path))?;
        let name = entry.file_name();
        let walkable = matches!(entry.file_type(), FileType::Directory | FileType::Unknown);
        if !walkable || matches!(name.to_bytes(), b"." | b"..") {
            continue;
        }
        let child_path = path.join(OsStr::from_bytes(name.to_bytes()));
        let child = match openat(parent, name, directory(), Mode::empty()) {
            Ok(child) => child,
            Err(Errno::NOTDIR) => continue,
            Err(error) => return Err(io(&child_path)(error)),
        };
        let stats = fstatvfs(&child).map_err(io(&child_path))?;
        if stats.f_flag.contains(StatVfsMountFlags::RDONLY) {
            visit(&child, &child_path, found)?;
        } else {
            found.push(child_path);
        }
    }
    Ok(())
}

fn writable_below(root: &Path) -> Result<Vec<PathBuf>, ExecError> {
    let descriptor = open(root, directory(), Mode::empty()).map_err(io(root))?;
    let mut found = Vec::new();
    visit(&descriptor, root, &mut found)?;
    Ok(found)
}

pub(super) async fn writable_directories(roots: Vec<PathBuf>) -> Result<Vec<PathBuf>, ExecError> {
    tokio::task::spawn_blocking(move || {
        let mut found = Vec::new();
        for root in &roots {
            found.extend(writable_below(root)?);
        }
        Ok(found)
    })
    .await
    .map_err(|error| ExecError::Invalid(format!("mise policy: the mount scan failed: {error}")))?
}
