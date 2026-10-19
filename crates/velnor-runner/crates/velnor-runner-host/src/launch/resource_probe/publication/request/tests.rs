use super::{PublicationRecord, PublicationRequest};
use velnor_resource_probe::{
    execution_channel_wire::ExecutionChannelDirectory,
    job_facts_wire::{
        JobFactsPublicationContext, JobFactsPublicationFile, JobFactsPublicationRecord,
    },
};

fn directory() -> ExecutionChannelDirectory {
    ExecutionChannelDirectory {
        target: "/opt/velnor/job-facts".to_owned(),
        inode: 23,
        owner_uid: 0,
        owner_gid: 0,
        mode: 0o555,
        mount_id: 7,
        statx_mount_id: 7,
        mount_root: "/job-facts".to_owned(),
        mount_source: "/dev/metadata".to_owned(),
        mount_device_major: 0,
        mount_device_minor: 8,
        statx_device_major: 0,
        statx_device_minor: 8,
        filesystem_type_magic: 0xef53,
        filesystem_type_name: "ext4".to_owned(),
        filesystem_id: 9,
        read_only: true,
        executable_mount: false,
    }
}

fn request() -> JobFactsPublicationContext {
    let payload_digest = "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a";
    JobFactsPublicationContext {
        schema_version: 1,
        operation_id: "0123456789abcdef0123456789abcdef".to_owned(),
        expected_guest_boot_id: "123e4567-e89b-12d3-a456-426614174000".to_owned(),
        payload_hex: "7b7d".to_owned(),
        payload_sha256: payload_digest.to_owned(),
        expected_runner_job_facts: directory(),
    }
}

fn record() -> JobFactsPublicationRecord {
    let payload_digest = "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a";
    JobFactsPublicationRecord {
        schema_version: 1,
        operation_id: "0123456789abcdef0123456789abcdef".to_owned(),
        guest_boot_id: "123e4567-e89b-12d3-a456-426614174000".to_owned(),
        mount_namespace_id: 10,
        payload_sha256: payload_digest.to_owned(),
        directory: directory(),
        file: JobFactsPublicationFile {
            path: "/opt/velnor/job-facts/storage-cache-admission.json".to_owned(),
            inode: 24,
            bytes: 2,
            owner_uid: 0,
            owner_gid: 0,
            mode: 0o444,
            readback_sha256: payload_digest.to_owned(),
        },
    }
}

#[test]
fn job_facts_request_uses_fixed_mode_and_target() {
    let request = PublicationRequest::JobFacts(request());

    assert_eq!(request.mode(), "job_facts_publication");
    assert_eq!(request.directory().target, "/opt/velnor/job-facts");
    assert!(request.validate().is_ok());
}

#[test]
fn job_facts_record_must_match_request_identity() {
    let request = PublicationRequest::JobFacts(request());
    let bytes = serde_json::to_vec(&record()).expect("record bytes");

    assert!(matches!(
        request.decode_record(&bytes),
        Ok(PublicationRecord::JobFacts(_))
    ));

    let mut changed = record();
    changed.operation_id = "f".repeat(32);
    let bytes = serde_json::to_vec(&changed).expect("record bytes");
    assert!(request.decode_record(&bytes).is_err());

    let mut changed = record();
    changed.guest_boot_id = "123e4567-e89b-12d3-a456-426614174001".to_owned();
    let bytes = serde_json::to_vec(&changed).expect("record bytes");
    assert!(request.decode_record(&bytes).is_err());
}

#[test]
fn job_facts_request_rejects_changed_target_and_oversized_payload() {
    let mut value = request();
    value.expected_runner_job_facts.target = "/tmp/job-facts".to_owned();
    assert!(PublicationRequest::JobFacts(value).validate().is_err());

    let mut value = request();
    value.payload_hex = "a".repeat(131_074);
    assert!(PublicationRequest::JobFacts(value).validate().is_err());
}

#[test]
fn job_facts_request_rejects_payload_digest_mismatch() {
    let mut value = request();
    value.payload_sha256 = "a".repeat(64);
    assert!(PublicationRequest::JobFacts(value).validate().is_err());
}
