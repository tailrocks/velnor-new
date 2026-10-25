use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::execution_channel_wire::ExecutionChannelDirectory;

/// One request to publish the job facts and guest image documents.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JobFactsPublicationContext {
    /// Version of this request format.
    pub schema_version: u8,
    /// Unique name for this publication.
    pub operation_id: String,
    /// Expected identity of the guest boot.
    pub expected_guest_boot_id: String,
    /// Hex form of the `JobFacts` document.
    pub payload_hex: String,
    /// SHA-256 digest of the `JobFacts` document.
    pub job_facts_sha256: String,
    /// SHA-256 digest of the paired publication envelope.
    pub payload_sha256: String,
    /// Hex form of the guest image document.
    pub guest_image_hex: String,
    /// SHA-256 digest of the guest image document.
    pub guest_image_sha256: String,
    /// SHA-256 digest of the source and storage binding.
    pub publication_binding_sha256: String,
    /// Expected `JobFacts` directory identity in the Runner.
    pub expected_runner_job_facts: ExecutionChannelDirectory,
    /// Expected runtime identity directory in the Runner.
    pub expected_runner_runtime_identity: JobFactsRuntimeIdentityDirectory,
}

/// One stable directory identity from the initialized Runner.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JobFactsRuntimeIdentityDirectory {
    /// Major number of the device.
    pub device_major: u32,
    /// Minor number of the device.
    pub device_minor: u32,
    /// Inode number of the directory.
    pub inode: u64,
    /// User ID of the directory owner.
    pub owner_uid: u32,
    /// Group ID of the directory owner.
    pub owner_gid: u32,
    /// Permission bits of the directory.
    pub mode: u32,
}

/// Measured facts for one completed paired publication.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JobFactsPublicationRecord {
    /// Version of this record format.
    pub schema_version: u8,
    /// Unique name for this publication.
    pub operation_id: String,
    /// Identity of the guest boot.
    pub guest_boot_id: String,
    /// Mount namespace ID for this record only.
    pub mount_namespace_id: u64,
    /// SHA-256 digest of both documents and directory identities.
    pub payload_sha256: String,
    /// SHA-256 digest of the guest image document.
    pub guest_image_sha256: String,
    /// SHA-256 digest of the source and storage binding.
    pub publication_binding_sha256: String,
    /// Measured `JobFacts` directory identity.
    pub directory: ExecutionChannelDirectory,
    /// Measured runtime identity directory.
    pub runtime_identity_directory: ExecutionChannelDirectory,
    /// Measured `JobFacts` file.
    pub file: JobFactsPublicationFile,
    /// Measured guest image file.
    pub guest_image_file: JobFactsPublicationFile,
}

/// Measured identity of one published file.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JobFactsPublicationFile {
    /// Absolute path of the file.
    pub path: String,
    /// Inode number of the file.
    pub inode: u64,
    /// File size in bytes.
    pub bytes: u64,
    /// User ID of the file owner.
    pub owner_uid: u32,
    /// Group ID of the file owner.
    pub owner_gid: u32,
    /// Permission bits of the file.
    pub mode: u32,
    /// SHA-256 digest of the file readback.
    pub readback_sha256: String,
}

/// Hash both documents and stable storage identities in one fixed envelope.
///
/// # Errors
///
/// Return an error if JSON serialization fails.
pub fn paired_payload_sha256(
    context: &JobFactsPublicationContext,
) -> Result<String, serde_json::Error> {
    #[derive(Serialize)]
    struct Envelope<'a> {
        domain: &'static str,
        operation_id: &'a str,
        guest_boot_id: &'a str,
        publication_binding_sha256: &'a str,
        job_facts_bytes: usize,
        job_facts_sha256: &'a str,
        guest_image_bytes: usize,
        guest_image_sha256: &'a str,
        job_facts_directory: &'a ExecutionChannelDirectory,
        runtime_identity_directory: &'a JobFactsRuntimeIdentityDirectory,
    }

    let job_facts_bytes = context.payload_hex.len() / 2;
    let guest_image_bytes = context.guest_image_hex.len() / 2;
    let envelope = Envelope {
        domain: "velnor-job-facts-and-guest-image-v1",
        operation_id: &context.operation_id,
        guest_boot_id: &context.expected_guest_boot_id,
        publication_binding_sha256: &context.publication_binding_sha256,
        job_facts_bytes,
        job_facts_sha256: &context.job_facts_sha256,
        guest_image_bytes,
        guest_image_sha256: &context.guest_image_sha256,
        job_facts_directory: &context.expected_runner_job_facts,
        runtime_identity_directory: &context.expected_runner_runtime_identity,
    };
    let bytes = serde_json::to_vec(&envelope)?;
    Ok(lower_hex(&Sha256::digest(bytes)))
}

/// Make sure that the measured metadata-volume root maps to a fixed child subpath.
#[must_use]
pub fn mount_root_contains_subpath(volume_root: &str, expected_root: &str, subpath: &str) -> bool {
    if volume_root.is_empty()
        || expected_root.is_empty()
        || !volume_root.starts_with('/')
        || !expected_root.starts_with('/')
        || subpath.is_empty()
        || subpath.starts_with('/')
        || subpath
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || volume_root.ends_with('/') && volume_root != "/"
        || expected_root.ends_with('/') && expected_root != "/"
    {
        return false;
    }
    let prefix = if volume_root == "/" {
        "/".to_owned()
    } else {
        format!("{volume_root}/")
    };
    expected_root
        .strip_prefix(&prefix)
        .is_some_and(|suffix| suffix == subpath)
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

#[cfg(test)]
mod mount_root_tests {
    use super::mount_root_contains_subpath;

    #[test]
    fn fixed_subpath_root_matches_metadata_volume_root() {
        assert!(mount_root_contains_subpath("/", "/job-facts", "job-facts"));
        assert!(mount_root_contains_subpath(
            "/_data",
            "/_data/job-facts",
            "job-facts"
        ));
        assert!(!mount_root_contains_subpath(
            "/foreign",
            "/_data/job-facts",
            "job-facts"
        ));
        assert!(!mount_root_contains_subpath(
            "/",
            "/job-facts/other",
            "job-facts"
        ));
        assert!(!mount_root_contains_subpath(
            "/",
            "/job-facts",
            "../job-facts"
        ));
    }
}
