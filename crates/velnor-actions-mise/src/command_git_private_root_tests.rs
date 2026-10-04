//! Cached paths never grant a destination beyond this private root.
#![cfg(any(target_os = "linux", target_os = "macos"))]
use super::{DIRECTORY_NAMES, FILE_NAMES, OwnedFile, PrivateGitRoot};
use std::io;

#[test]
fn named_mutable_file_alias_cannot_redirect_any_owned_slot() -> io::Result<()> {
    let mut root = PrivateGitRoot::create(&[])?;
    let foreign = PrivateGitRoot::create(&[])?;
    for index in 0..FILE_NAMES.len() {
        let original = root.files[index].clone();
        let files = &mut root.files;
        files[index] = foreign.slot_path(OwnedFile::Index).to_path_buf();
        let result = root.write_file(OwnedFile::Index, b"must not write");
        root.files[index] = original;
        assert_eq!(
            result.err().map(|error| error.to_string()).as_deref(),
            Some("private_git_root:cached_slot_changed")
        );
        assert!(std::fs::symlink_metadata(root.slot_path(OwnedFile::Index)).is_err());
        assert!(std::fs::symlink_metadata(foreign.slot_path(OwnedFile::Index)).is_err());
    }
    root.verify_binding()?;
    root.finish()?;
    foreign.finish()
}

#[test]
fn named_mutable_directory_alias_invalidates_the_whole_root() -> io::Result<()> {
    let mut root = PrivateGitRoot::create(&[])?;
    let foreign = PrivateGitRoot::create(&[])?;
    for index in 0..DIRECTORY_NAMES.len() {
        let original = root.directories[index].clone();
        let directories = &mut root.directories;
        directories[index] = foreign.path().to_path_buf();
        let result = root.write_file(OwnedFile::Index, b"must not write");
        root.directories[index] = original;
        assert_eq!(
            result.err().map(|error| error.to_string()).as_deref(),
            Some("private_git_root:cached_slot_changed")
        );
        assert!(std::fs::symlink_metadata(root.slot_path(OwnedFile::Index)).is_err());
    }
    root.verify_binding()?;
    root.finish()?;
    foreign.finish()
}

#[test]
fn shared_prefix_is_not_owned_slot_authority() -> io::Result<()> {
    let mut root = PrivateGitRoot::create(&[])?;
    let original = root.files[0].clone();
    let lookalike = root.path().with_extension("lookalike").join("index");
    let files = &mut root.files;
    files[0] = lookalike.clone();
    let result = root.write_file(OwnedFile::Index, b"must not write");
    root.files[0] = original;
    assert_eq!(
        result.err().map(|error| error.to_string()).as_deref(),
        Some("private_git_root:cached_slot_changed")
    );
    assert!(std::fs::symlink_metadata(lookalike).is_err());
    assert!(std::fs::symlink_metadata(root.slot_path(OwnedFile::Index)).is_err());
    root.verify_binding()?;
    root.finish()
}
