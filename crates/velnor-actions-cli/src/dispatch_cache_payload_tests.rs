use super::super::digest_stage;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::os::unix::net::UnixListener;

#[test]
fn empty_stage_has_no_useful_state_digest() {
    let stage = tempfile::tempdir().expect("stage");
    assert_eq!(digest_stage(stage.path()), Ok(None));
}

#[test]
fn tree_digest_is_path_independent_and_entry_order_independent() {
    let first = tempfile::tempdir().expect("first stage");
    let second = tempfile::tempdir().expect("second stage");
    fs::create_dir_all(first.path().join("registry/cache")).expect("first directories");
    fs::create_dir_all(second.path().join("registry/cache")).expect("second directories");
    fs::write(first.path().join("registry/cache/a.crate"), b"package-a").expect("first file");
    fs::write(first.path().join("registry/cache/b.crate"), b"package-b").expect("second file");
    fs::write(second.path().join("registry/cache/b.crate"), b"package-b").expect("second file");
    fs::write(second.path().join("registry/cache/a.crate"), b"package-a").expect("first file");

    let first_digest = digest_stage(first.path()).expect("first digest");
    let second_digest = digest_stage(second.path()).expect("second digest");
    assert_eq!(first_digest, second_digest);
    let digest = first_digest.expect("non-empty stage");
    assert!(digest.starts_with("b3-"));
    assert_eq!(digest.len(), 67);
}

#[test]
fn tree_digest_changes_for_file_bytes_and_execute_mode() {
    let stage = tempfile::tempdir().expect("stage");
    let path = stage.path().join("tool");
    fs::write(&path, b"tool-v1").expect("file");
    let first = digest_stage(stage.path()).expect("first digest");

    fs::write(&path, b"tool-v2").expect("changed file");
    let second = digest_stage(stage.path()).expect("second digest");
    assert_ne!(first, second);

    let mut mode = fs::metadata(&path).expect("metadata").permissions();
    mode.set_mode(0o700);
    fs::set_permissions(&path, mode).expect("executable mode");
    let third = digest_stage(stage.path()).expect("third digest");
    assert_ne!(second, third);
}

#[test]
fn tree_digest_preserves_relative_internal_symlinks() {
    let first = tempfile::tempdir().expect("first stage");
    let second = tempfile::tempdir().expect("second stage");
    for stage in [first.path(), second.path()] {
        fs::create_dir_all(stage.join("installs/node/24.17.0/bin")).expect("tool tree");
        fs::write(stage.join("installs/node/24.17.0/bin/node"), b"node").expect("tool");
        symlink("24.17.0", stage.join("installs/node/latest")).expect("version alias");
    }

    let first_digest = digest_stage(first.path()).expect("first digest");
    let second_digest = digest_stage(second.path()).expect("second digest");
    assert_eq!(first_digest, second_digest);

    fs::remove_file(second.path().join("installs/node/latest")).expect("remove alias");
    symlink("24.17.0/bin", second.path().join("installs/node/latest")).expect("changed alias");
    assert_ne!(
        first_digest,
        digest_stage(second.path()).expect("changed digest")
    );
}

#[test]
fn tree_digest_rejects_symlink_escape_and_absolute_targets() {
    let stage = tempfile::tempdir().expect("stage");
    symlink("../outside", stage.path().join("escape")).expect("escaping link");
    assert_eq!(
        digest_stage(stage.path()),
        Err("qualification_cache_payload_symlink_escape")
    );

    fs::remove_file(stage.path().join("escape")).expect("remove escaping link");
    symlink("/etc/passwd", stage.path().join("absolute")).expect("absolute link");
    assert_eq!(
        digest_stage(stage.path()),
        Err("qualification_cache_payload_symlink_invalid")
    );
}

#[test]
fn tree_digest_preserves_internal_hardlinks_and_rejects_external_links() {
    let linked = tempfile::tempdir().expect("linked stage");
    fs::write(linked.path().join("one"), b"linked").expect("file");
    fs::hard_link(linked.path().join("one"), linked.path().join("two")).expect("hard link");
    let linked_digest = digest_stage(linked.path()).expect("internal hard link");

    let copied = tempfile::tempdir().expect("copied stage");
    fs::write(copied.path().join("one"), b"linked").expect("first copy");
    fs::write(copied.path().join("two"), b"linked").expect("second copy");
    assert_ne!(
        linked_digest,
        digest_stage(copied.path()).expect("copied digest")
    );

    let outside = tempfile::tempdir().expect("outside");
    fs::write(outside.path().join("one"), b"linked").expect("external file");
    fs::hard_link(outside.path().join("one"), outside.path().join("two")).expect("outside link");
    let external = tempfile::tempdir().expect("external stage");
    fs::hard_link(outside.path().join("one"), external.path().join("inside")).expect("stage link");
    assert_eq!(
        digest_stage(external.path()),
        Err("qualification_cache_payload_hardlink_escape")
    );
}

#[test]
fn tree_digest_rejects_socket_files() {
    let socket_stage = tempfile::tempdir().expect("socket stage");
    let _socket = UnixListener::bind(socket_stage.path().join("socket")).expect("socket file");
    assert_eq!(
        digest_stage(socket_stage.path()),
        Err("qualification_cache_payload_entry_unsupported")
    );
}

#[test]
fn tree_digest_rejects_oversized_files_before_reading_them() {
    let stage = tempfile::tempdir().expect("stage");
    let path = stage.path().join("oversized");
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .expect("file");
    file.set_len(super::super::MAX_TOTAL_BYTES + 1)
        .expect("sparse oversized file");
    assert_eq!(
        digest_stage(stage.path()),
        Err("qualification_cache_payload_limit")
    );
}

#[test]
fn tree_digest_rejects_a_symlink_root() {
    let real = tempfile::tempdir().expect("real stage");
    let parent = tempfile::tempdir().expect("parent");
    let alias = parent.path().join("alias");
    symlink(real.path(), &alias).expect("root symlink");
    assert_eq!(
        digest_stage(&alias),
        Err("qualification_cache_payload_root_invalid")
    );
}
