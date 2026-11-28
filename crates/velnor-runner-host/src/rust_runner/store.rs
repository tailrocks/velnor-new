// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::DirBuilder;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rustix::fs::{AtFlags, FileType, FlockOperation, Mode, OFlags, flock, fstat, openat, unlinkat};
use rustix::io::Errno;
use rustix::path::Arg;
use rustix::process::geteuid;

use self::remove::bounds_on;
use super::OBSOLETE_DIRECTORIES;
use super::machine::failure;
use crate::error::{HostError, RustRunnerFailure};

mod remove;

const LOCK_FILE: &str = "host.lock";

#[derive(Debug)]
pub struct HostHold {
    _lock: OwnedFd,
}

fn others_can_reach() -> Mode {
    Mode::RWXG | Mode::RWXO
}

fn directory_flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
}

pub(super) struct Store {
    root: OwnedFd,
    path: PathBuf,
}

impl Store {
    pub(super) fn open(path: &Path) -> Result<Self, HostError> {
        let root = rustix::fs::open(path, directory_flags(), Mode::empty())
            .map_err(|_| failure(RustRunnerFailure::HostStore))?;
        require_directory(&root, others_can_reach(), RustRunnerFailure::StoreUnsafe)?;
        Ok(Self {
            root,
            path: path.to_path_buf(),
        })
    }

    pub(super) fn create(path: &Path) -> Result<Self, HostError> {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
            .map_err(|_| failure(RustRunnerFailure::HostStore))?;
        Self::open(path)
    }

    pub(super) fn hold(&self) -> Result<HostHold, HostError> {
        let descriptor = openat(
            &self.root,
            LOCK_FILE,
            OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(|_| failure(RustRunnerFailure::HostStore))?;
        match flock(&descriptor, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => Ok(HostHold { _lock: descriptor }),
            Err(Errno::WOULDBLOCK) => Err(failure(RustRunnerFailure::HostStoreHeld)),
            Err(_) => Err(failure(RustRunnerFailure::HostStore)),
        }
    }

    pub(super) fn require_no_acl(&self) -> Result<(), HostError> {
        require_no_acl(&self.path, RustRunnerFailure::StoreUnsafe)
    }

    pub(super) fn remove_obsolete(&self) -> Result<(), HostError> {
        OBSOLETE_DIRECTORIES
            .iter()
            .try_for_each(|name| self.remove_tree(name))
    }

    fn remove_tree(&self, name: &str) -> Result<(), HostError> {
        let stored = || failure(RustRunnerFailure::HostStore);
        let device = fstat(&self.root)
            .map(|stat| remove::device_of(&stat))
            .map_err(|_| stored())?;
        let tree = match openat(&self.root, name, directory_flags(), Mode::empty()) {
            Ok(tree) => tree,
            Err(Errno::NOENT) => return Ok(()),
            Err(Errno::ACCESS) => {
                remove::require_device(&self.root, name, device)
                    .map_err(|error| failure(error.failure()))?;
                remove::make_owner_accessible(&self.root, name).map_err(|_| stored())?;
                openat(&self.root, name, directory_flags(), Mode::empty()).map_err(|_| stored())?
            }
            Err(Errno::LOOP | Errno::NOTDIR) => {
                return remove_entry(&self.root, name).map_err(|_| stored());
            }
            Err(_) => return Err(stored()),
        };
        remove::contents(&tree, bounds_on(device)).map_err(|error| failure(error.failure()))?;
        unlinkat(&self.root, name, AtFlags::REMOVEDIR).map_err(|_| stored())
    }
}

fn require_directory(
    descriptor: &impl AsFd,
    forbidden: Mode,
    refusal: RustRunnerFailure,
) -> Result<(), HostError> {
    let stat = fstat(descriptor).map_err(|_| failure(RustRunnerFailure::HostStore))?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory {
        return Err(failure(RustRunnerFailure::HostStore));
    }
    if stat.st_uid != geteuid().as_raw() || Mode::from_raw_mode(stat.st_mode).intersects(forbidden)
    {
        return Err(failure(refusal));
    }
    Ok(())
}

pub(super) fn require_no_acl(path: &Path, refusal: RustRunnerFailure) -> Result<(), HostError> {
    let mut command = Command::new("/bin/ls");
    command.env_clear().arg(if cfg!(target_os = "macos") {
        "-lde"
    } else {
        "-ld"
    });
    let output = command
        .arg("--")
        .arg(path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| failure(refusal))?;
    let text = String::from_utf8(output.stdout).map_err(|_| failure(refusal))?;
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let marker = lines
        .next()
        .and_then(|line| line.split_whitespace().next())
        .and_then(|mode| mode.as_bytes().get(10).copied());
    if output.status.success() && lines.next().is_none() && marker != Some(b'+') {
        Ok(())
    } else {
        Err(failure(refusal))
    }
}

fn remove_entry(directory: &impl AsFd, name: impl Arg) -> Result<(), Errno> {
    match unlinkat(directory, name, AtFlags::empty()) {
        Ok(()) | Err(Errno::NOENT) => Ok(()),
        Err(error) => Err(error),
    }
}
