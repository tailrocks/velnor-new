use super::*;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use rustix::fs::{AtFlags, StatxFlags, statx};
use rustix::process::{getegid, geteuid};

fn context(kind: ExecutionPublicationKind, bytes: &[u8]) -> ExecutionPublicationContext {
    ExecutionPublicationContext {
        schema_version: 1,
        operation_id: "0123456789abcdef0123456789abcdef".to_owned(),
        expected_guest_boot_id: "01234567-89ab-cdef-0123-456789abcdef".to_owned(),
        kind,
        payload_hex: lower_hex(bytes),
        payload_sha256: lower_hex(&Sha256::digest(bytes)),
        expected_runner_control: ExecutionChannelDirectory {
            target: CONTROL_PATH.to_owned(),
            inode: 7,
            owner_uid: CONTROL_UID,
            owner_gid: CONTROL_GID,
            mode: CONTROL_DIRECTORY_MODE,
            mount_id: 11,
            statx_mount_id: 11,
            mount_root: "/source-proof/control".to_owned(),
            mount_source: "/var/lib/docker/volumes/metadata/_data".to_owned(),
            mount_device_major: 8,
            mount_device_minor: 1,
            statx_device_major: 8,
            statx_device_minor: 1,
            filesystem_type_magic: 0x9123_683e,
            filesystem_type_name: "btrfs".to_owned(),
            filesystem_id: 9,
            read_only: true,
            executable_mount: false,
        },
    }
}

#[test]
fn publication_names_are_fixed_by_kind() {
    assert_eq!(
        ExecutionPublicationKind::Challenge.file_name(),
        "challenge.json"
    );
    assert_eq!(
        ExecutionPublicationKind::Decision.file_name(),
        "decision.json"
    );
}

#[test]
fn request_accepts_bounded_canonical_json_and_digest() {
    let request = context(ExecutionPublicationKind::Challenge, br#"{"nonce":"a"}"#);
    assert!(validate_request(&request).is_ok());
}

#[test]
fn request_rejects_changed_payload_digest() {
    let mut request = context(ExecutionPublicationKind::Challenge, br#"{"nonce":"a"}"#);
    request.payload_sha256 = "0".repeat(64);
    assert!(matches!(
        validate_request(&request),
        Err(ProbeError::Invalid("publication_payload_digest"))
    ));
}

#[test]
fn request_rejects_wrong_control_target() {
    let mut request = context(ExecutionPublicationKind::Challenge, br#"{"nonce":"a"}"#);
    request.expected_runner_control.target = "/tmp/control".to_owned();
    assert!(matches!(
        validate_request(&request),
        Err(ProbeError::Invalid("publication_runner_binding"))
    ));
}

#[test]
fn request_rejects_non_json_payload() {
    let request = context(ExecutionPublicationKind::Decision, b"not json");
    assert!(matches!(
        validate_request(&request),
        Err(ProbeError::Invalid("publication_payload_json"))
    ));
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-publication-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("make test directory");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
            .expect("set test directory mode");
        Self(path)
    }

    fn open(&self) -> File {
        File::open(&self.0).expect("open test directory")
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _mode_result = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
        let _cleanup_result = fs::remove_dir_all(&self.0);
    }
}

fn mount_fixture(directory: &File) -> (MountEntry, Vec<u8>) {
    let value = statx(
        directory,
        "",
        AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )
    .expect("measure test mount");
    let mountinfo = format!(
        "{} 1 {}:{} /source-proof/control /opt/velnor rw - overlay /var/lib/docker/volumes/metadata/_data rw\n",
        value.stx_mnt_id, value.stx_dev_major, value.stx_dev_minor
    )
    .into_bytes();
    (
        attachment_mount(&mountinfo, PUBLISHER_ROOT).expect("parse test mount"),
        mountinfo,
    )
}

fn publish_as_current_user(
    directory: &File,
    kind: ExecutionPublicationKind,
    operation_id: &str,
    bytes: &[u8],
    mount: &MountEntry,
    mountinfo: &[u8],
) -> Result<OpenedControlFile, ProbeError> {
    publish_or_read_with_owner(
        directory,
        kind,
        operation_id,
        bytes,
        mount,
        mountinfo,
        geteuid().as_raw(),
        getegid().as_raw(),
    )
}

#[test]
fn publisher_creates_one_fixed_file_and_accepts_same_bytes_on_replay() {
    let directory = TestDirectory::new();
    let fd = directory.open();
    let (mount, mountinfo) = mount_fixture(&fd);
    let bytes = br#"{"nonce":"a"}"#;
    let first = publish_as_current_user(
        &fd,
        ExecutionPublicationKind::Challenge,
        "0123456789abcdef0123456789abcdef",
        bytes,
        &mount,
        &mountinfo,
    )
    .expect("publish challenge");
    assert_eq!(first.bytes, bytes);
    assert_eq!(first.identity.mode, CONTROL_FILE_MODE);
    assert_eq!(first.identity.owner_uid, geteuid().as_raw());
    assert_eq!(first.identity.owner_gid, getegid().as_raw());
    assert_eq!(first.identity.size, bytes.len() as u64);

    let replay = publish_as_current_user(
        &fd,
        ExecutionPublicationKind::Challenge,
        "0123456789abcdef0123456789abcdef",
        bytes,
        &mount,
        &mountinfo,
    )
    .expect("accept the same request");
    assert_eq!(replay, first);
    assert!(directory.0.join("challenge.json").is_file());
    assert!(!directory.0.join("decision.json").exists());
}

#[test]
fn publisher_rejects_conflicting_replay_without_replacing_the_file() {
    let directory = TestDirectory::new();
    let fd = directory.open();
    let (mount, mountinfo) = mount_fixture(&fd);
    publish_as_current_user(
        &fd,
        ExecutionPublicationKind::Decision,
        "0123456789abcdef0123456789abcdef",
        br#"{"decision":true}"#,
        &mount,
        &mountinfo,
    )
    .expect("publish decision");

    assert!(matches!(
        publish_as_current_user(
            &fd,
            ExecutionPublicationKind::Decision,
            "0123456789abcdef0123456789abcdef",
            br#"{"decision":false}"#,
            &mount,
            &mountinfo,
        ),
        Err(ProbeError::Invalid("publication_replay_conflict"))
    ));
    assert_eq!(
        fs::read(directory.0.join("decision.json")).expect("read published decision"),
        br#"{"decision":true}"#
    );
}

#[test]
fn production_wrapper_keeps_the_root_file_owner_contract() {
    let directory = TestDirectory::new();
    let fd = directory.open();
    let (mount, mountinfo) = mount_fixture(&fd);
    let result = publish_or_read(
        &fd,
        ExecutionPublicationKind::Decision,
        "0123456789abcdef0123456789abcdef",
        br#"{"decision":true}"#,
        &mount,
        &mountinfo,
    );

    if geteuid().as_raw() == CONTROL_UID {
        let published = result.expect("root can publish with the production owner");
        assert_eq!(published.identity.owner_uid, CONTROL_UID);
        assert_eq!(published.identity.owner_gid, CONTROL_GID);
    } else {
        assert!(matches!(
            result,
            Err(ProbeError::Invalid("publication_owner"))
        ));
    }
}

#[test]
fn file_observation_rejects_an_unexpected_owner() {
    let directory = TestDirectory::new();
    let fd = directory.open();
    let (mount, mountinfo) = mount_fixture(&fd);
    let path = directory.0.join("owned.json");
    fs::write(&path, br#"{"nonce":"a"}"#).expect("write owned file");
    fs::set_permissions(&path, fs::Permissions::from_mode(CONTROL_FILE_MODE))
        .expect("set published file mode");
    let file = File::open(&path).expect("open owned file");
    let metadata = file.metadata().expect("stat owned file");
    let unexpected_uid = geteuid().as_raw().saturating_add(1);

    assert!(matches!(
        observation::observe_control_file_identity(
            &file,
            &fd,
            &mount,
            &mountinfo,
            &metadata,
            br#"{"nonce":"a"}"#.len(),
            unexpected_uid,
            getegid().as_raw(),
        ),
        Err(ProbeError::Invalid("publication_file_statx_fields"))
    ));
}

#[test]
fn publisher_rejects_symlink_destination() {
    let directory = TestDirectory::new();
    let fd = directory.open();
    let (mount, mountinfo) = mount_fixture(&fd);
    let outside = directory.0.join("outside.json");
    fs::write(&outside, br#"{"nonce":"a"}"#).expect("write outside file");
    symlink(&outside, directory.0.join("challenge.json"))
        .expect("make the specified symbolic link");

    assert!(matches!(
        publish_as_current_user(
            &fd,
            ExecutionPublicationKind::Challenge,
            "0123456789abcdef0123456789abcdef",
            br#"{"nonce":"a"}"#,
            &mount,
            &mountinfo,
        ),
        Err(ProbeError::Invalid("publication_destination_open"))
    ));
    assert_eq!(
        fs::read(outside).expect("read outside file"),
        br#"{"nonce":"a"}"#
    );
}
