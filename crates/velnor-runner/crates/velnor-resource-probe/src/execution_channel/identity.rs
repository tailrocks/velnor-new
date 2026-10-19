use std::{fs, io::Read, os::unix::fs::MetadataExt};

use crate::ProbeError;

pub(super) fn read_boot_id() -> Result<String, ProbeError> {
    let file = fs::File::open("/proc/sys/kernel/random/boot_id")
        .map_err(|error| ProbeError::Read("execution_channel_boot_id", error))?;
    let mut bytes = Vec::with_capacity(36);
    file.take(65)
        .read_to_end(&mut bytes)
        .map_err(|error| ProbeError::Read("execution_channel_boot_id", error))?;
    if bytes.len() > 64 {
        return Err(ProbeError::InputTooLarge("execution_channel_boot_id"));
    }
    let value = std::str::from_utf8(&bytes)
        .map_err(|_| ProbeError::Invalid("execution_channel_boot_id"))?
        .trim_end_matches('\n');
    if value.len() != 36
        || !value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
    {
        return Err(ProbeError::Invalid("execution_channel_boot_id"));
    }
    Ok(value.to_owned())
}

pub(super) fn mount_namespace_id() -> Result<u64, ProbeError> {
    fs::metadata("/proc/self/ns/mnt")
        .map(|metadata| metadata.ino())
        .map_err(|error| ProbeError::Read("execution_channel_namespace", error))
}
