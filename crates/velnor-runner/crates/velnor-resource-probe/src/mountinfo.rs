// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{ProbeError, units::digits};

#[cfg(target_os = "linux")]
const TARGET: &str = "/velnor/docker-root";
const MOUNTINFO_LIMIT: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MountEntry {
    pub(crate) mount_id: u64,
    pub(crate) device_major: u32,
    pub(crate) device_minor: u32,
    pub(crate) root: String,
    pub(crate) filesystem_type: String,
    pub(crate) read_only: bool,
    pub(crate) executable: bool,
}

#[cfg(target_os = "linux")]
pub(crate) fn attachment_mount(input: &[u8], target: &str) -> Result<MountEntry, ProbeError> {
    if input.len() > MOUNTINFO_LIMIT {
        return Err(ProbeError::InputTooLarge("mountinfo"));
    }
    let text = std::str::from_utf8(input).map_err(|_| ProbeError::Invalid("mountinfo_utf8"))?;
    let mut found = None;
    for line in text.lines() {
        let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
        let Some(separator) = fields.iter().position(|field| *field == "-") else {
            return Err(ProbeError::Invalid("mountinfo_separator"));
        };
        if separator < 6 || fields.len() < separator + 3 {
            return Err(ProbeError::Invalid("mountinfo_fields"));
        }
        if decode_mount_field(fields[4])? != target {
            continue;
        }
        if found.is_some() {
            return Err(ProbeError::Invalid("attachment_mount_duplicate"));
        }
        let mount_id = parse_u64(fields[0], "mountinfo_id")?;
        let (device_major, device_minor) = parse_device(fields[2])?;
        let options = fields[5].split(',').collect::<Vec<_>>();
        let root = decode_mount_field(fields[3])?;
        let filesystem_type = fields[separator + 1];
        if filesystem_type.is_empty()
            || !filesystem_type
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        {
            return Err(ProbeError::Invalid("mountinfo_filesystem_type"));
        }
        found = Some(MountEntry {
            mount_id,
            device_major,
            device_minor,
            root,
            filesystem_type: filesystem_type.to_owned(),
            read_only: options.contains(&"ro"),
            executable: !options.contains(&"noexec"),
        });
    }
    found.ok_or(ProbeError::Invalid("attachment_mount_missing"))
}

#[cfg(target_os = "linux")]
pub(crate) fn attachment_mount_source(input: &[u8], target: &str) -> Result<String, ProbeError> {
    attachment_mount(input, target)?;
    let text = std::str::from_utf8(input).map_err(|_| ProbeError::Invalid("mountinfo_utf8"))?;
    let mut found = None;
    for line in text.lines() {
        let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
        let separator = fields
            .iter()
            .position(|field| *field == "-")
            .ok_or(ProbeError::Invalid("mountinfo_separator"))?;
        if decode_mount_field(fields[4])? != target {
            continue;
        }
        let source = decode_mount_field(fields[separator + 2])?;
        if source.is_empty() || found.replace(source).is_some() {
            return Err(ProbeError::Invalid("attachment_mount_source"));
        }
    }
    found.ok_or(ProbeError::Invalid("attachment_mount_missing"))
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn attachment_mount_source(_input: &[u8], _target: &str) -> Result<String, ProbeError> {
    Err(ProbeError::Invalid("linux_guest_required"))
}

pub(crate) fn cgroup2_root(input: &[u8]) -> Result<MountEntry, ProbeError> {
    unique_mount(
        input,
        "/sys/fs/cgroup",
        "cgroup2",
        "/",
        "cgroup2_mount_missing",
        "cgroup2_mount_duplicate",
        "cgroup2_mount_not_read_only",
    )
}

#[cfg(target_os = "linux")]
pub(crate) fn docker_root_mount(input: &[u8]) -> Result<MountEntry, ProbeError> {
    unique_mount(
        input,
        TARGET,
        "",
        "",
        "docker_root_mount_missing",
        "docker_root_mount_duplicate",
        "docker_root_mount_not_read_only",
    )
}

fn unique_mount(
    input: &[u8],
    target: &str,
    expected_filesystem: &str,
    expected_root: &str,
    missing_error: &'static str,
    duplicate_error: &'static str,
    read_only_error: &'static str,
) -> Result<MountEntry, ProbeError> {
    if input.len() > MOUNTINFO_LIMIT {
        return Err(ProbeError::InputTooLarge("mountinfo"));
    }
    let text = std::str::from_utf8(input).map_err(|_| ProbeError::Invalid("mountinfo_utf8"))?;
    let mut found = None;
    for line in text.lines() {
        let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
        let Some(separator) = fields.iter().position(|field| *field == "-") else {
            return Err(ProbeError::Invalid("mountinfo_separator"));
        };
        if separator < 6 || fields.len() < separator + 3 {
            return Err(ProbeError::Invalid("mountinfo_fields"));
        }
        if decode_mount_field(fields[4])? != target {
            continue;
        }
        if found.is_some() {
            return Err(ProbeError::Invalid(duplicate_error));
        }
        let mount_id = parse_u64(fields[0], "mountinfo_id")?;
        let (device_major, device_minor) = parse_device(fields[2])?;
        let read_only = fields[5].split(',').any(|option| option == "ro");
        let executable = !fields[5].split(',').any(|option| option == "noexec");
        let root = decode_mount_field(fields[3])?;
        let filesystem_type = fields[separator + 1];
        if filesystem_type.is_empty()
            || !filesystem_type
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        {
            return Err(ProbeError::Invalid("mountinfo_filesystem_type"));
        }
        found = Some(MountEntry {
            mount_id,
            device_major,
            device_minor,
            root,
            filesystem_type: filesystem_type.to_owned(),
            read_only,
            executable,
        });
    }
    let entry = found.ok_or(ProbeError::Invalid(missing_error))?;
    if !entry.read_only {
        return Err(ProbeError::Invalid(read_only_error));
    }
    if !expected_filesystem.is_empty() && entry.filesystem_type != expected_filesystem {
        return Err(ProbeError::Invalid("mount_filesystem_mismatch"));
    }
    if !expected_root.is_empty() && entry.root != expected_root {
        return Err(ProbeError::Invalid("mount_root_mismatch"));
    }
    Ok(entry)
}

fn parse_device(value: &str) -> Result<(u32, u32), ProbeError> {
    let Some((major, minor)) = value.split_once(':') else {
        return Err(ProbeError::Invalid("mountinfo_device"));
    };
    Ok((
        parse_u32(major, "mountinfo_device")?,
        parse_u32(minor, "mountinfo_device")?,
    ))
}

fn parse_u32(value: &str, name: &'static str) -> Result<u32, ProbeError> {
    if !digits(value) {
        return Err(ProbeError::Invalid(name));
    }
    value.parse().map_err(|_| ProbeError::Overflow(name))
}

fn parse_u64(value: &str, name: &'static str) -> Result<u64, ProbeError> {
    if !digits(value) {
        return Err(ProbeError::Invalid(name));
    }
    value.parse().map_err(|_| ProbeError::Overflow(name))
}

fn decode_mount_field(value: &str) -> Result<String, ProbeError> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'\\' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 3 >= bytes.len()
            || !bytes[index + 1..index + 4]
                .iter()
                .all(|byte| (b'0'..=b'7').contains(byte))
        {
            return Err(ProbeError::Invalid("mountinfo_escape"));
        }
        let octal = &bytes[index + 1..index + 4];
        let value = u16::from(octal[0] - b'0') * 64
            + u16::from(octal[1] - b'0') * 8
            + u16::from(octal[2] - b'0');
        if !matches!(value, 9 | 10 | 32 | 92) {
            return Err(ProbeError::Invalid("mountinfo_escape"));
        }
        decoded.push(u8::try_from(value).map_err(|_| ProbeError::Invalid("mountinfo_escape"))?);
        index += 4;
    }
    String::from_utf8(decoded).map_err(|_| ProbeError::Invalid("mountinfo_utf8"))
}
