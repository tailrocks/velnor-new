use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::Path;

use super::super::digest_stage;
use super::{copy_tree_to_new_dir, copy_tree_with_hook};

#[test]
fn copy_preserves_internal_links_modes_and_file_bytes() {
    let temp = tempfile::tempdir().expect("temporary root");
    let root = fs::canonicalize(temp.path()).expect("canonical root");
    let source = root.join("source");
    let destination = root.join("destination");
    fs::create_dir_all(source.join("installs/node/24.17.0/bin")).expect("tool tree");
    fs::write(source.join("installs/node/24.17.0/bin/node"), b"node-bytes").expect("node");
    symlink("24.17.0", source.join("installs/node/current")).expect("version link");
    fs::hard_link(
        source.join("installs/node/24.17.0/bin/node"),
        source.join("installs/node/24.17.0/bin/node-alias"),
    )
    .expect("hard link");
    let executable = source.join("installs/node/24.17.0/bin/node");
    let mut mode = fs::metadata(&executable)
        .expect("node metadata")
        .permissions();
    mode.set_mode(0o755);
    fs::set_permissions(&executable, mode).expect("node mode");
    assert!(
        super::open_directory(&source).is_ok(),
        "source {}",
        source.display()
    );
    assert!(
        super::open_directory(&root).is_ok(),
        "parent {}",
        root.display()
    );

    let expected = digest_stage(&source)
        .expect("source digest")
        .expect("non-empty source");
    let copied = copy_tree_to_new_dir(&source, &destination).expect("copy validated tree");
    assert_eq!(copied, expected);
    assert_eq!(digest_stage(&destination), Ok(Some(expected)));
    assert_eq!(
        fs::read_link(destination.join("installs/node/current"))
            .expect("copied link")
            .to_string_lossy(),
        "24.17.0"
    );
    assert_eq!(
        fs::read(destination.join("installs/node/24.17.0/bin/node")).expect("copied node"),
        b"node-bytes"
    );
    assert_eq!(
        fs::metadata(destination.join("installs/node/24.17.0/bin/node"))
            .expect("copied node metadata")
            .mode()
            & 0o777,
        0o755
    );
    let first = fs::metadata(destination.join("installs/node/24.17.0/bin/node"))
        .expect("first copied link");
    let second = fs::metadata(destination.join("installs/node/24.17.0/bin/node-alias"))
        .expect("second copied link");
    assert_eq!((first.dev(), first.ino()), (second.dev(), second.ino()));
}

#[test]
fn copy_rejects_an_escaping_link_before_creating_destination() {
    let temp = tempfile::tempdir().expect("temporary root");
    let root = fs::canonicalize(temp.path()).expect("canonical root");
    let source = root.join("source");
    let destination = root.join("destination");
    fs::create_dir(&source).expect("source");
    symlink("../outside", source.join("escape")).expect("escaping link");

    assert_eq!(
        copy_tree_to_new_dir(&source, &destination),
        Err("qualification_cache_payload_symlink_escape")
    );
    assert!(!destination.exists());
}

#[test]
fn copy_rejects_existing_destination_and_symlink_parent() {
    let temp = tempfile::tempdir().expect("temporary root");
    let root = fs::canonicalize(temp.path()).expect("canonical root");
    let source = root.join("source");
    fs::create_dir(&source).expect("source");
    fs::write(source.join("file"), b"payload").expect("source file");
    let existing = root.join("existing");
    fs::create_dir(&existing).expect("existing destination");
    assert_eq!(
        copy_tree_to_new_dir(&source, &existing),
        Err("qualification_cache_payload_destination_exists")
    );

    let external = tempfile::tempdir().expect("external parent");
    let external = fs::canonicalize(external.path()).expect("canonical external parent");
    let alias = root.join("alias");
    symlink(&external, &alias).expect("parent alias");
    let destination = alias.join("destination");
    assert_eq!(
        copy_tree_to_new_dir(&source, &destination),
        Err("qualification_cache_payload_path_invalid")
    );
    assert!(!external.join("destination").exists());
}

#[test]
fn copy_stops_when_source_entry_changes_after_open() {
    let temp = tempfile::tempdir().expect("temporary root");
    let root = fs::canonicalize(temp.path()).expect("canonical root");
    let source = root.join("source");
    let destination = root.join("destination");
    let outside = root.join("outside");
    fs::create_dir(&source).expect("source");
    fs::create_dir(&outside).expect("outside");
    let file = source.join("payload");
    let saved = source.join("saved-payload");
    let outside_file = outside.join("marker");
    fs::write(&file, vec![7; 16 * 1024 * 1024]).expect("payload bytes");
    fs::write(&outside_file, b"outside").expect("outside bytes");

    let copied = copy_tree_with_hook(&source, &destination, |relative| {
        if relative == Path::new("payload") {
            fs::rename(&file, &saved).expect("move opened source entry");
            symlink(&outside_file, &file).expect("replace source entry with external link");
        }
    });

    assert_eq!(copied, Err("qualification_cache_payload_file_changed"));
    assert!(!destination.exists());
    assert_eq!(fs::read(outside_file).expect("outside marker"), b"outside");
}
