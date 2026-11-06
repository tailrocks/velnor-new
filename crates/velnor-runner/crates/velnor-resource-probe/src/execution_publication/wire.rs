// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::execution_channel_wire::ExecutionChannelDirectory;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionPublicationKind {
    Challenge,

    Decision,
}

impl ExecutionPublicationKind {
    #[cfg(target_os = "linux")]
    pub(crate) const fn file_name(self) -> &'static str {
        match self {
            Self::Challenge => "challenge.json",
            Self::Decision => "decision.json",
        }
    }

    #[cfg(target_os = "linux")]
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Challenge => "challenge",
            Self::Decision => "decision",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPublicationContext {
    pub schema_version: u8,

    pub operation_id: String,

    pub expected_guest_boot_id: String,

    pub kind: ExecutionPublicationKind,

    pub payload_hex: String,

    pub payload_sha256: String,

    pub expected_runner_control: ExecutionChannelDirectory,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPublicationFile {
    pub path: String,

    pub control_directory_inode: u64,

    pub inode: u64,

    pub bytes: u64,

    pub owner_uid: u32,

    pub owner_gid: u32,

    pub mode: u32,

    pub mount_id: u64,

    pub statx_mount_id: u64,

    pub mount_device_major: u32,

    pub mount_device_minor: u32,

    pub statx_device_major: u32,

    pub statx_device_minor: u32,

    pub filesystem_type_magic: u64,

    pub filesystem_id: u64,

    pub mount_root: String,

    pub mount_source: String,

    pub readback_sha256: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPublicationRecord {
    pub schema_version: u8,

    pub operation_id: String,

    pub guest_boot_id: String,

    pub mount_namespace_id: u64,

    pub kind: ExecutionPublicationKind,

    pub payload_sha256: String,

    pub file: ExecutionPublicationFile,
}
