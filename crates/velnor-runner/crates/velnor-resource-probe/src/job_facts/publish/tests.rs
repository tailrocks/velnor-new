use super::{
    PAYLOAD_LIMIT, compare_runner_directory, decode_payload, valid_document_shape, validate_context,
};
use crate::{
    execution_channel_wire::ExecutionChannelDirectory, job_facts_wire::JobFactsPublicationContext,
};
use serde_json::json;
use sha2::{Digest, Sha256};

fn directory() -> ExecutionChannelDirectory {
    ExecutionChannelDirectory {
        target: "/opt/velnor/job-facts".to_owned(),
        inode: 9,
        owner_uid: 0,
        owner_gid: 0,
        mode: 0o555,
        mount_id: 4,
        statx_mount_id: 4,
        mount_root: "/job-facts".to_owned(),
        mount_source: "/var/lib/docker/volumes/velnor/_data".to_owned(),
        mount_device_major: 0,
        mount_device_minor: 42,
        statx_device_major: 0,
        statx_device_minor: 42,
        filesystem_type_magic: 0x0102_1994,
        filesystem_type_name: "tmpfs".to_owned(),
        filesystem_id: 7,
        read_only: true,
        executable_mount: false,
    }
}

fn document() -> serde_json::Value {
    json!({
        "schema": "velnor.storage-cache-admission",
        "version": 1,
        "scope": {},
        "reservation": {},
        "runner": {},
        "engine": {},
        "profile": {},
        "sample": {},
        "instance_binding": {},
        "instance_binding_digest": "a".repeat(64),
        "cache": {},
        "job_facts_mount": {},
        "projection_digest": "b".repeat(64)
    })
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    output
}

fn context(bytes: &[u8]) -> JobFactsPublicationContext {
    JobFactsPublicationContext {
        schema_version: 1,
        operation_id: "a".repeat(32),
        expected_guest_boot_id: "12345678-1234-1234-1234-123456789abc".to_owned(),
        payload_hex: hex(bytes),
        payload_sha256: hex(&Sha256::digest(bytes)),
        expected_runner_job_facts: directory(),
    }
}

#[test]
fn accepts_the_fixed_consumer_document_shape() {
    let bytes = serde_json::to_vec(&document()).expect("serialize document");
    assert!(valid_document_shape(
        &serde_json::from_slice(&bytes).expect("parse document")
    ));
    validate_context(&context(&bytes)).expect("valid request");
}

#[test]
fn rejects_wrong_schema_version_and_extra_top_level_fields() {
    let mut wrong_version = document();
    wrong_version["version"] = json!(2);
    assert!(!valid_document_shape(&wrong_version));

    let mut extra_field = document();
    extra_field["runner_secret"] = json!("secret-canary");
    assert!(!valid_document_shape(&extra_field));
}

#[test]
fn rejects_digest_mismatch_and_payload_over_limit() {
    let bytes = serde_json::to_vec(&document()).expect("serialize document");
    let mut request = context(&bytes);
    request.payload_sha256 = "0".repeat(64);
    assert!(validate_context(&request).is_err());
    assert!(decode_payload(&"00".repeat(PAYLOAD_LIMIT + 1)).is_err());
}

#[test]
fn rejects_runner_directory_identity_changes() {
    let expected = directory();
    let mut observed = expected.clone();
    observed.mount_source = "/foreign/volume".to_owned();
    assert!(compare_runner_directory(&expected, &observed).is_err());
}
