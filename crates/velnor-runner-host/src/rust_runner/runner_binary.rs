// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::File;
use std::io::Read;
use std::path::Path;

use rustix::fs::{FileType, Mode, OFlags, fstat};
use rustix::process::geteuid;
use sha2::{Digest, Sha256};

use super::machine::failure;
use crate::digest_hex::lowercase_hex;
use crate::error::{HostError, RustRunnerFailure};

pub(super) fn digest(path: &Path) -> Result<String, HostError> {
    let descriptor = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| failure(RustRunnerFailure::BinaryMissing))?;
    let stat = fstat(&descriptor).map_err(|_| failure(RustRunnerFailure::HostStore))?;
    let mode = Mode::from_raw_mode(stat.st_mode);
    let safe = FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile
        && stat.st_uid == geteuid().as_raw()
        && !mode.intersects(Mode::WGRP | Mode::WOTH)
        && mode.contains(Mode::XUSR);
    if !safe {
        return Err(failure(RustRunnerFailure::BinaryUnsafe));
    }
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
