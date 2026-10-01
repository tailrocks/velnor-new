//! Committed-manifest file-read tests.
//!
//! Declared via `#[path]` from `discover.rs` under `cfg(test)` so the
//! discovery module keeps the file size gate. The helper is
//! cfg-independent, so these debug-mode tests cover the exact read the
//! release twin relies on.

use std::fs;

use tempfile::TempDir;

use super::read_manifest_file;
use crate::safe_read::MAX_REPO_FILE_BYTES;

/// Present file returns its exact text.
#[test]
fn present_file_returns_its_text() {
    let root = TempDir::new().expect("temp root");
    let dir = root.path().join(".velnor");
    fs::create_dir_all(&dir).expect("velnor dir");
    fs::write(dir.join("release-manifest.json"), "{\"schema\":1}").expect("manifest");
    assert_eq!(
        read_manifest_file(root.path()).expect("readable"),
        Some("{\"schema\":1}".to_owned())
    );
}

/// Absent file returns `None` (release builds fail closed on this).
#[test]
fn absent_file_returns_none() {
    let root = TempDir::new().expect("temp root");
    assert_eq!(read_manifest_file(root.path()).expect("absent"), None);
    fs::create_dir_all(root.path().join(".velnor")).expect("velnor dir");
    assert_eq!(read_manifest_file(root.path()).expect("absent"), None);
}

/// Non-UTF-8 file errors instead of masking as absent (X6).
#[test]
fn non_utf8_file_errors() {
    let root = TempDir::new().expect("temp root");
    let dir = root.path().join(".velnor");
    fs::create_dir_all(&dir).expect("velnor dir");
    fs::write(dir.join("release-manifest.json"), [0xff, 0xfe]).expect("manifest");
    assert!(read_manifest_file(root.path()).is_err());
}

/// Symlink and directory stand-ins error instead of masking (X6).
#[test]
#[cfg(unix)]
fn symlink_file_errors() {
    let root = TempDir::new().expect("temp root");
    let dir = root.path().join(".velnor");
    fs::create_dir_all(&dir).expect("velnor dir");
    fs::write(dir.join("real.json"), "{\"schema\":1}").expect("target");
    std::os::unix::fs::symlink(dir.join("real.json"), dir.join("release-manifest.json"))
        .expect("symlink");
    let err = read_manifest_file(root.path()).expect_err("symlink refused");
    assert!(err.to_string().contains("symlink_refused"), "{err}");
}

/// A directory at the manifest path errors instead of masking (X6).
#[test]
fn directory_at_manifest_path_errors() {
    let root = TempDir::new().expect("temp root");
    let dir = root.path().join(".velnor");
    fs::create_dir_all(dir.join("release-manifest.json")).expect("dir at path");
    assert!(read_manifest_file(root.path()).is_err());
}

/// Oversize files error instead of exhausting memory (X6).
#[test]
fn oversize_file_errors() {
    let root = TempDir::new().expect("temp root");
    let dir = root.path().join(".velnor");
    fs::create_dir_all(&dir).expect("velnor dir");
    let big = "x".repeat(usize::try_from(MAX_REPO_FILE_BYTES + 1).expect("bound fits"));
    fs::write(dir.join("release-manifest.json"), big).expect("manifest");
    let err = read_manifest_file(root.path()).expect_err("oversize refused");
    assert!(err.to_string().contains("oversize"), "{err}");
}
