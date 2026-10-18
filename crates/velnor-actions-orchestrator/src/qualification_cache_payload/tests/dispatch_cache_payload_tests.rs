use super::super::digest_stage;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use tempfile::TempDir;

fn canonical_tempdir(label: &str) -> (TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect(label);
    let path = fs::canonicalize(temp.path()).expect("canonical temporary directory");
    (temp, path)
}

#[test]
fn empty_stage_has_no_useful_state_digest() {
    let (_stage, path) = canonical_tempdir("stage");
    assert_eq!(digest_stage(&path), Ok(None));
}

#[test]
fn tree_digest_is_path_independent_and_entry_order_independent() {
    let (_first, first) = canonical_tempdir("first stage");
    let (_second, second) = canonical_tempdir("second stage");
    fs::create_dir_all(first.join("registry/cache")).expect("first directories");
    fs::create_dir_all(second.join("registry/cache")).expect("second directories");
    fs::write(first.join("registry/cache/a.crate"), b"package-a").expect("first file");
    fs::write(first.join("registry/cache/b.crate"), b"package-b").expect("second file");
    fs::write(second.join("registry/cache/b.crate"), b"package-b").expect("second file");
    fs::write(second.join("registry/cache/a.crate"), b"package-a").expect("first file");

    let first_digest = digest_stage(&first).expect("first digest");
    let second_digest = digest_stage(&second).expect("second digest");
    assert_eq!(first_digest, second_digest);
    let digest = first_digest.expect("non-empty stage");
    assert!(digest.starts_with("b3-"));
    assert_eq!(digest.len(), 67);
}

#[test]
fn tree_digest_changes_for_file_bytes_and_execute_mode() {
    let (_stage, stage) = canonical_tempdir("stage");
    let path = stage.join("tool");
    fs::write(&path, b"tool-v1").expect("file");
    let first = digest_stage(&stage).expect("first digest");

    fs::write(&path, b"tool-v2").expect("changed file");
    let second = digest_stage(&stage).expect("second digest");
    assert_ne!(first, second);

    let mut mode = fs::metadata(&path).expect("metadata").permissions();
    mode.set_mode(0o700);
    fs::set_permissions(&path, mode).expect("executable mode");
    let third = digest_stage(&stage).expect("third digest");
    assert_ne!(second, third);
}

#[test]
fn tree_digest_preserves_relative_internal_symlinks() {
    let (_first, first) = canonical_tempdir("first stage");
    let (_second, second) = canonical_tempdir("second stage");
    for stage in [&first, &second] {
        fs::create_dir_all(stage.join("installs/node/24.17.0/bin")).expect("tool tree");
        fs::write(stage.join("installs/node/24.17.0/bin/node"), b"node").expect("tool");
        symlink("24.17.0", stage.join("installs/node/latest")).expect("version alias");
    }

    let first_digest = digest_stage(&first).expect("first digest");
    let second_digest = digest_stage(&second).expect("second digest");
    assert_eq!(first_digest, second_digest);

    fs::remove_file(second.join("installs/node/latest")).expect("remove alias");
    symlink("24.17.0/bin", second.join("installs/node/latest")).expect("changed alias");
    assert_ne!(first_digest, digest_stage(&second).expect("changed digest"));
}

#[test]
fn tree_digest_rejects_symlink_escape_and_absolute_targets() {
    let (_stage, stage) = canonical_tempdir("stage");
    symlink("../outside", stage.join("escape")).expect("escaping link");
    assert_eq!(
        digest_stage(&stage),
        Err("qualification_cache_payload_symlink_escape")
    );

    fs::remove_file(stage.join("escape")).expect("remove escaping link");
    symlink("/etc/passwd", stage.join("absolute")).expect("absolute link");
    assert_eq!(
        digest_stage(&stage),
        Err("qualification_cache_payload_symlink_invalid")
    );
}

#[test]
fn tree_digest_preserves_internal_hardlinks_and_rejects_external_links() {
    let (_linked, linked) = canonical_tempdir("linked stage");
    fs::write(linked.join("one"), b"linked").expect("file");
    fs::hard_link(linked.join("one"), linked.join("two")).expect("hard link");
    let linked_digest = digest_stage(&linked).expect("internal hard link");

    let (_copied, copied) = canonical_tempdir("copied stage");
    fs::write(copied.join("one"), b"linked").expect("first copy");
    fs::write(copied.join("two"), b"linked").expect("second copy");
    assert_ne!(linked_digest, digest_stage(&copied).expect("copied digest"));

    let (_outside, outside) = canonical_tempdir("outside");
    fs::write(outside.join("one"), b"linked").expect("external file");
    fs::hard_link(outside.join("one"), outside.join("two")).expect("outside link");
    let (_external, external) = canonical_tempdir("external stage");
    fs::hard_link(outside.join("one"), external.join("inside")).expect("stage link");
    assert_eq!(
        digest_stage(&external),
        Err("qualification_cache_payload_hardlink_escape")
    );
}

#[test]
fn tree_digest_rejects_socket_files() {
    let (_socket_stage, socket_stage) = canonical_tempdir("socket stage");
    let _socket = UnixListener::bind(socket_stage.join("socket")).expect("socket file");
    assert_eq!(
        digest_stage(&socket_stage),
        Err("qualification_cache_payload_entry_unsupported")
    );
}

#[test]
fn tree_digest_rejects_oversized_files_before_reading_them() {
    let (_stage, stage) = canonical_tempdir("stage");
    let path = stage.join("oversized");
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .expect("file");
    file.set_len(super::super::MAX_TOTAL_BYTES + 1)
        .expect("sparse oversized file");
    assert_eq!(
        digest_stage(&stage),
        Err("qualification_cache_payload_limit")
    );
}

#[test]
fn tree_digest_rejects_a_symlink_root() {
    let (_real, real) = canonical_tempdir("real stage");
    let (_parent, parent) = canonical_tempdir("parent");
    let alias = parent.join("alias");
    symlink(real, &alias).expect("root symlink");
    assert_eq!(
        digest_stage(&alias),
        Err("qualification_cache_payload_root_invalid")
    );
}
