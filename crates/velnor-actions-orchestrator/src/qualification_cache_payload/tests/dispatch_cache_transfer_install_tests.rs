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
