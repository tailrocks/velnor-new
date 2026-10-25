use super::*;

#[test]
fn publication_accepts_namespace_local_mount_ids_for_same_volume_directory() {
    let channel = fixture();
    let published = published_directory(&channel, 999);
    assert!(publication_matches_channel_directory(&channel, &published));
}

#[test]
fn publication_rejects_wrong_volume_even_when_mount_facts_match() {
    let channel = fixture();
    let published = published_directory(&channel, channel.control().mount_id());
    let mismatches = [
        PublishedChannelDirectory {
            volume_name: "foreign-volume",
            ..published
        },
        PublishedChannelDirectory {
            volume_id: "foreign-volume-id",
            ..published
        },
        PublishedChannelDirectory {
            publisher_mount_id: 0,
            ..published
        },
        PublishedChannelDirectory {
            control_directory_inode: published.control_directory_inode + 1,
            ..published
        },
        PublishedChannelDirectory {
            filesystem_id: published.filesystem_id + 1,
            ..published
        },
        PublishedChannelDirectory {
            filesystem_instance_digest: "d",
            ..published
        },
        PublishedChannelDirectory {
            device: (8, 2),
            ..published
        },
        PublishedChannelDirectory {
            statx_device: (8, 2),
            ..published
        },
        PublishedChannelDirectory {
            fs_type_magic: 1,
            ..published
        },
    ];
    assert!(
        mismatches
            .iter()
            .all(|observed| !publication_matches_channel_directory(&channel, observed))
    );

    let mut invalid_channel = fixture();
    invalid_channel.control.mount_id = 0;
    assert!(!publication_matches_channel_directory(
        &invalid_channel,
        &published_directory(&invalid_channel, 999),
    ));
}

fn published_directory(
    channel: &ProofChannelRecord,
    publisher_mount_id: u64,
) -> PublishedChannelDirectory<'_> {
    let control = channel.control();
    PublishedChannelDirectory {
        volume_name: channel.metadata_volume_name(),
        volume_id: channel.metadata_volume_id(),
        publisher_mount_id,
        control_directory_inode: control.directory_inode,
        filesystem_id: control.filesystem_id(),
        filesystem_instance_digest: control.filesystem_instance_digest(),
        device: control.device(),
        statx_device: control.statx_device(),
        fs_type_magic: control.fs_type_magic(),
    }
}

fn fixture() -> ProofChannelRecord {
    ProofChannelRecord {
        schema_version: 3,
        reservation_id: 7,
        operation_id: 11,
        connection_id: "connection-a".to_owned(),
        scale_set_id: 13,
        witness_request_id: 100,
        engine_id: "engine-a".to_owned(),
        docker_root_digest: "a".repeat(64),
        runner_id: 17,
        runner_name: "vnonce".to_owned(),
        runner_container_id: "runner-container-0001".to_owned(),
        dind_container_id: "dind-container-00001".to_owned(),
        instance_binding_digest: "b".repeat(64),
        metadata_volume_name: "wmetadata".to_owned(),
        metadata_volume_id: "metadata-volume-0001".to_owned(),
        control: control_mount(),
        inbox: inbox_mount(),
        job_facts: job_facts_mount(),
        observation: observation(),
        dind_attachment_absent: true,
    }
}

fn control_mount() -> ProofChannelMountRecord {
    mount_record(&MountRecordSpec {
        target: CONTROL_TARGET,
        subpath: CONTROL_SUBPATH,
        mount_id: 21,
        filesystem_id: 31,
        directory_inode: 101,
        owner: (0, 0),
        mode: 0o555,
        read_only: true,
    })
}

fn inbox_mount() -> ProofChannelMountRecord {
    mount_record(&MountRecordSpec {
        target: INBOX_TARGET,
        subpath: INBOX_SUBPATH,
        mount_id: 22,
        filesystem_id: 32,
        directory_inode: 102,
        owner: (1000, 1000),
        mode: 0o700,
        read_only: false,
    })
}

fn job_facts_mount() -> ProofChannelMountRecord {
    mount_record(&MountRecordSpec {
        target: JOB_FACTS_TARGET,
        subpath: JOB_FACTS_SUBPATH,
        mount_id: 23,
        filesystem_id: 33,
        directory_inode: 103,
        owner: (0, 0),
        mode: 0o555,
        read_only: true,
    })
}

fn observation() -> ExecutionChannelObservation {
    ExecutionChannelObservation {
        schema_version: 3,
        operation_id: "0123456789abcdef0123456789abcdef".to_owned(),
        guest_boot_id: "11111111-2222-4333-8444-555555555555".to_owned(),
        mount_namespace_id: 44,
        control: control_directory(),
        inbox: inbox_directory(),
        job_facts: job_facts_directory(),
    }
}

fn control_directory() -> velnor_resource_probe::execution_channel_wire::ExecutionChannelDirectory {
    directory_record(&DirectoryRecordSpec {
        target: CONTROL_TARGET,
        inode: 101,
        mount_id: 21,
        device_major: 8,
        device_minor: 1,
        fs_type_magic: 0xef53,
        filesystem_id: 31,
        read_only: true,
        filesystem_type_name: "ext4",
        mount_source: "/dev/vdb",
    })
}

fn inbox_directory() -> velnor_resource_probe::execution_channel_wire::ExecutionChannelDirectory {
    directory_record(&DirectoryRecordSpec {
        target: INBOX_TARGET,
        inode: 102,
        mount_id: 22,
        device_major: 0,
        device_minor: 42,
        fs_type_magic: 0x0102_1994,
        filesystem_id: 32,
        read_only: false,
        filesystem_type_name: "tmpfs",
        mount_source: "tmpfs",
    })
}

fn job_facts_directory() -> velnor_resource_probe::execution_channel_wire::ExecutionChannelDirectory
{
    directory_record(&DirectoryRecordSpec {
        target: JOB_FACTS_TARGET,
        inode: 103,
        mount_id: 23,
        device_major: 8,
        device_minor: 1,
        fs_type_magic: 0xef53,
        filesystem_id: 33,
        read_only: true,
        filesystem_type_name: "ext4",
        mount_source: "/dev/vdb",
    })
}

struct MountRecordSpec<'a> {
    target: &'a str,
    subpath: &'a str,
    mount_id: u64,
    filesystem_id: u64,
    directory_inode: u64,
    owner: (u32, u32),
    mode: u32,
    read_only: bool,
}

fn mount_record(spec: &MountRecordSpec<'_>) -> ProofChannelMountRecord {
    ProofChannelMountRecord {
        target: spec.target.to_owned(),
        subpath: spec.subpath.to_owned(),
        mount_id: spec.mount_id,
        device_major: if spec.target == INBOX_TARGET { 0 } else { 8 },
        device_minor: if spec.target == INBOX_TARGET { 42 } else { 1 },
        statx_device_major: if spec.target == INBOX_TARGET { 0 } else { 8 },
        statx_device_minor: if spec.target == INBOX_TARGET { 42 } else { 1 },
        fs_type_magic: if spec.target == INBOX_TARGET {
            0x0102_1994
        } else {
            0xef53
        },
        filesystem_id: spec.filesystem_id,
        filesystem_instance_digest: "c".repeat(64),
        directory_inode: spec.directory_inode,
        owner_uid: spec.owner.0,
        owner_gid: spec.owner.1,
        mode: spec.mode,
        read_only: spec.read_only,
        no_copy: true,
    }
}

struct DirectoryRecordSpec<'a> {
    target: &'a str,
    inode: u64,
    mount_id: u64,
    device_major: u32,
    device_minor: u32,
    fs_type_magic: u64,
    filesystem_id: u64,
    read_only: bool,
    filesystem_type_name: &'a str,
    mount_source: &'a str,
}

fn directory_record(
    spec: &DirectoryRecordSpec<'_>,
) -> velnor_resource_probe::execution_channel_wire::ExecutionChannelDirectory {
    velnor_resource_probe::execution_channel_wire::ExecutionChannelDirectory {
        target: spec.target.to_owned(),
        inode: spec.inode,
        owner_uid: if spec.target == INBOX_TARGET { 1000 } else { 0 },
        owner_gid: if spec.target == INBOX_TARGET { 1000 } else { 0 },
        mode: if spec.target == INBOX_TARGET {
            0o700
        } else {
            0o555
        },
        mount_id: spec.mount_id,
        statx_mount_id: spec.mount_id,
        mount_root: match spec.target {
            CONTROL_TARGET => "/volume/source-proof/control",
            INBOX_TARGET => "/source-proof/inbox",
            JOB_FACTS_TARGET => "/volume/job-facts",
            _ => "/invalid",
        }
        .to_owned(),
        mount_source: spec.mount_source.to_owned(),
        mount_device_major: spec.device_major,
        mount_device_minor: spec.device_minor,
        statx_device_major: spec.device_major,
        statx_device_minor: spec.device_minor,
        filesystem_type_magic: spec.fs_type_magic,
        filesystem_type_name: spec.filesystem_type_name.to_owned(),
        filesystem_id: spec.filesystem_id,
        read_only: spec.read_only,
        executable_mount: false,
    }
}

fn serialized(record: &ProofChannelRecord) -> (String, String) {
    let bytes = record.canonical_json().expect("canonical channel JSON");
    let json = String::from_utf8(bytes.clone()).expect("UTF-8 channel JSON");
    (json, digest(&bytes))
}

#[test]
fn canonical_channel_record_reopens_with_exact_mount_facts() {
    let (json, expected_digest) = serialized(&fixture());
    let reopened = ProofChannelRecord::parse(&json, &expected_digest)
        .expect("the canonical proof channel must reopen");

    assert!(reopened.valid());
    assert_eq!(reopened.reservation_id(), 7);
    assert_eq!(reopened.witness_request_id(), 100);
    assert_eq!(reopened.control().mode, 0o555);
    assert_eq!(reopened.inbox().mode, 0o700);
}

#[test]
fn channel_record_rejects_changed_digest_or_noncanonical_json() {
    let (json, expected_digest) = serialized(&fixture());
    assert!(ProofChannelRecord::parse(&json, &"d".repeat(64)).is_err());

    let noncanonical = format!(" {json}");
    assert!(ProofChannelRecord::parse(&noncanonical, &expected_digest).is_err());
}

#[test]
fn channel_record_rejects_wrong_target_owner_mode_and_dind_attachment() {
    let wrong_target = mutate(|record| record.control.target = "/tmp/control".to_owned());
    assert!(!wrong_target.valid());

    let wrong_owner = mutate(|record| record.inbox.owner_uid = 0);
    assert!(!wrong_owner.valid());

    let wrong_mode = mutate(|record| record.control.mode = 0o4555);
    assert!(!wrong_mode.valid());

    let dind_attached = mutate(|record| record.dind_attachment_absent = false);
    assert!(!dind_attached.valid());
}

#[test]
fn channel_record_rejects_changed_job_facts_mount_and_directory() {
    let mut wrong_mount = fixture();
    wrong_mount.job_facts.subpath = "source-proof/control".to_owned();
    assert!(!wrong_mount.valid());

    let mut wrong_directory = fixture();
    wrong_directory.observation.job_facts.mount_root = "/foreign/../job-facts".to_owned();
    assert!(!wrong_directory.valid());

    let mut writable_directory = fixture();
    writable_directory.observation.job_facts.read_only = false;
    assert!(!writable_directory.valid());
}

#[test]
fn channel_record_accepts_distinct_mount_filesystems_and_rejects_changed_observation() {
    assert!(fixture().valid());
    let same_directory = mutate(|record| {
        record.inbox.directory_inode = record.control.directory_inode;
    });
    assert!(!same_directory.valid());

    let changed_observation = mutate(|record| record.observation.inbox.filesystem_id += 1);
    assert!(!changed_observation.valid());
}

fn mutate(change: impl FnOnce(&mut ProofChannelRecord)) -> ProofChannelRecord {
    let mut record = fixture();
    change(&mut record);
    record
}
