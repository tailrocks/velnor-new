// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{DirBuilder, File};
use std::io::Write;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rustix::fs::{
    AtFlags, FileType, FlockOperation, Mode, OFlags, fchmod, flock, fstat, fsync, mkdirat, openat,
    renameat, statat, unlinkat,
};
use rustix::io::Errno;
use rustix::path::Arg;
use rustix::process::geteuid;

use self::remove::BOUNDS;
use super::machine::failure;
use super::{JIT_DIRECTORY, PRIVATE_DIRECTORIES};
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
        let store = Self::open(path)?;
        for name in PRIVATE_DIRECTORIES {
            store.ensure_directory(name)?;
        }
        Ok(store)
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
        require_no_acl(&self.path)
    }

    pub(super) fn write_jit(&self, name: &str, bytes: &[u8]) -> Result<(), HostError> {
        self.ensure_directory(JIT_DIRECTORY)?;
        let jit = self.jit_directory()?;
        let staging = format!(".{name}.tmp");
        let write = || -> Result<(), Errno> {
            remove_entry(&jit, &staging)?;
            let descriptor = openat(
                &jit,
                staging.as_str(),
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            )?;
            let mut file = File::from(descriptor);
            file.write_all(bytes).map_err(|_| Errno::IO)?;
            fsync(&file)?;
            renameat(&jit, staging.as_str(), &jit, name)
        };
        write().map_err(|_| {
            let _ = remove_entry(&jit, &staging);
            failure(RustRunnerFailure::JitFile)
        })
    }

    pub(super) fn remove_jit(&self, name: &str) -> Result<(), HostError> {
        let jit = self.jit_directory()?;
        remove_entry(&jit, name).map_err(|_| failure(RustRunnerFailure::HostStore))
    }

    pub(super) fn jit_remains(&self, name: &str) -> Result<bool, HostError> {
        exists(&self.jit_directory()?, name)
    }

    pub(super) fn empty_jit(&self) -> Result<(), HostError> {
        remove::contents(&self.jit_directory()?, BOUNDS).map_err(|error| failure(error.failure()))
    }

    fn jit_directory(&self) -> Result<OwnedFd, HostError> {
        let descriptor = openat(&self.root, JIT_DIRECTORY, directory_flags(), Mode::empty())
            .map_err(|_| failure(RustRunnerFailure::HostStore))?;
        require_directory(
            &descriptor,
            others_can_reach(),
            RustRunnerFailure::JitDirectoryAccessible,
        )?;
        Ok(descriptor)
    }

    fn ensure_directory(&self, name: &str) -> Result<(), HostError> {
        let descriptor = match openat(&self.root, name, directory_flags(), Mode::empty()) {
            Ok(descriptor) => descriptor,
            Err(Errno::NOENT) => self.create_directory(name)?,
            Err(Errno::ACCESS) => {
                remove::make_owner_accessible(&self.root, name)
                    .map_err(|_| failure(RustRunnerFailure::HostStore))?;
                openat(&self.root, name, directory_flags(), Mode::empty())
                    .map_err(|_| failure(RustRunnerFailure::HostStore))?
            }
            Err(Errno::LOOP | Errno::NOTDIR) => {
                remove_entry(&self.root, name)
                    .map_err(|_| failure(RustRunnerFailure::HostStore))?;
                self.create_directory(name)?
            }
            Err(_) => return Err(failure(RustRunnerFailure::HostStore)),
        };
        close_to_others(&descriptor)
    }

    fn create_directory(&self, name: &str) -> Result<OwnedFd, HostError> {
        match mkdirat(&self.root, name, Mode::RWXU) {
            Ok(()) | Err(Errno::EXIST) => {}
            Err(_) => return Err(failure(RustRunnerFailure::HostStore)),
        }
        openat(&self.root, name, directory_flags(), Mode::empty())
            .map_err(|_| failure(RustRunnerFailure::HostStore))
    }
}

fn close_to_others(descriptor: &OwnedFd) -> Result<(), HostError> {
    let stat = fstat(descriptor).map_err(|_| failure(RustRunnerFailure::HostStore))?;
    if Mode::from_raw_mode(stat.st_mode).intersects(others_can_reach())
        && FileType::from_raw_mode(stat.st_mode) == FileType::Directory
        && stat.st_uid == geteuid().as_raw()
    {
        fchmod(descriptor, Mode::RWXU).map_err(|_| failure(RustRunnerFailure::HostStore))?;
    }
    require_directory(
        descriptor,
        others_can_reach(),
        RustRunnerFailure::JitDirectoryAccessible,
    )
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

fn require_no_acl(path: &Path) -> Result<(), HostError> {
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
        .map_err(|_| failure(RustRunnerFailure::StoreUnsafe))?;
    let text =
        String::from_utf8(output.stdout).map_err(|_| failure(RustRunnerFailure::StoreUnsafe))?;
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let marker = lines
        .next()
        .and_then(|line| line.split_whitespace().next())
        .and_then(|mode| mode.as_bytes().get(10).copied());
    if output.status.success() && lines.next().is_none() && marker != Some(b'+') {
        Ok(())
    } else {
        Err(failure(RustRunnerFailure::StoreUnsafe))
    }
}

fn exists(directory: &OwnedFd, name: &str) -> Result<bool, HostError> {
    match statat(directory, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(_) => Ok(true),
        Err(Errno::NOENT) => Ok(false),
        Err(_) => Err(failure(RustRunnerFailure::HostStore)),
    }
}

fn remove_entry(directory: &impl AsFd, name: impl Arg) -> Result<(), Errno> {
    match unlinkat(directory, name, AtFlags::empty()) {
        Ok(()) | Err(Errno::NOENT) => Ok(()),
        Err(error) => Err(error),
    }
}
