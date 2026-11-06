// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::execution_channel_wire::ExecutionChannelDirectory;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JobFactsPublicationContext {
    pub schema_version: u8,

    pub operation_id: String,

    pub expected_guest_boot_id: String,

    pub payload_hex: String,

    pub job_facts_sha256: String,

    pub payload_sha256: String,

    pub guest_image_hex: String,

    pub guest_image_sha256: String,

    pub publication_binding_sha256: String,

    pub expected_runner_job_facts: ExecutionChannelDirectory,

    pub expected_runner_runtime_identity: JobFactsRuntimeIdentityDirectory,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JobFactsRuntimeIdentityDirectory {
    pub device_major: u32,

    pub device_minor: u32,

    pub inode: u64,

    pub owner_uid: u32,

    pub owner_gid: u32,

    pub mode: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JobFactsPublicationRecord {
    pub schema_version: u8,

    pub operation_id: String,

    pub guest_boot_id: String,

    pub mount_namespace_id: u64,

    pub payload_sha256: String,

    pub guest_image_sha256: String,

    pub publication_binding_sha256: String,

    pub directory: ExecutionChannelDirectory,

    pub runtime_identity_directory: ExecutionChannelDirectory,

    pub file: JobFactsPublicationFile,

    pub guest_image_file: JobFactsPublicationFile,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JobFactsPublicationFile {
    pub path: String,

    pub inode: u64,

    pub bytes: u64,

    pub owner_uid: u32,

    pub owner_gid: u32,

    pub mode: u32,

    pub readback_sha256: String,
}

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
