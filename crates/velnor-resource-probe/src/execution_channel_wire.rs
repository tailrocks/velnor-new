// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionChannelContext {
    pub schema_version: u8,

    pub operation_id: String,

    pub expected_guest_boot_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionChannelDirectory {
    pub target: String,

    pub inode: u64,

    pub owner_uid: u32,

    pub owner_gid: u32,

    pub mode: u32,

    pub mount_id: u64,

    pub statx_mount_id: u64,

    pub mount_root: String,

    pub mount_source: String,

    pub mount_device_major: u32,

    pub mount_device_minor: u32,

    pub statx_device_major: u32,

    pub statx_device_minor: u32,

    pub filesystem_type_magic: u64,

    pub filesystem_type_name: String,

    pub filesystem_id: u64,

    pub read_only: bool,

    pub executable_mount: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionChannelObservation {
    pub schema_version: u8,

    pub operation_id: String,

    pub guest_boot_id: String,

    pub mount_namespace_id: u64,

    pub control: ExecutionChannelDirectory,

    pub inbox: ExecutionChannelDirectory,

    pub job_facts: ExecutionChannelDirectory,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionRequestContext {
    pub schema_version: u8,

    pub operation_id: String,

    pub expected_guest_boot_id: String,

    pub expected_channel: ExecutionChannelObservation,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExecutionRequestOutcome {
    NotYetPresent {
        schema_version: u8,

        operation_id: String,

        guest_boot_id: String,

        channel: ExecutionChannelObservation,
    },

    Present {
        record: ExecutionRequestRecord,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionRequestRecord {
    pub schema_version: u8,

    pub operation_id: String,

    pub guest_boot_id: String,

    pub channel: ExecutionChannelObservation,

    pub bytes_hex: String,

    pub sha256: String,

    pub inode: u64,

    pub owner_uid: u32,

    pub owner_gid: u32,

    pub mode: u32,

    pub size: u64,
}
