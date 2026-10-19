use super::{
    AttachmentFact, CONTROL_PATH, CONTROL_SOURCE, INBOX_PATH, INBOX_SOURCE, JOB_FACTS_PATH,
    JOB_FACTS_SOURCE,
};
use crate::journal::StorageRole;
use velnor_resource_probe::execution_channel_wire::ExecutionChannelDirectory;

fn control() -> (AttachmentFact, ExecutionChannelDirectory) {
    let fact = AttachmentFact {
        role: StorageRole::EngineMetadata,
        source_target: None,
        volume_subpath: Some(CONTROL_SOURCE.to_owned()),
        mount_root: "/source-proof/control".to_owned(),
        read_only: true,
        mount_id: 12,
        device: (0, 41),
        statx_device: (0, 41),
        fs_type_magic: 0x0102_1994,
        filesystem_id: 27,
        filesystem_instance_digest: "a".repeat(64),
    };
    let directory = ExecutionChannelDirectory {
        target: CONTROL_PATH.to_owned(),
        inode: 99,
        owner_uid: 0,
        owner_gid: 0,
        mode: 0o555,
        mount_id: 12,
        statx_mount_id: 12,
        mount_device_major: 0,
        mount_device_minor: 41,
        statx_device_major: 0,
        statx_device_minor: 41,
        mount_root: "/source-proof/control".to_owned(),
        mount_source: "/owned/metadata-volume".to_owned(),
        filesystem_type_magic: 0x0102_1994,
        filesystem_type_name: "devpts".to_owned(),
        filesystem_id: 27,
        read_only: true,
        executable_mount: false,
    };
    (fact, directory)
}

#[test]
fn control_attachment_accepts_exact_measured_identity() {
    let (fact, directory) = control();
    assert!(fact.matches(
        &directory,
        CONTROL_PATH,
        CONTROL_SOURCE,
        StorageRole::EngineMetadata,
        true,
    ));
}

#[test]
fn control_attachment_rejects_wrong_target_source_or_mount_identity() {
    let (fact, mut directory) = control();
    directory.target.push_str("/other");
    assert!(!fact.matches(
        &directory,
        CONTROL_PATH,
        CONTROL_SOURCE,
        StorageRole::EngineMetadata,
        true,
    ));

    let (fact, directory) = control();
    assert!(!fact.matches(
        &directory,
        CONTROL_PATH,
        "other",
        StorageRole::EngineMetadata,
        true,
    ));

    let (fact, mut directory) = control();
    directory.statx_mount_id += 1;
    assert!(!fact.matches(
        &directory,
        CONTROL_PATH,
        CONTROL_SOURCE,
        StorageRole::EngineMetadata,
        true,
    ));
}

#[test]
fn control_attachment_rejects_unsafe_guest_directory_facts() {
    let (fact, mut directory) = control();
    directory.read_only = false;
    assert!(!fact.matches(
        &directory,
        CONTROL_PATH,
        CONTROL_SOURCE,
        StorageRole::EngineMetadata,
        true,
    ));

    let (fact, mut directory) = control();
    directory.mode = 0o777;
    assert!(!fact.matches(
        &directory,
        CONTROL_PATH,
        CONTROL_SOURCE,
        StorageRole::EngineMetadata,
        true,
    ));
}

#[test]
fn runner_home_inbox_is_verified_on_its_own_tmpfs() {
    let fact = AttachmentFact {
        role: StorageRole::RunnerHome,
        source_target: None,
        volume_subpath: Some(INBOX_SOURCE.to_owned()),
        mount_root: "/source-proof/inbox".to_owned(),
        read_only: false,
        mount_id: 23,
        device: (0, 42),
        statx_device: (0, 42),
        fs_type_magic: 0x0102_1994,
        filesystem_id: 31,
        filesystem_instance_digest: "b".repeat(64),
    };
    let directory = ExecutionChannelDirectory {
        target: INBOX_PATH.to_owned(),
        inode: 101,
        owner_uid: 1000,
        owner_gid: 1000,
        mode: 0o700,
        mount_id: 23,
        statx_mount_id: 23,
        mount_root: "/source-proof/inbox".to_owned(),
        mount_source: "tmpfs".to_owned(),
        mount_device_major: 0,
        mount_device_minor: 42,
        statx_device_major: 0,
        statx_device_minor: 42,
        filesystem_type_magic: 0x0102_1994,
        filesystem_type_name: "tmpfs".to_owned(),
        filesystem_id: 31,
        read_only: false,
        executable_mount: false,
    };
    assert!(fact.matches(
        &directory,
        INBOX_PATH,
        INBOX_SOURCE,
        StorageRole::RunnerHome,
        false,
    ));
    assert!(!fact.matches(
        &directory,
        INBOX_PATH,
        INBOX_SOURCE,
        StorageRole::EngineMetadata,
        false,
    ));
}

#[test]
fn job_facts_directory_is_verified_on_its_metadata_volume_subpath() {
    let fact = AttachmentFact {
        role: StorageRole::EngineMetadata,
        source_target: None,
        volume_subpath: Some(JOB_FACTS_SOURCE.to_owned()),
        mount_root: "/volume/job-facts".to_owned(),
        read_only: true,
        mount_id: 24,
        device: (8, 1),
        statx_device: (8, 1),
        fs_type_magic: 0xef53,
        filesystem_id: 32,
        filesystem_instance_digest: "c".repeat(64),
    };
    let directory = ExecutionChannelDirectory {
        target: JOB_FACTS_PATH.to_owned(),
        inode: 102,
        owner_uid: 0,
        owner_gid: 0,
        mode: 0o555,
        mount_id: 24,
        statx_mount_id: 24,
        mount_root: "/volume/job-facts".to_owned(),
        mount_source: "/dev/vda".to_owned(),
        mount_device_major: 8,
        mount_device_minor: 1,
        statx_device_major: 8,
        statx_device_minor: 1,
        filesystem_type_magic: 0xef53,
        filesystem_type_name: "ext4".to_owned(),
        filesystem_id: 32,
        read_only: true,
        executable_mount: false,
    };
    assert!(fact.matches(
        &directory,
        JOB_FACTS_PATH,
        JOB_FACTS_SOURCE,
        StorageRole::EngineMetadata,
        true,
    ));
    assert!(!fact.matches(
        &directory,
        JOB_FACTS_PATH,
        JOB_FACTS_SOURCE,
        StorageRole::RunnerHome,
        true,
    ));
    let mut wrong_root = directory.clone();
    wrong_root.mount_root = "/foreign/job-facts".to_owned();
    assert!(!fact.matches(
        &wrong_root,
        JOB_FACTS_PATH,
        JOB_FACTS_SOURCE,
        StorageRole::EngineMetadata,
        true,
    ));
}
