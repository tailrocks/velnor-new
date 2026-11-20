// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use crate::config::HostConfig;
use crate::error::HostError;

const MAX_CONFIG_BYTES: u64 = 1_048_576;

pub(super) fn read_host_config(path: &Path) -> Result<Option<HostConfig>, HostError> {
    let mut options = std::fs::OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(HostError::Config),
    };
    let before = file.metadata().map_err(|_| HostError::Config)?;
    if !before.file_type().is_file() || before.len() > MAX_CONFIG_BYTES {
        return Err(HostError::Config);
    }

    let first = read_snapshot(&mut file)?;
    let second = read_snapshot(&mut file)?;
    let after = file.metadata().map_err(|_| HostError::Config)?;
    if before.len() != after.len()
        || u64::try_from(first.len()).map_err(|_| HostError::Config)? != after.len()
        || first != second
    {
        return Err(HostError::Config);
    }
    let text = std::str::from_utf8(&first).map_err(|_| HostError::Config)?;
    HostConfig::parse(text).map(Some)
}

fn read_snapshot(file: &mut File) -> Result<Vec<u8>, HostError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| HostError::Config)?;
    let mut bytes = Vec::new();
    (&mut *file)
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| HostError::Config)?;
    if u64::try_from(bytes.len()).map_err(|_| HostError::Config)? > MAX_CONFIG_BYTES {
        return Err(HostError::Config);
    }
    Ok(bytes)
}
