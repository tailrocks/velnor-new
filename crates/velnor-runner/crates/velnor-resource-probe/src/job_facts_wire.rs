use serde::{Deserialize, Serialize};

use crate::execution_channel_wire::ExecutionChannelDirectory;

/// One request to publish the cache admission document.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JobFactsPublicationContext {
    /// Wire schema version. The value must be one.
    pub schema_version: u8,
    /// Publication operation identity recorded in the journal.
    pub operation_id: String,
    /// Guest boot identity measured by the host.
    pub expected_guest_boot_id: String,
    /// Document bytes in the specified JSON format, with hexadecimal encoding.
    pub payload_hex: String,
    /// SHA-256 digest of the document bytes.
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
    /// Publication operation identity recorded in the journal.
    pub operation_id: String,
    /// Guest boot identity read from procfs.
    pub guest_boot_id: String,
    /// Mount namespace inode read from procfs.
    pub mount_namespace_id: u64,
    /// SHA-256 digest of the expected document bytes.
    pub payload_sha256: String,
    /// Measured `JobFacts` directory in the publisher namespace.
    pub directory: ExecutionChannelDirectory,
    /// Published file and its filesystem data.
    pub file: JobFactsPublicationFile,
}

/// Measured identity of the admission file.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JobFactsPublicationFile {
    /// Destination path in the publisher container.
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
