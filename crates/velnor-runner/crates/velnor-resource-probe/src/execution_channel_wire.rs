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

/// One read result from the specified Runner inbox.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExecutionRequestOutcome {
    /// The request path was absent when the program first opened it without following links.
    NotYetPresent {
        /// Wire schema version. The value must be three.
        schema_version: u8,
        /// Request operation identity.
        operation_id: String,
        /// Guest boot identity measured during the read.
        guest_boot_id: String,
        /// Channel data read again during the request.
        channel: ExecutionChannelObservation,
    },
    /// A present request and its measured file identity.
    Present {
        /// Request record.
        record: ExecutionRequestRecord,
    },
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

#[cfg(test)]
mod tests {
    use super::{ExecutionChannelDirectory, ExecutionChannelObservation, ExecutionRequestOutcome};

    #[test]
    fn absence_wire_is_tagged_and_round_trips_without_request_bytes() {
        let directory = ExecutionChannelDirectory {
            target: "/opt/velnor/source-proof/control".to_owned(),
            inode: 10,
            owner_uid: 0,
            owner_gid: 0,
            mode: 0o555,
            mount_id: 11,
            statx_mount_id: 11,
            mount_root: "/source-proof/control".to_owned(),
            mount_source: "/dev/vda".to_owned(),
            mount_device_major: 254,
            mount_device_minor: 1,
            statx_device_major: 254,
            statx_device_minor: 1,
            filesystem_type_magic: 0x9123_683e,
            filesystem_type_name: "btrfs".to_owned(),
            filesystem_id: 12,
            read_only: true,
            executable_mount: false,
        };
        let channel = ExecutionChannelObservation {
            schema_version: 3,
            operation_id: "a".repeat(32),
            guest_boot_id: "boot".to_owned(),
            mount_namespace_id: 13,
            control: directory.clone(),
            inbox: ExecutionChannelDirectory {
                target: "/opt/velnor/source-proof/inbox".to_owned(),
                inode: 14,
                owner_uid: 1000,
                owner_gid: 1000,
                mode: 0o700,
                ..directory.clone()
            },
            job_facts: ExecutionChannelDirectory {
                target: "/opt/velnor/job-facts".to_owned(),
                inode: 15,
                owner_uid: 0,
                owner_gid: 0,
                mode: 0o555,
                ..directory
            },
        };
        let outcome = ExecutionRequestOutcome::NotYetPresent {
            schema_version: 3,
            operation_id: "b".repeat(32),
            guest_boot_id: "boot".to_owned(),
            channel,
        };
        let encoded = serde_json::to_vec(&outcome).expect("encode absence");
        assert!(
            encoded
                .windows(b"\"status\":\"not_yet_present\"".len())
                .any(|window| { window == b"\"status\":\"not_yet_present\"" })
        );
        assert_eq!(
            serde_json::from_slice::<ExecutionRequestOutcome>(&encoded).expect("decode absence"),
            outcome
        );
    }
}
