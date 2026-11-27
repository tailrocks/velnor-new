// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::{CStr, OsStr};
use std::fs::File;
use std::io::Read;
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rustix::fs::{AtFlags, CWD, Dir, FileType, Mode, OFlags};
use rustix::io::Errno;

use super::held::Held;
use super::remove_entry;

const MAX_DEPTH: usize = 32;
const MAX_REF_BYTES: u64 = 256;

#[derive(PartialEq, Eq)]
enum Refs {
    Sound,
    Replace,
}

fn is_ref(body: &[u8]) -> bool {
    let line = body.strip_suffix(b"\n").unwrap_or(body);
    let id = matches!(line.len(), 40 | 64) && line.iter().all(u8::is_ascii_hexdigit);
    let symbolic = line
        .strip_prefix(b"ref: refs/")
        .is_some_and(|name| !name.is_empty() && !name.contains(&b'\n'));
    id || symbolic
}

fn holds_a_ref(dir: &OwnedFd, name: &CStr) -> bool {
    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    let Ok(fd) = rustix::fs::openat(dir, name, flags, Mode::empty()) else {
        return false;
    };
    let mut body = Vec::new();
    File::from(fd)
        .take(MAX_REF_BYTES + 1)
        .read_to_end(&mut body)
        .is_ok_and(|read| read as u64 <= MAX_REF_BYTES && is_ref(&body))
}

fn unlink(dir: &OwnedFd, name: &CStr) -> Result<()> {
    match rustix::fs::unlinkat(dir, name, AtFlags::empty()) {
        Ok(()) | Err(Errno::NOENT) => Ok(()),
        Err(error) => Err(error).context("remove a refs entry"),
    }
}

fn remove_tree(path: &Path) -> Result<()> {
    match std::fs::remove_dir_all(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other.with_context(|| format!("remove {}", path.display())),
    }
}

fn tidy_dir(dir: &OwnedFd, path: &Path, depth: usize) -> Result<Refs> {
    let entries = Dir::read_from(dir).context("read a refs directory")?;
    for entry in entries {
        let entry = entry.context("read a refs entry")?;
        let name = entry.file_name();
        if matches!(name.to_bytes(), b"." | b"..") {
            continue;
        }
        let locked = name.to_bytes().ends_with(b".lock");
        let kind = rustix::fs::statat(dir, name, AtFlags::SYMLINK_NOFOLLOW)
            .map(|stat| FileType::from_raw_mode(stat.st_mode));
        match kind {
            Ok(FileType::Directory) if locked => {
                remove_tree(&path.join(OsStr::from_bytes(name.to_bytes())))?;
            }
            Ok(FileType::Directory) => {
                if depth >= MAX_DEPTH {
                    return Ok(Refs::Replace);
                }
                let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
                match rustix::fs::openat(dir, name, flags, Mode::empty()) {
                    Ok(child) => {
                        let child_path = path.join(OsStr::from_bytes(name.to_bytes()));
                        if tidy_dir(&child, &child_path, depth + 1)? == Refs::Replace {
                            return Ok(Refs::Replace);
                        }
                    }
                    Err(Errno::NOENT) => {}
                    Err(Errno::LOOP | Errno::NOTDIR) => unlink(dir, name)?,
                    Err(error) => return Err(error).context("open a refs directory"),
                }
            }
            Ok(FileType::RegularFile) if !locked && holds_a_ref(dir, name) => {}
            Ok(_) => unlink(dir, name)?,
            Err(Errno::NOENT) => {}
            Err(error) => return Err(error).context("inspect a refs entry"),
        }
    }
    Ok(Refs::Sound)
}

fn tidy_blocking(mirror: &Path) -> Result<Refs> {
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC;
    let mirror_fd = rustix::fs::openat(CWD, mirror, flags, Mode::empty())
        .with_context(|| format!("open {}", mirror.display()))?;
    match rustix::fs::openat(&mirror_fd, "refs", flags | OFlags::NOFOLLOW, Mode::empty()) {
        Ok(refs) => tidy_dir(&refs, &mirror.join("refs"), 0),
        Err(Errno::NOENT) => Ok(Refs::Sound),
        Err(Errno::LOOP | Errno::NOTDIR) => Ok(Refs::Replace),
        Err(error) => Err(error).context("open the mirror refs directory"),
    }
}

pub(super) async fn tidy(mirror: &Path, held: &Held) -> Result<()> {
    let replace = {
        let mirror: PathBuf = mirror.to_owned();
        held.blocking(move || tidy_blocking(&mirror)).await??
    };
    if replace == Refs::Replace {
        let refs = mirror.join("refs");
        remove_entry(&refs).await?;
        tokio::fs::create_dir(&refs)
            .await
            .with_context(|| format!("create {}", refs.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
