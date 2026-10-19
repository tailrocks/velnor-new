use serde::{Deserialize, Serialize};

use crate::execution_channel_wire::ExecutionChannelDirectory;

/// One fixed source-proof control file.
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

/// Bounded input for one control-file publication.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPublicationContext {
    /// Wire schema version. The value must be one.
    pub schema_version: u8,
    /// Operation identity supplied by the host.
    pub operation_id: String,
    /// Guest boot identity measured by the host.
    pub expected_guest_boot_id: String,
    /// Fixed control file selected for publication.
    pub kind: ExecutionPublicationKind,
    /// Canonical document bytes in lower-case hexadecimal form.
    pub payload_hex: String,
    /// SHA-256 digest of the canonical document bytes.
    pub payload_sha256: String,
    /// Control directory measured in the Runner namespace.
    pub expected_runner_control: ExecutionChannelDirectory,
}

/// Measured publisher control directory and mount source.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPublicationFile {
    /// Fixed destination path in the Runner container.
    pub path: String,
    /// Control directory inode from its open descriptor.
    pub control_directory_inode: u64,
    /// File inode from the open descriptor.
    pub inode: u64,
    /// File byte length from the open descriptor.
    pub bytes: u64,
    /// File owner UID.
    pub owner_uid: u32,
    /// File owner GID.
    pub owner_gid: u32,
    /// File mode bits.
    pub mode: u32,
    /// Mount ID from the publisher mount namespace.
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
    /// Lower-case SHA-256 digest read back from the file.
    pub readback_sha256: String,
}

/// Actual facts from one completed control-file publication.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPublicationRecord {
    /// Wire schema version. The value must be one.
    pub schema_version: u8,
    /// Operation identity supplied by the host.
    pub operation_id: String,
    /// Guest boot identity read from procfs.
    pub guest_boot_id: String,
    /// Mount namespace inode read from procfs.
    pub mount_namespace_id: u64,
    /// Fixed control file selected for publication.
    pub kind: ExecutionPublicationKind,
    /// SHA-256 digest of the expected canonical document.
    pub payload_sha256: String,
    /// Measured file and filesystem facts.
    pub file: ExecutionPublicationFile,
}
