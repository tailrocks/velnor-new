// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/executor.rs

use super::anchor::Anchor;
use super::pattern::GlobError;
use rustix::fs::{FileType, fstat};
use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};

pub(crate) struct Found {
    pub(crate) root: usize,
    pub(crate) path: PathBuf,
    pub(crate) file: File,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Hint {
    Directory,
    File,
    Symlink,
    Other,
    Unknown,
}

impl Hint {
    pub(super) fn of(kind: FileType) -> Self {
        match kind {
            FileType::Directory => Self::Directory,
            FileType::RegularFile => Self::File,
            FileType::Symlink => Self::Symlink,
            FileType::Unknown => Self::Unknown,
            _ => Self::Other,
        }
    }
}

pub(super) struct Visit {
    pub(super) root: usize,
    pub(super) path: PathBuf,
    pub(super) reals: Vec<PathBuf>,
    pub(super) comps: Vec<String>,
    pub(super) anchor: Anchor,
    pub(super) name: OsString,
    pub(super) hint: Hint,
    pub(super) matched: bool,
    pub(super) partial: bool,
    pub(super) missing_ok: bool,
}

pub(super) struct Child {
    pub(super) anchor: Anchor,
    pub(super) name: OsString,
    pub(super) hint: Hint,
}

pub(super) fn skip_missing<T>(result: io::Result<T>, path: &Path) -> Result<Option<T>, GlobError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(GlobError::io(path, error)),
    }
}

pub(super) fn kind_of(fd: &OwnedFd, path: &Path) -> Result<FileType, GlobError> {
    fstat(fd)
        .map(|stat| FileType::from_raw_mode(stat.st_mode))
        .map_err(|error| GlobError::io(path, error.into()))
}

pub(super) fn not_regular(path: &Path) -> GlobError {
    GlobError::NotRegularFile(path.display().to_string())
}
