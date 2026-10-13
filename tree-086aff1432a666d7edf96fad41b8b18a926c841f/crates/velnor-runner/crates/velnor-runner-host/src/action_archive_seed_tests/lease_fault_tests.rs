use std::error::Error;
use std::fs;
use std::io::Cursor;

use super::*;
use crate::action_archive_seed::PublicationStage;

#[test]
fn publication_faults_leave_no_visible_lease_or_staging_directory() -> Result<(), Box<dyn Error>> {
    let root = TestRoot::new()?;
    let store = ActionArchiveStore::open(root.path().join("store"))?;
    let bytes = archive_file("package/action.yml", b"safe\n")?;
    let identity = identity(101, "owner/action", &bytes);
    store.publish(&identity, Cursor::new(bytes))?;
    let lease_root = root.path().join("store/leases");

    for (index, stage) in [
        PublicationStage::AfterFirstLink,
        PublicationStage::BeforeDirectorySync,
        PublicationStage::BeforeRename,
    ]
    .into_iter()
    .enumerate()
    {
        let launch_id = format!("launch-{index}");
        assert!(
            store
                .lease_with_fault(&launch_id, 202, std::slice::from_ref(&identity), stage)
                .is_err()
        );
        assert_eq!(fs::read_dir(&lease_root)?.count(), 0);
    }
    Ok(())
}

#[test]
fn release_resumes_after_partial_tombstone_deletion() -> Result<(), Box<dyn Error>> {
    let root = TestRoot::new()?;
    let store = ActionArchiveStore::open(root.path().join("store"))?;
    let bytes = archive_file("package/action.yml", b"safe\n")?;
    let identity = identity(101, "owner/action", &bytes);
    store.publish(&identity, Cursor::new(bytes))?;
    let lease = store.lease(
        "launch-partial-delete",
        202,
        std::slice::from_ref(&identity),
        None,
    )?;
    let leases = root.path().join("store/leases");
    let retired = leases.join(format!(
        ".retired-{}-{}",
        lease.launch_id(),
        lease.generation_id()
    ));
    fs::rename(
        lease.cache_path().parent().ok_or("missing lease root")?,
        &retired,
    )?;
    set_test_dir_writable(&retired)?;
    fs::remove_file(retired.join("manifest.json"))?;

    store.release_after_confirmed_cleanup(lease.launch_id(), lease.generation_id())?;
    store.release_after_confirmed_cleanup(lease.launch_id(), lease.generation_id())?;
    assert!(!retired.exists());
    assert!(!lease.cache_path().exists());
    Ok(())
}
