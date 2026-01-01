use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};

use rustix::process::geteuid;

use super::super::*;
use crate::daemon_lock::DaemonLock;

#[test]
fn prelock_validation_is_read_only_and_returns_a_stable_identity() -> Result<(), String> {
    let state = tempfile::tempdir().map_err(|error| error.to_string())?;
    fs::set_permissions(state.path(), fs::Permissions::from_mode(0o750))
        .map_err(|error| error.to_string())?;

    let validated =
        validate_protected_state_directory(state.path()).map_err(|error| error.to_string())?;
    let identity = validated.identity().map_err(|error| error.to_string())?;
    let metadata = fs::symlink_metadata(state.path()).map_err(|error| error.to_string())?;
    assert_eq!(identity.device(), metadata.dev());
    assert_eq!(identity.inode(), metadata.ino());
    assert!(!state.path().join("diagnostics").exists());
    assert!(!state.path().join("daemon.lock").exists());
    assert_eq!(
        fs::read_dir(state.path())
            .map_err(|error| error.to_string())?
            .count(),
        0
    );
    Ok(())
}

#[test]
fn rejected_symlink_and_writable_parents_are_not_modified() -> Result<(), String> {
    let root = tempfile::tempdir().map_err(|error| error.to_string())?;
    let target = root.path().join("target");
    fs::create_dir(&target).map_err(|error| error.to_string())?;
    fs::set_permissions(&target, fs::Permissions::from_mode(0o750))
        .map_err(|error| error.to_string())?;
    let alias = root.path().join("state-link");
    symlink(&target, &alias).map_err(|error| error.to_string())?;
    assert!(matches!(
        validate_protected_state_directory(&alias),
        Err(HostError::Path)
    ));
    assert_eq!(
        fs::read_dir(&target)
            .map_err(|error| error.to_string())?
            .count(),
        0
    );

    let writable = root.path().join("writable-state");
    fs::create_dir(&writable).map_err(|error| error.to_string())?;
    fs::set_permissions(&writable, fs::Permissions::from_mode(0o770))
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        validate_protected_state_directory(&writable),
        Err(HostError::Path)
    ));
    assert_eq!(
        fs::read_dir(&writable)
            .map_err(|error| error.to_string())?
            .count(),
        0
    );
    assert!(!writable.join("diagnostics").exists());
    assert!(!writable.join("daemon.lock").exists());
    Ok(())
}

#[test]
fn diagnostics_and_lock_creation_use_the_retained_directory_after_path_replacement()
-> Result<(), String> {
    let root = tempfile::tempdir().map_err(|error| error.to_string())?;
    let state_path = root.path().join("state");
    fs::create_dir(&state_path).map_err(|error| error.to_string())?;
    fs::set_permissions(&state_path, fs::Permissions::from_mode(0o750))
        .map_err(|error| error.to_string())?;

    let validated =
        validate_protected_state_directory(&state_path).map_err(|error| error.to_string())?;
    let original_identity = validated.identity().map_err(|error| error.to_string())?;
    let moved_path = root.path().join("state-original");
    fs::rename(&state_path, &moved_path).map_err(|error| error.to_string())?;
    fs::create_dir(&state_path).map_err(|error| error.to_string())?;
    fs::set_permissions(&state_path, fs::Permissions::from_mode(0o750))
        .map_err(|error| error.to_string())?;
    let replacement =
        validate_protected_state_directory(&state_path).map_err(|error| error.to_string())?;
    assert_ne!(
        original_identity,
        replacement.identity().map_err(|error| error.to_string())?
    );

    let store = validated
        .open_diagnostics_store()
        .map_err(|error| error.to_string())?;
    let lock = DaemonLock::try_acquire_in(&validated).map_err(|error| error.to_string())?;
    assert!(lock.is_held());
    assert!(matches!(
        DaemonLock::try_acquire_in(&validated),
        Err(HostError::Lock)
    ));
    assert!(moved_path.join("diagnostics").is_dir());
    assert!(moved_path.join("daemon.lock").is_file());
    assert!(!state_path.join("diagnostics").exists());
    assert!(!state_path.join("daemon.lock").exists());

    let lock_metadata =
        fs::symlink_metadata(moved_path.join("daemon.lock")).map_err(|error| error.to_string())?;
    assert_eq!(lock_metadata.uid(), geteuid().as_raw());
    assert_eq!(lock_metadata.mode() & 0o777, 0o600);
    drop(lock);
    drop(store);
    Ok(())
}

#[test]
fn prelock_daemon_lock_rejects_symlinked_leaf_without_following_it() -> Result<(), String> {
    let root = tempfile::tempdir().map_err(|error| error.to_string())?;
    let state = root.path().join("state");
    fs::create_dir(&state).map_err(|error| error.to_string())?;
    fs::set_permissions(&state, fs::Permissions::from_mode(0o750))
        .map_err(|error| error.to_string())?;
    let target = root.path().join("outside-lock");
    fs::write(&target, b"unchanged").map_err(|error| error.to_string())?;
    symlink(&target, state.join("daemon.lock")).map_err(|error| error.to_string())?;

    let validated = validate_protected_state_directory(&state).map_err(|e| e.to_string())?;
    assert!(matches!(
        DaemonLock::try_acquire_in(&validated),
        Err(HostError::Lock)
    ));
    assert_eq!(
        fs::read(&target).map_err(|error| error.to_string())?,
        b"unchanged"
    );
    Ok(())
}
