//! Make sure that there are no worker cgroups in the selected Linux guest.

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
const MOUNTINFO_LIMIT: usize = 65_536;
const BOOT_ID_LIMIT: usize = 128;
const MAX_PATH_BYTES: usize = 256;

/// Request one read-only measurement of two worker cgroups recorded in the journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupObservationRequest {
    /// Wire schema version. The value must be one.
    pub schema_version: u8,
    /// 32-character lower-case hexadecimal operation identity.
    pub operation_id: String,
    /// Guest boot identity recorded with the worker binding.
    pub expected_guest_boot_id: String,
    /// Runner and `DinD` leaf paths, in that order.
    pub cgroup_paths: [String; 2],
}

/// Read-only record that there are no worker cgroup leaves at the specified paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupObservation {
    /// Producer mode name.
    pub wire_mode: String,
    /// Wire schema version.
    pub schema_version: u8,
    /// Request identity.
    pub operation_id: String,
    /// Measured Linux guest boot identity.
    pub guest_boot_id: String,
    /// Measured Runner and `DinD` leaf paths with no worker cgroups.
    pub absent_cgroup_paths: [String; 2],
    /// Selected Docker-root mount identity measured before and after the sample.
    pub docker_root: GuestDockerRootIdentity,
}

/// Stable identity for the selected Docker-root bind in this guest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuestDockerRootIdentity {
    /// Specified mount target.
    pub target: String,
    /// Mount ID from mount information.
    pub mount_id: u64,
    /// Mount ID from `statx`.
    pub statx_mount_id: u64,
    /// Mount root from mount information.
    pub mount_root: String,
    /// Device values from mount information.
    pub mount_device: [u32; 2],
    /// Device values from `statx`.
    pub statx_device: [u32; 2],
    /// Filesystem type value from `statfs`.
    pub filesystem_type_magic: u64,
    /// Filesystem ID from `statvfs`.
    pub filesystem_id: u64,
    /// Read-only state from mount information.
    pub read_only: bool,
}

/// Measure worker cgroup status and selected guest identity.
///
/// # Errors
///
/// Returns an error if an identity or mount is incorrect, or if there is a worker cgroup.
pub fn observe_cleanup(
    request: CleanupObservationRequest,
) -> Result<CleanupObservation, ProbeError> {
    validate_request(&request)?;
    let boot_before = read_boot_id()?;
    if boot_before != request.expected_guest_boot_id {
        return Err(ProbeError::Invalid("cleanup_boot_id_mismatch"));
    }
    let root_before = observe_guest_root()?;
    let root = open_cgroup_root(Path::new(CGROUP_ROOT))?;
    observe_absent(&root, &request.cgroup_paths[0])?;
    observe_absent(&root, &request.cgroup_paths[1])?;
    observe_absent(&root, &request.cgroup_paths[0])?;
    observe_absent(&root, &request.cgroup_paths[1])?;
    let root_after = observe_guest_root()?;
    let boot_after = read_boot_id()?;
    if boot_before != boot_after || root_before != root_after {
        return Err(ProbeError::Invalid("cleanup_guest_identity_changed"));
    }
    Ok(CleanupObservation {
        wire_mode: "cleanup_observation".to_owned(),
        schema_version: 1,
        operation_id: request.operation_id,
        guest_boot_id: boot_after,
        absent_cgroup_paths: request.cgroup_paths,
        docker_root: root_after,
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
    let mountinfo = read_bounded(MOUNTINFO_PATH, MOUNTINFO_LIMIT, "mountinfo")?;
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

#[cfg(test)]
#[path = "cleanup_observation/tests.rs"]
mod tests;
