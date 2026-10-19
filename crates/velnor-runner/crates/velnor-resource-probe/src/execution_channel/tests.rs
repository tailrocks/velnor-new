use std::{
    fs,
    os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
    path::PathBuf,
};

use rustix::process::{getegid, geteuid};

use crate::mountinfo::attachment_mount;

use super::{
    ExecutionChannelDirectory, INBOX_PATH, JOB_FACTS_PATH, REQUEST_FILE, ensure_channel_directory,
    request::{RequestMetadata, ensure_stable_content, ensure_stable_identity, read_request_file},
};

fn job_facts_mount(options: &str) -> crate::mountinfo::MountEntry {
    let input = format!(
        "35 20 0:48 /job-facts /opt/velnor/job-facts {options} - btrfs /dev/mapper/metadata rw\n"
    );
    attachment_mount(input.as_bytes(), JOB_FACTS_PATH).expect("read job facts mount")
}

#[test]
fn opened_inbox_must_match_the_measured_directory_binding() {
    let expected = ExecutionChannelDirectory {
        target: "/opt/velnor/source-proof/inbox".to_owned(),
        inode: 11,
        owner_uid: 1000,
        owner_gid: 1000,
        mode: 0o700,
        mount_id: 12,
        statx_mount_id: 12,
        mount_root: "/source-proof/inbox".to_owned(),
        mount_source: "/var/lib/docker/volumes/metadata/_data".to_owned(),
        mount_device_major: 0,
        mount_device_minor: 33,
        statx_device_major: 0,
        statx_device_minor: 33,
        filesystem_type_magic: 0x9123_683e,
        filesystem_type_name: "btrfs".to_owned(),
        filesystem_id: 44,
        read_only: false,
        executable_mount: false,
    };
    assert!(ensure_channel_directory(&expected, &expected).is_ok());
    let replaced_mount = ExecutionChannelDirectory {
        mount_id: 13,
        statx_mount_id: 13,
        ..expected.clone()
    };
    assert!(ensure_channel_directory(&expected, &replaced_mount).is_err());
    let replaced_inode = ExecutionChannelDirectory {
        inode: 14,
        ..expected.clone()
    };
    assert!(ensure_channel_directory(&expected, &replaced_inode).is_err());
}

#[test]
fn channel_directories_may_use_distinct_filesystems_with_matching_inode_numbers() {
    let inbox = ExecutionChannelDirectory {
        target: "/opt/velnor/source-proof/inbox".to_owned(),
        inode: 11,
        owner_uid: 1000,
        owner_gid: 1000,
        mode: 0o700,
        mount_id: 12,
        statx_mount_id: 12,
        mount_root: "/source-proof/inbox".to_owned(),
        mount_source: "/var/lib/docker/volumes/home/_data".to_owned(),
        mount_device_major: 0,
        mount_device_minor: 33,
        statx_device_major: 0,
        statx_device_minor: 33,
        filesystem_type_magic: 0x9123_683e,
        filesystem_type_name: "btrfs".to_owned(),
        filesystem_id: 44,
        read_only: false,
        executable_mount: false,
    };
    let control = ExecutionChannelDirectory {
        target: "/opt/velnor/source-proof/control".to_owned(),
        owner_uid: 0,
        owner_gid: 0,
        mode: 0o555,
        mount_id: 13,
        statx_mount_id: 13,
        mount_root: "/".to_owned(),
        mount_source: "/var/lib/docker/volumes/metadata/_data".to_owned(),
        mount_device_minor: 35,
        statx_device_minor: 35,
        filesystem_type_magic: 0x0102_1994,
        filesystem_type_name: "tmpfs".to_owned(),
        filesystem_id: 45,
        read_only: true,
        ..inbox.clone()
    };
    assert!(super::ensure_distinct_mounts(&control, &inbox).is_ok());
    let job_facts = ExecutionChannelDirectory {
        target: JOB_FACTS_PATH.to_owned(),
        inode: 12,
        owner_uid: 0,
        owner_gid: 0,
        mode: 0o555,
        mount_id: 14,
        statx_mount_id: 14,
        mount_root: "/job-facts".to_owned(),
        read_only: true,
        ..control.clone()
    };
    assert!(super::ensure_distinct_mounts(&control, &job_facts).is_ok());
    assert!(super::ensure_distinct_mounts(&inbox, &job_facts).is_ok());
    let same_object = ExecutionChannelDirectory {
        inode: control.inode,
        mount_root: control.mount_root.clone(),
        mount_device_major: control.mount_device_major,
        mount_device_minor: control.mount_device_minor,
        statx_device_major: control.statx_device_major,
        statx_device_minor: control.statx_device_minor,
        filesystem_type_magic: control.filesystem_type_magic,
        filesystem_type_name: control.filesystem_type_name.clone(),
        filesystem_id: control.filesystem_id,
        mount_id: control.mount_id + 1,
        statx_mount_id: control.statx_mount_id + 1,
        ..inbox.clone()
    };
    assert!(super::ensure_distinct_mounts(&control, &same_object).is_err());
    let same_mount = ExecutionChannelDirectory {
        mount_id: control.mount_id,
        statx_mount_id: control.statx_mount_id,
        ..inbox
    };
    assert!(super::ensure_distinct_mounts(&control, &same_mount).is_err());
}

#[test]
fn job_facts_directory_requires_its_fixed_target_and_root_read_only_contract() {
    let readonly = job_facts_mount("ro,nosuid,nodev");
    assert!(super::validate_directory_contract(JOB_FACTS_PATH, (0, 0), 0o555, &readonly).is_ok());
    assert!(super::validate_directory_contract(INBOX_PATH, (0, 0), 0o555, &readonly).is_err());
    assert!(super::validate_directory_contract(JOB_FACTS_PATH, (0, 0), 0o2555, &readonly).is_err());
    assert!(super::validate_directory_contract(JOB_FACTS_PATH, (1, 0), 0o555, &readonly).is_err());
    assert!(super::validate_directory_contract(JOB_FACTS_PATH, (0, 1), 0o555, &readonly).is_err());
    assert!(super::validate_directory_contract(JOB_FACTS_PATH, (0, 0), 0o755, &readonly).is_err());
}

#[test]
fn job_facts_directory_rejects_a_writable_mount() {
    let writable = job_facts_mount("rw,nosuid,nodev");
    assert!(super::validate_directory_contract(JOB_FACTS_PATH, (0, 0), 0o555, &writable).is_err());
}

#[test]
fn job_facts_binding_rejects_a_different_metadata_subpath() {
    let expected = ExecutionChannelDirectory {
        target: JOB_FACTS_PATH.to_owned(),
        inode: 61,
        owner_uid: 0,
        owner_gid: 0,
        mode: 0o555,
        mount_id: 35,
        statx_mount_id: 35,
        mount_root: "/job-facts".to_owned(),
        mount_source: "/var/lib/docker/volumes/metadata/_data".to_owned(),
        mount_device_major: 0,
        mount_device_minor: 48,
        statx_device_major: 0,
        statx_device_minor: 48,
        filesystem_type_magic: 0x9123_683e,
        filesystem_type_name: "btrfs".to_owned(),
        filesystem_id: 77,
        read_only: true,
        executable_mount: true,
    };
    assert!(ensure_channel_directory(&expected, &expected).is_ok());
    let other_subpath = ExecutionChannelDirectory {
        mount_root: "/source-proof/control".to_owned(),
        ..expected.clone()
    };
    assert!(ensure_channel_directory(&expected, &other_subpath).is_err());
    let other_source = ExecutionChannelDirectory {
        mount_source: "/var/lib/docker/volumes/other/_data".to_owned(),
        ..expected.clone()
    };
    assert!(ensure_channel_directory(&expected, &other_source).is_err());
}

struct TestInbox(PathBuf);

impl TestInbox {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-execution-inbox-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("make test inbox");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).expect("set inbox mode");
        Self(path)
    }

    fn file(&self, bytes: &[u8], mode: u32) {
        fs::write(self.0.join(REQUEST_FILE), bytes).expect("write request file");
        fs::set_permissions(self.0.join(REQUEST_FILE), fs::Permissions::from_mode(mode))
            .expect("set request mode");
    }

    fn open(&self) -> std::fs::File {
        std::fs::File::open(&self.0).expect("open test inbox")
    }
}

impl Drop for TestInbox {
    fn drop(&mut self) {
        let _cleanup_result = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn reads_exact_regular_request_and_rejects_changed_owner_or_mode() {
    let inbox = TestInbox::new();
    let payload = br#"{"schema_version":1}"#;
    inbox.file(payload, 0o600);
    let (bytes, identity) =
        read_request_file(&inbox.open(), geteuid().as_raw(), getegid().as_raw())
            .expect("read request");
    assert_eq!(bytes, payload);
    assert_eq!(identity.size, payload.len() as u64);
    assert_eq!(identity.mode, 0o600);
    assert_eq!(
        identity.user_id,
        inbox
            .0
            .join(REQUEST_FILE)
            .metadata()
            .expect("read request metadata")
            .uid()
    );

    assert!(
        read_request_file(
            &inbox.open(),
            geteuid().as_raw().wrapping_add(1),
            getegid().as_raw()
        )
        .is_err()
    );
    fs::set_permissions(
        inbox.0.join(REQUEST_FILE),
        fs::Permissions::from_mode(0o644),
    )
    .expect("change mode");
    assert!(read_request_file(&inbox.open(), geteuid().as_raw(), getegid().as_raw()).is_err());
}

#[test]
fn nofollow_and_nonblocking_open_reject_symlinks_and_fifos() {
    let inbox = TestInbox::new();
    std::os::unix::fs::symlink("missing", inbox.0.join(REQUEST_FILE)).expect("make symlink");
    assert!(read_request_file(&inbox.open(), geteuid().as_raw(), getegid().as_raw()).is_err());
    fs::remove_file(inbox.0.join(REQUEST_FILE)).expect("remove symlink");
    let status = std::process::Command::new("mkfifo")
        .arg(inbox.0.join(REQUEST_FILE))
        .status()
        .expect("start mkfifo");
    assert!(status.success());
    assert!(
        inbox
            .0
            .join(REQUEST_FILE)
            .metadata()
            .expect("read FIFO metadata")
            .file_type()
            .is_fifo()
    );
    assert!(read_request_file(&inbox.open(), geteuid().as_raw(), getegid().as_raw()).is_err());
}

#[test]
fn bounded_read_rejects_an_oversized_request() {
    let inbox = TestInbox::new();
    inbox.file(&vec![b'x'; super::REQUEST_LIMIT + 1], 0o600);
    assert!(read_request_file(&inbox.open(), geteuid().as_raw(), getegid().as_raw()).is_err());
}

#[test]
fn changed_inode_between_read_and_reopen_fails_closed() {
    let before = RequestMetadata {
        inode: 11,
        user_id: 1000,
        group_id: 1000,
        mode: 0o600,
        size: 4,
        modified_seconds: 1,
        modified_nanoseconds: 2,
        changed_seconds: 3,
        changed_nanoseconds: 4,
    };
    let changed_inode = RequestMetadata {
        inode: 12,
        ..before
    };
    let changed_time = RequestMetadata {
        changed_nanoseconds: 5,
        ..before
    };
    assert!(ensure_stable_identity(before, before, before, before).is_ok());
    assert!(ensure_stable_identity(before, changed_inode, changed_inode, changed_inode).is_err());
    assert!(ensure_stable_identity(before, changed_time, changed_time, changed_time).is_err());
    assert!(ensure_stable_content(b"same", b"same").is_ok());
    assert!(ensure_stable_content(b"first", b"other").is_err());
}
