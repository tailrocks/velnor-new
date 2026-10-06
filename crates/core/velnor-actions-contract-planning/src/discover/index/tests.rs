//! Unit tests for [`relative_posix`](super::relative_posix); cache-pruning
//! cases live in [`pruning`](self::pruning).
mod pruning;

use super::*;

/// Decodable names render as relative POSIX paths.
#[test]
fn relative_posix_renders_decodable() {
    let root = Path::new("/repo");
    let rendered = relative_posix(root, &root.join("alpha/src/lib.rs")).expect("decodable renders");
    assert_eq!(rendered.as_deref(), Some("alpha/src/lib.rs"));
}

/// Non-UTF-8 names skip explicitly instead of erroring.
#[test]
#[cfg(unix)]
fn relative_posix_skips_non_utf8() {
    use std::os::unix::ffi::OsStrExt;
    let root = Path::new("/repo");
    let raw = b"/repo/alpha/src/\xffinvalid.rs";
    let path = Path::new(std::ffi::OsStr::from_bytes(raw));
    let rendered = relative_posix(root, path).expect("skip is not an error");
    assert_eq!(rendered, None);
}

/// Paths outside the root still fail closed.
#[test]
fn relative_posix_rejects_escape() {
    let root = Path::new("/repo");
    let err = relative_posix(root, Path::new("/other/lib.rs")).expect_err("escape fails");
    assert!(
        matches!(err, IndexError::SymlinkEscape(_)),
        "fails closed: {err}"
    );
}
