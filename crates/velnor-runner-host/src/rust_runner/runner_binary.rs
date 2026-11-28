// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::File;
use std::io::Read;
use std::path::Path;

use rustix::fs::{FileType, Mode, OFlags, fstat};
use rustix::io::Errno;
use rustix::process::geteuid;
use sha2::{Digest, Sha256};

use super::machine::failure;
use super::store::require_no_acl;
use crate::digest_hex::lowercase_hex;
use crate::error::{HostError, RustRunnerFailure};

pub(super) fn open(path: &Path) -> Result<File, HostError> {
    let descriptor = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| {
        failure(if error == Errno::LOOP {
            RustRunnerFailure::BinaryUnsafe
        } else {
            RustRunnerFailure::BinaryMissing
        })
    })?;
    let stat = fstat(&descriptor).map_err(|_| failure(RustRunnerFailure::HostStore))?;
    let mode = Mode::from_raw_mode(stat.st_mode);
    let safe = FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile
        && stat.st_uid == geteuid().as_raw()
        && !mode.intersects(Mode::WGRP | Mode::WOTH)
        && mode.contains(Mode::XUSR);
    if !safe {
        return Err(failure(RustRunnerFailure::BinaryUnsafe));
    }
    require_no_acl(path, RustRunnerFailure::BinaryUnsafe)?;
    Ok(File::from(descriptor))
}

pub(super) fn digest(path: &Path) -> Result<String, HostError> {
    let mut file = open(path)?;
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
