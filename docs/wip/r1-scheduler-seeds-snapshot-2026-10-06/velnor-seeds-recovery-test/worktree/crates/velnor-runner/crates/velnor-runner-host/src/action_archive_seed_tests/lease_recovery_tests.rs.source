use std::error::Error;
use std::fs;
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};
use tar::{Builder, Header};

use crate::action_archive_seed::{
    ActionArchiveIdentity, ActionArchiveSeedError, ActionArchiveStore, PublicationStage,
};

static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Result<Self, Box<dyn Error>> {
        let sequence = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-action-lease-recovery-{}-{sequence}",
            std::process::id()
        ));
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
        make_writable(path)?;
        for entry in fs::read_dir(path)? {
            remove_tree(&entry?.path())?;
        }
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    }
}

fn make_writable(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    {
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions)
    }
}

fn archive_file(body: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut archive = Builder::new(encoder);
    let mut header = Header::new_gnu();
    header.set_path("package/action.yml")?;
    header.set_size(u64::try_from(body.len())?);
    header.set_mode(0o644);
    header.set_cksum();
    archive.append(&header, body)?;
    Ok(archive.into_inner()?.finish()?)
}

fn identity(bytes: &[u8]) -> Result<ActionArchiveIdentity, Box<dyn Error>> {
    Ok(ActionArchiveIdentity {
        repository_id: 101,
        name_with_owner: "owner/action".to_owned(),
        commit_sha: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        sha256: Sha256::digest(bytes).into(),
        size: u64::try_from(bytes.len())?,
    })
}

fn projected_archive(
    lease: &crate::action_archive_seed::ActionArchiveLease,
    action: &ActionArchiveIdentity,
) -> PathBuf {
    lease
        .cache_path()
        .join("owner_action")
        .join(format!("{}.tar.gz", action.commit_sha))
}

#[test]
fn recovery_reopens_exact_lease_after_restart_for_concurrent_readers() -> Result<(), Box<dyn Error>>
{
    let root = TestRoot::new()?;
    let store_path = root.path().join("store");
    let store = ActionArchiveStore::open(&store_path)?;
    let bytes = archive_file(b"name: action\n")?;
    let action = identity(&bytes)?;
    store.publish(&action, Cursor::new(&bytes))?;
    let original = store.lease(
        "instance-17-row-3",
        202,
        std::slice::from_ref(&action),
        None,
    )?;
    let generation = original.generation_id().to_owned();
    drop(store);

    let store = ActionArchiveStore::open(&store_path)?;
    let barrier = Arc::new(Barrier::new(6));
    let handles: Vec<_> = (0..6)
        .map(|_| {
            let store = store.clone();
            let barrier = Arc::clone(&barrier);
            let generation = generation.clone();
            thread::spawn(move || {
                barrier.wait();
                store.open_existing_lease("instance-17-row-3", &generation)
            })
        })
        .collect();
    for handle in handles {
        let recovered = handle
            .join()
            .map_err(|_| io::Error::other("recovery thread panicked"))??;
        assert_eq!(recovered.generation_id(), generation);
        assert_eq!(fs::read(projected_archive(&recovered, &action))?, bytes);
    }
    store.release_after_confirmed_cleanup(original.launch_id(), &generation)?;
    assert_eq!(
        store
            .open_existing_lease(original.launch_id(), &generation)
            .err(),
        Some(ActionArchiveSeedError::MissingArchive)
    );
    Ok(())
}

#[test]
fn recovery_rejects_missing_mismatched_interrupted_and_retired_leases() -> Result<(), Box<dyn Error>>
{
    let root = TestRoot::new()?;
    let store = ActionArchiveStore::open(root.path().join("store"))?;
    let bytes = archive_file(b"name: action\n")?;
    let action = identity(&bytes)?;
    store.publish(&action, Cursor::new(&bytes))?;
    assert_eq!(
        store
            .open_existing_lease("missing-row", &"0".repeat(64))
            .err(),
        Some(ActionArchiveSeedError::MissingArchive)
    );

    let lease = store.lease(
        "instance-17-row-3",
        202,
        std::slice::from_ref(&action),
        None,
    )?;
    let mut wrong_generation = lease.generation_id().to_owned();
    let replacement = if wrong_generation.starts_with('0') {
        '1'
    } else {
        '0'
    };
    wrong_generation.replace_range(..1, &replacement.to_string());
    assert_eq!(
        store
            .open_existing_lease(lease.launch_id(), &wrong_generation)
            .err(),
        Some(ActionArchiveSeedError::LeaseConflict)
    );

    assert_eq!(
        store.lease_with_fault(
            "partial-row",
            202,
            std::slice::from_ref(&action),
            PublicationStage::BeforeRename,
        ),
        Err(ActionArchiveSeedError::Io)
    );
    assert_eq!(
        store
            .open_existing_lease("partial-row", lease.generation_id())
            .err(),
        Some(ActionArchiveSeedError::MissingArchive)
    );

    assert_eq!(
        store.release_with_sync_fault(lease.launch_id(), lease.generation_id(), 1),
        Err(ActionArchiveSeedError::Io)
    );
    assert_eq!(
        store
            .open_existing_lease(lease.launch_id(), lease.generation_id())
            .err(),
        Some(ActionArchiveSeedError::LeaseConflict)
    );
    Ok(())
}

#[test]
fn recovery_rejects_corrupt_lease_manifest_projection_and_object() -> Result<(), Box<dyn Error>> {
    let root = TestRoot::new()?;
    let store_path = root.path().join("store");
    let store = ActionArchiveStore::open(&store_path)?;
    let bytes = archive_file(b"name: action\n")?;
    let action = identity(&bytes)?;
    store.publish(&action, Cursor::new(&bytes))?;
    let manifest_lease = store.lease("manifest-row", 202, std::slice::from_ref(&action), None)?;
    let projection_lease =
        store.lease("projection-row", 202, std::slice::from_ref(&action), None)?;
    let object_lease = store.lease("object-row", 202, std::slice::from_ref(&action), None)?;

    let lease_manifest = store_path.join("leases/manifest-row/manifest.json");
    make_writable(&lease_manifest)?;
    fs::write(&lease_manifest, b"{}")?;
    assert_eq!(
        store
            .open_existing_lease(manifest_lease.launch_id(), manifest_lease.generation_id())
            .err(),
        Some(ActionArchiveSeedError::Manifest)
    );

    let projection_file = projected_archive(&projection_lease, &action);
    make_writable(
        projection_file
            .parent()
            .ok_or("projection has no parent directory")?,
    )?;
    fs::remove_file(&projection_file)?;
    assert_eq!(
        store
            .open_existing_lease(
                projection_lease.launch_id(),
                projection_lease.generation_id()
            )
            .err(),
        Some(ActionArchiveSeedError::StoreIntegrity)
    );

    let object_path = fs::read_dir(store_path.join("objects"))?
        .next()
        .ok_or("object directory is empty")??
        .path();
    let object_manifest = object_path.join("manifest.json");
    make_writable(&object_manifest)?;
    fs::write(&object_manifest, b"{}")?;
    assert_eq!(
        store
            .open_existing_lease(object_lease.launch_id(), object_lease.generation_id())
            .err(),
        Some(ActionArchiveSeedError::Manifest)
    );
    Ok(())
}
