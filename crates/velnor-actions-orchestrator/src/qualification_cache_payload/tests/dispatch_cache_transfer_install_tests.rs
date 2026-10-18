use super::*;
use rustix::fs::{OFlags, openat};
use std::fs;

#[test]
fn cleanup_preserves_replacement_at_temporary_name() {
    let scratch = tempfile::tempdir().expect("scratch");
    let base = fs::canonicalize(scratch.path()).expect("canonical path");
    let temporary_path = base.join("temporary");
    fs::write(&temporary_path, b"owned bytes").expect("owned file");
    let parent = open_owned_directory(&base).expect("parent");
    let temporary_name = c_name(std::ffi::OsStr::new("temporary")).expect("name");
    let descriptor = openat(
        &parent,
        temporary_name.as_c_str(),
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .expect("owned descriptor");
    let stat = rustix::fs::fstat(&descriptor).expect("owned stat");
    let created = CreatedPayload {
        digest: String::new(),
        descriptor,
        stat,
    };
    fs::rename(&temporary_path, base.join("retained-owned")).expect("retain original inode");
    fs::write(&temporary_path, b"foreign bytes").expect("replacement file");
    let prepared = PreparedRoot {
        parent,
        destination_name: c_name(std::ffi::OsStr::new("live")).expect("live name"),
        temporary_name,
        backup_name: c_name(std::ffi::OsStr::new("backup")).expect("backup name"),
        previous: None,
        temporary: Some(created),
        backup_moved: false,
        replacement_installed: false,
    };

    assert_eq!(
        cleanup_prepared(&[prepared]),
        Err("qualification_cache_remove_changed")
    );
    assert_eq!(
        fs::read(temporary_path).expect("preserved replacement"),
        b"foreign bytes"
    );
}

#[test]
fn rollback_rejects_replaced_backup_before_removing_imported_root() {
    let scratch = tempfile::tempdir().expect("scratch");
    let base = fs::canonicalize(scratch.path()).expect("canonical path");
    let live_path = base.join("live");
    let backup_path = base.join("backup");
    let saved_original_path = base.join("saved-original");
    fs::create_dir(&live_path).expect("original root");
    fs::write(live_path.join("state"), b"original").expect("original state");

    let parent = open_owned_directory(&base).expect("parent");
    let destination_name = c_name(std::ffi::OsStr::new("live")).expect("live name");
    let backup_name = c_name(std::ffi::OsStr::new("backup")).expect("backup name");
    let previous = stat_optional(&parent, destination_name.as_c_str())
        .expect("original stat")
        .expect("original root exists");
    fs::rename(&live_path, &backup_path).expect("move original to backup");

    fs::create_dir(&live_path).expect("imported root");
    fs::write(live_path.join("state"), b"imported").expect("imported state");
    let descriptor = openat(
        &parent,
        destination_name.as_c_str(),
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .expect("imported descriptor");
    let temporary = CreatedPayload {
        digest: String::new(),
        stat: rustix::fs::fstat(&descriptor).expect("imported stat"),
        descriptor,
    };
    fs::rename(&backup_path, &saved_original_path).expect("preserve original elsewhere");
    fs::create_dir(&backup_path).expect("foreign backup replacement");
    fs::write(backup_path.join("state"), b"foreign").expect("foreign state");

    let mut prepared = [PreparedRoot {
        parent,
        destination_name,
        temporary_name: c_name(std::ffi::OsStr::new("temporary")).expect("temporary name"),
        backup_name,
        previous: Some(previous),
        temporary: Some(temporary),
        backup_moved: true,
        replacement_installed: true,
    }];
    assert_eq!(
        rollback_prepared(&mut prepared),
        Err("qualification_cache_import_rollback_failed")
    );
    assert_eq!(
        fs::read(live_path.join("state")).expect("imported root preserved"),
        b"imported"
    );
    assert_eq!(
        fs::read(backup_path.join("state")).expect("foreign backup preserved"),
        b"foreign"
    );
    assert_eq!(
        fs::read(saved_original_path.join("state")).expect("original root preserved"),
        b"original"
    );
}

#[test]
fn rollback_rejects_replaced_backup_before_restoring_to_empty_root() {
    let scratch = tempfile::tempdir().expect("scratch");
    let base = fs::canonicalize(scratch.path()).expect("canonical path");
    let live_path = base.join("live");
    let backup_path = base.join("backup");
    let saved_original_path = base.join("saved-original");
    fs::create_dir(&live_path).expect("original root");
    fs::write(live_path.join("state"), b"original").expect("original state");

    let parent = open_owned_directory(&base).expect("parent");
    let destination_name = c_name(std::ffi::OsStr::new("live")).expect("live name");
    let previous = stat_optional(&parent, destination_name.as_c_str())
        .expect("original stat")
        .expect("original root exists");
    fs::rename(&live_path, &backup_path).expect("move original to backup");
    fs::rename(&backup_path, &saved_original_path).expect("preserve original elsewhere");
    fs::create_dir(&backup_path).expect("foreign backup replacement");
    fs::write(backup_path.join("state"), b"foreign").expect("foreign state");

    let mut prepared = [PreparedRoot {
        parent,
        destination_name,
        temporary_name: c_name(std::ffi::OsStr::new("temporary")).expect("temporary name"),
        backup_name: c_name(std::ffi::OsStr::new("backup")).expect("backup name"),
        previous: Some(previous),
        temporary: None,
        backup_moved: true,
        replacement_installed: false,
    }];
    assert_eq!(
        rollback_prepared(&mut prepared),
        Err("qualification_cache_import_rollback_failed")
    );
    assert!(
        !live_path.exists(),
        "rollback must not install the foreign backup"
    );
    assert_eq!(
        fs::read(backup_path.join("state")).expect("foreign backup preserved"),
        b"foreign"
    );
    assert_eq!(
        fs::read(saved_original_path.join("state")).expect("original root preserved"),
        b"original"
    );
}

#[test]
fn rollback_restores_the_original_inode_and_removes_the_imported_root() {
    let scratch = tempfile::tempdir().expect("scratch");
    let base = fs::canonicalize(scratch.path()).expect("canonical path");
    let live_path = base.join("live");
    let backup_path = base.join("backup");
    fs::create_dir(&live_path).expect("original root");
    fs::write(live_path.join("state"), b"original").expect("original state");

    let parent = open_owned_directory(&base).expect("parent");
    let destination_name = c_name(std::ffi::OsStr::new("live")).expect("live name");
    let backup_name = c_name(std::ffi::OsStr::new("backup")).expect("backup name");
    let previous = stat_optional(&parent, destination_name.as_c_str())
        .expect("original stat")
        .expect("original root exists");
    fs::rename(&live_path, &backup_path).expect("move original to backup");
    fs::create_dir(&live_path).expect("imported root");
    fs::write(live_path.join("state"), b"imported").expect("imported state");
    let descriptor = openat(
        &parent,
        destination_name.as_c_str(),
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .expect("imported descriptor");
    let temporary = CreatedPayload {
        digest: String::new(),
        stat: rustix::fs::fstat(&descriptor).expect("imported stat"),
        descriptor,
    };

    let mut prepared = [PreparedRoot {
        parent,
        destination_name,
        temporary_name: c_name(std::ffi::OsStr::new("temporary")).expect("temporary name"),
        backup_name,
        previous: Some(previous),
        temporary: Some(temporary),
        backup_moved: true,
        replacement_installed: true,
    }];
    rollback_prepared(&mut prepared).expect("restore original root");
    assert_eq!(
        fs::read(live_path.join("state")).expect("restored state"),
        b"original"
    );
    assert!(!backup_path.exists(), "imported backup should be removed");
}
