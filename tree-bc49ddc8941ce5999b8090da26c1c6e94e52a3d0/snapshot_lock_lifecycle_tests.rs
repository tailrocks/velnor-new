//! Actual descriptor locking; cooperative ownership, not a hostile same-UID seal.
use super::{lock, lock_with, same_file};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

const CONTENTS: &[u8] = b"nonempty persistent publication lock\n";

fn fixture() -> (tempfile::TempDir, PathBuf) {
    let parent = std::env::temp_dir().canonicalize().unwrap();
    let directory = tempfile::tempdir_in(parent).unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = directory.path().join(".mbx-publish.lock");
    std::fs::write(&path, CONTENTS).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    (directory, path)
}

fn assert_unchanged(path: &Path, before: &std::fs::Metadata) {
    let after = std::fs::symlink_metadata(path).unwrap();
    assert!(same_file(before, &after));
    assert_eq!(before.uid(), after.uid());
    assert_eq!(before.nlink(), after.nlink());
    assert_eq!(std::fs::read(path).unwrap(), CONTENTS);
}

#[test]
fn nonempty_lock_bytes_and_metadata_survive_repeated_descriptor_drop() {
    let (directory, path) = fixture();
    let before = std::fs::symlink_metadata(&path).unwrap();
    for _ in 0..8 {
        let held = lock(directory.path()).unwrap();
        assert_unchanged(&path, &before);
        drop(held);
        assert_unchanged(&path, &before);
    }
}

#[test]
fn independent_waiter_acquires_after_actual_holder_drop_without_mutation() {
    let (directory, path) = fixture();
    let before = std::fs::symlink_metadata(&path).unwrap();
    let held = lock(directory.path()).unwrap();
    let parent = directory.path().to_owned();
    let (blocked_sender, blocked_receiver) = std::sync::mpsc::channel();
    let waiter = std::thread::spawn(move || {
        lock_with(&parent, |descriptor| {
            assert!(matches!(
                descriptor.try_lock(),
                Err(std::fs::TryLockError::WouldBlock)
            ));
            blocked_sender.send(()).unwrap();
            Ok(())
        })
        .unwrap()
    });
    blocked_receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("independent descriptor observes actual held lock");
    assert_unchanged(&path, &before);
    drop(held);
    let acquired = waiter.join().expect("waiter acquires after owner closes");
    assert_unchanged(&path, &before);
    drop(acquired);
    assert_unchanged(&path, &before);
}

#[test]
fn identical_bytes_path_swap_after_open_is_refused_by_descriptor_identity() {
    let (directory, path) = fixture();
    let original = std::fs::symlink_metadata(&path).unwrap();
    let displaced = directory.path().join("displaced-lock");
    let error = lock_with(directory.path(), |_| {
        std::fs::rename(&path, &displaced)?;
        std::fs::write(&path, CONTENTS)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        Ok(())
    })
    .expect_err("same bytes cannot authenticate a replaced lock inode");
    assert!(
        error
            .to_string()
            .contains("publication lock changed while acquiring")
    );
    assert_eq!(std::fs::read(&path).unwrap(), CONTENTS);
    assert_eq!(std::fs::read(&displaced).unwrap(), CONTENTS);
    assert_eq!(std::fs::metadata(&displaced).unwrap().ino(), original.ino());
    assert_ne!(std::fs::metadata(&path).unwrap().ino(), original.ino());
    // Error drops the old descriptor; both independent files remain lockable.
    drop(lock(directory.path()).unwrap());
    let old = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(displaced)
        .unwrap();
    old.try_lock()
        .expect("failed acquisition releases displaced inode");
}
