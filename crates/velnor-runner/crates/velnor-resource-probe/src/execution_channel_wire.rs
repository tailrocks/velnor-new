use serde::{Deserialize, Serialize};

/// Input data for one measured Runner channel operation.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionChannelContext {
    /// Wire schema version. The value must be three.
    pub schema_version: u8,
    /// Operation identity supplied by the host.
    pub operation_id: String,
    /// Guest boot identity specified by the host.
    pub expected_guest_boot_id: String,
}

/// Measured facts for one specified channel directory.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionChannelDirectory {
    /// Target specified in the container.
    pub target: String,
    /// Directory inode from the open descriptor.
    pub inode: u64,
    /// Directory owner UID.
    pub owner_uid: u32,
    /// Directory owner GID.
    pub owner_gid: u32,
    /// Directory mode bits.
    pub mode: u32,
    /// Mount ID from mountinfo.
    pub mount_id: u64,
    /// Mount ID from statx.
    pub statx_mount_id: u64,
    /// Mount root from mount information.
    pub mount_root: String,
    /// Mount source from mount information.
    pub mount_source: String,
    /// Device major number from mountinfo.
    pub mount_device_major: u32,
    /// Device minor number from mountinfo.
    pub mount_device_minor: u32,
    /// Device major number from statx.
    pub statx_device_major: u32,
    /// Device minor number from statx.
    pub statx_device_minor: u32,
    /// Filesystem magic value from statfs.
    pub filesystem_type_magic: u64,
    /// Filesystem name from mount information.
    pub filesystem_type_name: String,
    /// Filesystem ID from statvfs.
    pub filesystem_id: u64,
    /// Read-only state from mountinfo.
    pub read_only: bool,
    /// Executable state from mountinfo.
    pub executable_mount: bool,
}

/// Measured identities for the Runner source-proof and job-facts directories.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionChannelObservation {
    /// Wire schema version. The value must be three.
    pub schema_version: u8,
    /// Operation identity supplied by the host.
    pub operation_id: String,
    /// Guest boot identity read from procfs.
    pub guest_boot_id: String,
    /// Mount namespace inode read from procfs.
    pub mount_namespace_id: u64,
    /// Measured control directory.
    pub control: ExecutionChannelDirectory,
    /// Measured inbox directory.
    pub inbox: ExecutionChannelDirectory,
    /// Measured job facts directory.
    pub job_facts: ExecutionChannelDirectory,
}

/// Read the request from the specified inbox after a check of its measured binding.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionRequestContext {
    /// Wire schema version. The value must be three.
    pub schema_version: u8,
    /// Operation identity supplied by the host.
    pub operation_id: String,
    /// Guest boot identity specified by the host.
    pub expected_guest_boot_id: String,
    /// Observation from the earlier channel measurement.
    pub expected_channel: ExecutionChannelObservation,
}

/// Request bytes and measured identity of the Runner request.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionRequestRecord {
    /// Wire schema version. The value must be three.
    pub schema_version: u8,
    /// Operation identity supplied by the host.
    pub operation_id: String,
    /// Measured guest boot identity.
    pub guest_boot_id: String,
    /// Measured channel facts at read time.
    pub channel: ExecutionChannelObservation,
    /// Lower-case hexadecimal encoding of the request bytes.
    pub bytes_hex: String,
    /// Lower-case SHA-256 digest of the request bytes.
    pub sha256: String,
    /// Request file inode.
    pub inode: u64,
    /// Request file owner UID.
    pub owner_uid: u32,
    /// Request file owner GID.
    pub owner_gid: u32,
    /// Request file mode bits.
    pub mode: u32,
    /// Request file byte length.
    pub size: u64,
}
