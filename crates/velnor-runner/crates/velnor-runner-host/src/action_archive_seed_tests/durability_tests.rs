use std::error::Error;
use std::io::Cursor;
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
