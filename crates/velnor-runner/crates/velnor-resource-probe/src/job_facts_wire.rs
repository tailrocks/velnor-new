use serde::{Deserialize, Serialize};

use crate::execution_channel_wire::ExecutionChannelDirectory;

/// One fixed request to publish the cache admission document.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JobFactsPublicationContext {
    /// Wire schema version. The value must be one.
    pub schema_version: u8,
    /// One durable publication operation identity.
    pub operation_id: String,
    /// Guest boot identity measured by the host.
    pub expected_guest_boot_id: String,
    /// Canonical document bytes in lower-case hexadecimal form.
    pub payload_hex: String,
    /// SHA-256 digest of the canonical document bytes.
    pub payload_sha256: String,
    /// Read-only `JobFacts` directory measured in the Runner namespace.
    pub expected_runner_job_facts: ExecutionChannelDirectory,
}

/// Measured facts for one completed cache admission publication.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JobFactsPublicationRecord {
    /// Wire schema version. The value must be one.
    pub schema_version: u8,
    /// One durable publication operation identity.
    pub operation_id: String,
    /// Guest boot identity read from procfs.
    pub guest_boot_id: String,
    /// Mount namespace inode read from procfs.
    pub mount_namespace_id: u64,
    /// SHA-256 digest of the expected document bytes.
    pub payload_sha256: String,
    /// Measured `JobFacts` directory in the publisher namespace.
    pub directory: ExecutionChannelDirectory,
    /// Actual published file and its backing filesystem facts.
    pub file: JobFactsPublicationFile,
}

/// Measured identity of the fixed admission file.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JobFactsPublicationFile {
    /// Fixed destination path in the publisher container.
    pub path: String,
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
    /// Lower-case SHA-256 digest read back from the file.
    pub readback_sha256: String,
}
