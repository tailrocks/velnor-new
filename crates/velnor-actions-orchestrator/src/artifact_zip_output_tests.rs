use super::*;
use std::io::Write;

#[test]
fn existing_user_bytes_and_symlinks_are_never_removed_or_followed() {
    let dir = tempfile::tempdir().expect("directory");
    fs::write(dir.path().join("user"), b"preserve").expect("user bytes");
    assert!(Output::open(dir.path()).is_err());
    assert_eq!(
        fs::read(dir.path().join("user")).expect("still user"),
        b"preserve"
    );
    fs::remove_file(dir.path().join("user")).expect("fixture reset");
    let mut output = Output::open(dir.path()).expect("empty root");
    std::os::unix::fs::symlink("foreign", dir.path().join("file")).expect("planted symlink");
    assert!(output.create("file").is_err());
    output.rollback().expect("no owned children");
    assert_eq!(
        fs::read_link(dir.path().join("file")).expect("symlink preserved"),
        Path::new("foreign")
    );
}

#[test]
fn replaced_output_file_is_not_deleted_as_if_it_were_owned() {
    let dir = tempfile::tempdir().expect("directory");
    let mut output = Output::open(dir.path()).expect("root");
    output
        .create("file")
        .expect("owned")
        .write_all(b"owned")
        .expect("bytes");
    fs::rename(dir.path().join("file"), dir.path().join("moved-owned")).expect("move owned");
    fs::write(dir.path().join("file"), b"foreign").expect("replacement");
    assert!(output.rollback().is_err());
    assert_eq!(
        fs::read(dir.path().join("file")).expect("foreign preserved"),
        b"foreign"
    );
    assert_eq!(
        fs::read(dir.path().join("moved-owned")).expect("moved preserved"),
        b"owned"
    );
}

#[test]
fn root_replacement_cannot_redirect_writes_or_identity_cleanup() {
    let parent = tempfile::tempdir().expect("parent");
    let root = parent.path().join("root");
    fs::create_dir(&root).expect("root");
    let mut output = Output::open(&root).expect("owned root");
    output
        .create("file")
        .expect("file")
        .write_all(b"owned")
        .expect("bytes");
    let moved = parent.path().join("moved");
    fs::rename(&root, &moved).expect("move root");
    fs::create_dir(&root).expect("foreign root");
    fs::write(root.join("user"), b"foreign").expect("user");
    assert!(output.validate().is_err());
    assert!(output.create("later").is_err());
    output.rollback().expect("cleanup original pinned file");
    assert!(!moved.join("file").exists());
    assert_eq!(
        fs::read(root.join("user")).expect("replacement untouched"),
        b"foreign"
    );
    assert!(
        moved.is_dir() && root.is_dir(),
        "neither caller-owned root is removed"
    );
}

#[test]
fn late_scope_error_rolls_back_only_created_children_and_preserves_directory() {
    let dir = tempfile::tempdir().expect("root");
    let mut output = Output::open(dir.path()).expect("root");
    output
        .create("partial")
        .expect("partial")
        .write_all(b"bytes")
        .expect("bytes");
    assert!(super::super::finish(Err(invalid("deadline")), &mut output).is_err());
    assert!(dir.path().is_dir());
    assert_eq!(fs::read_dir(dir.path()).expect("empty").count(), 0);
}
