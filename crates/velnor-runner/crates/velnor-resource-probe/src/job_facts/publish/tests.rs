use super::{
    PAYLOAD_LIMIT, compare_runner_directory, decode_payload, valid_document_shape, validate_context,
};
use crate::{
    execution_channel_wire::ExecutionChannelDirectory, job_facts_wire::JobFactsPublicationContext,
};
use serde_json::json;
use sha2::{Digest, Sha256};

#[cfg(target_os = "linux")]
#[test]
#[ignore = "Run in the root OrbStack test container."]
fn job_facts_pair_publication_writes_and_reads_both_documents() {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{
        DIRECTORY_MODE, FILE_MODE, FILE_NAME, FILE_PATH, GUEST_IMAGE_NAME, GUEST_IMAGE_PATH,
        inspect_file, publish_pair,
    };
    use crate::mountinfo::MountEntry;

    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "velnor-job-facts-pair-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let facts_path = root.join("job-facts");
    let image_path = root.join("runtime-identity");
    fs::create_dir_all(&facts_path).expect("create test root");
    fs::create_dir_all(&image_path).expect("create runtime directory");
    for path in [&facts_path, &image_path] {
        fs::set_permissions(path, fs::Permissions::from_mode(DIRECTORY_MODE))
            .expect("set test directory mode");
    }
    let facts_directory = fs::File::open(&facts_path).expect("open facts directory");
    let image_directory = fs::File::open(&image_path).expect("open image directory");
    let statx = rustix::fs::statx(
        &facts_directory,
        "",
        rustix::fs::AtFlags::EMPTY_PATH,
        rustix::fs::StatxFlags::BASIC_STATS | rustix::fs::StatxFlags::MNT_ID,
    )
    .expect("measure test mount");
    let mount = MountEntry {
        mount_id: statx.stx_mnt_id,
        device_major: statx.stx_dev_major,
        device_minor: statx.stx_dev_minor,
        root: "/".to_owned(),
        filesystem_type: "overlay".to_owned(),
        read_only: false,
        executable: true,
    };
    let facts = br#"{"schema":"velnor.storage-cache-admission","version":1}"#;
    let image = br#"{"schema":"velnor.guest-image/v1","guest_platform":"linux/amd64"}"#;
    publish_pair(
        &facts_directory,
        &image_directory,
        &"a".repeat(32),
        FILE_NAME,
        facts,
        GUEST_IMAGE_NAME,
        image,
    )
    .expect("publish both documents");
    let facts_readback = inspect_file(
        &facts_directory,
        FILE_NAME,
        FILE_PATH,
        facts,
        &mount,
        PAYLOAD_LIMIT,
    )
    .expect("read back JobFacts");
    let image_readback = inspect_file(
        &image_directory,
        GUEST_IMAGE_NAME,
        GUEST_IMAGE_PATH,
        image,
        &mount,
        16 * 1024,
    )
    .expect("read back guest image");
    assert_eq!(facts_readback.mode, FILE_MODE);
    assert_eq!(image_readback.mode, FILE_MODE);
    remove_pair_test_files(&root, &facts_path, &image_path);
}

#[cfg(target_os = "linux")]
fn remove_pair_test_files(
    root: &std::path::Path,
    facts: &std::path::Path,
    image: &std::path::Path,
) {
    use std::os::unix::fs::PermissionsExt;

    for path in [facts, image] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .expect("allow test cleanup");
    }
    std::fs::remove_dir_all(root).expect("remove test files");
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "Run in the root OrbStack test container."]
fn job_facts_pair_partial_publish_fails_without_replacing_either_document() {
    use std::{
        fs,
        io::Read,
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{DIRECTORY_MODE, FILE_MODE, FILE_NAME, GUEST_IMAGE_NAME, publish_pair};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "velnor-job-facts-partial-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let facts_path = root.join("job-facts");
    let image_path = root.join("runtime-identity");
    fs::create_dir_all(&facts_path).expect("create test root");
    fs::create_dir_all(&image_path).expect("create runtime directory");
    let original_image = b"foreign existing image";
    fs::write(image_path.join(GUEST_IMAGE_NAME), original_image).expect("create conflict");
    for path in [&facts_path, &image_path] {
        fs::set_permissions(path, fs::Permissions::from_mode(DIRECTORY_MODE))
            .expect("set test directory mode");
    }
    let facts_directory = fs::File::open(&facts_path).expect("open facts directory");
    let image_directory = fs::File::open(&image_path).expect("open image directory");
    fs::set_permissions(
        image_path.join(GUEST_IMAGE_NAME),
        fs::Permissions::from_mode(FILE_MODE),
    )
    .expect("set conflict mode");
    let facts = b"job facts";
    let image = b"guest image";
    let operation = "b".repeat(32);
    assert!(
        publish_pair(
            &facts_directory,
            &image_directory,
            &operation,
            FILE_NAME,
            facts,
            GUEST_IMAGE_NAME,
            image,
        )
        .is_err()
    );
    assert_eq!(
        fs::read(facts_path.join(FILE_NAME)).expect("first file remains"),
        facts
    );
    let mut existing = Vec::new();
    fs::File::open(image_path.join(GUEST_IMAGE_NAME))
        .expect("open conflict")
        .read_to_end(&mut existing)
        .expect("read conflict");
    assert_eq!(existing, original_image);
    assert!(
        publish_pair(
            &facts_directory,
            &image_directory,
            &operation,
            FILE_NAME,
            b"replacement facts",
            GUEST_IMAGE_NAME,
            b"replacement image",
        )
        .is_err()
    );
    assert_eq!(
        fs::read(facts_path.join(FILE_NAME)).expect("first file stays fixed"),
        facts
    );
    for path in [&facts_path, &image_path] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("allow test cleanup");
    }
    fs::remove_dir_all(root).expect("remove test files");
}

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

fn guest_document() -> serde_json::Value {
    json!({
        "schema": "velnor.guest-image/v1",
        "source_sha": "a".repeat(40),
        "guest_platform": "linux/amd64",
        "scale_set": { "name": "runner", "labels": ["velnor"] },
        "images": { "runner": {}, "dind": {} }
    })
}

fn context(bytes: &[u8]) -> JobFactsPublicationContext {
    let guest_bytes = serde_json::to_vec(&guest_document()).expect("serialize guest image");
    let mut value = JobFactsPublicationContext {
        schema_version: 2,
        operation_id: "a".repeat(32),
        expected_guest_boot_id: "12345678-1234-1234-1234-123456789abc".to_owned(),
        payload_hex: hex(bytes),
        job_facts_sha256: hex(&Sha256::digest(bytes)),
        payload_sha256: String::new(),
        guest_image_hex: hex(&guest_bytes),
        guest_image_sha256: hex(&Sha256::digest(&guest_bytes)),
        publication_binding_sha256: "c".repeat(64),
        expected_runner_job_facts: directory(),
        expected_runner_runtime_identity: crate::job_facts_wire::JobFactsRuntimeIdentityDirectory {
            device_major: 0,
            device_minor: 42,
            inode: 10,
            owner_uid: 0,
            owner_gid: 0,
            mode: 0o555,
        },
    };
    value.payload_sha256 =
        crate::job_facts_wire::paired_payload_sha256(&value).expect("paired payload digest");
    value
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

#[test]
fn accepts_volume_root_and_runner_subpath_mount_roots() {
    let expected = directory();
    let mut observed = expected.clone();
    observed.mount_root = "/".to_owned();
    observed.read_only = false;
    assert!(compare_runner_directory(&expected, &observed).is_ok());

    observed.mount_root = "/foreign".to_owned();
    assert!(compare_runner_directory(&expected, &observed).is_err());
}

#[test]
fn rejects_changes_to_guest_image_or_runtime_directory_identity() {
    let bytes = serde_json::to_vec(&document()).expect("serialize document");
    let mut request = context(&bytes);
    request.guest_image_hex = hex(b"{}");
    assert!(validate_context(&request).is_err());

    let mut request = context(&bytes);
    request.expected_runner_runtime_identity.inode += 1;
    assert!(validate_context(&request).is_err());
}
