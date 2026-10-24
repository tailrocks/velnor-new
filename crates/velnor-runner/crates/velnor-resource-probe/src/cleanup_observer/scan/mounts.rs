use std::{collections::HashSet, fs, os::unix::fs::MetadataExt};

use super::super::CleanupObserverRequest;
use super::helpers::{add_issue, decode_mount_path, device_pair, parse_device};
use rustix::fs::{AtFlags, Mode, OFlags, StatxFlags, open, openat, statx};

const PROC_FILE_LIMIT: usize = 1_048_576;

#[derive(Debug)]
struct MountObservationError;

pub(super) fn read_mountinfo(pid: u32, issues: &mut Vec<String>) -> Option<Vec<u8>> {
    read_proc_mountinfo(
        &format!("/proc/{pid}/mountinfo"),
        "cleanup_mountinfo",
        issues,
    )
}

pub(super) fn read_task_mountinfo(pid: u32, tid: u32, issues: &mut Vec<String>) -> Option<Vec<u8>> {
    read_proc_mountinfo(
        &format!("/proc/{pid}/task/{tid}/mountinfo"),
        "cleanup_task_mountinfo",
        issues,
    )
}

fn read_proc_mountinfo(
    path: &str,
    name: &'static str,
    issues: &mut Vec<String>,
) -> Option<Vec<u8>> {
    if let Ok(value) = super::helpers::read_bounded(path, PROC_FILE_LIMIT, name) {
        Some(value)
    } else {
        add_issue(issues, "mountinfo_unreadable");
        None
    }
}

pub(super) fn check_owned_mounts(
    pid: u32,
    tid: u32,
    input: &[u8],
    request: &CleanupObserverRequest,
    references: &mut u32,
    issues: &mut Vec<String>,
) {
    let Ok(text) = std::str::from_utf8(input) else {
        add_issue(issues, "mountinfo_invalid");
        return;
    };
    let root_path = format!("/proc/{pid}/task/{tid}/root");
    let Ok(root) = open(
        &root_path,
        OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    ) else {
        add_issue(issues, "volume_root_unreadable");
        return;
    };
    let Ok(root_identity) = root_stat(&root) else {
        add_issue(issues, "volume_root_identity_unreadable");
        return;
    };
    for line in text.lines() {
        let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
        let Some(device) = fields.get(2).and_then(|value| parse_device(value)) else {
            add_issue(issues, "mountinfo_device_invalid");
            continue;
        };
        let Some(target) = fields.get(4).and_then(|value| decode_mount_path(value)) else {
            add_issue(issues, "mountinfo_row_invalid");
            continue;
        };
        for volume in request.binding.volume_mountpoints.iter().filter(|volume| {
            device
                == super::helpers::linux_device(
                    u64::from(volume.mount_device[0]),
                    u64::from(volume.mount_device[1]),
                )
        }) {
            match open_mount_target(pid, tid, &root, &target) {
                Ok((device, inode)) if volume_identity_matches(volume, device, inode) => {
                    *references = references.saturating_add(1);
                }
                Ok(_) => {}
                Err(_) => add_issue(issues, "volume_mount_identity_unreadable"),
            }
        }
    }
    if !matches!(root_stat(&root), Ok(value) if value == root_identity) {
        add_issue(issues, "volume_root_identity_changed");
    }
}

pub(super) fn volume_identity_matches(
    volume: &super::super::CleanupVolume,
    statx_device: [u32; 2],
    inode: u64,
) -> bool {
    volume.statx_device == statx_device && volume.inode == inode
}

fn open_mount_target(
    pid: u32,
    tid: u32,
    root: &rustix::fd::OwnedFd,
    target: &str,
) -> Result<([u32; 2], u64), MountObservationError> {
    if target == "/" || !target.starts_with('/') || target.contains("//") {
        return Err(MountObservationError);
    }
    let mut current = None;
    for component in target.split('/').skip(1) {
        if component.is_empty() || matches!(component, "." | "..") {
            return Err(MountObservationError);
        }
        let parent = current.as_ref().unwrap_or(root);
        current = Some(
            openat(
                parent,
                component,
                OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|_| MountObservationError)?,
        );
    }
    let path = format!("/proc/{pid}/task/{tid}/root");
    let live_root = open(
        path,
        OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| MountObservationError)?;
    if root_stat(root).map_err(|_| MountObservationError)?
        != root_stat(&live_root).map_err(|_| MountObservationError)?
    {
        return Err(MountObservationError);
    }
    let target = current.ok_or(MountObservationError)?;
    let identity = statx(&target, "", AtFlags::EMPTY_PATH, StatxFlags::BASIC_STATS)
        .map_err(|_| MountObservationError)?;
    Ok((
        [identity.stx_dev_major, identity.stx_dev_minor],
        identity.stx_ino,
    ))
}

fn root_stat(root: &rustix::fd::OwnedFd) -> Result<([u32; 2], u64), MountObservationError> {
    let identity = statx(root, "", AtFlags::EMPTY_PATH, StatxFlags::BASIC_STATS)
        .map_err(|_| MountObservationError)?;
    Ok((
        [identity.stx_dev_major, identity.stx_dev_minor],
        identity.stx_ino,
    ))
}

pub(super) fn check_tmpfs_mounts(
    pid: u32,
    input: &[u8],
    devices: &HashSet<u64>,
    namespace_ids: &[([u32; 2], u64); 2],
    references: &mut u32,
    issues: &mut Vec<String>,
) {
    let Ok(text) = std::str::from_utf8(input) else {
        add_issue(issues, "mountinfo_invalid");
        return;
    };
    for line in text.lines() {
        let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
        let is_nsfs = fields
            .iter()
            .position(|field| *field == "-")
            .and_then(|separator| fields.get(separator + 1).copied())
            == Some("nsfs");
        if is_nsfs {
            let Some(target) = fields.get(4).and_then(|value| decode_mount_path(value)) else {
                add_issue(issues, "nsfs_mountpoint_invalid");
                continue;
            };
            let path = format!("/proc/{pid}/root{target}");
            match fs::metadata(path) {
                Ok(metadata)
                    if namespace_ids.iter().any(|(device, inode)| {
                        *inode == metadata.ino() && *device == device_pair(metadata.dev())
                    }) =>
                {
                    *references = references.saturating_add(1);
                }
                Ok(_) => {}
                Err(_) => add_issue(issues, "nsfs_mount_identity_unreadable"),
            }
        }
        let Some(device) = fields.get(2).and_then(|value| parse_device(value)) else {
            add_issue(issues, "mountinfo_device_invalid");
            continue;
        };
        if devices.contains(&device) {
            *references = references.saturating_add(1);
        }
    }
}
