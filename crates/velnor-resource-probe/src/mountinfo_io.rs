// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{fs::File, io::Read};

use crate::ProbeError;

pub const MOUNTINFO_LIMIT: usize = 1024 * 1024;

pub fn read_mountinfo(path: &str, name: &'static str) -> Result<Vec<u8>, ProbeError> {
    let file = File::open(path).map_err(|error| ProbeError::Read(name, error))?;
    let mut bytes = Vec::with_capacity(4096);
    file.take(u64::try_from(MOUNTINFO_LIMIT + 1).map_err(|_| ProbeError::Overflow("input_limit"))?)
        .read_to_end(&mut bytes)
        .map_err(|error| ProbeError::Read(name, error))?;
    if bytes.len() > MOUNTINFO_LIMIT {
        return Err(ProbeError::InputTooLarge(name));
    }
    Ok(bytes)
}
