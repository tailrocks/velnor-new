use std::error::Error;
use std::fs;
use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

use super::*;

#[test]
fn object_publication_sync_retries_after_failed_rename_sync() -> Result<(), Box<dyn Error>> {
    let root = TestRoot::new()?;
    let store_path = root.path().join("store");
    let store = ActionArchiveStore::open(&store_path)?;
    let bytes = archive_file("package/action.yml", b"safe\n")?;
    let archive = identity(101, "owner/action", &bytes);
    assert_eq!(
        store
            .publish_with_sync_fault(&archive, Cursor::new(&bytes), 1)
            .err(),
        Some(ActionArchiveSeedError::Io)
    );
    drop(store);

    let store = ActionArchiveStore::open(&store_path)?;
    assert_eq!(
        store
            .publish_with_sync_fault(&archive, Cursor::new(Vec::new()), 1)
            .err(),
        Some(ActionArchiveSeedError::Io)
    );
    drop(store);

    let store = ActionArchiveStore::open(&store_path)?;
    let barrier = Arc::new(Barrier::new(4));
    let handles: Vec<_> = (0..4)
        .map(|index| {
            let store = store.clone();
            let archive = archive.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                if index == 0 {
                    store.publish_with_sync_fault(&archive, Cursor::new(Vec::new()), 1)
                } else {
                    store.publish(&archive, Cursor::new(Vec::new()))
                }
            })
        })
        .collect();
    let mut errors = 0;
    let mut successes = 0;
    for handle in handles {
        match handle.join().map_err(|_| "object replay thread panicked")? {
            Ok(_) => successes += 1,
            Err(ActionArchiveSeedError::Io) => errors += 1,
            Err(error) => return Err(error.into()),
        }
    }
    assert_eq!(errors, 1);
    assert_eq!(successes, 3);
    Ok(())
}

#[test]
fn losing_concurrent_object_publisher_syncs_parent_before_retry() -> Result<(), Box<dyn Error>> {
    let root = TestRoot::new()?;
    let store = ActionArchiveStore::open(root.path().join("store"))?;
    let bytes = archive_file("package/action.yml", b"safe\n")?;
    let archive = identity(101, "owner/action", &bytes);
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let store = store.clone();
            let bytes = bytes.clone();
            let archive = archive.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let syncs = Arc::new(AtomicUsize::new(0));
                let recorded_syncs = Arc::clone(&syncs);
                let result = store.publish_with_parent_sync(
                    &archive,
                    Cursor::new(bytes),
                    |_| {
                        recorded_syncs.fetch_add(1, Ordering::Relaxed);
                        Err(ActionArchiveSeedError::Io)
                    },
                    || {
                        barrier.wait();
                        Ok(())
                    },
                );
                (result, syncs.load(Ordering::Relaxed))
            })
        })
        .collect();
    for handle in handles {
        let (result, syncs) = handle.join().map_err(|_| "object race thread panicked")?;
        assert_eq!(result.err(), Some(ActionArchiveSeedError::Io));
        assert_eq!(syncs, 1);
    }
    assert_eq!(fs::read_dir(root.path().join("store/objects"))?.count(), 1);
    store.publish(&archive, Cursor::new(Vec::new()))?;
    Ok(())
}

#[test]
fn lease_publication_sync_retries_after_failed_rename_sync() -> Result<(), Box<dyn Error>> {
    let root = TestRoot::new()?;
    let store_path = root.path().join("store");
    let store = ActionArchiveStore::open(&store_path)?;
    let bytes = archive_file("package/action.yml", b"safe\n")?;
    let archive = identity(101, "owner/action", &bytes);
    store.publish(&archive, Cursor::new(&bytes))?;
    let allowlist = [archive.clone()];
    let launch_id = "launch-durable-replay";
    assert_eq!(
        store
            .lease_with_sync_fault(launch_id, 202, &allowlist, None, 1)
            .err(),
        Some(ActionArchiveSeedError::Io)
    );
    drop(store);

    let store = ActionArchiveStore::open(&store_path)?;
    assert_eq!(
        store
            .lease_with_sync_fault(launch_id, 202, &allowlist, None, 1)
            .err(),
        Some(ActionArchiveSeedError::Io)
    );
    drop(store);

    let store = ActionArchiveStore::open(&store_path)?;
    let barrier = Arc::new(Barrier::new(4));
    let handles: Vec<_> = (0..4)
        .map(|index| {
            let store = store.clone();
            let archive = archive.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                if index == 0 {
                    store.lease_with_sync_fault(launch_id, 202, &[archive], None, 1)
                } else {
                    store.lease(launch_id, 202, &[archive], None)
                }
            })
        })
        .collect();
    let mut errors = 0;
    let mut successes = 0;
    for handle in handles {
        match handle.join().map_err(|_| "lease replay thread panicked")? {
            Ok(lease) => {
                assert_eq!(lease.launch_id(), launch_id);
                successes += 1;
            }
            Err(ActionArchiveSeedError::Io) => errors += 1,
            Err(error) => return Err(error.into()),
        }
    }
    assert_eq!(errors, 1);
    assert_eq!(successes, 3);
    Ok(())
}

#[test]
fn losing_concurrent_lease_publisher_syncs_parent_before_retry() -> Result<(), Box<dyn Error>> {
    let root = TestRoot::new()?;
    let store = ActionArchiveStore::open(root.path().join("store"))?;
    let bytes = archive_file("package/action.yml", b"safe\n")?;
    let archive = identity(101, "owner/action", &bytes);
    store.publish(&archive, Cursor::new(&bytes))?;
    let barrier = Arc::new(Barrier::new(2));
    let launch_id = "launch-concurrent-publication";
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let store = store.clone();
            let archive = archive.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let syncs = Arc::new(AtomicUsize::new(0));
                let recorded_syncs = Arc::clone(&syncs);
                let result = store.lease_with_test_hooks(
                    launch_id,
                    202,
                    &[archive],
                    None,
                    |stage| {
                        if stage == crate::action_archive_seed::PublicationStage::BeforeRename {
                            barrier.wait();
                        }
                        Ok(())
                    },
                    |_| {
                        recorded_syncs.fetch_add(1, Ordering::Relaxed);
                        Err(ActionArchiveSeedError::Io)
                    },
                );
                (result, syncs.load(Ordering::Relaxed))
            })
        })
        .collect();
    for handle in handles {
        let (result, syncs) = handle.join().map_err(|_| "lease race thread panicked")?;
        assert_eq!(result.err(), Some(ActionArchiveSeedError::Io));
        assert_eq!(syncs, 1);
    }
    assert_eq!(fs::read_dir(root.path().join("store/leases"))?.count(), 1);
    store.lease(launch_id, 202, &[archive], None)?;
    Ok(())
}

#[test]
fn release_sync_retries_tombstone_and_final_removal() -> Result<(), Box<dyn Error>> {
    let root = TestRoot::new()?;
    let store_path = root.path().join("store");
    let store = ActionArchiveStore::open(&store_path)?;
    let bytes = archive_file("package/action.yml", b"safe\n")?;
    let archive = identity(101, "owner/action", &bytes);
    store.publish(&archive, Cursor::new(&bytes))?;
    let lease = store.lease("launch-release-retry", 202, &[archive], None)?;
    let launch_id = lease.launch_id().to_owned();
    let generation_id = lease.generation_id().to_owned();
    let leases = store_path.join("leases");
    let retired = leases.join(format!(".retired-{launch_id}-{generation_id}"));

    assert_eq!(
        store
            .release_with_sync_fault(&launch_id, &generation_id, 1)
            .err(),
        Some(ActionArchiveSeedError::Io)
    );
    assert!(retired.exists());
    drop(store);

    let store = ActionArchiveStore::open(&store_path)?;
    assert_eq!(
        store
            .release_with_sync_fault(&launch_id, &generation_id, 1)
            .err(),
        Some(ActionArchiveSeedError::Io)
    );
    assert!(retired.join("manifest.json").exists());
    drop(store);

    let store = ActionArchiveStore::open(&store_path)?;
    assert_eq!(
        store
            .release_with_sync_fault(&launch_id, &generation_id, 2)
            .err(),
        Some(ActionArchiveSeedError::Io)
    );
    assert!(!retired.exists());
    drop(store);

    let store = ActionArchiveStore::open(&store_path)?;
    assert_eq!(
        store
            .release_with_sync_fault(&launch_id, &generation_id, 1)
            .err(),
        Some(ActionArchiveSeedError::Io)
    );
    drop(store);

    let store = ActionArchiveStore::open(&store_path)?;
    store.release_after_confirmed_cleanup(&launch_id, &generation_id)?;
    assert!(!retired.exists());
    assert!(!leases.join(&launch_id).exists());
    Ok(())
}

#[test]
fn release_rejects_foreign_tombstone_with_active_lease() -> Result<(), Box<dyn Error>> {
    let root = TestRoot::new()?;
    let store_path = root.path().join("store");
    let store = ActionArchiveStore::open(&store_path)?;
    let bytes = archive_file("package/action.yml", b"safe\n")?;
    let archive = identity(101, "owner/action", &bytes);
    store.publish(&archive, Cursor::new(&bytes))?;
    let lease = store.lease("launch-active-conflict", 202, &[archive], None)?;
    let active = lease.cache_path().parent().ok_or("missing active lease")?;
    let leases = store_path.join("leases");
    let foreign = leases.join(format!(".retired-{}-{}", lease.launch_id(), "f".repeat(64)));
    assert_ne!(lease.generation_id(), "f".repeat(64));
    fs::create_dir(&foreign)?;

    assert_eq!(
        store
            .release_after_confirmed_cleanup(lease.launch_id(), lease.generation_id())
            .err(),
        Some(ActionArchiveSeedError::LeaseConflict)
    );
    assert!(active.exists());
    assert!(foreign.exists());
    Ok(())
}

#[test]
fn release_rejects_multiple_retired_generations() -> Result<(), Box<dyn Error>> {
    let root = TestRoot::new()?;
    let store_path = root.path().join("store");
    let store = ActionArchiveStore::open(&store_path)?;
    let bytes = archive_file("package/action.yml", b"safe\n")?;
    let archive = identity(101, "owner/action", &bytes);
    store.publish(&archive, Cursor::new(&bytes))?;
    let lease = store.lease("launch-retired-conflict", 202, &[archive], None)?;
    let leases = store_path.join("leases");
    let active = lease.cache_path().parent().ok_or("missing active lease")?;
    let retired = leases.join(format!(
        ".retired-{}-{}",
        lease.launch_id(),
        lease.generation_id()
    ));
    let foreign = leases.join(format!(".retired-{}-{}", lease.launch_id(), "f".repeat(64)));
    assert_ne!(lease.generation_id(), "f".repeat(64));
    fs::rename(active, &retired)?;
    fs::create_dir(&foreign)?;

    assert_eq!(
        store
            .release_after_confirmed_cleanup(lease.launch_id(), lease.generation_id())
            .err(),
        Some(ActionArchiveSeedError::LeaseConflict)
    );
    assert!(retired.exists());
    assert!(foreign.exists());
    Ok(())
}
