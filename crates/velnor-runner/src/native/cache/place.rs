// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::fs::File;
use std::io::Read;
use std::os::fd::BorrowedFd;

use anyhow::{Context, Result, bail, ensure};
use rustix::fs::{
    AtFlags, FileType, Mode, OFlags, RawMode, fchmod, linkat, mkdirat, openat, statat, symlinkat,
    unlinkat,
};
use rustix::io::Errno;
use tar::{Entry, EntryType};

use super::archive::checkpoint;
use super::guard::Watched;
use super::stamp::Stamp;
use crate::broker::safe_fs::directory_flags;
use crate::exec::JobCancel;
use crate::native::Umask;

const OWNER_ACCESS: u32 = 0o700;

pub(super) enum Placed {
    Directory { bits: u32 },
    Other,
}

pub(super) fn mode_of(bits: u32) -> Mode {
    Mode::from_bits_truncate(RawMode::try_from(bits).unwrap_or_default())
}

fn kind_of(parent: BorrowedFd<'_>, name: &OsStr) -> Result<Option<FileType>> {
    match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => Ok(Some(FileType::from_raw_mode(stat.st_mode))),
        Err(Errno::NOENT) => Ok(None),
        Err(error) => Err(error).with_context(|| format!("inspect {}", name.display())),
    }
}

pub(super) fn remove_existing(parent: BorrowedFd<'_>, name: &OsStr) -> Result<()> {
    match kind_of(parent, name)? {
        None => return Ok(()),
        Some(FileType::Directory) => bail!(
            "cache restore never writes through a link: {} is a directory that a link or file would replace",
            name.display()
        ),
        Some(_) => {}
    }
    match unlinkat(parent, name, AtFlags::empty()) {
        Ok(()) | Err(Errno::NOENT) => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("replace {} in the restore", name.display()))
        }
    }
}

fn make_directory(parent: BorrowedFd<'_>, name: &OsStr, bits: u32) -> Result<()> {
    let mode = mode_of(bits | OWNER_ACCESS);
    match mkdirat(parent, name, mode) {
        Ok(()) => {}
        Err(Errno::EXIST) => match kind_of(parent, name)? {
            Some(FileType::Directory) => {}
            Some(FileType::Symlink) => bail!(
                "cache restore never writes through a link: {} is not a directory",
                name.display()
            ),
            _ => {
                remove_existing(parent, name)?;
                mkdirat(parent, name, mode)
                    .with_context(|| format!("create {}", name.display()))?;
            }
        },
        Err(error) => return Err(error).with_context(|| format!("create {}", name.display())),
    }
    let directory = openat(parent, name, directory_flags(), Mode::empty())
        .with_context(|| format!("open {}", name.display()))?;
    fchmod(&directory, mode).with_context(|| format!("set the mode of {}", name.display()))
}

fn make_file<R: Read>(
    parent: BorrowedFd<'_>,
    name: &OsStr,
    entry: &mut Entry<'_, R>,
    bits: u32,
    stamp: Option<Stamp>,
    cancel: Option<&JobCancel>,
) -> Result<()> {
    remove_existing(parent, name)?;
    let fd = openat(
        parent,
        name,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        mode_of(0o600),
    )
    .with_context(|| format!("create {}", name.display()))?;
    let mut file = File::from(fd);
    let copied = std::io::copy(
        &mut Watched {
            inner: entry,
            cancel,
        },
        &mut file,
    );
    checkpoint(cancel)?;
    copied.with_context(|| format!("write {}", name.display()))?;
    fchmod(&file, mode_of(bits)).with_context(|| format!("set the mode of {}", name.display()))?;
    match stamp {
        Some(stamp) => stamp
            .apply(&file)
            .with_context(|| format!("set the time of {}", name.display())),
        None => Ok(()),
    }
}

fn make_link<R: Read>(
    parent: BorrowedFd<'_>,
    name: &OsStr,
    entry: &Entry<'_, R>,
    stamp: Option<Stamp>,
) -> Result<()> {
    let target = entry
        .link_name()
        .context("read a symlink target")?
        .context("a symlink member has no target")?;
    remove_existing(parent, name)?;
    symlinkat(&*target, parent, name)
        .with_context(|| format!("create the link {}", name.display()))?;
    match stamp {
        Some(stamp) => stamp
            .apply_to_link(parent, name)
            .with_context(|| format!("set the time of {}", name.display())),
        None => Ok(()),
    }
}

pub(super) fn link_file(
    source: BorrowedFd<'_>,
    from: &OsStr,
    parent: BorrowedFd<'_>,
    name: &OsStr,
) -> Result<()> {
    let target = statat(source, from, AtFlags::SYMLINK_NOFOLLOW)
        .with_context(|| format!("inspect the target of the hard link {}", name.display()))?;
    ensure!(
        FileType::from_raw_mode(target.st_mode) == FileType::RegularFile,
        "the hard link {} does not name a regular file of this archive",
        name.display()
    );
    match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(seen) if (seen.st_dev, seen.st_ino) == (target.st_dev, target.st_ino) => {
            return Ok(());
        }
        Ok(_) | Err(Errno::NOENT) => {}
        Err(error) => {
            return Err(error).with_context(|| format!("inspect {}", name.display()));
        }
    }
    remove_existing(parent, name)?;
    linkat(source, from, parent, name, AtFlags::empty())
        .with_context(|| format!("create the hard link {}", name.display()))
}

pub(super) fn place<R: Read>(
    parent: BorrowedFd<'_>,
    name: &OsStr,
    entry: &mut Entry<'_, R>,
    stamp: Option<Stamp>,
    umask: Umask,
    cancel: Option<&JobCancel>,
) -> Result<Placed> {
    let bits = umask.restored_mode(entry.header());
    match entry.header().entry_type() {
        EntryType::Directory => {
            make_directory(parent, name, bits)?;
            Ok(Placed::Directory { bits })
        }
        EntryType::Regular => {
            make_file(parent, name, entry, bits, stamp, cancel).map(|()| Placed::Other)
        }
        EntryType::Symlink => make_link(parent, name, entry, stamp).map(|()| Placed::Other),
        other => bail!(
            "cache archive member '{}' has unsupported type {other:?}",
            name.display()
        ),
    }
}
