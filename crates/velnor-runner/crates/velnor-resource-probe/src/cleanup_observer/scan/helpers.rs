use std::{
    fs,
    io::Read,
    os::fd::AsFd,
    os::unix::fs::MetadataExt,
    path::Path,
    time::{Duration, Instant},
};

use crate::ProbeError;
use rustix::fs::{AtFlags, Mode, OFlags, StatxFlags, fstatfs, open, statx};

const NSFS_MAGIC: i64 = 0x6e73_6673;
const TIME_LIMIT: Duration = Duration::from_secs(30);
const TMPFS_MAGIC: i64 = 0x0102_1994;

pub(super) fn decode_mount_path(value: &str) -> Option<String> {
    let mut output = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            let escape = bytes.get(index + 1..index + 4)?;
            let decoded = match escape {
                b"040" => b' ',
                b"011" => b'\t',
                b"012" => b'\n',
                b"134" => b'\\',
                _ => return None,
            };
            output.push(decoded);
            index += 4;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).ok()
}

pub(super) fn parse_device(value: &str) -> Option<u64> {
    let (major, minor) = value.split_once(':')?;
    Some(linux_device(major.parse().ok()?, minor.parse().ok()?))
}

pub(super) fn linux_device(major: u64, minor: u64) -> u64 {
    (minor & 0xff) | ((major & 0xfff) << 8) | ((minor & !0xff) << 12) | ((major & !0xfff) << 32)
}

pub(super) fn namespace_identity(path: &str) -> Result<([u32; 2], u64), ProbeError> {
    let file =
        fs::File::open(path).map_err(|error| ProbeError::Read("cleanup_namespace", error))?;
    let metadata = file
        .metadata()
        .map_err(|error| ProbeError::Read("cleanup_namespace", error))?;
    let filesystem = rustix::fs::fstatfs(file.as_fd())
        .map_err(|error| ProbeError::Read("cleanup_namespace_fs", error.into()))?;
    if filesystem.f_type != NSFS_MAGIC {
        return Err(ProbeError::Invalid("cleanup_namespace_fs"));
    }
    Ok((device_pair(metadata.dev()), metadata.ino()))
}

pub(super) fn observe_descriptor(
    path: &std::path::Path,
) -> Result<DescriptorObservation, ProbeError> {
    let link_before =
        fs::read_link(path).map_err(|error| ProbeError::Read("cleanup_fd_link", error))?;
    let descriptor = open(path, OFlags::PATH | OFlags::CLOEXEC, Mode::empty())
        .map_err(|error| ProbeError::Read("cleanup_fd_open", error.into()))?;
    let stat = statx(
        &descriptor,
        "",
        AtFlags::EMPTY_PATH,
        StatxFlags::BASIC_STATS,
    )
    .map_err(|error| ProbeError::Read("cleanup_fd_statx", error.into()))?;
    let fs = fstatfs(&descriptor)
        .map_err(|error| ProbeError::Read("cleanup_fd_statfs", error.into()))?;
    if fs::read_link(path).map_err(|error| ProbeError::Read("cleanup_fd_link", error))?
        != link_before
    {
        return Err(ProbeError::Invalid("cleanup_fd_changed"));
    }
    Ok(DescriptorObservation {
        target: link_before,
        device: [stat.stx_dev_major, stat.stx_dev_minor],
        inode: stat.stx_ino,
        filesystem_magic: fs.f_type,
    })
}

pub(super) struct DescriptorObservation {
    pub target: std::path::PathBuf,
    pub device: [u32; 2],
    pub inode: u64,
    pub filesystem_magic: i64,
}

pub(super) fn tmpfs_magic() -> i64 {
    TMPFS_MAGIC
}

pub(super) fn device_pair(value: u64) -> [u32; 2] {
    let major = ((value >> 8) & 0xfff) | ((value >> 32) & !0xfff);
    let minor = (value & 0xff) | ((value >> 12) & !0xff);
    [
        u32::try_from(major).unwrap_or(u32::MAX),
        u32::try_from(minor).unwrap_or(u32::MAX),
    ]
}

pub(super) fn process_start_ticks(path: &str, expected_pid: u32) -> Result<u64, ProbeError> {
    let bytes = read_bounded(path, 4096, "cleanup_stat")?;
    let text = std::str::from_utf8(&bytes).map_err(|_| ProbeError::Invalid("cleanup_stat"))?;
    let pid = text
        .split_ascii_whitespace()
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or(ProbeError::Invalid("cleanup_stat_pid"))?;
    let close = text
        .rfind(')')
        .ok_or(ProbeError::Invalid("cleanup_stat_format"))?;
    let fields = text[close + 1..]
        .split_ascii_whitespace()
        .collect::<Vec<_>>();
    if pid != expected_pid {
        return Err(ProbeError::Invalid("cleanup_stat_pid"));
    }
    fields
        .get(19)
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value != 0)
        .ok_or(ProbeError::Invalid("cleanup_start_ticks"))
}

pub(super) fn read_bounded(
    path: &str,
    limit: usize,
    name: &'static str,
) -> Result<Vec<u8>, ProbeError> {
    let file = fs::File::open(path).map_err(|error| ProbeError::Read(name, error))?;
    let mut output = Vec::new();
    file.take(u64::try_from(limit + 1).map_err(|_| ProbeError::Overflow("cleanup_limit"))?)
        .read_to_end(&mut output)
        .map_err(|error| ProbeError::Read(name, error))?;
    if output.len() > limit {
        return Err(ProbeError::InputTooLarge(name));
    }
    Ok(output)
}

pub(super) fn namespace_link(path: &Path) -> Option<u64> {
    let target = fs::read_link(path).ok()?;
    let value = target.to_str()?.strip_prefix("mnt:[")?.strip_suffix(']')?;
    value.parse().ok()
}

pub(super) fn numeric_name(value: &str) -> Option<u32> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok().filter(|value| *value != 0)
}

pub(super) fn expired(started: Instant) -> bool {
    started.elapsed() >= TIME_LIMIT
}

pub(super) fn add_issue(issues: &mut Vec<String>, value: &str) {
    if !issues.iter().any(|issue| issue == value) {
        issues.push(value.to_owned());
    }
}
