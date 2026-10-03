//! Cooperative publication checks; same-UID lock bypass is not an exec seal.

use super::{C_LAUNCHER, launcher_script_name, write_launcher_script};
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};

const VARIABLE: &str = "CMAKE_C_COMPILER_LAUNCHER";
const WRITERS: usize = 8;

struct Fixture {
    _root: tempfile::TempDir,
    directory: PathBuf,
    destination: PathBuf,
    installed: PathBuf,
    reference: PathBuf,
    expected: Vec<u8>,
}

impl Fixture {
    fn new() -> Self {
        let parent = std::env::temp_dir()
            .canonicalize()
            .expect("canonical temp root");
        let root = tempfile::Builder::new()
            .prefix("mbx-cmake-publication-")
            .tempdir_in(parent)
            .expect("private temporary fixture");
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
            .expect("private fixture root");
        let directory = root.path().join("scripts");
        let reference_directory = root.path().join("reference");
        for path in [&directory, &reference_directory] {
            std::fs::create_dir(path).expect("fixture directory");
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
                .expect("private fixture directory");
            assert_eq!(path.canonicalize().expect("canonical directory"), *path);
        }
        let installed = directory.join(C_LAUNCHER);
        write_launcher_script(&reference_directory, VARIABLE, C_LAUNCHER, &installed)
            .expect("serialized reference publication");
        let reference = reference_directory.join(launcher_script_name(C_LAUNCHER));
        let expected = std::fs::read(&reference).expect("reference bytes");
        let text = std::str::from_utf8(&expected).expect("CMake script is UTF-8");
        assert!(text.contains(VARIABLE));
        assert!(text.contains(installed.to_str().expect("fixture path is UTF-8")));
        Self {
            destination: directory.join(launcher_script_name(C_LAUNCHER)),
            _root: root,
            directory,
            installed,
            reference,
            expected,
        }
    }

    fn publish(&self) -> eyre::Result<()> {
        write_launcher_script(&self.directory, VARIABLE, C_LAUNCHER, &self.installed)
    }

    fn seed(&self, bytes: &[u8], mode: u32) {
        std::fs::write(&self.destination, bytes).expect("cached winner fixture");
        std::fs::set_permissions(&self.destination, std::fs::Permissions::from_mode(mode))
            .expect("cached winner permissions");
    }
}

fn inode(path: &Path) -> (u64, u64) {
    let metadata = std::fs::symlink_metadata(path).expect("fixture metadata");
    (metadata.dev(), metadata.ino())
}

fn assert_independent_snapshot(fixture: &Fixture) {
    let metadata = std::fs::symlink_metadata(&fixture.destination).expect("published metadata");
    assert!(metadata.is_file());
    assert_eq!(metadata.nlink(), 1);
    assert_eq!(metadata.mode() & 0o7777, 0o400);
    assert_eq!(
        metadata.uid(),
        std::fs::metadata(&fixture.reference).unwrap().uid()
    );
    assert_ne!(inode(&fixture.destination), inode(&fixture.reference));
    assert_eq!(
        std::fs::read(&fixture.destination).unwrap(),
        fixture.expected
    );
}

#[test]
fn concurrent_first_publication_keeps_one_independent_exact_snapshot() {
    let fixture = Fixture::new();
    let barrier = Arc::new(Barrier::new(WRITERS));
    let identities = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..WRITERS)
            .map(|_| {
                let barrier = Arc::clone(&barrier);
                let fixture = &fixture;
                scope.spawn(move || {
                    barrier.wait();
                    fixture
                        .publish()
                        .expect("cooperative concurrent publication");
                    assert_independent_snapshot(fixture);
                    inode(&fixture.destination)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("publication thread"))
            .collect::<Vec<_>>()
    });
    assert_eq!(identities.len(), WRITERS);
    assert!(identities.iter().all(|identity| *identity == identities[0]));
    assert_eq!(inode(&fixture.destination), identities[0]);
    assert_independent_snapshot(&fixture);
}

#[test]
fn cached_different_script_is_rejected_without_replacement() {
    let fixture = Fixture::new();
    let bytes = b"foreign cached launcher script\n";
    fixture.seed(bytes, 0o400);
    let before = inode(&fixture.destination);
    assert!(fixture.publish().is_err());
    assert_eq!(inode(&fixture.destination), before);
    assert_eq!(std::fs::read(&fixture.destination).unwrap(), bytes);
}

#[test]
fn cached_equal_size_corruption_is_rejected_without_replacement() {
    let fixture = Fixture::new();
    let mut corrupted = fixture.expected.clone();
    corrupted[0] ^= 1;
    fixture.seed(&corrupted, 0o400);
    let before = inode(&fixture.destination);
    assert_eq!(corrupted.len(), fixture.expected.len());
    assert!(fixture.publish().is_err());
    assert_eq!(inode(&fixture.destination), before);
    assert_eq!(std::fs::read(&fixture.destination).unwrap(), corrupted);
}

#[test]
fn cached_writable_mode_is_rejected_without_replacement() {
    let fixture = Fixture::new();
    fixture.seed(&fixture.expected, 0o600);
    let before = inode(&fixture.destination);
    assert!(fixture.publish().is_err());
    assert_eq!(inode(&fixture.destination), before);
    let metadata = std::fs::symlink_metadata(&fixture.destination).unwrap();
    assert_eq!(metadata.mode() & 0o7777, 0o600);
    assert_eq!(
        std::fs::read(&fixture.destination).unwrap(),
        fixture.expected
    );
}

#[test]
fn cached_symlink_is_rejected_without_touching_its_target() {
    let fixture = Fixture::new();
    symlink(&fixture.reference, &fixture.destination).expect("symlink winner fixture");
    let before = inode(&fixture.destination);
    let referent = inode(&fixture.reference);
    assert!(fixture.publish().is_err());
    assert_eq!(inode(&fixture.destination), before);
    assert_eq!(
        std::fs::read_link(&fixture.destination).unwrap(),
        fixture.reference
    );
    assert_eq!(inode(&fixture.reference), referent);
    assert_eq!(std::fs::read(&fixture.reference).unwrap(), fixture.expected);
}

#[test]
fn cached_hardlink_is_rejected_without_touching_its_peer() {
    let fixture = Fixture::new();
    std::fs::hard_link(&fixture.reference, &fixture.destination).expect("hardlink winner fixture");
    let before = inode(&fixture.destination);
    assert_eq!(before, inode(&fixture.reference));
    assert_eq!(std::fs::metadata(&fixture.reference).unwrap().nlink(), 2);
    assert!(fixture.publish().is_err());
    assert_eq!(inode(&fixture.destination), before);
    assert_eq!(inode(&fixture.reference), before);
    assert_eq!(std::fs::metadata(&fixture.reference).unwrap().nlink(), 2);
    assert_eq!(
        std::fs::read(&fixture.destination).unwrap(),
        fixture.expected
    );
}

#[test]
fn concurrent_cached_invalid_winner_is_rejected_without_replacement() {
    let fixture = Fixture::new();
    let mut corrupted = fixture.expected.clone();
    corrupted[0] ^= 1;
    fixture.seed(&corrupted, 0o400);
    let before = inode(&fixture.destination);
    let barrier = Arc::new(Barrier::new(WRITERS));
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..WRITERS)
            .map(|_| {
                let barrier = Arc::clone(&barrier);
                let fixture = &fixture;
                let corrupted = &corrupted;
                scope.spawn(move || {
                    barrier.wait();
                    assert!(fixture.publish().is_err());
                    assert_eq!(inode(&fixture.destination), before);
                    assert_eq!(
                        std::fs::read(&fixture.destination).unwrap().as_slice(),
                        corrupted.as_slice()
                    );
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("invalid winner publication thread");
        }
    });
    assert_eq!(inode(&fixture.destination), before);
    assert_eq!(std::fs::read(&fixture.destination).unwrap(), corrupted);
}
