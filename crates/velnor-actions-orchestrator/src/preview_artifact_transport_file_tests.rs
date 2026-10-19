use super::*;
use std::{
    io::{Read, Write},
    time::Duration,
};

fn deadline() -> CheckDeadline {
    CheckDeadline::after(Duration::from_secs(5)).expect("deadline")
}
fn archive(root: &Path) -> SealedArchive {
    let mut archive = SealedArchive::create(
        root,
        7,
        3,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    )
    .expect("exclusive held file");
    archive
        .destination_file()
        .write_all(b"abc")
        .expect("download");
    archive
        .seal(deadline())
        .expect("exact independently known digest");
    archive
}

#[test]
fn sealed_archive_retains_descriptor_identity_and_removes_only_its_owned_name() {
    let root = tempfile::tempdir().expect("root");
    let root = root.path().canonicalize().expect("canonical");
    let mut archive = archive(&root);
    let mut bytes = Vec::new();
    archive
        .reader()
        .expect("reader")
        .read_to_end(&mut bytes)
        .expect("bytes");
    assert_eq!(bytes, b"abc");
    archive.verify(deadline()).expect("unchanged");
    assert!(archive.path.exists());
    drop(archive);
    assert!(fs::read_dir(root).expect("owned cleanup").next().is_none());
}

#[test]
fn archive_mutation_short_download_and_preexisting_output_fail_closed() {
    let root = tempfile::tempdir().expect("root");
    let root = root.path().canonicalize().expect("canonical");
    let mut original = archive(&root);
    assert!(SealedArchive::create(&root, 7, 3, &original.digest).is_err());
    fs::write(&original.path, b"abd").expect("same-size mutation");
    assert!(original.verify(deadline()).is_err());
    drop(original);
    let mut short = SealedArchive::create(&root, 8, 4, &"0".repeat(64)).expect("short archive");
    short
        .destination_file()
        .write_all(b"abc")
        .expect("short bytes");
    assert!(short.seal(deadline()).is_err());
    drop(short);
    assert_eq!(fs::read_dir(root).expect("cleanup").count(), 0);
}

#[test]
fn preseal_mode_change_cannot_replace_the_creation_identity_baseline() {
    let root = tempfile::tempdir().expect("root");
    let root = root.path().canonicalize().expect("canonical");
    let mut held = SealedArchive::create(
        &root,
        7,
        3,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    )
    .expect("exclusive file");
    held.destination_file()
        .write_all(b"abc")
        .expect("correct content");
    rustix::fs::fchmod(
        held.destination_file(),
        Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::ROTH,
    )
    .expect("mode mutation before seal");
    assert!(held.seal(deadline()).is_err());
    drop(held);
    assert_eq!(fs::read_dir(&root).expect("owned cleanup").count(), 0);
}

#[test]
fn renamed_archive_or_foreign_symlink_is_not_deleted_as_owned_cleanup() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().expect("root");
    let root = root.path().canonicalize().expect("canonical");
    let mut held = archive(&root);
    let path = held.path.clone();
    let moved = root.join("original.zip");
    fs::rename(&path, &moved).expect("move");
    symlink(&moved, &path).expect("foreign replacement");
    assert!(held.reader().is_err());
    assert!(held.verify(deadline()).is_err());
    drop(held);
    assert!(
        fs::symlink_metadata(&path)
            .expect("foreign link preserved")
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(&moved).expect("moved original preserved"), b"abc");
}
