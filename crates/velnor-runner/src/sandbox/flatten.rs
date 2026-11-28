// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::CString;
use std::fmt;
use std::os::fd::OwnedFd;
use std::path::Path;

use anyhow::{Context, Result};
use rustix::fs::{
    AtFlags, CWD, Dir, FileType, Mode, RenameFlags, fstat, openat, renameat_with, statat, unlinkat,
};
use rustix::io::Errno;

use super::tree::directory_flags;

const BATCH: usize = 512;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Wedged {
    OtherDevice(String),
    NotEmpty(String),
}

impl fmt::Display for Wedged {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OtherDevice(name) => write!(formatter, "{name} is on another device"),
            Self::NotEmpty(name) => write!(formatter, "{name} cannot be emptied"),
        }
    }
}

impl std::error::Error for Wedged {}

struct Flattening {
    top: OwnedFd,
    device: i128,
    next: u64,
}

fn lossy(name: &CString) -> String {
    name.to_string_lossy().into_owned()
}

fn skipped(name: &std::ffi::CStr) -> bool {
    matches!(name.to_bytes(), b"." | b"..")
}

impl Flattening {
    fn is_directory(&self, parent: &OwnedFd, name: &CString, listed: FileType) -> Result<bool> {
        if listed != FileType::Directory && listed != FileType::Unknown {
            return Ok(false);
        }
        let stat = statat(parent, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)
            .with_context(|| format!("inspect {}", lossy(name)))?;
        if FileType::from_raw_mode(stat.st_mode) != FileType::Directory {
            return Ok(false);
        }
        if i128::from(stat.st_dev) != self.device {
            return Err(Wedged::OtherDevice(lossy(name)).into());
        }
        Ok(true)
    }

    fn lift(&mut self, parent: &OwnedFd, name: &CString) -> Result<()> {
        loop {
            let target = CString::new(format!(".velnor-flat-{}", self.next))
                .context("name a flattened directory")?;
            self.next += 1;
            match renameat_with(
                parent,
                name.as_c_str(),
                &self.top,
                target.as_c_str(),
                RenameFlags::NOREPLACE,
            ) {
                Ok(()) => return Ok(()),
                Err(Errno::EXIST) => {}
                Err(error) => return Err(error).with_context(|| format!("lift {}", lossy(name))),
            }
        }
    }

    fn batch(directory: &OwnedFd) -> Result<Vec<(CString, FileType)>> {
        let mut entries = Dir::read_from(directory).context("list a directory")?;
        let mut batch = Vec::new();
        while batch.len() < BATCH {
            let Some(entry) = entries.read() else { break };
            let entry = entry.context("read a directory entry")?;
            if !skipped(entry.file_name()) {
                batch.push((entry.file_name().to_owned(), entry.file_type()));
            }
        }
        Ok(batch)
    }

    fn drain(&mut self, directory: &OwnedFd, is_top: bool) -> Result<()> {
        loop {
            let batch = Self::batch(directory)?;
            let complete = batch.len() < BATCH;
            let empty = batch.is_empty();
            for (name, kind) in batch {
                if !self.is_directory(directory, &name, kind)? {
                    unlinkat(directory, name.as_c_str(), AtFlags::empty())
                        .with_context(|| format!("remove {}", lossy(&name)))?;
                } else if is_top {
                    self.descend(&name)?;
                } else {
                    self.lift(directory, &name)?;
                }
            }
            if empty || (complete && !is_top) {
                return Ok(());
            }
        }
    }

    fn descend(&mut self, name: &CString) -> Result<()> {
        let child = openat(&self.top, name.as_c_str(), directory_flags(), Mode::empty())
            .with_context(|| format!("open {}", lossy(name)))?;
        self.drain(&child, false)?;
        drop(child);
        match unlinkat(&self.top, name.as_c_str(), AtFlags::REMOVEDIR) {
            Ok(()) => Ok(()),
            Err(Errno::NOTEMPTY) => Err(Wedged::NotEmpty(lossy(name)).into()),
            Err(error) => Err(error).with_context(|| format!("remove {}", lossy(name))),
        }
    }
}

pub(super) fn empty(root: &Path) -> Result<()> {
    let top = match openat(CWD, root, directory_flags(), Mode::empty()) {
        Ok(top) => top,
        Err(Errno::NOENT) => return Ok(()),
        Err(error) => return Err(error).with_context(|| format!("open {}", root.display())),
    };
    let device = i128::from(fstat(&top).context("inspect the top directory")?.st_dev);
    let mut flattening = Flattening {
        top,
        device,
        next: 0,
    };
    let top = openat(&flattening.top, ".", directory_flags(), Mode::empty())
        .context("reopen the top directory")?;
    flattening.drain(&top, true)
}
