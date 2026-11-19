// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{fs, os::fd::OwnedFd, path::Path};

use rustix::{
    fs::{Mode, OFlags, open, openat},
    io::Errno,
};
use serde::{Deserialize, Serialize};

use crate::{
    ProbeError,
    filesystem::{FilesystemObservation, observe_docker_root, parse_boot_id},
    mountinfo::cgroup2_root,
};

const BOOT_ID_PATH: &str = "/proc/sys/kernel/random/boot_id";
const MOUNTINFO_PATH: &str = "/proc/self/mountinfo";
const CGROUP_ROOT: &str = "/sys/fs/cgroup";
use crate::mountinfo_io::read_mountinfo;
const BOOT_ID_LIMIT: usize = 128;
const MAX_PATH_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupObservationRequest {
    pub schema_version: u8,

    pub operation_id: String,

    pub expected_guest_boot_id: String,

    pub cgroup_paths: [String; 2],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupObservation {
    pub wire_mode: String,

    pub schema_version: u8,

    pub operation_id: String,

    pub guest_boot_id: String,

    pub absent_cgroup_paths: [String; 2],

    pub docker_root: GuestDockerRootIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuestDockerRootIdentity {
    pub target: String,

    pub mount_id: u64,

    pub statx_mount_id: u64,

    pub mount_root: String,

    pub mount_device: [u32; 2],

    pub statx_device: [u32; 2],

    pub filesystem_type_magic: u64,

    pub filesystem_id: u64,

    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestObservation {
    pub boot_id: String,
    pub docker_root: GuestDockerRootIdentity,
}

pub fn observe_cleanup(
    request: CleanupObservationRequest,
) -> Result<CleanupObservation, ProbeError> {
    observe_cleanup_in(request, Path::new(CGROUP_ROOT), observe_guest)
}

pub fn observe_cleanup_in(
    request: CleanupObservationRequest,
    cgroup_root: &Path,
    mut observe_guest: impl FnMut() -> Result<GuestObservation, ProbeError>,
) -> Result<CleanupObservation, ProbeError> {
    validate_request(&request)?;
    let before = observe_guest()?;
    if before.boot_id != request.expected_guest_boot_id {
        return Err(ProbeError::Invalid("cleanup_boot_id_mismatch"));
    }
    let root = open_cgroup_root(cgroup_root)?;
    observe_absent(&root, &request.cgroup_paths[0])?;
    observe_absent(&root, &request.cgroup_paths[1])?;
    let after = observe_guest()?;
    if before != after {
        return Err(ProbeError::Invalid("cleanup_guest_identity_changed"));
    }
    observe_absent(&root, &request.cgroup_paths[0])?;
    observe_absent(&root, &request.cgroup_paths[1])?;
    Ok(CleanupObservation {
        wire_mode: "cleanup_observation".to_owned(),
        schema_version: 1,
        operation_id: request.operation_id,
        guest_boot_id: after.boot_id,
        absent_cgroup_paths: request.cgroup_paths,
        docker_root: after.docker_root,
    })
}

fn observe_guest() -> Result<GuestObservation, ProbeError> {
    Ok(GuestObservation {
        boot_id: read_boot_id()?,
        docker_root: observe_guest_root()?,
    })
}

fn validate_request(request: &CleanupObservationRequest) -> Result<(), ProbeError> {
    if request.schema_version != 1
        || request.operation_id.len() != 32
        || !lower_hex(&request.operation_id)
        || request.expected_guest_boot_id.len() != 36
        || !request
            .expected_guest_boot_id
            .bytes()
            .enumerate()
            .all(|(index, byte)| {
                if matches!(index, 8 | 13 | 18 | 23) {
                    byte == b'-'
                } else {
                    byte.is_ascii_hexdigit()
                }
            })
        || request.cgroup_paths[0] == request.cgroup_paths[1]
    {
        return Err(ProbeError::Invalid("cleanup_request_identity"));
    }
    for path in &request.cgroup_paths {
        validate_cgroup_leaf(path)?;
    }
    Ok(())
}

fn validate_cgroup_leaf(path: &str) -> Result<(), ProbeError> {
    if path.len() > MAX_PATH_BYTES
        || path == "/"
        || !path.starts_with('/')
        || path.contains("//")
        || path.split('/').skip(1).any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.bytes().any(|byte| {
                    !(byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
                })
        })
    {
        return Err(ProbeError::Invalid("cleanup_cgroup_path"));
    }
    Ok(())
}

fn observe_absent(root: &OwnedFd, path: &str) -> Result<(), ProbeError> {
    validate_cgroup_leaf(path)?;
    let mut current = None;
    for component in path.split('/').skip(1) {
        let parent = current.as_ref().unwrap_or(root);
        match openat(
            parent,
            component,
            OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(next) => current = Some(next),
            Err(Errno::NOENT) => return Ok(()),
            Err(error) => return Err(ProbeError::Read("cleanup_cgroup_open", error.into())),
        }
    }
    Err(ProbeError::Invalid("cleanup_cgroup_present"))
}

fn open_cgroup_root(path: &Path) -> Result<OwnedFd, ProbeError> {
    if !path.is_absolute() || path == Path::new("/") {
        return Err(ProbeError::Invalid("cleanup_cgroup_root"));
    }
    let root = open(
        "/",
        OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| ProbeError::Read("cleanup_cgroup_root", error.into()))?;
    let mut current = root;
    for component in path.components().skip(1) {
        let std::path::Component::Normal(component) = component else {
            return Err(ProbeError::Invalid("cleanup_cgroup_root"));
        };
        current = openat(
            &current,
            component,
            OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| ProbeError::Read("cleanup_cgroup_root", error.into()))?;
    }
    Ok(current)
}

fn observe_guest_root() -> Result<GuestDockerRootIdentity, ProbeError> {
    let mountinfo = read_mountinfo(MOUNTINFO_PATH, "mountinfo")?;
    cgroup2_root(&mountinfo)?;
    let observation = observe_docker_root(&mountinfo)?;
    Ok(root_identity(&observation))
}

fn root_identity(observation: &FilesystemObservation) -> GuestDockerRootIdentity {
    GuestDockerRootIdentity {
        target: observation.mount_target.to_owned(),
        mount_id: observation.mount.mount_id,
        statx_mount_id: observation.statx_mount_id,
        mount_root: observation.mount.root.clone(),
        mount_device: [
            observation.mount.device_major,
            observation.mount.device_minor,
        ],
        statx_device: [
            observation.statx_device_major,
            observation.statx_device_minor,
        ],
        filesystem_type_magic: observation.fs_type_magic,
        filesystem_id: observation.filesystem_id,
        read_only: observation.mount.read_only,
    }
}

fn read_boot_id() -> Result<String, ProbeError> {
    let bytes = read_bounded(BOOT_ID_PATH, BOOT_ID_LIMIT, "boot_id")?;
    parse_boot_id(&bytes)
}

fn read_bounded(path: &str, limit: usize, name: &'static str) -> Result<Vec<u8>, ProbeError> {
    use std::io::Read;

    let file = fs::File::open(path).map_err(|error| ProbeError::Read(name, error))?;
    let mut bytes = Vec::with_capacity(limit.min(4096));
    file.take(u64::try_from(limit + 1).map_err(|_| ProbeError::Overflow("input_limit"))?)
        .read_to_end(&mut bytes)
        .map_err(|error| ProbeError::Read(name, error))?;
    if bytes.len() > limit {
        return Err(ProbeError::InputTooLarge(name));
    }
    Ok(bytes)
}

fn lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
