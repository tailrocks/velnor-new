// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use sha2::{Digest, Sha256};
use velnor_resource_probe::ProbeError;
use velnor_resource_probe::execution_channel_wire::ExecutionChannelDirectory;
use velnor_resource_probe::job_facts::publish_job_facts;
use velnor_resource_probe::job_facts_wire::{
    JobFactsPublicationContext, JobFactsRuntimeIdentityDirectory, paired_payload_sha256,
};

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    text
}

fn context(guest_platform: &str) -> Result<JobFactsPublicationContext, serde_json::Error> {
    let guest_image = serde_json::to_vec(&serde_json::json!({
        "schema": "velnor.guest-image/v1",
        "source_sha": "0123456789abcdef0123456789abcdef01234567",
        "guest_platform": guest_platform,
        "scale_set": {"name": "ubuntu-26.04-arm-scale-set", "labels": ["velnor"]},
        "images": {"runner": {}, "dind": {}},
    }))?;
    let guest_image_sha256 = hex(&Sha256::digest(&guest_image));
    let payload = serde_json::to_vec(&serde_json::json!({
        "schema": "velnor.storage-cache-admission",
        "version": 1,
        "scope": {},
        "reservation": {},
        "runner": {},
        "engine": {},
        "profile": {},
        "sample": {},
        "instance_binding": {},
        "instance_binding_digest": "0".repeat(64),
        "guest_image_sha256": guest_image_sha256,
        "cache": {},
        "job_facts_mount": {},
        "projection_digest": "0".repeat(64),
    }))?;
    let mut context = JobFactsPublicationContext {
        schema_version: 2,
        operation_id: "0123456789abcdef0123456789abcdef".to_owned(),
        expected_guest_boot_id: "01234567-89ab-cdef-0123-456789abcdef".to_owned(),
        payload_hex: hex(&payload),
        job_facts_sha256: hex(&Sha256::digest(&payload)),
        payload_sha256: String::new(),
        guest_image_hex: hex(&guest_image),
        guest_image_sha256,
        publication_binding_sha256: "1".repeat(64),
        expected_runner_job_facts: ExecutionChannelDirectory {
            target: "/opt/velnor/job-facts".to_owned(),
            inode: 2,
            owner_uid: 0,
            owner_gid: 0,
            mode: 0o555,
            mount_id: 3,
            statx_mount_id: 3,
            mount_root: "/".to_owned(),
            mount_source: "tmpfs".to_owned(),
            mount_device_major: 0,
            mount_device_minor: 40,
            statx_device_major: 0,
            statx_device_minor: 40,
            filesystem_type_magic: 0x0102_1994,
            filesystem_type_name: "tmpfs".to_owned(),
            filesystem_id: 5,
            read_only: true,
            executable_mount: false,
        },
        expected_runner_runtime_identity: JobFactsRuntimeIdentityDirectory {
            device_major: 0,
            device_minor: 41,
            inode: 7,
            owner_uid: 0,
            owner_gid: 0,
            mode: 0o555,
        },
    };
    context.payload_sha256 = paired_payload_sha256(&context)?;
    Ok(context)
}

#[test]
fn job_facts_accept_a_guest_image_of_either_runner_platform()
-> Result<(), Box<dyn std::error::Error>> {
    let refused = |result: &Result<_, ProbeError>| {
        matches!(
            result,
            Err(ProbeError::Invalid("job_facts_guest_image_json"))
        )
    };
    for platform in ["linux/amd64", "linux/arm64"] {
        let result = publish_job_facts(context(platform)?);
        assert!(!refused(&result), "{platform}: {result:?}");
    }
    let result = publish_job_facts(context("linux/riscv64")?);
    assert!(refused(&result), "{result:?}");
    Ok(())
}
