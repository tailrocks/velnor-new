use super::super::super::stage::open_parent;
use super::super::{create_archive, open_archive, resolve_under};
use super::common::archive_path;
use std::os::unix::fs::PermissionsExt;

#[test]
fn archive_parent_accepts_a_sticky_shared_temporary_ancestor() {
    let root = tempfile::tempdir().expect("temporary root");
    let shared = root.path().join("shared-temp");
    std::fs::create_dir(&shared).expect("create shared temp");
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o1777))
        .expect("protect shared temp with sticky bit");
    let runner_temp_path = shared.join("runner-temp");
    std::fs::create_dir(&runner_temp_path).expect("create private runner temp");
    std::fs::set_permissions(&runner_temp_path, std::fs::Permissions::from_mode(0o700))
        .expect("protect runner temp");
    let runner_temp = std::fs::canonicalize(runner_temp_path).expect("canonical runner temp");
    let archive = archive_path(&runner_temp);
    let resolved = resolve_under(&runner_temp, archive.as_os_str())
        .expect("resolve archive under runner temp");
    open_parent(&resolved).expect("open protected archive parent");

    let (file, _parent, _name) = create_archive(&runner_temp, archive.as_os_str())
        .expect("create archive under sticky shared ancestor");
    drop(file);
    assert_eq!(
        std::fs::metadata(&archive)
            .expect("read created archive metadata")
            .permissions()
            .mode()
            & 0o7777,
        0o600
    );
    open_archive(&runner_temp, archive_path(&runner_temp).as_os_str())
        .expect("open archive under sticky shared ancestor");
}

#[test]
fn archive_path_alias_resolves_under_the_trusted_root_before_descriptor_checks() {
    let root = tempfile::tempdir().expect("temporary root");
    let runner_temp_path = root.path().join("runner-temp");
    std::fs::create_dir(&runner_temp_path).expect("create runner temp");
    std::fs::set_permissions(&runner_temp_path, std::fs::Permissions::from_mode(0o700))
        .expect("protect runner temp");
    let runner_temp = std::fs::canonicalize(runner_temp_path).expect("canonical runner temp");
    let alias = root.path().join("runner-temp-alias");
    std::os::unix::fs::symlink(&runner_temp, &alias).expect("create runner temp alias");
    let archive = alias.join("cache.tar");

    let (file, _parent, _name) = create_archive(&runner_temp, archive.as_os_str())
        .expect("resolve SDK path alias under the trusted root");
    drop(file);
    assert!(runner_temp.join("cache.tar").is_file());
}

#[test]
fn archive_parent_rejects_writable_nonsticky_ancestor_before_creation() {
    let root = tempfile::tempdir().expect("temporary root");
    let shared = root.path().join("shared-temp");
    std::fs::create_dir(&shared).expect("create shared temp");
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o777))
        .expect("make ancestor unsafe");
    let runner_temp_path = shared.join("runner-temp");
    std::fs::create_dir(&runner_temp_path).expect("create private runner temp");
    std::fs::set_permissions(&runner_temp_path, std::fs::Permissions::from_mode(0o700))
        .expect("protect runner temp");
    let runner_temp = std::fs::canonicalize(runner_temp_path).expect("canonical runner temp");
    let archive = archive_path(&runner_temp);

    assert!(create_archive(&runner_temp, archive.as_os_str()).is_err());
    assert!(open_archive(&runner_temp, archive.as_os_str()).is_err());
    assert!(!archive.exists());
}

#[test]
fn archive_parent_rejects_unsafe_final_permissions() {
    let root = tempfile::tempdir().expect("temporary root");
    let runner_temp_path = root.path().join("runner-temp");
    std::fs::create_dir(&runner_temp_path).expect("create runner temp");
    std::fs::set_permissions(&runner_temp_path, std::fs::Permissions::from_mode(0o777))
        .expect("make final parent unsafe");
    let runner_temp = std::fs::canonicalize(runner_temp_path).expect("canonical runner temp");
    let archive = archive_path(&runner_temp);

    assert!(create_archive(&runner_temp, archive.as_os_str()).is_err());
    assert!(!archive.exists());
}

#[test]
fn archive_parent_rejects_symlink_replacement() {
    let root = tempfile::tempdir().expect("temporary root");
    let runner_temp_path = root.path().join("runner-temp");
    std::fs::create_dir(&runner_temp_path).expect("create runner temp");
    std::fs::set_permissions(&runner_temp_path, std::fs::Permissions::from_mode(0o700))
        .expect("protect runner temp");
    let runner_temp = std::fs::canonicalize(runner_temp_path).expect("canonical runner temp");
    let moved = root.path().join("runner-temp-moved");
    let archive = archive_path(&runner_temp);
    std::fs::rename(&runner_temp, &moved).expect("move runner temp");
    std::os::unix::fs::symlink(&moved, &runner_temp).expect("replace with symlink");

    assert!(create_archive(&runner_temp, archive.as_os_str()).is_err());
    assert!(open_archive(&runner_temp, archive.as_os_str()).is_err());
    assert!(!moved.join("cache.tar").exists());
}

#[test]
fn archive_create_preserves_an_existing_foreign_file() {
    let root = tempfile::tempdir().expect("temporary root");
    let runner_temp_path = root.path().join("runner-temp");
    std::fs::create_dir(&runner_temp_path).expect("create runner temp");
    let runner_temp = std::fs::canonicalize(runner_temp_path).expect("canonical runner temp");
    let archive = archive_path(&runner_temp);
    std::fs::write(&archive, b"foreign archive").expect("write foreign archive");

    assert!(create_archive(&runner_temp, archive.as_os_str()).is_err());
    assert_eq!(
        std::fs::read(archive).expect("read preserved file"),
        b"foreign archive"
    );
}
