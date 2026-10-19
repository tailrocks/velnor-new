use std::{
    fs,
    os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
    path::PathBuf,
};

use rustix::process::{getegid, geteuid};

use super::{
    ExecutionChannelDirectory, REQUEST_FILE, ensure_channel_directory,
    request::{RequestMetadata, ensure_stable_content, ensure_stable_identity, read_request_file},
};

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
        mount_device_major: 0,
        mount_device_minor: 33,
        statx_device_major: 0,
        statx_device_minor: 33,
        filesystem_type_magic: 0x9123_683e,
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
