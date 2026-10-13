use std::error::Error;
use std::fs;
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};
use tar::{Builder, EntryType, Header};

use crate::action_archive_seed::{
    ActionArchiveIdentity, ActionArchiveSeedError, ActionArchiveStore,
};

#[path = "action_archive_seed_tests/archive_validation_tests.rs"]
mod archive_validation_tests;
#[path = "action_archive_seed_tests/lease_fault_tests.rs"]
mod lease_fault_tests;

static TEST_ID: AtomicU64 = AtomicU64::new(0);

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Result<Self, Box<dyn Error>> {
        let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("velnor-action-seed-{}-{id}", std::process::id()));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        if let Err(error) = remove_tree(&self.0) {
            eprintln!("test directory cleanup failed: {error}");
        }
    }
}

fn remove_tree(path: &Path) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        set_test_dir_writable(path)?;
        for entry in fs::read_dir(path)? {
            remove_tree(&entry?.path())?;
        }
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    }
}

#[cfg(unix)]
fn set_test_dir_writable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn set_test_dir_writable(path: &Path) -> io::Result<()> {
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions)
}

fn archive_file(name: &str, body: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut archive = Builder::new(encoder);
    let mut header = Header::new_gnu();
    header.set_path(name)?;
    header.set_size(u64::try_from(body.len())?);
    header.set_mode(0o644);
    header.set_cksum();
    archive.append(&header, body)?;
    Ok(archive.into_inner()?.finish()?)
}

fn archive_with_raw_path(name: &str, body: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let compressed = archive_file("package/action.yml", body)?;
    let mut decoder = GzDecoder::new(Cursor::new(compressed));
    let mut tar_bytes = Vec::new();
    decoder.read_to_end(&mut tar_bytes)?;
    let header = tar_bytes.get_mut(..512).ok_or("short tar fixture")?;
    header[..100].fill(0);
    header[..name.len()].copy_from_slice(name.as_bytes());
    header[148..156].fill(b' ');
    let checksum = header.iter().map(|byte| u32::from(*byte)).sum::<u32>();
    let formatted = format!("{checksum:06o}");
    header[148..154].copy_from_slice(formatted.as_bytes());
    header[154] = 0;
    header[155] = b' ';
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&tar_bytes)?;
    Ok(encoder.finish()?)
}

fn archive_symlink(name: &str, target: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut archive = Builder::new(encoder);
    let mut header = Header::new_gnu();
    header.set_path(name)?;
    header.set_link_name(target)?;
    header.set_entry_type(EntryType::Symlink);
    header.set_size(0);
    header.set_mode(0o777);
    header.set_cksum();
    archive.append(&header, io::empty())?;
    Ok(archive.into_inner()?.finish()?)
}

fn identity(repository_id: u64, name: &str, bytes: &[u8]) -> ActionArchiveIdentity {
    ActionArchiveIdentity {
        repository_id,
        name_with_owner: name.to_owned(),
        commit_sha: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        sha256: Sha256::digest(bytes).into(),
        size: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
    }
}

#[test]
fn publishes_verified_archive_and_projects_readonly_hardlink() -> Result<(), Box<dyn Error>> {
    let root = TestRoot::new()?;
    let store = ActionArchiveStore::open(root.path().join("store"))?;
    let bytes = archive_file("package/action.yml", b"name: action\n")?;
    let identity = identity(101, "ChainArgos/demo-action", &bytes);
    store.publish(&identity, Cursor::new(&bytes))?;
    let lease = store.lease("instance-17", 202, std::slice::from_ref(&identity), None)?;
    let cached = lease
        .cache_path()
        .join("ChainArgos_demo-action")
        .join(format!("{}.tar.gz", identity.commit_sha));
    assert_eq!(fs::read(&cached)?, bytes);
    assert_eq!(lease.launch_id(), "instance-17");
    assert_eq!(lease.generation_id().len(), 64);
    assert!(fs::metadata(&cached)?.permissions().readonly());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let object = root.path().join("store/objects");
        let object_file = fs::read_dir(object)?
            .next()
            .ok_or("missing object")??
            .path()
            .join("archive.tar.gz");
        assert_eq!(
            fs::metadata(object_file)?.ino(),
            fs::metadata(cached)?.ino()
        );
    }
    Ok(())
}

#[test]
fn rejects_wrong_digest_identity_and_wrong_repository_binding() -> Result<(), Box<dyn Error>> {
    let root = TestRoot::new()?;
    let store = ActionArchiveStore::open(root.path().join("store"))?;
    let bytes = archive_file("package/action.yml", b"safe\n")?;
    let correct = identity(101, "ChainArgos/demo-action", &bytes);
    let wrong_bytes = archive_file("package/action.yml", b"evil\n")?;
    assert_eq!(
        store.publish(&correct, Cursor::new(wrong_bytes)).err(),
        Some(ActionArchiveSeedError::DigestMismatch)
    );
    store.publish(&correct, Cursor::new(&bytes))?;
    assert_eq!(
        store
            .lease(
                "launch-mismatch",
                202,
                std::slice::from_ref(&correct),
                Some(&"f".repeat(64))
            )
            .err(),
        Some(ActionArchiveSeedError::LeaseConflict)
    );
    assert_eq!(fs::read_dir(root.path().join("store/leases"))?.count(), 0);
    let wrong_repository = identity(303, "ChainArgos/demo-action", &bytes);
    assert_eq!(
        store
            .lease("launch-1", 202, &[wrong_repository], None)
            .err(),
        Some(ActionArchiveSeedError::MissingArchive)
    );
    Ok(())
}

#[test]
fn rejects_case_aliases_and_noncanonical_commit_shas() -> Result<(), Box<dyn Error>> {
    let root = TestRoot::new()?;
    let store = ActionArchiveStore::open(root.path().join("store"))?;
    let bytes = archive_file("package/action.yml", b"safe\n")?;
    let first = identity(101, "Owner/action", &bytes);
    let second = identity(303, "owner/action", &bytes);
    assert_eq!(
        store.lease("launch-1", 202, &[first, second], None).err(),
        Some(ActionArchiveSeedError::InvalidIdentity)
    );
    let mut uppercase = identity(404, "owner/other", &bytes);
    uppercase.commit_sha.make_ascii_uppercase();
    assert_eq!(
        store.publish(&uppercase, Cursor::new(bytes)).err(),
        Some(ActionArchiveSeedError::InvalidIdentity)
    );
    Ok(())
}

#[test]
fn rejects_corrupt_gzip_traversal_absolute_and_escaping_symlink() -> Result<(), Box<dyn Error>> {
    let root = TestRoot::new()?;
    let store = ActionArchiveStore::open(root.path().join("store"))?;
    let traversal = archive_with_raw_path("../outside", b"bad\n")?;
    let traversal_identity = identity(101, "owner/action", &traversal);
    assert_eq!(
        store
            .publish(&traversal_identity, Cursor::new(traversal))
            .err(),
        Some(ActionArchiveSeedError::UnsafeEntry)
    );
    let absolute = archive_with_raw_path("/outside", b"bad\n")?;
    let absolute_identity = identity(101, "owner/action", &absolute);
    assert_eq!(
        store
            .publish(&absolute_identity, Cursor::new(absolute))
            .err(),
        Some(ActionArchiveSeedError::UnsafeEntry)
    );
    let escaping = archive_symlink("package/link", "../../outside")?;
    let escaping_identity = identity(101, "owner/action", &escaping);
    assert_eq!(
        store
            .publish(&escaping_identity, Cursor::new(escaping))
            .err(),
        Some(ActionArchiveSeedError::UnsafeEntry)
    );
    let mut corrupt = archive_file("package/action.yml", b"safe\n")?;
    let last = corrupt.len().checked_sub(1).ok_or("empty archive")?;
    corrupt[last] ^= 0xff;
    let corrupt_identity = identity(101, "owner/action", &corrupt);
    assert!(matches!(
        store.publish(&corrupt_identity, Cursor::new(corrupt)),
        Err(ActionArchiveSeedError::InvalidArchive)
    ));
    Ok(())
}

struct InterruptedReader {
    bytes: Cursor<Vec<u8>>,
    remaining: usize,
}

impl Read for InterruptedReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::other("interrupted seed"));
        }
        let limit = output.len().min(self.remaining);
        let count = self.bytes.read(&mut output[..limit])?;
        self.remaining = self.remaining.saturating_sub(count);
        Ok(count)
    }
}

#[test]
fn interrupted_publication_leaves_no_partial_generation() -> Result<(), Box<dyn Error>> {
    let root = TestRoot::new()?;
    let store = ActionArchiveStore::open(root.path().join("store"))?;
    let bytes = archive_file("package/action.yml", b"safe\n")?;
    let identity = identity(101, "owner/action", &bytes);
    let reader = InterruptedReader {
        bytes: Cursor::new(bytes),
        remaining: 7,
    };
    assert_eq!(
        store.publish(&identity, reader).err(),
        Some(ActionArchiveSeedError::Io)
    );
    let objects = fs::read_dir(root.path().join("store/objects"))?;
    assert_eq!(objects.count(), 0);
    Ok(())
}

#[test]
fn launch_lease_is_immutable_and_release_is_repeatable() -> Result<(), Box<dyn Error>> {
    let root = TestRoot::new()?;
    let store = ActionArchiveStore::open(root.path().join("store"))?;
    let bytes = archive_file("package/action.yml", b"safe\n")?;
    let identity = identity(101, "owner/action", &bytes);
    store.publish(&identity, Cursor::new(&bytes))?;
    let lease = store.lease(
        "instance-17-row-3",
        202,
        std::slice::from_ref(&identity),
        None,
    )?;
    let replay = store.lease(
        lease.launch_id(),
        202,
        std::slice::from_ref(&identity),
        Some(lease.generation_id()),
    )?;
    assert_eq!(replay.generation_id(), lease.generation_id());
    assert_eq!(
        store
            .lease(
                lease.launch_id(),
                202,
                std::slice::from_ref(&identity),
                Some(&"f".repeat(64)),
            )
            .err(),
        Some(ActionArchiveSeedError::LeaseConflict)
    );
    assert_eq!(
        store
            .lease(
                "instance-17-row-3",
                303,
                std::slice::from_ref(&identity),
                None
            )
            .err(),
        Some(ActionArchiveSeedError::LeaseConflict)
    );
    assert_eq!(
        store
            .release_after_confirmed_cleanup(lease.launch_id(), "f".repeat(64).as_str())
            .err(),
        Some(ActionArchiveSeedError::LeaseConflict)
    );
    store.release_after_confirmed_cleanup(lease.launch_id(), lease.generation_id())?;
    store.release_after_confirmed_cleanup(lease.launch_id(), lease.generation_id())?;
    assert!(!lease.cache_path().exists());
    assert_eq!(fs::read_dir(root.path().join("store/objects"))?.count(), 1);
    Ok(())
}

#[test]
fn concurrent_lease_requests_publish_one_exact_projection() -> Result<(), Box<dyn Error>> {
    let root = TestRoot::new()?;
    let store = ActionArchiveStore::open(root.path().join("store"))?;
    let bytes = archive_file("package/action.yml", b"safe\n")?;
    let identity = identity(101, "owner/action", &bytes);
    store.publish(&identity, Cursor::new(&bytes))?;
    let barrier = Arc::new(Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let store = store.clone();
            let barrier = Arc::clone(&barrier);
            let identity = identity.clone();
            thread::spawn(move || {
                barrier.wait();
                store.lease("instance-19-row-7", 202, &[identity], None)
            })
        })
        .collect();
    let mut generation = None;
    for handle in handles {
        let lease = handle.join().map_err(|_| "lease thread panicked")??;
        if let Some(expected) = &generation {
            assert_eq!(lease.generation_id(), expected);
        }
        generation = Some(lease.generation_id().to_owned());
        assert!(lease.cache_path().exists());
    }
    Ok(())
}
