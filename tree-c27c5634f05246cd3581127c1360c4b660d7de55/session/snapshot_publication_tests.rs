//! Cooperative malformed-lock checks, not a same-UID hostile execution seal.

use super::publish_bytes;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};

const LOCK_BYTES: &[u8] = b"existing cooperative lock bytes\n";
const SCRIPT_BYTES: &[u8] = b"set(OWNED_PUBLICATION_CONTROL true)\n";

struct Fixture {
    root: tempfile::TempDir,
    lock: PathBuf,
    destination: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let parent = std::env::temp_dir()
            .canonicalize()
            .expect("canonical temp root");
        let root = tempfile::Builder::new()
            .prefix("mbx-snapshot-lock-")
            .tempdir_in(parent)
            .expect("private publication fixture");
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
            .expect("private fixture permissions");
        assert_eq!(root.path().canonicalize().unwrap(), root.path());
        Self {
            lock: root.path().join(".mbx-publish.lock"),
            destination: root.path().join("control.cmake"),
            root,
        }
    }

    fn publish(&self) -> eyre::Result<()> {
        publish_bytes(&self.destination, SCRIPT_BYTES, 0o400)
    }

    fn assert_rejected_before_publication(&self) {
        let error = self
            .publish()
            .expect_err("malformed existing lock must fail");
        assert!(
            error
                .to_string()
                .contains("invalid owned snapshot publication lock"),
            "unexpected failure predicate: {error:#}"
        );
        assert_eq!(
            std::fs::symlink_metadata(&self.destination)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::NotFound
        );
        assert_no_staging_files(self.root.path());
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Entry {
    identity: (u64, u64, u64, u32, u32, u64),
    times: (i64, i64, i64, i64),
    bytes: Option<Vec<u8>>,
    link: Option<PathBuf>,
}

fn entry(path: &Path) -> Entry {
    let metadata = std::fs::symlink_metadata(path).expect("existing fixture entry");
    Entry {
        identity: (
            metadata.dev(),
            metadata.ino(),
            metadata.nlink(),
            metadata.mode(),
            metadata.uid(),
            metadata.len(),
        ),
        times: (
            metadata.mtime(),
            metadata.mtime_nsec(),
            metadata.ctime(),
            metadata.ctime_nsec(),
        ),
        bytes: metadata.is_file().then(|| std::fs::read(path).unwrap()),
        link: metadata
            .file_type()
            .is_symlink()
            .then(|| std::fs::read_link(path).unwrap()),
    }
}

fn write_lock(path: &Path, mode: u32) {
    std::fs::write(path, LOCK_BYTES).expect("lock fixture bytes");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .expect("lock fixture mode");
    assert_eq!(
        std::fs::symlink_metadata(path).unwrap().mode() & 0o7777,
        mode
    );
}

fn assert_no_staging_files(directory: &Path) {
    for item in std::fs::read_dir(directory).expect("fixture directory listing") {
        let name = item.expect("fixture directory entry").file_name();
        assert!(!name.to_string_lossy().starts_with(".mbx-script-"));
    }
}

fn valid_existing_lock_control() {
    let fixture = Fixture::new();
    write_lock(&fixture.lock, 0o600);
    let before = entry(&fixture.lock);
    fixture
        .publish()
        .expect("valid existing owned lock permits publication");
    assert_eq!(entry(&fixture.lock), before);
    let metadata = std::fs::symlink_metadata(&fixture.destination).unwrap();
    assert!(metadata.is_file());
    assert_eq!(metadata.nlink(), 1);
    assert_eq!(metadata.mode() & 0o7777, 0o400);
    assert_eq!(std::fs::read(&fixture.destination).unwrap(), SCRIPT_BYTES);
    assert_no_staging_files(fixture.root.path());
}

#[test]
fn existing_lock_symlink_is_rejected_without_touching_referent() {
    valid_existing_lock_control();
    let fixture = Fixture::new();
    let referent = fixture.root.path().join("referent.lock");
    write_lock(&referent, 0o600);
    symlink(&referent, &fixture.lock).expect("malformed symlink lock");
    let lock_before = entry(&fixture.lock);
    let referent_before = entry(&referent);
    fixture.assert_rejected_before_publication();
    assert_eq!(entry(&fixture.lock), lock_before);
    assert_eq!(entry(&referent), referent_before);
}

#[test]
fn existing_lock_directory_is_rejected_without_touching_contents() {
    valid_existing_lock_control();
    let fixture = Fixture::new();
    std::fs::create_dir(&fixture.lock).expect("malformed directory lock");
    std::fs::set_permissions(&fixture.lock, std::fs::Permissions::from_mode(0o700))
        .expect("protected malformed directory");
    let sentinel = fixture.lock.join("sentinel");
    std::fs::write(&sentinel, b"preserve directory contents").unwrap();
    let lock_before = entry(&fixture.lock);
    let sentinel_before = entry(&sentinel);
    fixture.assert_rejected_before_publication();
    assert_eq!(entry(&fixture.lock), lock_before);
    assert_eq!(entry(&sentinel), sentinel_before);
    assert_eq!(std::fs::read_dir(&fixture.lock).unwrap().count(), 1);
}

#[test]
fn existing_lock_writable_mode_is_rejected_without_repair() {
    valid_existing_lock_control();
    let fixture = Fixture::new();
    write_lock(&fixture.lock, 0o666);
    let before = entry(&fixture.lock);
    fixture.assert_rejected_before_publication();
    assert_eq!(entry(&fixture.lock), before);
}

#[test]
fn existing_lock_set_id_mode_is_rejected_without_repair() {
    valid_existing_lock_control();
    let fixture = Fixture::new();
    write_lock(&fixture.lock, 0o4600);
    let before = entry(&fixture.lock);
    fixture.assert_rejected_before_publication();
    assert_eq!(entry(&fixture.lock), before);
}

#[test]
fn existing_lock_hardlink_is_rejected_without_touching_peer() {
    valid_existing_lock_control();
    let fixture = Fixture::new();
    let peer = fixture.root.path().join("peer.lock");
    write_lock(&peer, 0o600);
    std::fs::hard_link(&peer, &fixture.lock).expect("malformed hardlink lock");
    let lock_before = entry(&fixture.lock);
    let peer_before = entry(&peer);
    assert_eq!(lock_before.identity.0, peer_before.identity.0);
    assert_eq!(lock_before.identity.1, peer_before.identity.1);
    assert_eq!(lock_before.identity.2, 2);
    fixture.assert_rejected_before_publication();
    assert_eq!(entry(&fixture.lock), lock_before);
    assert_eq!(entry(&peer), peer_before);
}
