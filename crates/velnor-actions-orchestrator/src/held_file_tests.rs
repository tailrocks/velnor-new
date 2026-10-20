use super::*;
use std::time::Duration;

fn deadline() -> CheckDeadline {
    CheckDeadline::after(Duration::from_secs(10)).expect("deadline")
}
fn asset() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("temp");
    let path = temp
        .path()
        .canonicalize()
        .expect("canonical")
        .join("payload");
    fs::write(&path, b"abc").expect("payload");
    (temp, path)
}

#[test]
fn held_digest_rejects_same_size_in_place_change_and_replaced_inode() {
    let (_temp, path) = asset();
    let held = HeldFile::capture(&path, deadline()).expect("capture");
    fs::write(&path, b"def").expect("same size");
    assert!(held.verify(deadline()).is_err());
    fs::remove_file(&path).expect("remove");
    fs::write(&path, b"abc").expect("replace same bytes");
    assert!(held.verify(deadline()).is_err());
}

#[test]
fn refreshed_metadata_cannot_bless_new_hardlink() {
    let (_temp, path) = asset();
    let mut held = HeldFile::capture(&path, deadline()).expect("capture");
    fs::hard_link(&path, path.with_extension("alias")).expect("hardlink");
    // Deliberately model the old normalization refresh: link count is still a
    // separate mandatory admission invariant even with a forged fresh snapshot.
    held.original = held.file.metadata().expect("metadata");
    assert_eq!(held.original.nlink(), 2);
    assert!(held.verify(deadline()).is_err());
}

#[test]
fn symlink_and_wrong_declared_digest_never_admit() {
    let (_temp, path) = asset();
    assert!(HeldFile::new(&path, &"0".repeat(64), 3, deadline()).is_err());
    let link = path.with_extension("link");
    std::os::unix::fs::symlink(&path, &link).expect("symlink");
    assert!(HeldFile::capture(&link, deadline()).is_err());
}
