// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    pub(super) operation_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[cfg(target_os = "linux")]
pub(super) struct Acknowledgment {
    pub(super) operation_id: String,
}

#[derive(Debug, Serialize)]
#[cfg(target_os = "linux")]
pub(super) struct ResultRecord {
    pub(super) schema: u8,
    pub(super) operation_id: String,
    pub(super) home: DirectoryRecord,
    pub(super) work: DirectoryRecord,
    pub(super) metadata: DirectoryRecord,
    pub(super) runtime_identity: DirectoryRecord,
    pub(super) job_facts: DirectoryRecord,
    pub(super) source_proof: DirectoryRecord,
    pub(super) control: DirectoryRecord,
    pub(super) inbox: DirectoryRecord,
}

#[derive(Debug, Serialize)]
#[cfg(target_os = "linux")]
pub(super) struct DirectoryRecord {
    pub(super) target: &'static str,
    pub(super) mount_id: u64,
    pub(super) parent_mount_id: u64,
    pub(super) device_major: u32,
    pub(super) device_minor: u32,
    pub(super) inode: u64,
    pub(super) uid: u32,
    pub(super) gid: u32,
    pub(super) mode: u32,
}

#[derive(Debug, Serialize)]
#[cfg(target_os = "linux")]
pub(super) struct FailureRecord {
    pub(super) schema: u8,
    pub(super) operation_id: String,
    pub(super) status: &'static str,
    pub(super) error: &'static str,
}

pub(super) fn valid_operation_id(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[cfg(target_os = "linux")]
pub(super) fn validate_acknowledgment(
    acknowledgment: &Acknowledgment,
    expected_operation_id: &str,
) -> Result<(), &'static str> {
    if !valid_operation_id(expected_operation_id)
        || acknowledgment.operation_id != expected_operation_id
    {
        return Err("acknowledgment_operation_id");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn validate_initial_root(uid: u32, gid: u32, empty: bool) -> Result<(), &'static str> {
    if uid != 0 || gid != 0 {
        return Err("target_owner");
    }
    if !empty {
        return Err("target_not_empty");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn validate_mount_pair(home: u64, work: u64) -> Result<(), &'static str> {
    if home == work {
        Err("mounts_not_distinct")
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
#[cfg(target_os = "linux")]
struct MountIdentity {
    mount_id: u64,
    parent_mount_id: u64,
    device_major: u32,
    device_minor: u32,
    inode: u64,
}

#[cfg(target_os = "linux")]
fn validate_mount_identity(identity: MountIdentity) -> Result<(), &'static str> {
    if identity.mount_id == 0 || identity.parent_mount_id == 0 {
        return Err("target_mount_id");
    }
    if identity.mount_id == identity.parent_mount_id {
        return Err("target_not_mountpoint");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn validate_distinct_roots(home: MountIdentity, work: MountIdentity) -> Result<(), &'static str> {
    validate_mount_pair(home.mount_id, work.mount_id)?;
    if home.device_major == work.device_major
        && home.device_minor == work.device_minor
        && home.inode == work.inode
    {
        return Err("mount_roots_not_distinct");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn validate_child_mount(parent_mount_id: u64, child_mount_id: u64) -> Result<(), &'static str> {
    if parent_mount_id == 0 || child_mount_id != parent_mount_id {
        return Err("child_mount_changed");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn validate_directory_state(
    uid: u32,
    gid: u32,
    mode: u32,
    required_user: u32,
    required_group: u32,
    expected_mode: u32,
) -> Result<(), &'static str> {
    if uid != required_user || gid != required_group || mode != expected_mode {
        return Err("target_verify");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
#[path = "storage_init/linux.rs"]
mod linux;

#[cfg(target_os = "linux")]
pub(super) fn initialize(operation_id: String) -> Result<ResultRecord, &'static str> {
    linux::initialize(operation_id)
}
