use serde::{Deserialize, Serialize};

use crate::execution_channel_wire::ExecutionChannelDirectory;

/// One specified source-proof control file.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionPublicationKind {
    /// The one challenge document.
    Challenge,
    /// The one decision document.
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

/// Input with a size limit to publish one control file.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPublicationContext {
    /// Wire schema version. The value must be one.
    pub schema_version: u8,
    /// Operation identity supplied by the host.
    pub operation_id: String,
    /// Guest boot ID measured by the host.
    pub expected_guest_boot_id: String,
    /// Specified control file to publish.
    pub kind: ExecutionPublicationKind,
    /// Document bytes in the required JSON format, with hexadecimal encoding.
    pub payload_hex: String,
    /// SHA-256 digest of the required JSON document bytes.
    pub payload_sha256: String,
    /// Control directory measured in the Runner mount namespace.
    pub expected_runner_control: ExecutionChannelDirectory,
}

/// Measured control directory and mount source.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPublicationFile {
    /// Specified path in the Runner container.
    pub path: String,
    /// Control directory inode from its open file descriptor.
    pub control_directory_inode: u64,
    /// File inode from the open file descriptor.
    pub inode: u64,
    /// File byte length from the open file descriptor.
    pub bytes: u64,
    /// File owner UID.
    pub owner_uid: u32,
    /// File owner GID.
    pub owner_gid: u32,
    /// File mode bits.
    pub mode: u32,
    /// Mount ID from the mount namespace of the process that publishes the file.
    pub mount_id: u64,
    /// Mount ID from statx.
    pub statx_mount_id: u64,
    /// Device major number from mount information.
    pub mount_device_major: u32,
    /// Device minor number from mount information.
    pub mount_device_minor: u32,
    /// Device major number from statx.
    pub statx_device_major: u32,
    /// Device minor number from statx.
    pub statx_device_minor: u32,
    /// Filesystem magic value from statfs.
    pub filesystem_type_magic: u64,
    /// Filesystem ID from statvfs.
    pub filesystem_id: u64,
    /// Mount root from mount information.
    pub mount_root: String,
    /// Mount source from mount information.
    pub mount_source: String,
    /// SHA-256 digest with hexadecimal encoding, calculated from the file bytes.
    pub readback_sha256: String,
}

/// Facts measured after the control file is published.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPublicationRecord {
    /// Wire schema version. The value must be one.
    pub schema_version: u8,
    /// Operation identity supplied by the host.
    pub operation_id: String,
    /// Guest boot ID read from procfs.
    pub guest_boot_id: String,
    /// Mount namespace inode read from procfs.
    pub mount_namespace_id: u64,
    /// Specified control file to publish.
    pub kind: ExecutionPublicationKind,
    /// SHA-256 digest of the required JSON document.
    pub payload_sha256: String,
    /// Measured file and filesystem facts.
    pub file: ExecutionPublicationFile,
}
