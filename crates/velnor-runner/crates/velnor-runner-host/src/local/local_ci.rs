// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

use crate::config::RunnerPlatform;
use crate::error::HostError;

const MAX_BYTES: u64 = 256 << 20;
const ELF_HEADER: usize = 20;
const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELF_CLASS_64: u8 = 2;
const ELF_LITTLE_ENDIAN: u8 = 1;

/// Reads the velnor-ci that a pair's runner serves as the local setup-velnor binary.
///
/// One `open` without following a symlink, every check on that handle, then the bytes
/// and the ELF check on the same read, so the file cannot change between check and use.
pub(super) async fn load(path: &str, platform: RunnerPlatform) -> Result<Vec<u8>, HostError> {
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || read(&path, platform))
        .await
        .map_err(|_| HostError::Config)?
}

fn read(path: &str, platform: RunnerPlatform) -> Result<Vec<u8>, HostError> {
    if !super::request::valid_local_ci(path) {
        return Err(HostError::Config);
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| HostError::Config)?;
    let meta = file.metadata().map_err(|_| HostError::Config)?;
    if !meta.is_file() || meta.mode() & 0o111 == 0 || meta.len() > MAX_BYTES {
        return Err(HostError::Config);
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| HostError::Config)?;
    if bytes.len() as u64 > MAX_BYTES || !elf_for(&bytes, platform) {
        return Err(HostError::Config);
    }
    Ok(bytes)
}

fn elf_for(bytes: &[u8], platform: RunnerPlatform) -> bool {
    let machine: [u8; 2] = match platform {
        RunnerPlatform::LinuxArm64 => [0xb7, 0],
        RunnerPlatform::LinuxAmd64 => [0x3e, 0],
    };
    bytes.len() >= ELF_HEADER
        && bytes[..4] == ELF_MAGIC
        && bytes[4] == ELF_CLASS_64
        && bytes[5] == ELF_LITTLE_ENDIAN
        && bytes[18..20] == machine
}

#[cfg(test)]
#[path = "local_ci_tests.rs"]
mod tests;
