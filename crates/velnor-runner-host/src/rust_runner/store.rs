// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{DirBuilder, File};
use std::io::Read;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rustix::fs::{
    AtFlags, FileType, FlockOperation, Mode, OFlags, flock, fstat, mkdirat, openat, statat,
};
use rustix::io::Errno;
use rustix::process::geteuid;
use sha2::{Digest, Sha256};

use super::HOST_DIRECTORIES;
use super::machine::failure;
use crate::digest_hex::lowercase_hex;
use crate::error::{HostError, RustRunnerFailure};

const RUNNER_BINARY: &str = "velnor-runner";
const BIN_DIRECTORY: &str = "bin";
const LOCK_FILE: &str = "host.lock";

#[derive(Debug)]
pub struct HostHold {
    _lock: OwnedFd,
}

fn others_can_write() -> Mode {
    Mode::WGRP | Mode::WOTH
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

    pub(super) fn prepare(path: &Path) -> Result<Self, HostError> {
        let store = Self::create(path)?;
        store.require_runner_binary()?;
        Ok(store)
    }

    pub(super) fn create(path: &Path) -> Result<Self, HostError> {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
            .map_err(|_| failure(RustRunnerFailure::HostStore))?;
        let store = Self::open(path)?;
        for name in HOST_DIRECTORIES {
            store.ensure_directory(name, others_can_write(), RustRunnerFailure::StoreUnsafe)?;
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

    pub(super) fn require_runner_binary(&self) -> Result<(), HostError> {
        let bin = self.directory(
            BIN_DIRECTORY,
            others_can_write(),
            RustRunnerFailure::StoreUnsafe,
        )?;
        let stat = match statat(&bin, RUNNER_BINARY, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => stat,
            Err(Errno::NOENT) => return Err(failure(RustRunnerFailure::BinaryMissing)),
            Err(_) => return Err(failure(RustRunnerFailure::HostStore)),
        };
        let mode = Mode::from_raw_mode(stat.st_mode);
        let safe = FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile
            && stat.st_uid == geteuid().as_raw()
            && !mode.intersects(others_can_write())
            && mode.contains(Mode::XUSR);
        if !safe {
            return Err(failure(RustRunnerFailure::BinaryUnsafe));
        }
        let bin_path = self.path.join(BIN_DIRECTORY);
        for path in [
            self.path.clone(),
            bin_path.clone(),
            bin_path.join(RUNNER_BINARY),
        ] {
            require_no_acl(&path)?;
        }
        Ok(())
    }

    pub(super) fn runner_digest(&self) -> Result<String, HostError> {
        let bin = self.directory(
            BIN_DIRECTORY,
            others_can_write(),
            RustRunnerFailure::StoreUnsafe,
        )?;
        let descriptor = openat(
            &bin,
            RUNNER_BINARY,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| failure(RustRunnerFailure::BinaryMissing))?;
        let mut file = File::from(descriptor);
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; 64 * 1024];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|_| failure(RustRunnerFailure::HostStore))?;
            if read == 0 {
                return Ok(lowercase_hex(&hasher.finalize()));
            }
            hasher.update(&buffer[..read]);
        }
    }

    fn directory(
        &self,
        name: &str,
        forbidden: Mode,
        refusal: RustRunnerFailure,
    ) -> Result<OwnedFd, HostError> {
        let descriptor = openat(&self.root, name, directory_flags(), Mode::empty())
            .map_err(|_| failure(RustRunnerFailure::HostStore))?;
        require_directory(&descriptor, forbidden, refusal)?;
        Ok(descriptor)
    }

    fn ensure_directory(
        &self,
        name: &str,
        forbidden: Mode,
        refusal: RustRunnerFailure,
    ) -> Result<(), HostError> {
        match mkdirat(&self.root, name, Mode::RWXU) {
            Ok(()) | Err(Errno::EXIST) => {}
            Err(_) => return Err(failure(RustRunnerFailure::HostStore)),
        }
        self.directory(name, forbidden, refusal).map(drop)
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
