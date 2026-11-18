// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::File;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use rustix::fs::{Mode, OFlags};
use zeroize::Zeroizing;

use crate::error::HostError;
use crate::keychain::read_secret;

const REQUIRED_MODE: u32 = 0o600;

pub(crate) fn load(path: &Path) -> Result<Zeroizing<Vec<u8>>, HostError> {
    let fd = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|_| HostError::Keychain)?;
    let mut file = File::from(fd);
    let metadata = file.metadata().map_err(|_| HostError::Keychain)?;
    if !metadata.is_file()
        || metadata.permissions().mode() & 0o7777 != REQUIRED_MODE
        || metadata.uid() != rustix::process::geteuid().as_raw()
    {
        return Err(HostError::Keychain);
    }
    let secret = read_secret(&mut file)?;
    Ok(secret.into_bytes())
}

#[cfg(test)]
mod tests;
